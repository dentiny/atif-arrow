//! Run cargo bench -p atif-arrow --bench append_sparse -- [rows] [rounds].
use atif_arrow::TrajectoryBatchBuilder;
use std::{hint::black_box, time::Instant};
fn main() {
    let args: Vec<_> = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--bench")
        .collect();
    let number =
        |i: usize, default: usize| args.get(i).map(|x| x.parse().unwrap()).unwrap_or(default);
    let rows = number(0, 20_000);
    let rounds = number(1, 15);
    assert!(rows > 0 && rounds > 0);
    let text = "payload 你好 ".repeat(8);
    let input = serde_json::json!({"schema_version":"ATIF-v1.8","trajectory_id":"bench","agent":{"name":"bench","version":"1"},"steps":[{"step_id":1,"source":"user","message":text}]}).to_string();
    for iteration in 0..=rounds {
        let start = Instant::now();
        let mut builder = TrajectoryBatchBuilder::new(1024, None).unwrap();
        let mut count = 0;
        for row in 0..rows {
            builder.append_json(&input, None, row as u64 + 1).unwrap();
            if (row + 1) % 1024 == 0 {
                let batch = builder.flush().unwrap().unwrap();
                count += batch.num_rows();
                black_box(batch);
            }
        }
        if let Some(batch) = builder.flush().unwrap() {
            count += batch.num_rows();
            black_box(batch);
        }
        assert_eq!(count, rows);
        if iteration > 0 {
            println!(
                "iteration={iteration},rows={rows},seconds={:.9}",
                start.elapsed().as_secs_f64()
            );
        }
    }
}
