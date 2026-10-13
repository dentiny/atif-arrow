use std::error::Error;

use arrow_array::{
    cast::AsArray,
    types::{Float64Type, Int64Type},
    Array,
};
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
    let agent = batch["agent"].as_struct();
    let name = agent["name"].as_string::<i32>();
    let steps = batch["steps"].as_list::<i32>();
    let metrics = batch["final_metrics"].as_struct();
    let tokens = metrics["total_prompt_tokens"].as_primitive::<Int64Type>();
    let cost = metrics["total_cost_usd"].as_primitive::<Float64Type>();
    let raw = batch["raw_json"].as_string::<i32>();
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
