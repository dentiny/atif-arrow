//! Compare full and projected conversion in separate processes with an RSS profiler.
//! `cargo run -p atif-arrow --example projection_memory -- projected`
use atif_arrow::{trajectory_schema, TrajectoryBatchBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "full".into());
    let input = serde_json::json!({"schema_version":"ATIF-v1.8",
        "agent":{"name":"example", "version":"1"},
        "steps":[{"step_id":1,"source":"agent","message":"x".repeat(512 * 1024)}]
    })
    .to_string();
    let mut builder = match mode.as_str() {
        "full" => TrajectoryBatchBuilder::new(64)?,
        "projected" => TrajectoryBatchBuilder::new_with_projection(
            64,
            &[trajectory_schema().index_of("source_record_index")?],
        )?,
        _ => return Err("expected full or projected".into()),
    };
    for index in 1..=64 {
        builder.append_json(&input, None, index)?;
    }
    let batch = builder.flush()?.unwrap();
    println!(
        "mode={mode} rows={} columns={} retained_arrow_bytes={}",
        batch.num_rows(),
        batch.num_columns(),
        batch.get_array_memory_size()
    );
    Ok(())
}
