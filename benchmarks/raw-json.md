# Avoid the raw JSON staging copy

`TrajectoryBatchBuilder` previously cloned the entire input into a JSON string
before copying it again into the Arrow `raw_json` column. Append the borrowed
input directly, checking all offsets before any column changes. Projection,
duplicate columns, verbatim input and failed-append atomicity remain intact.

Reproduce the focused workload:

```sh
cargo build -p atif-arrow --release --example bench_raw_json
# Save baseline and candidate executables separately, then alternate processes.
target/release/examples/bench_raw_json full
target/release/examples/bench_raw_json raw
target/release/examples/bench_raw_json session
```

Each process runs for two seconds with 1 MiB messages and 16-row batches. Five
alternating A/B pairs on macOS ARM64 gave a median paired wall-time reduction of
9.3% for full conversion (all five pairs improved), 8.6% for raw-only projection
(two pairs regressed), and -0.4% for the session-only control. Median full-conversion
process peak RSS was 73.5 → 58.5 MiB. RSS includes setup and warmup and depends on
the allocator; these are local synthetic measurements, not a general speedup.

Raw samples and the broader audit are attached to
https://github.com/dentiny/open-arrow-access/pull/228 under
`benchmarks/matrix/results/atif-upstream.jsonl`. The measured baseline is
`87345d7`; this change is based on `0a54d93`, whose intervening change only
consolidates tests. The candidate adds one borrowed-string append path; parser
validation and normalized user fields are unchanged.
