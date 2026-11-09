use std::{hint::black_box, time::Instant};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let iterations: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(10);
    let n = 32768;
    let input = r#"{"schema_version":"ATIF-v1.8","trajectory_id":"bench","agent":{"name":"bench","version":"1"},"steps":[{"step_id":1,"source":"user","message":"benchmark payload"}]}"#;
    let start = Instant::now();
    for _ in 0..iterations {
        let mut builder = atif_arrow::TrajectoryBatchBuilder::new(1024, None).unwrap();
        for i in 0..n {
            builder
                .append_json(input, Some("bench"), i as u64 + 1)
                .unwrap();
            if (i + 1) % 1024 == 0 {
                black_box(builder.flush().unwrap());
            }
        }
    }
    println!("{{\"case\":\"atif-native\",\"iterations\":{},\"input_bytes\":{},\"units\":{},\"seconds\":{}}}",iterations,input.len()*n,n*iterations,start.elapsed().as_secs_f64());
}
