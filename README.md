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

## Try it locally

You need a Rust toolchain with Cargo. From a fresh checkout:

```sh
git clone https://github.com/dentiny/atif-arrow.git
cd atif-arrow
cargo run --locked -p atif-arrow --example convert
```

Already cloned the repository? Run the last command from its root. The example
uses an embedded two-step trajectory with token and cost metrics; it needs no
external data or credentials. Cargo may download dependencies on the first build.

Expected output:

```text
ATIF-v1.8 -> Arrow: 1 row, 13 columns
Agent: example
Steps: 2
Total prompt tokens: 12
Total cost (USD): 0.001
Original JSON preserved: true
```

See [the runnable example](examples/convert.rs) to inspect how those values are
read from Arrow columns. To try a real public Terminal-Bench trajectory, follow
[the Harbor Hub example](examples/README.md).

## Use it in your Rust project

Add the conversion library to your `Cargo.toml`:

```toml
[dependencies]
atif-arrow = { git = "https://github.com/dentiny/atif-arrow.git" }
```

Convert one JSON document:

```rust
use atif_arrow::{parse_trajectory, to_record_batch};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let document = r#"{
      "schema_version": "ATIF-v1.8",
      "agent": {"name": "example", "version": "1"},
      "steps": [{"step_id": 1, "source": "user", "message": "hello"}]
    }"#;
    let parsed = parse_trajectory(document)?;
    let batch = to_record_batch(&parsed, Some("trajectory.json"), 1)?;
    println!("{} row, {} columns", batch.num_rows(), batch.num_columns());
    Ok(())
}
```

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

The I/O crates can also be added as Git dependencies from this repository.
See the [usage guide](docs/usage.md) for batching, JSONL, storage configuration,
provenance, and error handling.

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

To check the workspace locally, run `cargo test --locked --workspace`.
Licensed under [MIT](LICENSE).
