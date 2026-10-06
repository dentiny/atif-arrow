use arrow_array::RecordBatch;
use atif_arrow::TrajectoryBatchBuilder;
use futures::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt},
    stream, Stream,
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
        let builder =
            TrajectoryBatchBuilder::new(batch_size).map_err(|source| ReadError::Record {
                source_uri: source_uri.clone(),
                record_index: 1,
                source: Box::new(source),
            })?;
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

    /// Reads one document into the converter, keeping partial reads in the input buffer.
    async fn read_record(&mut self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        if self.done {
            return Ok(false);
        }
        loop {
            let eof = match self.format {
                InputFormat::Json => {
                    self.reader.read_to_end(&mut self.buffer).await?;
                    true
                }
                InputFormat::JsonLines => {
                    self.reader.read_until(b'\n', &mut self.buffer).await? == 0
                }
            };
            self.done = eof;
            let input = std::str::from_utf8(&self.buffer)?;
            if matches!(self.format, InputFormat::JsonLines) && input.trim().is_empty() {
                self.buffer.clear();
                if eof {
                    return Ok(false);
                }
                continue;
            }
            self.builder
                .append_json(input, self.source_uri.as_deref(), self.record_index)?;
            self.buffer.clear();
            return Ok(true);
        }
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
                    return Err(ReadError::Record {
                        source_uri: self.source_uri.clone(),
                        record_index: self.record_index,
                        source,
                    });
                }
            }
        }
        self.builder.flush().map_err(|source| {
            self.done = true;
            ReadError::Record {
                source_uri: self.source_uri.clone(),
                record_index: first_index,
                source: Box::new(source),
            }
        })
    }

    /// Exposes batches as a stream that ends at EOF or after the first error.
    pub fn into_stream(self) -> impl Stream<Item = Result<RecordBatch, ReadError>> {
        stream::try_unfold(self, |mut reader| async move {
            Ok(reader.next_batch().await?.map(|batch| (batch, reader)))
        })
    }
}
