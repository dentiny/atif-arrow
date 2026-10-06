# ATIF Arrow

A Rust workspace for converting ATIF agent trajectory documents into Arrow.
The initial change defines the schema contract; conversion is not implemented yet.

| Crate | Responsibility |
| --- | --- |
| `atif-arrow` | Arrow schema, then ATIF parsing, normalization, and batch reading. |
| `atif-arrow-cli` | Separate executable for future schema inspection and IPC export. |

The library currently depends only on `arrow-schema`. CLI argument parsing and
IPC dependencies will stay in the CLI crate.
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

## Development

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The CLI is a scaffold and currently exits with a not-implemented message.
Subsequent changes add parsing and validation, text conversion, multimodal and
subagent support, batch reading, then CLI commands and Arrow IPC export.
