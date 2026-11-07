use arrow_array::{
    builder::{ArrayBuilder, StructBuilder},
    RecordBatch,
};
use arrow_schema::SchemaRef;

use crate::{
    builders::append_row, convert::normalize_trajectory, parse::parse_document, trajectory_schema,
    ConversionError, ParseError, Trajectory,
};

/// Converts JSON documents into Arrow batches without reading from an external source.
pub struct TrajectoryBatchBuilder {
    rows: StructBuilder,
    batch_size: usize,
    schema: SchemaRef,
}

impl TrajectoryBatchBuilder {
    /// Creates a converter with a positive maximum number of rows per batch.
    pub fn new(batch_size: usize) -> Result<Self, ConversionError> {
        Self::with_schema(batch_size, trajectory_schema())
    }

    /// Creates a converter that builds only the selected top-level columns.
    ///
    /// Indices refer to [`trajectory_schema`]. Output follows the supplied order,
    /// including repeated indices. An empty projection preserves the row count.
    /// Every document is still fully parsed and validated, including omitted fields.
    /// Unselected columns do not allocate Arrow builders or accumulate batch data;
    /// parsing and validation still require memory for one complete document.
    pub fn new_with_projection(
        batch_size: usize,
        projection: &[usize],
    ) -> Result<Self, ConversionError> {
        let schema = trajectory_schema().project(projection)?;
        Self::with_schema(batch_size, std::sync::Arc::new(schema))
    }

    fn with_schema(batch_size: usize, schema: SchemaRef) -> Result<Self, ConversionError> {
        if batch_size == 0 {
            return Err(ConversionError::Field(ParseError::new(
                "batch_size",
                "batch size must be greater than zero",
            )));
        }
        let rows = StructBuilder::from_fields(schema.fields().clone(), batch_size);
        Ok(Self {
            rows,
            batch_size,
            schema,
        })
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
        let row = normalize_trajectory(
            trajectory,
            raw_json,
            source_uri,
            record_index,
            self.schema.fields(),
        )?;
        append_row(&mut self.rows, &row)
    }

    /// Returns the accumulated rows and resets the converter for the next batch.
    pub fn flush(&mut self) -> Result<Option<RecordBatch>, ConversionError> {
        if self.rows.is_empty() {
            return Ok(None);
        }
        Ok(Some(RecordBatch::from(self.rows.finish())))
    }
}

#[cfg(test)]
mod tests {
    use super::TrajectoryBatchBuilder;
    use arrow_array::Array;

    #[test]
    fn omitted_payloads_do_not_accumulate_in_arrow_builders() {
        let input = serde_json::json!({"schema_version":"ATIF-v1.8",
            "agent":{"name":"test", "version":"1"},
            "steps":[{"step_id":1,"source":"agent","message":"x".repeat(64 * 1024)}]
        })
        .to_string();
        let mut builder = TrajectoryBatchBuilder::new_with_projection(64, &[1]).unwrap();
        assert_eq!(builder.rows.field_builders().len(), 1);
        for index in 1..=64 {
            builder.append_json(&input, None, index).unwrap();
        }
        assert!(builder.rows.finish_cloned().get_array_memory_size() < 4096);
        assert_eq!(builder.flush().unwrap().unwrap().num_rows(), 64);
    }
}
