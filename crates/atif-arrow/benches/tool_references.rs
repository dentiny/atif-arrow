//! Reproduce fan-out reference validation without storage or network costs.
use atif_arrow::{trajectory_schema, TrajectoryBatchBuilder};
use serde_json::json;
use std::{hint::black_box, time::Instant};

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let calls: usize = args.first().map(|s| s.parse().unwrap()).unwrap_or(1024);
    let iterations: usize = args.get(1).map(|s| s.parse().unwrap()).unwrap_or(100);
    let input = json!({
        "schema_version":"ATIF-v1.8", "trajectory_id":"bench",
        "agent":{"name":"bench","version":"1"},
        "steps":[{"step_id":1,"source":"agent","message":"tools",
            "tool_calls":(0..calls).map(|i| json!({"tool_call_id":format!("call-{i}"),"function_name":"echo","arguments":{}})).collect::<Vec<_>>(),
            "observation":{"results":(0..calls).rev().map(|i| json!({"source_call_id":format!("call-{i}"),"content":"ok"})).collect::<Vec<_>>()}}]
    }).to_string();
    let index = trajectory_schema().index_of("trajectory_id").unwrap();
    let mut builder = TrajectoryBatchBuilder::new(1, Some(&[index])).unwrap();
    builder.append_json(&input, None, 1).unwrap();
    black_box(builder.flush().unwrap());
    let start = Instant::now();
    for _ in 0..iterations {
        builder.append_json(black_box(&input), None, 1).unwrap();
        black_box(builder.flush().unwrap());
    }
    println!(
        "calls={calls} iterations={iterations} seconds={:.6}",
        start.elapsed().as_secs_f64()
    );
}
