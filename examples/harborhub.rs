use std::error::Error;

use arrow_json::{writer::LineDelimited, WriterBuilder};
use atif_arrow::trajectory_schema;
use atif_io::{InputFormat, TrajectoryReader};
use opendal::Operator;
use opendal_service_harborhub::HarborHub;
use serde_json::Value;

// Public Terminal-Bench trial; see examples/README.md for its job and provenance.
const PATH: &str = "trials/d4f5439d-8367-467f-938a-005585690681/trajectory.json";

/// Fetches a real public trajectory and checks the resulting Arrow batch.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    opendal::install_default();
    let operator = Operator::new(HarborHub::default())?;
    let metadata = operator.stat(PATH).await?;
    let source_uri = format!("harborhub://{PATH}");
    let mut reader = TrajectoryReader::open(
        &operator,
        PATH,
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
    assert_eq!(row["atif_schema_version"], "ATIF-v1.7");
    assert_eq!(row["agent"]["name"], "codex");
    assert_eq!(row["agent"]["model_name"], "gpt-6-astra");
    assert_eq!(row["final_metrics"]["total_prompt_tokens"], 417_384);
    let raw = row["raw_json"].as_str().ok_or("missing raw_json")?;
    assert_eq!(raw.len() as u64, metadata.content_length());
    let steps = row["steps"].as_array().ok_or("missing steps")?;
    assert_eq!(steps.len(), 17);
    let tool_calls: usize = steps
        .iter()
        .filter_map(|step| step["tool_calls"].as_array())
        .map(Vec::len)
        .sum();
    assert_eq!(tool_calls, 15);

    println!("Source: {source_uri}");
    println!(
        "ATIF-v1.7 -> Arrow: {} row, {} columns, {} steps, {tool_calls} tool calls, {} bytes",
        batch.num_rows(),
        batch.num_columns(),
        steps.len(),
        raw.len()
    );
    println!("Schema: {:?}", batch.schema());
    Ok(())
}
