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
fn rejects_unsupported_content_and_values_that_arrow_would_coerce() {
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
            json!([{"tool_call_id":"x","function_name":"run"}]),
            "steps[1].tool_calls[0].arguments",
        ),
        ("llm_call_count", json!(-1), "steps[1].llm_call_count"),
        (
            "message",
            json!([{"type":"image","source":{"media_type":"image/png","path":"a.png"}}]),
            "steps[1].message[0].type",
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
