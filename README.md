# ATIF Arrow

**Turn agent trajectories into Arrow data.**

A Rust library for converting
[ATIF](https://github.com/harbor-framework/harbor/blob/f9f974aee0d3e52670427bfd298caecb64fdde3a/rfcs/0001-trajectory-format.md)
(Agent Trajectory Interchange Format) logs into
[Apache Arrow](https://arrow.apache.org/) `RecordBatch` values. Each trajectory
becomes one row containing its agent, ordered steps, tool calls, and observations.

- **Compare runs:** a fixed schema for supplied token/cost metrics and tool usage.
- **Use your storage:** the same JSON/JSONL reader works with any readable OpenDAL
  backend, including local files, S3, GCS, and Harbor Hub.
- **Keep the evidence:** validate during ingestion and retain verbatim JSON,
  unknown fields, and source provenance.

## Convert your first file

With Rust and Cargo installed:

```sh
cargo new atif-demo --edition 2021
cd atif-demo
cargo add atif-arrow@0.1.0
```

Replace `src/main.rs` with:

```rust
use atif_arrow::{parse_trajectory, to_record_batch};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: atif-demo <trajectory.json>")?;
    let json = std::fs::read_to_string(&path)?;
    let parsed = parse_trajectory(&json)?;
    let batch = to_record_batch(&parsed, Some(&path), 1)?;
    println!("{} row, {} columns", batch.num_rows(), batch.num_columns());
    Ok(())
}
```

Pass an ATIF JSON filepath:

```sh
cargo run -- /path/to/trajectory.json
```

For a first run, save [the sample](examples/trajectory.json) as `trajectory.json`
and run `cargo run -- trajectory.json`. Output: `1 row, 13 columns`.
The path is retained in `source_uri`; the input text is retained in `raw_json`.

Message and observation content text is emitted as UTF-8 bytes in Arrow
`LargeBinary`. Media paths and metadata remain strings; the converter does not
read image/audio files or select a downstream Blob/compression policy. See the
[schema contract](docs/schema.md#normalization-and-preservation).

Reading JSONL or cloud objects? The [storage guide](docs/usage.md) uses one reader
with local, S3, and GCS operators. To inspect actual token/cost values, see
[the conversion example](examples/README.md#convert-a-local-json-file).

## Choose a crate

| Crate | Use it for |
| --- | --- |
| [atif-arrow](https://crates.io/crates/atif-arrow) | Convert documents already supplied by your code. |
| `atif-io` | Read JSON/JSONL as Arrow batches through OpenDAL or an async input. |
| `opendal-service-harborhub` | Access known public Harbor Hub result objects through OpenDAL. |

`atif-arrow 0.1.0` is on crates.io. The two I/O crates currently require a local
checkout; their dependencies are shown in the [storage guide](docs/usage.md) and
[Harbor example](examples/README.md#harbor-hub). Cargo names use hyphens;
Rust imports use underscores, such as `atif_arrow`.

## Scope

- **ATIF v1.0–v1.8.** Embedded subagents are validated and retained as JSON.
- **In-memory Arrow batches.** CLI commands, Python bindings, and built-in
  Parquet/IPC/Lance export are not implemented.
- **Batch size limits rows, not bytes.** Individual JSON/JSONL records are buffered
  in full, and each row includes its raw JSON.
- **Harbor Hub requires a public object path.** Discovery, listing, authenticated
  access, and archive extraction are not implemented. External references are
  retained without fetching their targets.

[API docs](https://docs.rs/atif-arrow/0.1.0/atif_arrow/) ·
[Schema](docs/schema.md) · [Reference](docs/reference.md) · [Examples](examples/README.md)

For development: `cargo test --locked --workspace`. Licensed under [MIT](LICENSE).
