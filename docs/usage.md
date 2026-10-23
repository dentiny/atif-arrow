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

## Embed image and audio bytes

Use the same `Operator` for the trajectory and its media. Media paths are
resolved against the trajectory object's directory, not the working directory
or a caller-supplied display/provenance URI:

```rust,no_run
use atif_io::{InputFormat, MediaOptions, MediaResolver, TrajectoryReader};

# async fn read(operator: opendal::Operator) -> Result<(), Box<dyn std::error::Error>> {
let path = "runs/trial/agent/trajectory.json";
let options = MediaOptions {
    max_media_bytes: 256 * 1024 * 1024,
    max_trajectory_media_bytes: 2 * 1024 * 1024 * 1024,
    max_batch_media_bytes: 2 * 1024 * 1024 * 1024,
    ..MediaOptions::default()
};
let resolver = MediaResolver::new(&operator, path, options)?;
let mut reader = TrajectoryReader::open(
    &operator, path, InputFormat::Json, 1, None,
).await?.with_media(resolver);

while let Some(batch) = reader.next_batch().await? {
    assert_eq!(batch.schema(), atif_arrow::trajectory_schema_with_media());
    // Consume media[].payload as LargeBinary; the original ATIF fields remain.
}
# Ok(())
# }
```

For this input, `images/one.png` resolves to
`runs/trial/agent/images/one.png` under the operator's root. Use
`TrajectoryReader::open_gzip(...).await?.with_media(resolver)` for gzip;
`InputFormat::JsonLines` handles JSONL. `MediaReader::into_stream()` emits ordered
Arrow batches. Use batch size **1** for MB-scale media.

This works with the filesystem, S3/GCS, and the Harbor Hub operator. For Hub,
configure the operator root to contain both the trajectory object and its media;
archives still require a caller-provided extraction/storage layer.

Absolute object URIs require an explicit prefix binding to the already configured
operator root, for example:

```rust,no_run
# use atif_io::{MediaOptions, MediaResolver};
# fn configure(operator: opendal::Operator) -> Result<(), Box<dyn std::error::Error>> {
let resolver = MediaResolver::new(&operator, "agent/trajectory.json", MediaOptions::default())?
    .with_uri_prefix("s3://bucket/trials/abc/")?;
# Ok(())
# }
```

Here, `s3://bucket/trials/abc/agent/image.png` maps to `agent/image.png` in
that operator. It does not create an S3 client or change its credentials. Other
origins, paths outside the prefix/root, URI credentials, query strings, and
fragments are rejected. Filesystem operators also accept local absolute paths
and `file://` URIs under their canonical root, resolving symlinks before reading.
The operator/root remains the caller's trust boundary, not a filesystem sandbox.
Backend configuration, retries, timeouts, and credentials remain caller-owned.

`data:` URIs support base64 and percent encoding. They require no storage call.
The media MIME type is preserved from ATIF; bytes are not decoded as an image or
audio waveform, and file signatures are not validated against the MIME type.

Set `MediaOptions { resolve_media: false, ..Default::default() }` to get the same
14-column schema with null payload/size and `not_resolved` status, without any
media path resolution or I/O. To keep only the old 13 columns and skip media
discovery entirely, use `TrajectoryReader` without `with_media`.

For an existing unprojected Arrow stream, `MediaResolver::resolve_batch(batch)`
provides the same operation without the reader wrapper. Supply one resolver per
source object. All rows in each input batch must belong to that source object;
the resolver does not choose a storage backend from `source_uri`.

Missing/unreadable media, malformed data URIs, and exceeded byte budgets fail the
whole batch. `ReadError::Media` includes source provenance, record index, content
locator, and the underlying storage/decoding error. `MediaReader` stops after the
first error. Cancelling `next_batch()` and calling it again resumes pending input
and media work without dropping or duplicating the trajectory.

The defaults limit each payload to 256 MiB, each trajectory to 2 GiB, and each
batch to 2 GiB. Limits apply both before reading (when `stat` is available) and
during streaming. They count payload bytes, including duplicates, and do not
bound JSON text, encoded data URIs, Arrow overhead, or total process memory.
There is no deduplication or cross-batch media cache.

To inspect a local JSON trajectory with its media bytes, run the bundled example:

```sh
cargo run -p atif-io --example multimodal -- /absolute/storage/root runs/trajectory.json
```

The self-contained local integration test generates a 64×48 PNG, a 125 ms PCM
WAV, and an ATIF-v1.8 document referencing both through relative paths. It reads
the files into Arrow, checks payload bytes/MIME/size/locators, then removes the
source directory and checks that the Arrow bytes remain available:

```sh
cargo test --locked -p atif-io --test local_multimodal
```

Fixtures are created under a temporary directory and cleaned up automatically;
the test needs no network, Python, or external media tools.

[Harbor Hub](../examples/README.md#harbor-hub) ·
[Reader behavior and limits](reference.md#reader-behavior) · [Schema](schema.md)
