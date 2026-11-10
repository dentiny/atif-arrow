//! Run with `cargo run -p atif-arrow --release --example bench_raw_json`.
//! Workload setup is excluded from wall time; process RSS includes it.
use atif_arrow::{trajectory_schema, TrajectoryBatchBuilder};
use std::{hint::black_box, time::Instant};
fn main() {
    let projection = std::env::args().nth(1).unwrap_or_else(|| "raw".into());
    let schema = trajectory_schema();
    let indices: Vec<_> = match projection.as_str() {
        "raw" => vec![schema.index_of("raw_json").unwrap()],
        "session" => vec![schema.index_of("session_id").unwrap()],
        "full" => (0..schema.fields().len()).collect(),
        _ => panic!("use raw, session or full"),
    };
    let input=serde_json::json!({"schema_version":"ATIF-v1.6","session_id":"bench","agent":{"name":"bench","version":"1"},"steps":[{"step_id":1,"source":"user","message":"x".repeat(1024*1024)}]}).to_string();
    let start = Instant::now();
    let mut rows = 0;
    while start.elapsed().as_secs_f64() < 2.0 {
        let mut builder = TrajectoryBatchBuilder::new(16, Some(&indices)).unwrap();
        for i in 1..=16 {
            builder.append_json(&input, Some("bench"), i).unwrap();
        }
        let batch = builder.flush().unwrap().unwrap();
        assert_eq!(batch.num_rows(), 16);
        black_box(batch);
        rows += 16;
    }
    println!(
        "{{\"case\":\"atif-{projection}\",\"rows\":{rows},\"seconds\":{},\"ns_per_row\":{}}}",
        start.elapsed().as_secs_f64(),
        start.elapsed().as_nanos() as f64 / rows as f64
    );
}
