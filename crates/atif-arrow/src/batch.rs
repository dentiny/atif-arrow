use arrow_array::{
    builder::{ArrayBuilder, StructBuilder},
    RecordBatch,
};

use crate::{
    builders::append_object, convert::normalize_trajectory, parse::parse_document,
    trajectory_schema, ConversionError, ParseError, Trajectory,
};

/// Converts JSON documents into Arrow batches without reading from an external source.
pub struct TrajectoryBatchBuilder {
    rows: StructBuilder,
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
        let rows = StructBuilder::from_fields(trajectory_schema().fields().clone(), batch_size);
        Ok(Self { rows, batch_size })
    }

    /// Appends one complete document; flush a full batch before adding another row.
    pub fn append_json(
        &mut self,
        input: &str,
        source_uri: Option<&str>,
        record_index: u64,
    ) -> Result<(), ConversionError> {
        if self.rows.len() == self.batch_size {
            return Err(ConversionError::Field(ParseError::new(
                "batch_size",
                "batch is full; flush before appending another document",
            )));
        }
        let trajectory = parse_document(input).map_err(ConversionError::Field)?;
        self.append_trajectory(&trajectory, input, source_uri, record_index)
    }

    /// Callers check capacity before parsing; normalize the whole row before
    /// appending so invalid input cannot leave partially written Arrow arrays.
    pub(crate) fn append_trajectory(
        &mut self,
        trajectory: &Trajectory,
        raw_json: &str,
        source_uri: Option<&str>,
        record_index: u64,
    ) -> Result<(), ConversionError> {
        let row = normalize_trajectory(trajectory, raw_json, source_uri, record_index)?;
        append_object(&mut self.rows, &row);
        Ok(())
    }

    /// Returns the accumulated rows and resets the converter for the next batch.
    pub fn flush(&mut self) -> Result<Option<RecordBatch>, ConversionError> {
        if self.rows.is_empty() {
            return Ok(None);
        }
        Ok(Some(RecordBatch::from(self.rows.finish())))
    }
}
