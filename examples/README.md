# Examples

## Convert a local JSON file

```sh
cargo new atif-demo --edition 2021
cd atif-demo
cargo add atif-arrow@0.1.0 arrow-array@58
```

Copy [convert.rs](convert.rs) into `src/main.rs`, then run:

```sh
cargo run -- /path/to/trajectory.json
```

Use [trajectory.json](trajectory.json) for a first run. The example prints the
agent, step count, available token/cost totals, and verifies raw JSON preservation.
Cargo names use hyphens; imports use `atif_arrow` and `arrow_array`.

## JSON/JSONL on OpenDAL

The [usage guide](../docs/usage.md) shows one reader with local filesystem, S3,
and GCS operators. Select a backend and pass an object path; conversion stays the same.

## Harbor Hub

With a sibling repository checkout, add these dependencies:

```toml
[dependencies]
atif-arrow = "0.1.0"
atif-io = { path = "../atif-arrow/crates/atif-io" }
opendal-service-harborhub = { path = "../atif-arrow/crates/opendal-service-harborhub" }
arrow-json = "58"
opendal = { version = "0.59.3", default-features = false, features = ["executors-tokio", "http-transport-reqwest"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt"] }
```

Copy [harborhub.rs](harborhub.rs) into `src/main.rs` and supply a public object path:

```sh
cargo run -- trials/d4f5439d-8367-467f-938a-005585690681/trajectory.json
```

Imports use `atif_io` and `opendal_service_harborhub`. This sample needs network
access. See [sample provenance and access limits](../docs/reference.md#public-harbor-hub-sample).

## Run from a repository checkout

```sh
cargo run --locked -p atif-arrow --example convert -- examples/trajectory.json
cargo run --locked -p opendal-service-harborhub --example harborhub -- trials/d4f5439d-8367-467f-938a-005585690681/trajectory.json
```
