//! Run with cargo bench -p atif-arrow --bench trajectory.
use atif_arrow::{trajectory_schema, TrajectoryBatchBuilder};
use serde_json::json;
use std::{hint::black_box, time::Instant};
fn main() {
    let id = trajectory_schema().index_of("trajectory_id").unwrap();
    for unknown_bytes in [0, 4096] {
        let document = json!({"schema_version":"ATIF-v1.8", "trajectory_id":"bench", "agent":{"name":"bench","version":"1"}, "steps":[{"step_id":1,"source":"user","message":"hello"}], "unknown_payload":"x".repeat(unknown_bytes)}).to_string();
        for projected in [false, true] {
            for iteration in 0..8 {
                let start = Instant::now();
                let indices = [id];
                let mut builder =
                    TrajectoryBatchBuilder::new(1024, projected.then_some(indices.as_slice()))
                        .unwrap();
                let mut rows = 0;
                for index in 1..=20000 {
                    builder
                        .append_json(black_box(&document), None, index)
                        .unwrap();
                    if index % 1024 == 0 {
                        rows += black_box(builder.flush().unwrap().unwrap()).num_rows();
                    }
                }
                if let Some(batch) = builder.flush().unwrap() {
                    rows += black_box(batch).num_rows();
                }
                assert_eq!(rows, 20000);
                if iteration > 0 {
                    println!(
                        "unknown_bytes={unknown_bytes} projected={projected} seconds={:.6}",
                        start.elapsed().as_secs_f64()
                    );
                }
            }
        }
    }
}
