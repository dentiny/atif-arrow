# Read trajectories from storage

Pass an OpenDAL `Operator` and an object path. The same reader works with any
OpenDAL backend that supports reading, including local files, S3, GCS, and Harbor Hub.

## Dependencies

`atif-io` currently requires a local checkout. The path below assumes the checkout
and your application are sibling directories. Enable only the backends you use:

```toml
[dependencies]
atif-io = { path = "../atif-arrow/crates/atif-io" }
opendal = { version = "0.59.3", default-features = false, features = [
    "services-fs", "services-s3", "services-gcs",
    "executors-tokio", "http-transport-reqwest",
] }
tokio = { version = "1", features = ["macros", "rt"] }
```

## One reader for every backend

```rust
use atif_io::{InputFormat, ReadError, TrajectoryReader};
use opendal::{services, Operator};

async fn read_jsonl(operator: &Operator, path: &str) -> Result<(), ReadError> {
    let mut reader = TrajectoryReader::open(
        operator, path, InputFormat::JsonLines, 256, None,
    ).await?;
    while let Some(batch) = reader.next_batch().await? {
        println!("{} trajectories", batch.num_rows());
    }
    Ok(())
}
```

Use `InputFormat::Json` for a single JSON document. Run the calls below inside a
Tokio async runtime, after calling `opendal::install_default()` once.

### Local filesystem

```rust
let operator = Operator::new(services::Fs::default().root("/data"))?;
read_jsonl(&operator, "runs/trajectories.jsonl").await?;
```

Reads `/data/runs/trajectories.jsonl`.

### S3

```rust
let operator = Operator::new(
    services::S3::default().bucket("agent-runs").region("us-east-1"),
)?;
read_jsonl(&operator, "runs/trajectories.jsonl").await?;
```

Reads `s3://agent-runs/runs/trajectories.jsonl`; configure AWS credentials through
OpenDAL's credential providers. For S3-compatible storage, also set `endpoint`.

### GCS

```rust
let operator = Operator::new(services::Gcs::default().bucket("agent-runs"))?;
read_jsonl(&operator, "runs/trajectories.jsonl").await?;
```

Reads `gs://agent-runs/runs/trajectories.jsonl`; configure Google credentials through
OpenDAL's credential providers.

Object paths are relative to the operator's root, rather than full storage URIs.
For full source identity, pass `Some("s3://bucket/path".into())` as the final
argument to `TrajectoryReader::open`. Batch size limits rows, not bytes.

See the [Harbor Hub example](../examples/README.md#harbor-hub),
[reader and validation reference](reference.md), and [schema contract](schema.md).
