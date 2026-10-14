use arrow_array::RecordBatch;
use atif_arrow::{ConversionError, TrajectoryBatchBuilder};
use futures::{
    future,
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt},
    stream, Stream, StreamExt,
};

use crate::ReadError;

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
    // Keeps converted rows across cancellation until the batch is flushed.
    builder: TrajectoryBatchBuilder,
    source_uri: Option<String>,
    // Reused document buffer; partial bytes survive cancellation, including split UTF-8.
    buffer: Vec<u8>,
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
        let builder = TrajectoryBatchBuilder::new(batch_size)
            .map_err(|source| record_error(&source_uri, 1, source.into()))?;
        Ok(Self {
            reader,
            format,
            batch_size,
            builder,
            source_uri,
            buffer: Vec::new(),
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
                self.recycle(input);
                continue;
            }
            return Ok(Some(input));
        }
        Ok(None)
    }

    /// Reads one document into the converter.
    async fn read_record(&mut self) -> Result<bool, BoxError> {
        let Some(input) = self.read_document().await? else {
            return Ok(false);
        };
        self.builder
            .append_json(&input, self.source_uri.as_deref(), self.record_index)?;
        self.recycle(input);
        Ok(true)
    }

    fn recycle(&mut self, input: String) {
        self.buffer = input.into_bytes();
        self.buffer.clear();
    }

    fn record_error(&self, record_index: u64, source: BoxError) -> ReadError {
        record_error(&self.source_uri, record_index, source)
    }

    /// Returns the next batch, or None at EOF; pending reads can be cancelled and resumed.
    pub async fn next_batch(&mut self) -> Result<Option<RecordBatch>, ReadError> {
        if self.done {
            return Ok(None);
        }
        let first_index = self.record_index - self.builder.num_rows() as u64;
        while self.builder.num_rows() < self.batch_size {
            match self.read_record().await {
                Ok(true) => self.record_index += 1,
                Ok(false) => break,
                Err(source) => {
                    self.done = true;
                    return Err(self.record_error(self.record_index, source));
                }
            }
        }
        self.builder.flush().map_err(|source| {
            self.done = true;
            self.record_error(first_index, source.into())
        })
    }

    /// Exposes batches as a stream that ends at EOF or after the first error.
    pub fn into_stream(self) -> impl Stream<Item = Result<RecordBatch, ReadError>> {
        stream::try_unfold(self, |mut reader| async move {
            Ok(reader.next_batch().await?.map(|batch| (batch, reader)))
        })
    }

    /// Like [`into_stream`](Self::into_stream), but converts up to `parallelism` batches at
    /// once on Tokio's blocking pool. Batches keep input order; must be polled in a Tokio runtime.
    pub fn into_parallel_stream(
        self,
        parallelism: usize,
    ) -> impl Stream<Item = Result<RecordBatch, ReadError>> {
        stream::try_unfold(self, |mut reader| async move {
            let first_index = reader.record_index;
            let mut documents = Vec::new();
            while documents.len() < reader.batch_size {
                match reader.read_document().await {
                    Ok(Some(document)) => documents.push(document),
                    Ok(None) => break,
                    Err(source) => {
                        reader.done = true;
                        let record_index = first_index + documents.len() as u64;
                        return Err(reader.record_error(record_index, source));
                    }
                }
            }
            if documents.is_empty() {
                return Ok(None);
            }
            reader.record_index += documents.len() as u64;
            let source_uri = reader.source_uri.clone();
            let task =
                tokio::task::spawn_blocking(move || convert(&documents, &source_uri, first_index));
            Ok(Some((task, reader)))
        })
        .map(|task| async {
            task?
                .await
                .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()))
        })
        .buffered(parallelism.max(1))
        .scan(false, |failed, batch| {
            future::ready((!std::mem::replace(failed, batch.is_err())).then_some(batch))
        })
    }
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

fn record_error(source_uri: &Option<String>, record_index: u64, source: BoxError) -> ReadError {
    ReadError::Record {
        source_uri: source_uri.clone(),
        record_index,
        source,
    }
}

/// Converts nonempty documents numbered from `first_index` into one batch.
fn convert(
    documents: &[String],
    source_uri: &Option<String>,
    first_index: u64,
) -> Result<RecordBatch, ReadError> {
    let error = |record_index, source: ConversionError| {
        record_error(source_uri, record_index, source.into())
    };
    let mut builder = TrajectoryBatchBuilder::new(documents.len())
        .map_err(|source| error(first_index, source))?;
    for (record_index, document) in (first_index..).zip(documents) {
        builder
            .append_json(document, source_uri.as_deref(), record_index)
            .map_err(|source| error(record_index, source))?;
    }
    let batch = builder
        .flush()
        .map_err(|source| error(first_index, source))?;
    Ok(batch.expect("documents are nonempty"))
}
