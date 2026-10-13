# Runnable examples

## Convert and inspect an ATIF JSON file

Create a standalone project:

```sh
cargo new atif-demo --edition 2021
cd atif-demo
cargo add atif-arrow@0.1.0 arrow-array@58
```

These commands add crates.io dependencies equivalent to:

```toml
[dependencies]
atif-arrow = "0.1.0"
arrow-array = "58"
```

Copy [convert.rs](convert.rs) into `src/main.rs`, then pass your file path:

```sh
cargo run -- /path/to/trajectory.json
```

For a first run, save [trajectory.json](trajectory.json) in the application's root
and run `cargo run -- trajectory.json`.
`use atif_arrow::...` imports the published `atif-arrow` package;
`use arrow_array::...` imports Arrow's typed arrays.

Expected output:

```text
Source: trajectory.json
ATIF-v1.8 -> Arrow: 1 row, 13 columns
Agent: example
Steps: 2
Total prompt tokens: 12
Total cost (USD): 0.001
Original JSON preserved: true
```

Missing optional totals are omitted from the output. The example converts one
JSON document per file; see the [JSONL file example](../docs/usage.md#read-an-atif-jsonl-file)
for batches of trajectories.

## Read a Harbor Hub object path

`atif-io` and `opendal-service-harborhub` currently require the repository source.
For a standalone application, clone the repository alongside your project and
use these dependencies in the application's `Cargo.toml`:

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

Paths are relative to the application's `Cargo.toml`. See the
[usage guide](../docs/usage.md#async-readers-and-storage) for the clone command.
Copy [harborhub.rs](harborhub.rs) into `src/main.rs`, then pass a publicly readable
object path relative to Harbor's `results` bucket:

```sh
cargo run -- trials/d4f5439d-8367-467f-938a-005585690681/trajectory.json
```

Rust imports use underscores: `atif_arrow`, `atif_io`, and
`opendal_service_harborhub`.

To run the examples while developing inside the repository instead:

```sh
cargo run --locked -p atif-arrow --example convert -- examples/trajectory.json
cargo run --locked -p opendal-service-harborhub --example harborhub -- trials/d4f5439d-8367-467f-938a-005585690681/trajectory.json
```

The example reads a public Terminal-Bench trial from the job featured by
[Harbor Hub](https://hub.harborframework.com/):

- [Job](https://hub.harborframework.com/jobs/0f01715e-2f98-40e3-836f-1ac4fdce39a4?tab=results): Terminal-Bench 4.0.0, GPT-6 Astra / Codex.
- Trial: `foodstuff-beta-activity__53bc4621` (`d4f5439d-8367-467f-938a-005585690681`).
- Object: `results/trials/d4f5439d-8367-467f-938a-005585690681/trajectory.json`.
- Agent: `codex` 0.151.0; model: `gpt-6-astra`.

Verified on 2026-10-06: **ATIF-v1.7 → 1 Arrow row, 13 columns, 17 steps,
15 tool calls**. The original JSON is 65,886 bytes; its SHA-256 is
`208c8399d78b49d9b210fde24814d72f9f85dec7ceb27aa89b204d10dba0f545`.
The object is served as `application/octet-stream`; its payload is ATIF JSON.

The example uses the production Harbor backend and `TrajectoryReader`, asserts the
fixed schema, provenance, and raw JSON byte length, then prints the actual
step/tool-call counts and schema. The counts above describe this public sample;
other object paths can contain different trajectories. Arrow's JSON writer is
used only to inspect the converted values. See [the schema contract](../docs/schema.md).

It requires network access and uses Harbor's default public gateway key, with no
personal credentials. The live source can change or disappear, so this is an
explicit example run; regular workspace tests continue to use mocked HTTP responses.
This sample validates a direct ATIF trajectory; other trial artifacts and trajectories
stored only inside archives require separate handling.
