# ATIF Arrow

Convert ATIF v1.0–v1.8 trajectory JSON into Arrow record batches. Each trajectory
becomes one row with a fixed schema; the original JSON is retained in `raw_json`.

| Crate | Responsibility |
| --- | --- |
| `atif-arrow` | Schema, parsing, validation, and conversion. No storage I/O. |
| `atif-io` | Async JSON/JSONL readers, provenance, and OpenDAL storage access. |
| `opendal-service-harborhub` | Read-only OpenDAL backend for Harbor Hub result objects. |
| `atif-arrow-cli` | Placeholder; CLI commands are not implemented yet. |

## Convert a trajectory

```rust
use atif_arrow::{parse_trajectory, to_record_batch, trajectory_schema};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let document = r#"{
      "schema_version": "ATIF-v1.8",
      "agent": {"name": "example", "version": "1"},
      "steps": [{"step_id": 1, "source": "user", "message": "hello"}]
    }"#;
    let parsed = parse_trajectory(document)?;
    let batch = to_record_batch(&parsed, Some("trajectory.json"), 1)?;
    assert_eq!(batch.schema(), trajectory_schema());
    assert_eq!(batch.num_rows(), 1);
    Ok(())
}
```

`parse_trajectory` accepts exactly one JSON document and preserves its text through
`raw_json()`. Unmodeled fields remain in `additional_fields` and `raw_json`.
`to_record_batch` uses a caller-supplied source URI (or `None`) and one-based record
index. See [the schema contract](docs/schema.md) for the complete field mapping.

For documents already supplied by the caller, `TrajectoryBatchBuilder` provides
`append_json` and `flush` without storage I/O. Flush a full batch before appending
another document.

## Read JSON and JSONL

```rust
use atif_io::{InputFormat, ReadError, TrajectoryReader};
use futures::io::AsyncBufRead;

async fn read_jsonl(input: impl AsyncBufRead + Unpin) -> Result<(), ReadError> {
    let mut reader = TrajectoryReader::new(input, InputFormat::JsonLines, 256, None)?;
    while let Some(batch) = reader.next_batch().await? {
        println!("{} trajectories", batch.num_rows());
    }
    Ok(())
}
```

- `InputFormat::Json` reads one complete document, including pretty-printed JSON.
- `InputFormat::JsonLines` reads one document per nonblank line. Empty input yields
  no batches; empty `Json` input is an error.
- Batch size must be positive and limits rows, not bytes. Each trajectory is read
  in full; JSONL records are read incrementally.
- `into_stream()` exposes batches as a `Stream<Item = Result<RecordBatch, ReadError>>`.
  Pending reads can be cancelled and resumed without losing partial input or rows.

Record indices start at 1 and exclude blank lines. `raw_json` preserves whitespace,
including JSONL line endings. Errors include the source URI and record index,
stop the reader, and discard the current batch; previously yielded batches remain
available. Reading is async; parsing and conversion run synchronously.

### OpenDAL

`TrajectoryReader::open(&operator, path, format, batch_size, source_uri).await`
reads an object through a caller-configured `opendal::Operator`. Paths are relative
to the operator's root; `None` uses the path as provenance. Supply a full source
URI to distinguish storage roots or buckets.

`atif-io` enables OpenDAL's Tokio executor and reqwest HTTP transport with default
features disabled. Memory is built in. For local files, enable
`opendal/services-fs` and configure `services::Fs::default().root("/data")`.
Filesystem support is enabled for tests; S3 and GCS are not enabled.

The reader streams object bytes directly into conversion without an intermediate
download or per-row batch concatenation. It reuses its input buffer, but parsed
JSON, normalized JSON, and Arrow buffers still allocate; conversion is not zero-copy.
OpenDAL access may be deferred until `next_batch`, and errors preserve its cause.

### Harbor Hub

```rust
use atif_io::{InputFormat, TrajectoryReader};
use opendal::Operator;
use opendal_service_harborhub::HarborHub;

async fn read_trial(trial_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    opendal::install_default();
    let operator = Operator::new(HarborHub::default())?;
    let path = format!("trials/{trial_id}/trajectory.json");
    let mut reader = TrajectoryReader::open(
        &operator, &path, InputFormat::Json, 256,
        Some(format!("harborhub://{path}")),
    ).await?;
    while let Some(batch) = reader.next_batch().await? {
        println!("{} trajectories", batch.num_rows());
    }
    Ok(())
}
```

The backend supports anonymous `read`, byte ranges, and `stat` in Harbor's
`results` bucket. `root("trials/<trial-id>")` makes object paths relative to that
trial. `endpoint` and `publishable_key` override the Supabase project URL and public
gateway key; defaults follow [Harbor's public configuration](https://github.com/harbor-framework/harbor/blob/main/src/harbor/auth/constants.py).
No user bearer token is sent; private results remain subject to server permissions.

Input currently requires a known object path. Job/repo URL discovery, listing,
authenticated access, writes, and URI registration are not implemented. Direct
trajectory uploads are optional: `trajectory.json` may only exist inside
`trial.tar.gz`. Archive bytes can be read, but archive extraction is not implemented.
See [Harbor's uploader](https://github.com/harbor-framework/harbor/blob/main/src/harbor/upload/uploader.py).

For a runnable example using a verified public Terminal-Bench trajectory, see
[examples/README.md](examples/README.md).

## Validation

Parsing checks core fields and step rules. Conversion checks mapped field types,
content, and references, and revalidates step rules for modified inputs and embedded
subagents. Errors report field paths such as `steps[0].timestamp`.

| Area | Rules |
| --- | --- |
| Core fields | ATIF v1.0–v1.8; valid step sources; nonempty steps numbered sequentially from 1; `session_id` required through v1.6. |
| Timestamps | Valid ISO 8601 values; original precision and timezone representation are preserved. |
| Agent-only fields | `model_name`, `reasoning_effort`, `reasoning_content`, `tool_calls`, and `metrics` require an agent step. Missing and null fields are unset. |
| Deterministic agent steps | `llm_call_count = 0` forbids metrics and reasoning content. |
| Content | Ordered text/image/audio parts; compatible content and media sources, supported MIME types, required paths, and finite nonnegative audio duration. Content arrays require v1.6+; audio requires v1.8+. Harbor audio MIME aliases are accepted. |
| Embedded subagents | Require v1.7+ and a unique `trajectory_id` within each parent's array. Sibling session IDs may repeat or be omitted. Children are validated recursively. |
| References | Observation `source_call_id` must match a tool call in the same step. Before v1.7, subagent references require `session_id`; v1.7+ requires `trajectory_id` or `trajectory_path`. ID-only references must match an embedded child. |
| Mapped types | Reject incompatible types, integer overflow, and non-finite numeric columns. |

## Arrow types and limits

The schema is fixed; arbitrary JSON schemas are not inferred. This workspace
produces Arrow batches; Lance conversion and IPC export are not implemented.

| Input | Arrow representation |
| --- | --- |
| Text | `Utf8` |
| Boolean | `Boolean` |
| Numeric columns | `Int64`, `UInt64`, `Float64` |
| Nested objects | `Struct` |
| Arrays | `List`, including `List<Struct>` |
| Timestamps | Validated original text in `Utf8` |
| Dynamic JSON and embedded subagents | JSON text in `Utf8`, preserving arbitrary-precision numbers |

String messages and observations become text content-part lists. Optional nulls
and empty collections remain distinct; list elements are non-null. Media paths,
MIME spelling, and subagent/continuation references are preserved. Referenced files
are not fetched, extracted, or verified. Metrics and identifiers are not synthesized.

The converter does not produce native Arrow temporal types (`Timestamp`, dates,
times, `Duration`, `Interval`), `Decimal`, `Binary`, `Map`, `Union`, `LargeList`,
`FixedSizeList`, smaller integer types, or `Float16`/`Float32`. Supporting these
requires changes to the schema and mapping.
