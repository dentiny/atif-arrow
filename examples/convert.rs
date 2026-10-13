use std::error::Error;

use arrow_array::{Array, Float64Array, Int64Array, ListArray, StringArray, StructArray};
use atif_arrow::{parse_trajectory, to_record_batch, trajectory_schema};

// cargo run -- /path/to/trajectory.json
fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: convert <trajectory.json>")?;
    let json = std::fs::read_to_string(&path)?;
    let parsed = parse_trajectory(&json)?;
    let batch = to_record_batch(&parsed, Some(&path), 1)?;
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
    assert_eq!(raw.value(0), json);

    println!("Source: {path}");
    println!(
        "{} -> Arrow: {} row, {} columns",
        parsed.trajectory.schema_version,
        batch.num_rows(),
        batch.num_columns()
    );
    println!("Agent: {}", name.value(0));
    println!("Steps: {}", steps.value_length(0));
    if !metrics.is_null(0) && !tokens.is_null(0) {
        println!("Total prompt tokens: {}", tokens.value(0));
    }
    if !metrics.is_null(0) && !cost.is_null(0) {
        println!("Total cost (USD): {}", cost.value(0));
    }
    println!("Original JSON preserved: {}", raw.value(0) == json);
    Ok(())
}
