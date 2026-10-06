# Arrow schema contract

One row represents one input ATIF trajectory document. `steps` retains input
order as `List<Struct>`. Embedded subagents remain in a JSON field rather than
adding rows or requiring a recursive Arrow type. `session_id` is run-scoped,
can repeat, and is not a primary key; neither identifier is synthesized.

This contract targets ATIF v1.0–v1.8. It defines the planned converter output;
the current implementation provides the schema, core ATIF parsing, and text
conversion. Image/audio conversion and detailed subagent validation follow separately.
Reference: [Harbor's ATIF RFC at f9f974a](https://github.com/harbor-framework/harbor/blob/f9f974aee0d3e52670427bfd298caecb64fdde3a/rfcs/0001-trajectory-format.md).

## Root fields

`String` means Arrow `Utf8`. All list elements are non-null.

| Output field | Type | Nullable | Source / meaning |
| --- | --- | --- | --- |
| `source_uri` | String | yes | Caller-supplied source location; file readers will use an absolute path. |
| `source_record_index` | UInt64 | no | One-based document index within a source; not a trajectory identifier. |
| `atif_schema_version` | String | no | Original `schema_version`, unchanged. |
| `session_id` | String | yes | Original run identity; required in ATIF ≤1.6. |
| `trajectory_id` | String | yes | Original document identity, when supplied. |
| `agent` | Struct | no | Agent configuration. |
| `steps` | List<Struct> | no | Ordered interactions. |
| `notes` | String | yes | Original notes. |
| `final_metrics` | Struct | yes | Original aggregate metrics; not recomputed. |
| `continued_trajectory_ref` | String | yes | Original continuation reference; not followed. |
| `extra_json` | String / JSON | yes | Root `extra` object. |
| `subagent_trajectories_json` | String / JSON | yes | Complete original embedded trajectories, including their unknown fields. |
| `raw_json` | String / JSON | no | Verbatim JSON text of the input document, preserving unknown fields and original representation. |

## Nested fields

Fields below are nullable unless marked **required**. A nullable struct can be
absent even when its child fields are required.

| Struct | Fields |
| --- | --- |
| `agent` | **name: String**, **version: String**, model_name: String, tool_definitions_json: String/JSON, extra_json: String/JSON |
| `steps[]` | **step_id: UInt64**, timestamp: String, **source: String**, model_name: String, reasoning_effort_json: String/JSON, **message: List<ContentPart>**, reasoning_content: String, tool_calls: List<ToolCall>, observation: Observation, metrics: Metrics, is_copied_context: Boolean, llm_call_count: UInt64, extra_json: String/JSON |
| `ContentPart` | **type: String**, text: String, source: MediaSource |
| `MediaSource` | **media_type: String**, **path: String**, duration_sec: Float64 |
| `ToolCall` | **tool_call_id: String**, **function_name: String**, **arguments_json: String/JSON**, extra_json: String/JSON |
| `Observation` | **results: List<ObservationResult>** |
| `ObservationResult` | source_call_id: String, content: List<ContentPart>, subagent_trajectory_ref: List<SubagentRef>, extra_json: String/JSON |
| `SubagentRef` | trajectory_id: String, session_id: String, trajectory_path: String, extra_json: String/JSON |
| `Metrics` | prompt_tokens: Int64, completion_tokens: Int64, cached_tokens: Int64, cost_usd: Float64, prompt_token_ids: List<Int64>, completion_token_ids: List<Int64>, logprobs: List<Float64>, extra_json: String/JSON |
| `final_metrics` | total_prompt_tokens: Int64, total_completion_tokens: Int64, total_cached_tokens: Int64, total_cost_usd: Float64, total_steps: UInt64, extra_json: String/JSON |

## Normalization and preservation

- A string message or observation content becomes one `{type: "text", text: ...}`
  part, including empty strings. Multimodal arrays preserve part order.
- Missing or null optional fields become Arrow null; empty arrays/objects stay
  empty. `raw_json` retains the distinction between omitted and explicit null.
- Every `*_json` field carries `atif-arrow.encoding=json` field metadata and
  contains valid JSON text. String reasoning effort is JSON-quoted; numeric
  effort remains a JSON number. Arbitrary tool arguments are not inferred into columns.
- Timestamps remain strings, preserving precision and timezone representation.
  No timezone, missing metric, token IDs, or logprobs are inferred.
- Media paths remain unchanged. Resolve relative paths against the source
  document's directory; this library will not fetch, embed, or verify media files.
- Subagent and continuation references are retained, not dereferenced. Validation
  will use version-specific rules (pre-v1.7 session refs versus v1.7+ document refs).
- Copied-context steps and deterministic dispatches remain present; conversion
  does not choose which records a training pipeline should use.

Supported structural types are `Struct` and `List`; the contract contains no
Arrow `Union` or `Null` type.
