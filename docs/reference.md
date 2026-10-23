# Conversion and reader reference

For short examples, see the [usage guide](usage.md). For field mappings, see the
[schema contract](schema.md).

## In-memory conversion

`parse_trajectory` accepts one JSON document and retains its text in `raw_json()`.
Unmodeled fields remain in `additional_fields` and `raw_json`.
`to_record_batch` accepts a source URI (or `None`) and a one-based record index.
`TrajectoryBatchBuilder` provides `append_json` and `flush` for caller-supplied
documents; flush a full batch before appending another document.

## Reader behavior

- `InputFormat::Json` reads one complete document, including pretty-printed JSON.
- `InputFormat::JsonLines` reads one document per nonblank line. Empty input yields
  no batches; empty `Json` input is an error.
- Batch size must be positive and limits rows, not bytes.
- `into_stream()` exposes batches as a `Stream<Item = Result<RecordBatch, ReadError>>`.
  Pending reads can be cancelled and resumed without losing partial input or rows.
- `into_parallel_stream(n)` yields the same batches and first error, but converts up to
  `n` batches at once on Tokio's blocking pool. It must be polled in a Tokio runtime.

Record indices start at 1 and exclude blank lines. `raw_json` preserves whitespace,
including JSONL line endings. Errors include the source URI and record index,
stop the reader, and discard the current batch; previously yielded batches remain
available. Reading is async; parsing and conversion run synchronously.

**Memory use is not bounded.** `Json` input is read into memory in full before
parsing. `JsonLines` reads one record at a time, but there is no line-length limit,
so a single long line is also buffered in full. A batch holds the text of up to
`batch_size` documents until conversion, then the converted rows, each including its
`raw_json`. Peak memory grows with record size, `batch_size`, and, for
`into_parallel_stream(n)`, the `n` batches in flight. For large input, check object
sizes first (for example with OpenDAL `stat`) or lower `batch_size`.

## OpenDAL access

`TrajectoryReader::open` reads through a caller-configured OpenDAL operator.
Object paths are relative to its root. `None` uses the path as provenance; supply
a full URI to distinguish storage roots or buckets.

`atif-io` enables OpenDAL's Tokio executor and reqwest HTTP transport with default
features disabled. Enable each storage backend's `services-*` feature in your
application, such as `services-fs`, `services-s3`, or `services-gcs`.

The reader streams object bytes directly into conversion without an intermediate
download or per-row batch concatenation. Document text, parsed JSON, normalized JSON,
and Arrow buffers allocate; conversion is not zero-copy.
OpenDAL access may be deferred until `next_batch`, and errors preserve its cause.

## Harbor Hub behavior

The backend supports anonymous `read`, byte ranges, and `stat` in Harbor's
`results` bucket. `root("trials/<trial-id>")` makes object paths relative to that
trial. `endpoint` and `publishable_key` override the Supabase project URL and public
gateway key; defaults follow [Harbor's public configuration](https://github.com/harbor-framework/harbor/blob/main/src/harbor/auth/constants.py).
No user bearer token is sent; private results remain subject to server permissions.

Input currently requires a known object path. Job/repo URL discovery, listing,
authenticated access, writes, and URI registration are not implemented. Direct
trajectory uploads are optional: `trajectory.json` may only exist inside
`trial.tar.gz`. Archive bytes can be read, but archive extraction is not implemented.
See [Harbor's uploader](https://github.com/harbor-framework/harbor/blob/main/src/harbor/upload/uploader.py).

For a runnable example using a verified public Terminal-Bench trajectory, see
[examples/README.md](../examples/README.md).

## Validation

Parsing checks core fields and step rules. Conversion checks mapped field types,
content, and references, and revalidates step rules for modified inputs and embedded
subagents. Errors report field paths such as `steps[0].timestamp`.

| Area | Rules |
| --- | --- |
| Core fields | ATIF v1.0–v1.8; valid step sources; nonempty steps numbered sequentially from 1; `session_id` required through v1.6. |
| Timestamps | Valid ISO 8601 values; original precision and timezone representation are preserved. |
| Agent-only fields | `model_name`, `reasoning_effort`, `reasoning_content`, `tool_calls`, and `metrics` require an agent step. Missing and null fields are unset. |
| Deterministic agent steps | `llm_call_count = 0` forbids metrics and reasoning content. |
| Content | Ordered text/image/audio parts; compatible content and media sources, supported MIME types, required paths, and finite nonnegative audio duration. Content arrays require v1.6+; audio requires v1.8+. Harbor audio MIME aliases are accepted. |
| Embedded subagents | Require v1.7+ and a unique `trajectory_id` within each parent's array. Sibling session IDs may repeat or be omitted. Children are validated recursively. |
| References | Observation `source_call_id` must match a tool call in the same step. Before v1.7, subagent references require `session_id`; v1.7+ requires `trajectory_id` or `trajectory_path`. ID-only references must match an embedded child. |
| Mapped types | Reject incompatible types, integer overflow, and non-finite numeric columns. |

## Arrow types and limits

The schema is fixed; arbitrary JSON schemas are not inferred. This workspace
produces Arrow batches; Lance conversion and IPC export are not implemented.

| Input | Arrow representation |
| --- | --- |
| Message/observation content text | UTF-8 bytes in `LargeBinary` |
| Other string fields | `Utf8` |
| Boolean | `Boolean` |
| Numeric columns | `Int64`, `UInt64`, `Float64` |
| Nested objects | `Struct` |
| Arrays | `List`, including `List<Struct>` |
| Timestamps | Validated original text in `Utf8` |
| Dynamic JSON and embedded subagents | JSON text in `Utf8`, preserving arbitrary-precision numbers |

String messages and observations become text content-part lists. Optional nulls
and empty collections remain distinct; list elements are non-null. Media paths,
MIME spelling, and subagent/continuation references are preserved. Referenced files
are not fetched, extracted, or verified. Metrics and identifiers are not synthesized.

`ContentPart.text` is a nullable `LargeBinary` array. Decode its bytes as UTF-8
when displaying text. Empty strings produce valid zero-length binary values;
media parts keep null text. The single-document converter, batch builder, and
serial/parallel readers use the same output schema. Arrow's standard JSON writer
represents binary values as hex strings, rather than the original readable text.

The converter does not produce native Arrow temporal types (`Timestamp`, dates,
times, `Duration`, `Interval`), `Decimal`, `Binary`, `Map`, `Union`, `LargeList`,
`FixedSizeList`, smaller integer types, or `Float16`/`Float32`. Supporting these
requires changes to the schema and mapping.

## Public Harbor Hub sample

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
