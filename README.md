# ATIF Arrow

**Turn agent trajectory JSON into structured Arrow data for analysis and training pipelines.**

Already collecting agent runs with Harbor or another ATIF producer? ATIF Arrow
converts those logs into Rust `RecordBatch` values with a consistent schema for
messages, tool calls, observations, and token/cost metrics. You can work with the
same columns across runs while keeping the original JSON for inspection.

[ATIF](https://github.com/harbor-framework/harbor/blob/f9f974aee0d3e52670427bfd298caecb64fdde3a/rfcs/0001-trajectory-format.md)
(Agent Trajectory Interchange Format) records an agent's interaction history as
JSON. [Apache Arrow](https://arrow.apache.org/) represents that data as typed,
columnar batches that downstream data tools can consume.

```text
ATIF JSON / JSONL  →  validate and convert  →  Arrow RecordBatch
local input or Harbor Hub                   one row per trajectory
```

## Why use it?

- **Analyze agent runs with a consistent schema.** Agent identity, ordered steps,
  tool calls, and supplied token/cost metrics have defined Arrow fields, so your
  pipeline does not need to infer a new schema for each input.
- **Keep the context behind the numbers.** Each row retains the trajectory's
  nested steps, source location, record index, and verbatim `raw_json`, including
  unknown fields. Text, image, and audio content share a content-part structure.
- **Catch malformed data during ingestion.** Version-aware checks cover core
  fields, mapped types, content, and references, with errors that identify field
  paths such as `steps[0].timestamp`.
- **Start with the data you already have.** Convert an in-memory document, read
  JSON/JSONL in batches, or read known public Harbor Hub trajectory objects
  through OpenDAL.

Use it when you are building a **Rust ingestion pipeline** to compare agent runs,
inspect tool usage, or prepare trajectories for downstream analysis or training.
The library supplies structured data; your pipeline chooses queries, training
samples, and export formats.

## Quick start from crates.io

You need a Rust toolchain with Cargo. Create a project and add the published crate:

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

Pass the path to an ATIF JSON file:

```sh
cargo run -- /path/to/trajectory.json
```

For a first run, save [this sample trajectory](examples/trajectory.json) as
`trajectory.json` and run `cargo run -- trajectory.json`. Expected output:

```text
1 row, 13 columns
```

The file path is recorded in the row's `source_uri`. Cargo may download
dependencies on the first build.

For an existing project, `cargo add atif-arrow@0.1.0` adds the equivalent
`Cargo.toml` dependency:

```toml
[dependencies]
atif-arrow = "0.1.0"
```

The package name is `atif-arrow`; Rust imports use `atif_arrow`.
See [crates.io](https://crates.io/crates/atif-arrow) for the published package and
[docs.rs](https://docs.rs/atif-arrow/0.1.0/atif_arrow/) for its API documentation.

This produces one row with 13 top-level columns. `agent` is a struct, `steps` is
an ordered list of structs, and `final_metrics` holds totals supplied by the
producer. Missing optional values become null; metrics are not recomputed.
The full mapping is in the [schema contract](docs/schema.md).

Choose the entry point that matches your input:

| Your input | Entry point |
| --- | --- |
| One JSON document already in memory | `atif-arrow`: `parse_trajectory` + `to_record_batch` |
| Multiple documents already in memory | `atif-arrow`: `TrajectoryBatchBuilder` |
| An async JSON/JSONL source or OpenDAL object | `atif-io`: `TrajectoryReader` |
| A known public Harbor Hub object path | `opendal-service-harborhub` + `atif-io` |

`atif-arrow` is available on crates.io. The `atif-io` and
`opendal-service-harborhub` crates currently require the repository source; the
[usage guide](docs/usage.md) gives their local path dependencies alongside each
example. It also covers batching, JSONL, storage configuration, provenance, and
error handling.

For a file-based example that reads token/cost metrics from Arrow columns, or a
real public Terminal-Bench trajectory, see [the examples guide](examples/README.md).

## Current scope

- Supports **ATIF v1.0–v1.8** with a fixed trajectory schema. Each input document
  becomes one row; embedded subagents are validated and retained as JSON.
- Produces in-memory Arrow batches. CLI commands, Python bindings, and built-in
  Parquet/IPC/Lance export are not implemented.
- Reads JSONL incrementally, but batch size limits **rows, not bytes**. Full JSON
  documents and individual JSONL records are buffered, and each row retains its
  raw JSON. Size inputs and batches for your available memory.
- Harbor Hub access currently requires a known, publicly readable object path.
  Discovery, listing, authenticated access, and archive extraction are not
  implemented. Media and external trajectory references are retained without
  fetching their targets.

## Learn more

- [Usage guide](docs/usage.md): conversion APIs, readers, storage, and validation rules.
- [Schema contract](docs/schema.md): columns, nested types, and preservation behavior.
- [Harbor Hub example](examples/README.md): a real trajectory and its provenance.

For development from a repository checkout, run `cargo test --locked --workspace`.
Licensed under [MIT](LICENSE).
