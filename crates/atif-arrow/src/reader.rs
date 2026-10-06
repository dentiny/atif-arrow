use std::io::BufRead;

use arrow_array::RecordBatch;
use arrow_json::{reader::Decoder, ReaderBuilder};

use crate::{convert::encode_trajectory, parse::parse_document, trajectory_schema, ReadError};

/// The document layout supplied to a trajectory reader.
#[derive(Debug, Clone, Copy)]
pub enum InputFormat {
    Json,
    JsonLines,
}

/// Reads trajectories in order and yields batches with at most `batch_size` rows.
/// Blank JSONL lines are skipped. Any error stops reading and discards the current batch.
pub struct TrajectoryReader<R> {
    reader: R,
    format: InputFormat,
    batch_size: usize,
    decoder: Decoder,
    source_uri: Option<String>,
    // Reused input buffer holding the current document, including its original whitespace.
    buffer: String,
    // One-based index of the next document; blank JSONL lines do not advance it.
    record_index: u64,
    done: bool,
}

impl<R: BufRead> TrajectoryReader<R> {
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
        let decoder = ReaderBuilder::new(trajectory_schema())
            .with_batch_size(batch_size)
            .build_decoder()
            .map_err(|source| ReadError::Record {
                source_uri: source_uri.clone(),
                record_index: 1,
                source: Box::new(source),
            })?;
        Ok(Self {
            reader,
            format,
            batch_size,
            decoder,
            source_uri,
            buffer: String::new(),
            record_index: 1,
            done: false,
        })
    }

    /// Reads one document into the batch decoder, retaining its original whitespace.
    fn read_record(&mut self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        if self.done {
            return Ok(false);
        }
        match self.format {
            InputFormat::Json => {
                self.done = true;
                self.buffer.clear();
                self.reader.read_to_string(&mut self.buffer)?;
            }
            InputFormat::JsonLines => loop {
                self.buffer.clear();
                if self.reader.read_line(&mut self.buffer)? == 0 {
                    self.done = true;
                    return Ok(false);
                }
                if !self.buffer.trim().is_empty() {
                    break;
                }
            },
        }
        let trajectory = parse_document(&self.buffer)?;
        let encoded = encode_trajectory(
            &trajectory,
            &self.buffer,
            self.source_uri.as_deref(),
            self.record_index,
        )?;
        let consumed = self.decoder.decode(encoded.as_bytes())?;
        debug_assert_eq!(consumed, encoded.len());
        Ok(true)
    }
}

impl<R: BufRead> Iterator for TrajectoryReader<R> {
    type Item = Result<RecordBatch, ReadError>;

    /// Collects a batch, reporting the source and document index if a record fails.
    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let first_index = self.record_index;
        for _ in 0..self.batch_size {
            match self.read_record() {
                Ok(true) => self.record_index += 1,
                Ok(false) => break,
                Err(source) => {
                    self.done = true;
                    return Some(Err(ReadError::Record {
                        source_uri: self.source_uri.clone(),
                        record_index: self.record_index,
                        source,
                    }));
                }
            }
        }
        self.decoder
            .flush()
            .map_err(|source| {
                self.done = true;
                ReadError::Record {
                    source_uri: self.source_uri.clone(),
                    record_index: first_index,
                    source: Box::new(source),
                }
            })
            .transpose()
    }
}
