use arrow_array::RecordBatch;
use arrow_json::{reader::Decoder, ReaderBuilder};

use crate::{
    convert::encode_trajectory, parse::parse_document, trajectory_schema, ConversionError,
    ParseError,
};

/// Converts JSON documents into Arrow batches without reading from an external source.
pub struct TrajectoryBatchBuilder {
    decoder: Decoder,
    batch_size: usize,
}

impl TrajectoryBatchBuilder {
    /// Creates a converter with a positive maximum number of rows per batch.
    pub fn new(batch_size: usize) -> Result<Self, ConversionError> {
        if batch_size == 0 {
            return Err(ConversionError::Field(ParseError::new(
                "batch_size",
                "batch size must be greater than zero",
            )));
        }
        let decoder = ReaderBuilder::new(trajectory_schema())
            .with_batch_size(batch_size)
            .build_decoder()?;
        Ok(Self {
            decoder,
            batch_size,
        })
    }

    /// Appends one complete document; flush a full batch before adding another row.
    pub fn append_json(
        &mut self,
        input: &str,
        source_uri: Option<&str>,
        record_index: u64,
    ) -> Result<(), ConversionError> {
        if self.decoder.len() == self.batch_size {
            return Err(ConversionError::Field(ParseError::new(
                "batch_size",
                "batch is full; flush before appending another document",
            )));
        }
        let trajectory = parse_document(input).map_err(ConversionError::Field)?;
        let encoded = encode_trajectory(&trajectory, input, source_uri, record_index)?;
        let consumed = self.decoder.decode(encoded.as_bytes())?;
        debug_assert_eq!(consumed, encoded.len());
        Ok(())
    }

    /// Returns the accumulated rows and resets the converter for the next batch.
    pub fn flush(&mut self) -> Result<Option<RecordBatch>, ConversionError> {
        self.decoder.flush().map_err(ConversionError::Arrow)
    }
}
