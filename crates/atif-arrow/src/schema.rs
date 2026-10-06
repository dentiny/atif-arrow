use std::{collections::HashMap, sync::Arc};

use arrow_schema::{DataType, Field, Schema, SchemaRef};
use DataType::{Boolean, Float64, Int64, UInt64, Utf8};

/// One row per input trajectory document.
///
/// List elements are non-null; optional lists preserve null versus empty.
/// The schema is independent of which optional fields appear in the input.
pub fn trajectory_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("source_uri", Utf8, true),
        Field::new("source_record_index", UInt64, false),
        Field::new("atif_schema_version", Utf8, false),
        Field::new("session_id", Utf8, true),
        Field::new("trajectory_id", Utf8, true),
        Field::new("agent", agent(), false),
        list("steps", step(), false),
        Field::new("notes", Utf8, true),
        Field::new("final_metrics", final_metrics(), true),
        Field::new("continued_trajectory_ref", Utf8, true),
        json_field("extra_json", true),
        json_field("subagent_trajectories_json", true),
        json_field("raw_json", false),
    ]))
}

/// Schema for ATIF agent configuration; tool definitions remain serialized JSON.
fn agent() -> DataType {
    record(vec![
        Field::new("name", Utf8, false),
        Field::new("version", Utf8, false),
        Field::new("model_name", Utf8, true),
        json_field("tool_definitions_json", true),
        json_field("extra_json", true),
    ])
}

/// Schema for an ATIF interaction, including ordered content, tool calls, and LLM data.
fn step() -> DataType {
    record(vec![
        Field::new("step_id", UInt64, false),
        Field::new("timestamp", Utf8, true),
        Field::new("source", Utf8, false),
        Field::new("model_name", Utf8, true),
        json_field("reasoning_effort_json", true),
        list("message", content_part(), false),
        Field::new("reasoning_content", Utf8, true),
        list("tool_calls", tool_call(), true),
        Field::new("observation", observation(), true),
        Field::new("metrics", metrics(), true),
        Field::new("is_copied_context", Boolean, true),
        Field::new("llm_call_count", UInt64, true),
        json_field("extra_json", true),
    ])
}

/// Tagged text, image, or audio content; media variants share a nullable source struct.
fn content_part() -> DataType {
    record(vec![
        Field::new("type", Utf8, false),
        Field::new("text", Utf8, true),
        Field::new(
            "source",
            record(vec![
                Field::new("media_type", Utf8, false),
                Field::new("path", Utf8, false),
                Field::new("duration_sec", Float64, true),
            ]),
            true,
        ),
    ])
}

/// Schema for a tool invocation; arbitrary argument objects remain serialized JSON.
fn tool_call() -> DataType {
    record(vec![
        Field::new("tool_call_id", Utf8, false),
        Field::new("function_name", Utf8, false),
        json_field("arguments_json", false),
        json_field("extra_json", true),
    ])
}

/// Tool or environment results, with references to calls and delegated subagent trajectories.
fn observation() -> DataType {
    record(vec![list(
        "results",
        record(vec![
            Field::new("source_call_id", Utf8, true),
            list("content", content_part(), true),
            list(
                "subagent_trajectory_ref",
                record(vec![
                    Field::new("trajectory_id", Utf8, true),
                    Field::new("session_id", Utf8, true),
                    Field::new("trajectory_path", Utf8, true),
                    json_field("extra_json", true),
                ]),
                true,
            ),
            json_field("extra_json", true),
        ]),
        false,
    )])
}

/// Per-step LLM usage and cost, including optional token IDs and token-level logprobs.
fn metrics() -> DataType {
    record(vec![
        Field::new("prompt_tokens", Int64, true),
        Field::new("completion_tokens", Int64, true),
        Field::new("cached_tokens", Int64, true),
        Field::new("cost_usd", Float64, true),
        list("prompt_token_ids", Int64, true),
        list("completion_token_ids", Int64, true),
        list("logprobs", Float64, true),
        json_field("extra_json", true),
    ])
}

/// Trajectory-level totals supplied by the producer; all fields are optional.
fn final_metrics() -> DataType {
    record(vec![
        Field::new("total_prompt_tokens", Int64, true),
        Field::new("total_completion_tokens", Int64, true),
        Field::new("total_cached_tokens", Int64, true),
        Field::new("total_cost_usd", Float64, true),
        Field::new("total_steps", UInt64, true),
        json_field("extra_json", true),
    ])
}

/// Groups named fields into an Arrow struct.
fn record(fields: Vec<Field>) -> DataType {
    DataType::Struct(fields.into())
}

/// Creates a list with non-null elements; nullable controls the list itself.
fn list(name: &str, item: DataType, nullable: bool) -> Field {
    Field::new(
        name,
        DataType::List(Arc::new(Field::new("item", item, false))),
        nullable,
    )
}

/// Marks a UTF-8 field as serialized JSON to keep arbitrary payloads in a fixed type.
fn json_field(name: &str, nullable: bool) -> Field {
    Field::new(name, Utf8, nullable).with_metadata(HashMap::from([(
        "atif-arrow.encoding".into(),
        "json".into(),
    )]))
}
