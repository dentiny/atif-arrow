# ATIF Arrow

A Rust workspace for converting ATIF agent trajectory documents into Arrow.
The workspace provides a fixed Arrow schema, core ATIF parsing, conversion, and batch reading.

| Crate | Responsibility |
| --- | --- |
| `atif-arrow` | Arrow schema, parsing, validation, and conversion, including batch construction. |
| `atif-io` | JSON/JSONL reading and source provenance; future storage backend adapters. |
| `atif-arrow-cli` | Separate executable for future schema inspection and IPC export. |

The library uses `arrow-schema` and Serde for JSON parsing. CLI argument parsing
and IPC dependencies stay in the CLI crate.
Arrow 58 matches the major version used by `duckdb_lance_conversion`.

## Schema API

```rust
use atif_arrow::trajectory_schema;

let schema = trajectory_schema();
assert_eq!(schema.field_with_name("atif_schema_version")?.data_type(),
           &arrow_schema::DataType::Utf8);
# Ok::<(), arrow_schema::ArrowError>(())
```

See [the schema contract](docs/schema.md) for field mappings and null semantics.

## Parsing API

```rust
use atif_arrow::parse_trajectory;

let document = r#"{
  "schema_version": "ATIF-v1.8",
  "agent": {"name": "example", "version": "1"},
  "steps": [{"step_id": 1, "source": "user", "message": "hello"}]
}"#;
let parsed = parse_trajectory(document)?;
println!("{}", parsed.trajectory.agent.name);
```

`parse_trajectory` accepts exactly one JSON document. It checks required core
fields and types, ATIF v1.0–v1.8, step sources, nonempty sequential steps starting
at 1, and the session ID required through v1.6. Errors include field paths such
as `steps[0].step_id`.

The result preserves the original input through `raw_json()` and unmodeled fields
through `additional_fields`. Multimodal parts remain JSON values at parsing time;
conversion validates their content and embedded subagent documents.

## Conversion API

```rust
use atif_arrow::{parse_trajectory, to_record_batch};

let parsed = parse_trajectory(document)?;
let batch = to_record_batch(&parsed, Some("trajectory.json"), 1)?;
assert_eq!(batch.num_rows(), 1);
```

`to_record_batch` converts one parsed document into one row with the fixed schema.
The caller supplies the source URI (or `None`) and a one-based record index.
Text messages and observations become content-part lists; tool arguments and
other JSON payloads stay serialized JSON. Optional nulls and empty collections
remain distinct, and `raw_json` preserves the original document.

Conversion supports ordered text/image/audio parts in messages and observations.
It checks content/source compatibility, supported MIME types, required media paths,
and finite nonnegative audio duration. Harbor's common audio MIME aliases are
accepted; MIME spelling and paths remain unchanged. No media files are fetched.
Content arrays require ATIF v1.6+, and audio requires v1.8+.

Embedded subagents remain complete JSON documents in one column. Conversion
recursively checks their core fields, mapped field types, content, and references.
Embedding requires ATIF v1.7+: each child needs a unique `trajectory_id` within
its parent's array; sibling `session_id`s may repeat or be omitted.

In v1.7+, a subagent reference needs `trajectory_id` or `trajectory_path`. An
ID-only reference must match an embedded child in the same document. External
paths are retained without loading files. Pre-v1.7 references require `session_id`
and may omit `trajectory_path`. Observation `source_call_id`s must match a tool
call in the same step.

Conversion also rejects incompatible field types, integer overflow, and non-finite
numeric columns. JSON normalization uses the schema, and Arrow constructs the
nested arrays.

`TrajectoryBatchBuilder` appends JSON documents supplied by the caller and flushes
them into Arrow batches. It shares the single-document conversion rules without
reading external data. A full batch must be flushed before appending another row.

## Batch reading API (`atif-io`)

```rust
use atif_io::{InputFormat, TrajectoryReader};
use std::{fs::File, io::BufReader};

let input = BufReader::new(File::open("trajectories.jsonl")?);
let reader = TrajectoryReader::new(
    input, InputFormat::JsonLines, 256, Some("trajectories.jsonl".into()),
)?;
for batch in reader {
    let batch = batch?;
    println!("{} trajectories", batch.num_rows());
}
```

`InputFormat::Json` reads one complete document, including pretty-printed JSON.
`JsonLines` reads one document per nonblank line and yields batches with at most
`batch_size` rows. Empty JSONL input yields no batches; empty JSON input is an error.
Batch size must be positive. Input streams and source URIs are supplied by the caller.
`atif-io` owns the reader and its errors; `atif-arrow` owns conversion. Future
OpenDAL adapters will live in `atif-io`.

Record indices start at 1 and count documents, excluding blank lines. `raw_json`
retains the record's original whitespace, including JSONL line endings. I/O, parsing,
and conversion errors include the source URI and record index and stop the reader.
A failed batch is discarded; previously yielded batches remain available.
Batch size limits rows rather than bytes; each trajectory is read in full.
Rows feed one `TrajectoryBatchBuilder` per reader, avoiding intermediate single-row batches
and concatenation of their column buffers. The reader reuses its input buffer and
does not retain a separate raw document copy. Conversion still allocates parsed
JSON, normalized JSON, and Arrow buffers; it is not zero-copy.

## Supported types and limitations

Conversion follows the fixed ATIF schema; it does not infer arbitrary JSON schemas.

| Input / field | Arrow representation |
| --- | --- |
| Text | `Utf8` |
| Boolean | `Boolean` |
| Numeric columns | `Int64`, `UInt64`, `Float64` |
| Nested objects | `Struct` |
| Arrays, including object arrays | `List`, including `List<Struct>` |
| Timestamps | `Utf8`, preserving the original text without parsing or timezone conversion |
| Dynamic JSON payloads and embedded subagents | JSON text in `Utf8` columns |

Optional fields support null values; empty collections remain distinct from null.
List elements are non-null. JSON payload columns preserve arbitrary-precision
numbers as JSON text rather than converting them into numeric columns.

The current converter does not produce these native Arrow types:

- `Timestamp`, `Date32`/`Date64`, `Time32`/`Time64`, `Duration`, or `Interval`.
- `Decimal`, `Binary`, or `Map`.
- `LargeList`, `FixedSizeList`, or `Union`.
- `Int8`/`Int16`/`Int32`, `UInt8`/`UInt16`/`UInt32`, or `Float16`/`Float32`.

These types would require explicit additions to the schema and conversion mapping.

Timestamp syntax and agent-only / `llm_call_count` cross-field rules remain
unchecked. Parsing checks core fields; conversion applies the additional checks
described above.

## Development

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The CLI is a scaffold and currently exits with a not-implemented message.
The CLI remains deferred while the I/O crate is developed.
