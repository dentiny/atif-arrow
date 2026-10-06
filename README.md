# ATIF Arrow

A Rust workspace for converting ATIF agent trajectory documents into Arrow.
The library provides an Arrow schema and core ATIF parsing; Arrow conversion is not implemented yet.

| Crate | Responsibility |
| --- | --- |
| `atif-arrow` | Arrow schema and core parsing; normalization and batch reading follow. |
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

## Development

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The CLI is a scaffold and currently exits with a not-implemented message.
Subsequent changes add text conversion, detailed multimodal and subagent
validation, batch reading, then CLI commands and Arrow IPC export.
