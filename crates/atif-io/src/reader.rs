use arrow_array::RecordBatch;
use atif_arrow::{ConversionError, TrajectoryBatchBuilder};
use futures::{
    future,
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt},
    stream, Stream, StreamExt,
};

use crate::{error::BoxError, ReadError};

/// The document layout supplied to a trajectory reader.
#[derive(Debug, Clone, Copy)]
pub enum InputFormat {
    Json,
    JsonLines,
}

/// Reads trajectories asynchronously in order, with at most `batch_size` rows per batch.
/// Blank JSONL lines are skipped. Any error stops reading and discards the current batch.
pub struct TrajectoryReader<R> {
    reader: R,
    format: InputFormat,
    batch_size: usize,
    source_uri: Option<String>,
    // Partial bytes survive cancellation, including split UTF-8.
    buffer: Vec<u8>,
    // Documents of the pending batch; survive cancellation until converted.
    documents: Vec<String>,
    // One-based index of the next document; blank JSONL lines do not advance it.
    record_index: u64,
    done: bool,
}

impl<R: AsyncBufRead + Unpin> TrajectoryReader<R> {
    /// Creates a reader with a positive batch size and optional source provenance.
    pub fn new(
        reader: R,
        format: InputFormat,
        batch_size: usize,
        source_uri: Option<String>,
    ) -> Result<Self, ReadError> {
        if batch_size == 0 {
            return Err(ReadError::InvalidBatchSize);
        }
        Ok(Self {
            reader,
            format,
            batch_size,
            source_uri,
            buffer: Vec::new(),
            documents: Vec::new(),
            record_index: 1,
            done: false,
        })
    }

    /// Reads the next document, keeping partial reads in the input buffer.
    async fn read_document(&mut self) -> Result<Option<String>, BoxError> {
        while !self.done {
            self.done = match self.format {
                InputFormat::Json => {
                    self.reader.read_to_end(&mut self.buffer).await?;
                    true
                }
                InputFormat::JsonLines => {
                    self.reader.read_until(b'\n', &mut self.buffer).await? == 0
                }
            };
            let input = String::from_utf8(std::mem::take(&mut self.buffer))?;
            if matches!(self.format, InputFormat::JsonLines) && input.trim().is_empty() {
                self.buffer = input.into_bytes();
                self.buffer.clear();
                continue;
            }
            return Ok(Some(input));
        }
        Ok(None)
    }

    /// Reads up to `batch_size` documents; a read error ends input after them.
    async fn read_batch(&mut self) -> Batch {
        let mut error = None;
        while !self.done && self.documents.len() < self.batch_size {
            match self.read_document().await {
                Ok(Some(document)) => {
                    self.documents.push(document);
                    self.record_index += 1;
                }
                Ok(None) => {}
                Err(source) => {
                    self.done = true;
                    error = Some(ReadError::record(
                        &self.source_uri,
                        self.record_index,
                        source,
                    ));
                }
            }
        }
        let documents = std::mem::take(&mut self.documents);
        Batch {
            first_index: self.record_index - documents.len() as u64,
            documents,
            source_uri: self.source_uri.clone(),
            error,
        }
    }

    /// Returns the next batch, or None at EOF; pending reads can be cancelled and resumed.
    pub async fn next_batch(&mut self) -> Result<Option<RecordBatch>, ReadError> {
        let result = self.read_batch().await.convert();
        self.done |= result.is_err();
        result
    }

    /// Exposes batches as a stream that ends at EOF or after the first error.
    pub fn into_stream(self) -> impl Stream<Item = Result<RecordBatch, ReadError>> {
        stream::try_unfold(self, |mut reader| async move {
            Ok(reader.next_batch().await?.map(|batch| (batch, reader)))
        })
    }

    /// Yields the same batches and first error as [`into_stream`](Self::into_stream), in order.
    pub fn into_parallel_stream(
        self,
        parallelism: usize,
    ) -> impl Stream<Item = Result<RecordBatch, ReadError>> {
        stream::unfold(self, |mut reader| async move {
            let batch = reader.read_batch().await;
            if batch.is_empty() {
                return None;
            }
            Some((tokio::task::spawn_blocking(move || batch.convert()), reader))
        })
        .buffered(parallelism.max(1))
        .filter_map(|result| {
            let result =
                result.unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()));
            future::ready(result.transpose())
        })
        .scan(false, |failed, batch| {
            if *failed {
                return future::ready(None);
            }
            *failed = batch.is_err();
            future::ready(Some(batch))
        })
    }
}

/// Documents numbered from `first_index`, followed by the read error that ended input.
struct Batch {
    documents: Vec<String>,
    first_index: u64,
    source_uri: Option<String>,
    error: Option<ReadError>,
}

impl Batch {
    fn is_empty(&self) -> bool {
        self.documents.is_empty() && self.error.is_none()
    }

    /// Converts the documents, reporting the read error only if they all convert.
    fn convert(self) -> Result<Option<RecordBatch>, ReadError> {
        let mut batch = None;
        if !self.documents.is_empty() {
            let error = |record_index, source: ConversionError| {
                ReadError::record(&self.source_uri, record_index, source.into())
            };
            let mut builder = TrajectoryBatchBuilder::new(self.documents.len())
                .map_err(|source| error(self.first_index, source))?;
            for (record_index, document) in (self.first_index..).zip(&self.documents) {
                builder
                    .append_json(document, self.source_uri.as_deref(), record_index)
                    .map_err(|source| error(record_index, source))?;
            }
            batch = builder
                .flush()
                .map_err(|source| error(self.first_index, source))?;
        }
        self.error.map_or(Ok(batch), Err)
    }
}
