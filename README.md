# ATIF Arrow

A Rust workspace for converting ATIF agent trajectory documents into Arrow.
The library provides a fixed Arrow schema, core ATIF parsing, and text trajectory conversion.

| Crate | Responsibility |
| --- | --- |
| `atif-arrow` | Arrow schema, parsing, and text conversion; batch reading follows. |
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
through `additional_fields`. Multimodal parts remain opaque JSON; detailed
validation of those parts, tool calls, metrics, and subagents follows separately.

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

Conversion rejects incompatible field types, integer overflow, non-finite floats,
and image/audio content. Embedded subagents are retained as opaque JSON; detailed
ATIF relationship and version-specific validation follows separately. The JSON
normalization uses the schema, and Arrow constructs the nested arrays.

## Development

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The CLI is a scaffold and currently exits with a not-implemented message.
Subsequent changes add multimodal conversion and detailed subagent validation,
batch reading, then CLI commands and Arrow IPC export.
