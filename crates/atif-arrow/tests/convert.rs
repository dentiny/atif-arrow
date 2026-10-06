use arrow_array::RecordBatch;
use arrow_json::{writer::LineDelimited, WriterBuilder};
use atif_arrow::{parse_trajectory, to_record_batch, trajectory_schema, ConversionError};
use serde_json::{json, Value};

const DOCUMENT: &str = r#"{
  "schema_version": "ATIF-v1.8",
  "agent": {"name": "example", "version": "1", "tool_definitions": []},
  "steps": [
    {"step_id": 1, "source": "user", "message": ""},
    {"step_id": 2, "source": "agent", "message": [{"type":"text","text":"done"}],
     "tool_calls": [{"tool_call_id":"call-1","function_name":"run",
       "arguments":{"id":123456789012345678901234567890}}],
     "observation":{"results":[{"source_call_id":"call-1","content":"ok"}]},
     "reasoning_effort":"high",
     "metrics":{"prompt_tokens":9223372036854775807,"cost_usd":0.25,
       "prompt_token_ids":[],"completion_token_ids":[7,8],"logprobs":[-0.5,-0.25]}}
  ],
  "final_metrics":{"total_steps":2},
  "vendor":{"retained":true}
}"#;

/// Reads the actual Arrow output through its standard JSON writer.
fn output_row(batch: &RecordBatch) -> Value {
    let mut writer = WriterBuilder::new()
        .with_explicit_nulls(true)
        .build::<_, LineDelimited>(Vec::new());
    writer.write(batch).unwrap();
    writer.finish().unwrap();
    serde_json::from_slice(&writer.into_inner()).unwrap()
}

#[test]
fn converts_text_tools_and_metrics_without_losing_original_data() {
    let parsed = parse_trajectory(DOCUMENT).unwrap();
    let batch = to_record_batch(&parsed, Some("s3://bucket/trajectory.json"), u64::MAX).unwrap();
    assert_eq!(batch.schema(), trajectory_schema());
    assert_eq!(batch.num_rows(), 1);
    let row = output_row(&batch);
    assert_eq!(row["source_record_index"], json!(u64::MAX));
    assert_eq!(row["source_uri"], "s3://bucket/trajectory.json");
    assert_eq!(row["raw_json"], DOCUMENT);
    assert_eq!(row["agent"]["tool_definitions_json"], "[]");
    assert_eq!(row["final_metrics"]["total_steps"], 2);
    let steps = row["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(
        steps[0]["message"],
        json!([{"type":"text","text":"","source":null}])
    );
    assert_eq!(steps[1]["message"][0]["text"], "done");
    let arguments: Value = serde_json::from_str(
        steps[1]["tool_calls"][0]["arguments_json"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        arguments["id"].to_string(),
        "123456789012345678901234567890"
    );
    assert_eq!(steps[0]["metrics"], Value::Null);
    assert_eq!(steps[1]["metrics"]["prompt_tokens"], json!(i64::MAX));
    assert_eq!(steps[1]["metrics"]["cost_usd"], 0.25);
    assert_eq!(steps[1]["metrics"]["prompt_token_ids"], json!([]));
    assert_eq!(steps[1]["metrics"]["completion_token_ids"], json!([7, 8]));
    assert_eq!(steps[1]["metrics"]["logprobs"], json!([-0.5, -0.25]));
    assert_eq!(steps[1]["reasoning_effort_json"], "\"high\"");
    assert_eq!(
        steps[1]["observation"]["results"][0]["content"][0]["text"],
        "ok"
    );

    for literal in ["0.5", "1.0", "1e2"] {
        let document = DOCUMENT.replace(
            "\"reasoning_effort\":\"high\"",
            &format!("\"reasoning_effort\":{literal}"),
        );
        let parsed = parse_trajectory(&document).unwrap();
        let row = output_row(&to_record_batch(&parsed, None, 1).unwrap());
        let effort: Value =
            serde_json::from_str(row["steps"][1]["reasoning_effort_json"].as_str().unwrap())
                .unwrap();
        assert_eq!(effort, serde_json::from_str::<Value>(literal).unwrap());
        assert_eq!(row["raw_json"], document);
    }
}

#[test]
fn preserves_missing_null_and_empty_optional_values() {
    for value in [None, Some(Value::Null), Some(json!([]))] {
        let mut document: Value = serde_json::from_str(DOCUMENT).unwrap();
        if let Some(value) = &value {
            document["steps"][0]["tool_calls"] = value.clone();
        }
        document["steps"][0]["metrics"] = json!({});
        let parsed = parse_trajectory(&document.to_string()).unwrap();
        let row = output_row(&to_record_batch(&parsed, None, 1).unwrap());
        assert_eq!(row["source_uri"], Value::Null);
        assert_eq!(row["steps"][0]["tool_calls"], value.unwrap_or(Value::Null));
        assert!(row["steps"][0]["metrics"].is_object());
        assert_eq!(row["steps"][0]["metrics"]["prompt_tokens"], Value::Null);
    }
}

#[test]
fn rejects_invalid_content_and_values_that_arrow_would_coerce() {
    let overflow: Value = serde_json::from_str("9223372036854775808").unwrap();
    let huge_float: Value = serde_json::from_str("1e1000").unwrap();
    let cases = [
        (
            "metrics",
            json!({"prompt_tokens":"3"}),
            "steps[1].metrics.prompt_tokens",
        ),
        (
            "metrics",
            json!({"prompt_tokens":1.5}),
            "steps[1].metrics.prompt_tokens",
        ),
        (
            "metrics",
            json!({"prompt_tokens":overflow}),
            "steps[1].metrics.prompt_tokens",
        ),
        (
            "metrics",
            json!({"cost_usd":huge_float}),
            "steps[1].metrics.cost_usd",
        ),
        (
            "metrics",
            json!({"completion_token_ids":[null]}),
            "steps[1].metrics.completion_token_ids[0]",
        ),
        (
            "tool_calls",
            json!([{"tool_call_id":"call-1","function_name":"run"}]),
            "steps[1].tool_calls[0].arguments",
        ),
        ("llm_call_count", json!(-1), "steps[1].llm_call_count"),
        (
            "message",
            json!([{"type":"image","source":{"media_type":"audio/wav","path":"a.wav"}}]),
            "steps[1].message[0].source.media_type",
        ),
        (
            "message",
            json!([{"type":"text"}]),
            "steps[1].message[0].text",
        ),
    ];
    for (field, value, expected_path) in cases {
        let mut document: Value = serde_json::from_str(DOCUMENT).unwrap();
        document["steps"][1][field] = value;
        let parsed = parse_trajectory(&document.to_string()).unwrap();
        let error = to_record_batch(&parsed, None, 1).unwrap_err();
        match error {
            ConversionError::Field(error) => assert_eq!(error.path, expected_path),
            error => panic!("expected field error, got {error}"),
        }
    }
    let parsed = parse_trajectory(DOCUMENT).unwrap();
    assert!(to_record_batch(&parsed, None, 0).is_err());
}

#[test]
fn converts_multimodal_content_and_rejects_invalid_media() {
    let mut document: Value = serde_json::from_str(DOCUMENT).unwrap();
    let parts = json!([
        {"type":"text","text":"look and listen"},
        {"type":"image","source":{"media_type":"image/png","path":"../images/a.png"}},
        {"type":"audio","source":{"media_type":"audio/mp3","path":"s3://bucket/a.mp3","duration_sec":1.25}}
    ]);
    document["steps"][0]["message"] = parts.clone();
    document["steps"][1]["observation"]["results"][0]["content"] = parts.clone();
    document["session_id"] = json!("run-1");
    let input = document.to_string();
    let row = output_row(&to_record_batch(&parse_trajectory(&input).unwrap(), None, 1).unwrap());
    assert_eq!(row["raw_json"], input);
    for content in [
        &row["steps"][0]["message"],
        &row["steps"][1]["observation"]["results"][0]["content"],
    ] {
        assert_eq!(content.as_array().unwrap().len(), 3);
        assert_eq!(content[1]["source"]["path"], parts[1]["source"]["path"]);
        assert_eq!(content[2]["source"], parts[2]["source"]);
    }
    for version in ["ATIF-v1.5", "ATIF-v1.6", "ATIF-v1.7"] {
        document["schema_version"] = json!(version);
        assert!(
            to_record_batch(&parse_trajectory(&document.to_string()).unwrap(), None, 1).is_err()
        );
    }
    document["schema_version"] = json!("ATIF-v1.6");
    for pointer in ["/steps/0/message", "/steps/1/observation/results/0/content"] {
        *document.pointer_mut(pointer).unwrap() = json!([parts[0], parts[1]]);
    }
    to_record_batch(&parse_trajectory(&document.to_string()).unwrap(), None, 1).unwrap();
    document["schema_version"] = json!("ATIF-v1.8");
    for part in [
        json!({"type":"audio"}),
        json!({"type":"image","text":"unexpected","source":{"media_type":"image/png","path":"a"}}),
        json!({"type":"audio","source":{"media_type":"audio/wav","path":"a","duration_sec":-1}}),
        json!({"type":"image","source":{"media_type":"image/png"}}),
        json!({"type":"video"}),
    ] {
        document["steps"][0]["message"] = json!([part]);
        assert!(
            to_record_batch(&parse_trajectory(&document.to_string()).unwrap(), None, 1).is_err()
        );
    }
}

#[test]
fn preserves_subagents_and_validates_nested_documents_and_references() {
    let child = json!({"schema_version":"ATIF-v1.8","session_id":"shared-run","trajectory_id":"worker-1",
        "agent":{"name":"worker","version":"1"},"steps":[{"step_id":1,"source":"agent","message":"done","metrics":{}}],
        "vendor":{"unmodeled":true}});
    let mut sibling = child.clone();
    sibling["trajectory_id"] = json!("worker-2");
    let mut document: Value = serde_json::from_str(DOCUMENT).unwrap();
    document["session_id"] = json!("shared-run");
    document["subagent_trajectories"] = json!([child, sibling]);
    document["steps"][1]["observation"]["results"][0]["subagent_trajectory_ref"] = json!([
        {"trajectory_id":"worker-1","session_id":"shared-run"},
        {"trajectory_id":"external","trajectory_path":"s3://bucket/external.json"}
    ]);
    let row = output_row(
        &to_record_batch(&parse_trajectory(&document.to_string()).unwrap(), None, 1).unwrap(),
    );
    assert_eq!(
        serde_json::from_str::<Value>(row["subagent_trajectories_json"].as_str().unwrap()).unwrap(),
        document["subagent_trajectories"]
    );
    assert_eq!(
        row["steps"][1]["observation"]["results"][0]["subagent_trajectory_ref"][1]
            ["trajectory_path"],
        "s3://bucket/external.json"
    );
    let mut leaf = child.clone();
    leaf["trajectory_id"] = json!("leaf");
    leaf["session_id"] = Value::Null;
    document["subagent_trajectories"][0]["subagent_trajectories"] = json!([leaf]);
    document["subagent_trajectories"][0]["steps"][0]["observation"] =
        json!({"results":[{"subagent_trajectory_ref":[{"trajectory_id":"leaf"}]}]});
    to_record_batch(&parse_trajectory(&document.to_string()).unwrap(), None, 1).unwrap();
    for (pointer, value) in [
        ("/subagent_trajectories/1/trajectory_id", json!("worker-1")),
        ("/subagent_trajectories/0/trajectory_id", Value::Null),
        ("/subagent_trajectories/0/steps/0/step_id", json!(2)),
        (
            "/subagent_trajectories/0/steps/0/metrics",
            json!({"prompt_tokens":"3"}),
        ),
        (
            "/subagent_trajectories/0/subagent_trajectories/0/steps/0/message",
            json!([{"type":"audio"}]),
        ),
        (
            "/steps/1/observation/results/0/subagent_trajectory_ref",
            json!([{"trajectory_id":"missing"}]),
        ),
        (
            "/steps/1/observation/results/0/subagent_trajectory_ref",
            json!([{"session_id":"shared-run"}]),
        ),
        ("/schema_version", json!("ATIF-v1.6")),
    ] {
        let mut invalid = document.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        let error =
            to_record_batch(&parse_trajectory(&invalid.to_string()).unwrap(), None, 1).unwrap_err();
        assert!(matches!(error, ConversionError::Field(_)));
    }
    // Legacy session-only references remain valid; missing legacy session IDs fail.
    document
        .as_object_mut()
        .unwrap()
        .remove("subagent_trajectories");
    document["schema_version"] = json!("ATIF-v1.6");
    let pointer = "/steps/1/observation/results/0/subagent_trajectory_ref";
    *document.pointer_mut(pointer).unwrap() = json!([{"session_id":"child-run"}]);
    to_record_batch(&parse_trajectory(&document.to_string()).unwrap(), None, 1).unwrap();
    *document.pointer_mut(pointer).unwrap() = json!([{"trajectory_path":"child.json"}]);
    assert!(to_record_batch(&parse_trajectory(&document.to_string()).unwrap(), None, 1).is_err());
}

#[test]
fn batch_builder_requires_flushing_and_can_be_reused() {
    use atif_arrow::TrajectoryBatchBuilder;

    assert!(TrajectoryBatchBuilder::new(0).is_err());
    let mut builder = TrajectoryBatchBuilder::new(2).unwrap();
    for index in 1..=2 {
        builder.append_json(DOCUMENT, None, index).unwrap();
    }
    assert!(builder.append_json(DOCUMENT, None, 3).is_err());
    let batch = builder.flush().unwrap().unwrap();
    assert_eq!(batch.num_rows(), 2);
    assert_eq!(batch.schema(), trajectory_schema());
    assert!(builder.flush().unwrap().is_none());
    builder.append_json(DOCUMENT, None, 3).unwrap();
    let row = output_row(&builder.flush().unwrap().unwrap());
    assert_eq!(row["source_record_index"], 3);
    assert_eq!(row["raw_json"], DOCUMENT);
}
