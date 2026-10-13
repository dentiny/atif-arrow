use std::error::Error;

use arrow_json::{writer::LineDelimited, WriterBuilder};
use atif_arrow::trajectory_schema;
use atif_io::{InputFormat, TrajectoryReader};
use opendal::Operator;
use opendal_service_harborhub::HarborHub;
use serde_json::Value;

/// Reads an ATIF trajectory from a supplied public Harbor Hub object path.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: harborhub <trials/<trial-id>/trajectory.json>")?;
    opendal::install_default();
    let operator = Operator::new(HarborHub::default())?;
    let metadata = operator.stat(&path).await?;
    let source_uri = format!("harborhub://{path}");
    let mut reader = TrajectoryReader::open(
        &operator,
        &path,
        InputFormat::Json,
        1,
        Some(source_uri.clone()),
    )
    .await?;
    let batch = reader.next_batch().await?.ok_or("empty trajectory")?;
    assert_eq!(batch.schema(), trajectory_schema());
    assert_eq!(batch.num_rows(), 1);
    assert!(reader.next_batch().await?.is_none());

    // Inspect the actual Arrow values using its standard JSON writer.
    let mut writer = WriterBuilder::new().build::<_, LineDelimited>(Vec::new());
    writer.write(&batch)?;
    writer.finish()?;
    let row: Value = serde_json::from_slice(&writer.into_inner())?;
    assert_eq!(row["source_uri"], source_uri);
    assert_eq!(row["source_record_index"], 1);
    let raw = row["raw_json"].as_str().ok_or("missing raw_json")?;
    assert_eq!(raw.len() as u64, metadata.content_length());
    let steps = row["steps"].as_array().ok_or("missing steps")?;
    let tool_calls: usize = steps
        .iter()
        .filter_map(|step| step["tool_calls"].as_array())
        .map(Vec::len)
        .sum();

    println!("Source: {source_uri}");
    println!(
        "{} -> Arrow: {} row, {} columns, {} steps, {tool_calls} tool calls, {} bytes",
        row["atif_schema_version"]
            .as_str()
            .ok_or("missing schema version")?,
        batch.num_rows(),
        batch.num_columns(),
        steps.len(),
        raw.len()
    );
    println!("Schema: {:?}", batch.schema());
    Ok(())
}
