use std::error::Error;

use arrow_array::{Float64Array, Int64Array, ListArray, StringArray, StructArray};
use atif_arrow::{parse_trajectory, to_record_batch, trajectory_schema};

// A complete local input: no data download or credentials required.
const DOCUMENT: &str = r#"{
  "schema_version": "ATIF-v1.8",
  "agent": {"name": "example", "version": "1"},
  "steps": [
    {"step_id": 1, "source": "user", "message": "Say hello."},
    {"step_id": 2, "source": "agent", "message": "Hello!",
     "metrics": {"prompt_tokens": 12, "completion_tokens": 4, "cost_usd": 0.001}}
  ],
  "final_metrics": {
    "total_prompt_tokens": 12,
    "total_completion_tokens": 4,
    "total_cost_usd": 0.001,
    "total_steps": 2
  }
}"#;

fn main() -> Result<(), Box<dyn Error>> {
    let parsed = parse_trajectory(DOCUMENT)?;
    let batch = to_record_batch(&parsed, Some("embedded://example"), 1)?;
    assert_eq!(batch.schema(), trajectory_schema());

    // Read typed Arrow columns. Each row contains one entire trajectory.
    let agent = batch["agent"]
        .as_any()
        .downcast_ref::<StructArray>()
        .unwrap();
    let name = agent["name"]
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let steps = batch["steps"].as_any().downcast_ref::<ListArray>().unwrap();
    let metrics = batch["final_metrics"]
        .as_any()
        .downcast_ref::<StructArray>()
        .unwrap();
    let tokens = metrics["total_prompt_tokens"]
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    let cost = metrics["total_cost_usd"]
        .as_any()
        .downcast_ref::<Float64Array>()
        .unwrap();
    let raw = batch["raw_json"]
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    assert_eq!(raw.value(0), DOCUMENT);

    println!(
        "ATIF-v1.8 -> Arrow: {} row, {} columns",
        batch.num_rows(),
        batch.num_columns()
    );
    println!("Agent: {}", name.value(0));
    println!("Steps: {}", steps.value_length(0));
    println!("Total prompt tokens: {}", tokens.value(0));
    println!("Total cost (USD): {}", cost.value(0));
    println!("Original JSON preserved: {}", raw.value(0) == DOCUMENT);
    Ok(())
}
