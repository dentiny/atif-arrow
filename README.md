# ATIF Arrow

Convert [ATIF](https://github.com/harbor-framework/harbor/blob/f9f974aee0d3e52670427bfd298caecb64fdde3a/rfcs/0001-trajectory-format.md)
agent trajectories into [Apache Arrow](https://arrow.apache.org/) batches for
analysis and dataset processing.

Each trajectory becomes one row with structured agent details, steps, tool calls,
observations, and metrics. The original JSON, unknown fields, and source identity
are preserved alongside the converted data. Supports ATIF v1.0–v1.8.

## Quick start

Add the conversion library to your Rust project:

```sh
cargo add atif-arrow@0.4.0
```

Read an ATIF JSON file and convert it to an Arrow `RecordBatch`:

```rust
use atif_arrow::{parse_trajectory, to_record_batch};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let json = std::fs::read_to_string("trajectory.json")?;
    let trajectory = parse_trajectory(&json)?;
    let batch = to_record_batch(&trajectory, Some("trajectory.json"), 1)?;
    println!("{} row, {} columns", batch.num_rows(), batch.num_columns());
    Ok(())
}
```

Use [the sample trajectory](examples/trajectory.json) to try it. The result is one
row with 13 columns, including the source path and original JSON.

## Read files and cloud storage

For JSONL, GZIP, or data in local files, S3, and GCS, use `atif-io` with an OpenDAL
operator. See the [storage guide](docs/usage.md) for setup and examples.

| Crate | Use it to |
| --- | --- |
| [atif-arrow](https://crates.io/crates/atif-arrow) | Validate and convert documents supplied by your application. |
| [atif-io](crates/atif-io) | Read JSON/JSONL as Arrow batches from storage or an async input. |
| [opendal-service-harborhub](crates/opendal-service-harborhub) | Read public Harbor Hub result objects by path through OpenDAL. |

The I/O crates are available from a source checkout; the guides show how to add them.

## Learn more

- [Examples](examples/README.md): convert files and read Harbor Hub objects.
- [Schema](docs/schema.md): Arrow fields and how ATIF content is represented.
- [Reference](docs/reference.md): conversion APIs, validation, and reader behavior.
- [API docs](https://docs.rs/atif-arrow/0.4.0/atif_arrow/).

Licensed under [MIT](LICENSE).
