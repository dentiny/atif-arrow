# Read JSON/JSONL from any OpenDAL backend

Choose an `Operator`, then pass an object path. The reading and conversion code
stays the same for local files, S3, GCS, or another readable OpenDAL backend.

## Setup

`atif-io` currently requires a local checkout alongside your application.
These dependencies enable all three examples; remove unused `services-*` features.

```toml
[dependencies]
atif-io = { path = "../atif-arrow/crates/atif-io" }
opendal = { version = "0.59.3", default-features = false, features = [
    "services-fs", "services-s3", "services-gcs",
    "executors-tokio", "http-transport-reqwest",
] }
tokio = { version = "1", features = ["macros", "rt"] }
```

## Read a JSONL object

Put this in `src/main.rs`. The filesystem operator uses your current directory as
its root; run `cargo run -- runs/trajectories.jsonl` to read that relative path.

```rust
use atif_io::{InputFormat, TrajectoryReader};
use opendal::{services, Operator};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: atif-demo <object-path>")?;
    opendal::install_default();
    let operator = Operator::new(services::Fs::default().root("."))?;
    let mut reader = TrajectoryReader::open(
        &operator, &path, InputFormat::JsonLines, 256, None,
    ).await?;
    while let Some(batch) = reader.next_batch().await? {
        println!("{} trajectories", batch.num_rows());
    }
    Ok(())
}
```

Use `InputFormat::Json` for a single JSON document.

For a GZIP object such as `runs/trajectories.jsonl.gz`, call
`TrajectoryReader::open_gzip` with the same arguments. Decompression is
incremental, and concatenated GZIP members read as one stream. It is enabled by
the default `gzip` feature; set `default-features = false` to drop it.

## Change the backend

Replace only the `let operator = ...` statement above. Keep the same command and
reader; `runs/trajectories.jsonl` now identifies an object in the chosen bucket.

**S3** — configure AWS credentials through OpenDAL's credential providers:

```rust
let operator = Operator::new(
    services::S3::default().bucket("agent-runs").region("us-east-1"),
)?;
```

For S3-compatible storage, also set `endpoint`.

**GCS** — configure Google credentials through OpenDAL's credential providers:

```rust
let operator = Operator::new(services::Gcs::default().bucket("agent-runs"))?;
```

Paths are relative to the operator's root. To retain a full source URI, replace
`None` with `Some("s3://agent-runs/runs/trajectories.jsonl".into())` for that object.
Batch size limits rows, not bytes.

## Read content text bytes

Message and observation content uses nullable `LargeBinary` for its `text`
field. With the existing `batch` from the reader:

```rust
use arrow_array::{cast::AsArray, Array};

let steps = batch["steps"].as_list::<i32>().value(0);
let first_message = steps.as_struct()["message"].as_list::<i32>().value(0);
let text = first_message.as_struct()["text"].as_binary::<i64>();
if !text.is_null(0) {
    let value = std::str::from_utf8(text.value(0))?;
    println!("{value}");
}
```

The example accesses a nonempty message. An empty message array has no first
part; a media part has null `text`. Empty text strings remain non-null empty
byte values. Metadata, media paths, reasoning content, and `raw_json` remain
`Utf8`. Media reads and Lance Blob conversion belong to the downstream consumer.

[Harbor Hub](../examples/README.md#harbor-hub) ·
[Reader behavior and limits](reference.md#reader-behavior) · [Schema](schema.md)
