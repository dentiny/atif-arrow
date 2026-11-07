use atif_arrow::{trajectory_schema, TrajectoryBatchBuilder};
use serde_json::{json, Value};

fn document() -> Value {
    json!({
        "schema_version": "ATIF-v1.8",
        "trajectory_id": "parent",
        "agent": {"name": "test", "version": "1", "tool_definitions": []},
        "steps": [{"step_id": 1, "source": "agent", "message": "hello",
            "tool_calls": [{"tool_call_id": "call", "function_name": "run", "arguments": {}}],
            "observation": {"results": [{"source_call_id": "call", "content": "done"}]}}],
        "extra": {"unknown": true}
    })
}

#[test]
fn projected_batches_match_full_conversion_including_order_duplicates_and_empty_columns() {
    let input = document().to_string();
    let mut full = TrajectoryBatchBuilder::new(2, None).unwrap();
    for index in 1..=2 {
        full.append_json(&input, Some("source"), index).unwrap();
    }
    let full = full.flush().unwrap().unwrap();
    let mut projections: Vec<Vec<usize>> = (0..trajectory_schema().fields().len())
        .map(|index| vec![index])
        .collect();
    projections.extend([vec![], vec![12, 1, 6, 0], vec![1, 1]]);
    for projection in projections {
        let mut builder = TrajectoryBatchBuilder::new(2, Some(&projection)).unwrap();
        for index in 1..=2 {
            builder.append_json(&input, Some("source"), index).unwrap();
        }
        assert!(builder.append_json(&input, None, 3).is_err());
        assert_eq!(
            builder.flush().unwrap().unwrap(),
            full.project(&projection).unwrap()
        );
        assert!(builder.flush().unwrap().is_none());
        builder.append_json(&input, Some("source"), 1).unwrap();
        assert_eq!(
            builder.flush().unwrap().unwrap(),
            full.slice(0, 1).project(&projection).unwrap()
        );
    }
}

#[test]
fn omitted_fields_keep_the_same_validation_errors_and_failed_appends_are_atomic() {
    let valid = document();
    for (pointer, value) in [
        ("/steps/0/tool_calls/0/arguments", json!([])),
        (
            "/steps/0/observation/results/0/source_call_id",
            json!("missing"),
        ),
        (
            "/steps/0/message",
            json!([{"type":"audio", "source":{"media_type":"audio/wav", "path":"a", "duration_sec":-1}}]),
        ),
        ("/extra", json!([])),
        (
            "/subagent_trajectories",
            json!([{"schema_version":"ATIF-v1.8", "trajectory_id":"child", "agent":{"name":"x", "version":"1"}, "steps":[{"step_id":1,"source":"agent","message":"ok", "metrics":{"prompt_tokens":"bad"}}]}]),
        ),
    ] {
        let mut invalid = valid.clone();
        // The subagent field is absent from the original document.
        if pointer == "/subagent_trajectories" {
            invalid["subagent_trajectories"] = value;
        } else {
            *invalid.pointer_mut(pointer).unwrap() = value;
        }
        let input = invalid.to_string();
        let expected = TrajectoryBatchBuilder::new(2, None)
            .unwrap()
            .append_json(&input, None, 2)
            .unwrap_err()
            .to_string();
        for projection in [vec![], vec![1], vec![12]] {
            let mut builder = TrajectoryBatchBuilder::new(2, Some(&projection)).unwrap();
            builder.append_json(&valid.to_string(), None, 1).unwrap();
            assert_eq!(
                builder
                    .append_json(&input, None, 2)
                    .unwrap_err()
                    .to_string(),
                expected
            );
            assert!(builder.append_json(&valid.to_string(), None, 0).is_err());
            builder.append_json(&valid.to_string(), None, 2).unwrap();
            let batch = builder.flush().unwrap().unwrap();
            assert_eq!(batch.num_rows(), 2);
            assert_eq!(batch.num_columns(), projection.len());
        }
    }
}

#[test]
fn invalid_projection_and_batch_size_are_rejected() {
    assert!(TrajectoryBatchBuilder::new(0, Some(&[])).is_err());
    assert!(TrajectoryBatchBuilder::new(1, Some(&[trajectory_schema().fields().len()])).is_err());
}
