use arrow_array::{cast::AsArray, Array, LargeBinaryArray, RecordBatch, StructArray};
use atif_arrow::{parse_trajectory, to_record_batch, trajectory_schema, TrajectoryBatchBuilder};
use serde_json::{json, Value};

fn document(message: Value, content: Value) -> Value {
    json!({
        "schema_version": "ATIF-v1.8",
        "agent": {"name": "test", "version": "1"},
        "steps": [
            {"step_id": 1, "source": "user", "message": message},
            {"step_id": 2, "source": "agent", "message": "reply",
             "reasoning_content": "reasoning stays a string",
             "observation": {"results": [{"content": content}]}}
        ]
    })
}

fn text(parts: &StructArray) -> &LargeBinaryArray {
    parts["text"]
        .as_any()
        .downcast_ref::<LargeBinaryArray>()
        .unwrap()
}

fn check_row(batch: &RecordBatch, row: usize, expected: &[u8]) {
    assert_eq!(batch.schema(), trajectory_schema());
    assert_eq!(batch.num_columns(), 13);
    let steps = batch["steps"].as_list::<i32>().value(row);
    let steps = steps.as_struct();
    let message = steps["message"].as_list::<i32>().value(0);
    let parts = message.as_struct();
    assert!(!text(parts).is_null(0));
    assert_eq!(text(parts).value(0), expected);
    assert_eq!(parts["type"].as_string::<i32>().value(0), "text");
    assert!(parts["source"].is_null(0));

    let observation = steps["observation"].as_struct();
    assert!(observation.is_null(0));
    let results = observation["results"].as_list::<i32>().value(1);
    let results = results.as_struct();
    let content = results["content"].as_list::<i32>().value(0);
    assert_eq!(text(content.as_struct()).value(0), expected);
    assert_eq!(
        steps["reasoning_content"].as_string::<i32>().value(1),
        "reasoning stays a string"
    );
}

#[test]
fn stores_content_as_exact_utf8_bytes_including_unicode_empty_and_hex_like_text() {
    let large = "大文本🙂\n".repeat(16_384);
    for input in ["", "deadbeef", "你好🙂\0\n\"quoted\"\\", large.as_str()] {
        for array_form in [false, true] {
            let content = if array_form {
                json!([{"type": "text", "text": input}])
            } else {
                json!(input)
            };
            let value = document(content.clone(), content);
            let raw = value.to_string();
            let batch =
                to_record_batch(&parse_trajectory(&raw).unwrap(), Some("local.json"), 1).unwrap();
            check_row(&batch, 0, input.as_bytes());
            assert_eq!(batch["raw_json"].as_string::<i32>().value(0), raw);
            assert_eq!(
                batch["source_uri"].as_string::<i32>().value(0),
                "local.json"
            );
        }
    }
}

#[test]
fn preserves_media_references_and_null_text_and_matches_streaming_conversion() {
    let parts = json!([
        {"type": "text", "text": "混合消息"},
        {"type": "image", "source": {"media_type": "image/png", "path": "missing/image.png"}},
        {"type": "audio", "source": {"media_type": "audio/wav", "path": "missing/audio.wav", "duration_sec": 0.5}}
    ]);
    let raw = document(parts.clone(), parts).to_string();
    let single = to_record_batch(&parse_trajectory(&raw).unwrap(), None, 1).unwrap();
    let mut builder = TrajectoryBatchBuilder::new(2).unwrap();
    builder.append_json(&raw, None, 1).unwrap();
    builder.append_json(&raw, None, 2).unwrap();
    let batch = builder.flush().unwrap().unwrap();
    assert_eq!(batch.slice(0, 1), single);
    assert_eq!(batch.num_rows(), 2);
    for row in 0..2 {
        check_row(&batch, row, "混合消息".as_bytes());
        let steps = batch["steps"].as_list::<i32>().value(row);
        let steps = steps.as_struct();
        let message = steps["message"].as_list::<i32>().value(0);
        let message = message.as_struct();
        assert!(text(message).is_null(1));
        assert!(text(message).is_null(2));
        let source = message["source"].as_struct();
        assert_eq!(
            source["path"].as_string::<i32>().value(1),
            "missing/image.png"
        );
        assert_eq!(
            source["path"].as_string::<i32>().value(2),
            "missing/audio.wav"
        );
        assert_eq!(
            source["media_type"].as_string::<i32>().value(1),
            "image/png"
        );
        assert_eq!(
            source["media_type"].as_string::<i32>().value(2),
            "audio/wav"
        );
    }
    assert!(builder.flush().unwrap().is_none());
    builder.append_json(&raw, None, 3).unwrap();
    check_row(&builder.flush().unwrap().unwrap(), 0, "混合消息".as_bytes());
}

#[test]
fn preserves_absent_content_and_empty_lists() {
    for content in [None, Some(Value::Null), Some(json!([]))] {
        let mut value = document(json!("message"), Value::Null);
        let result = value["steps"][1]["observation"]["results"][0]
            .as_object_mut()
            .unwrap();
        if let Some(content) = content.clone() {
            result.insert("content".into(), content);
        } else {
            result.remove("content");
        }
        let batch =
            to_record_batch(&parse_trajectory(&value.to_string()).unwrap(), None, 1).unwrap();
        let steps = batch["steps"].as_list::<i32>().value(0);
        let observation = steps.as_struct()["observation"].as_struct();
        let results = observation["results"].as_list::<i32>().value(1);
        let contents = results.as_struct()["content"].as_list::<i32>();
        assert_eq!(
            contents.is_null(0),
            content.as_ref().is_none_or(Value::is_null)
        );
        if !contents.is_null(0) {
            assert_eq!(contents.value_length(0), 0);
        }
    }
}
