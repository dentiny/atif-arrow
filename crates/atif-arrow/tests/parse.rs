use atif_arrow::{parse_trajectory, Message, StepSource};
use serde_json::{json, Value};

const DOCUMENT: &str = r#"{
  "schema_version": "ATIF-v1.5",
  "session_id": "run-1",
  "agent": {"name": "example", "version": "1", "config": {"enabled": true}},
  "steps": [{"step_id": 1, "source": "user", "message": "hello",
             "vendor_id": 123456789012345678901234567890}],
  "extra": {"reward": 0.75}
}"#;

#[test]
fn parses_core_fields_and_preserves_original_data_across_versions() {
    let parsed = parse_trajectory(DOCUMENT).unwrap();
    assert_eq!(parsed.raw_json(), DOCUMENT);
    let trajectory = &parsed.trajectory;
    assert_eq!(trajectory.agent.name, "example");
    assert_eq!(trajectory.steps[0].source, StepSource::User);
    assert!(matches!(&trajectory.steps[0].message, Message::Text(text) if text == "hello"));
    assert_eq!(
        trajectory.agent.additional_fields["config"]["enabled"],
        true
    );
    assert_eq!(trajectory.additional_fields["extra"]["reward"], 0.75);
    assert_eq!(
        trajectory.steps[0].additional_fields["vendor_id"].to_string(),
        "123456789012345678901234567890"
    );
    for minor in 0..=8 {
        let mut document: Value = serde_json::from_str(DOCUMENT).unwrap();
        document["schema_version"] = json!(format!("ATIF-v1.{minor}"));
        document["steps"]
            .as_array_mut()
            .unwrap()
            .push(json!({"step_id": 2, "source": "agent", "message": "done"}));
        if minor >= 7 {
            document.as_object_mut().unwrap().remove("session_id");
        }
        parse_trajectory(&document.to_string()).unwrap();
    }
}

#[test]
fn rejects_invalid_core_fields_and_multiple_documents_with_locations() {
    let cases = [
        ("schema_version", json!("ATIF-v2.0"), "schema_version"),
        ("session_id", Value::Null, "session_id"),
        ("agent", json!({"version": "1"}), "agent"),
        ("steps", json!([]), "steps"),
        (
            "steps",
            json!([{"step_id": 0, "source": "user", "message": "x"}]),
            "steps[0].step_id",
        ),
        (
            "steps",
            json!([
                {"step_id": 1, "source": "user", "message": "x"},
                {"step_id": 3, "source": "agent", "message": "y"}
            ]),
            "steps[1].step_id",
        ),
        (
            "steps",
            json!([{"step_id": 1, "source": "tool", "message": "x"}]),
            "steps[0].source",
        ),
        (
            "steps",
            json!([{"step_id": 1, "source": "user", "message": {}}]),
            "steps[0].message",
        ),
        (
            "steps",
            json!([{"step_id": 1, "source": "user"}]),
            "steps[0]",
        ),
    ];
    for (field, replacement, expected_path) in cases {
        let mut document: Value = serde_json::from_str(DOCUMENT).unwrap();
        document[field] = replacement;
        let error = parse_trajectory(&document.to_string()).unwrap_err();
        assert_eq!(error.path, expected_path, "{error}");
        assert!(!error.message.is_empty());
    }
    for input in [
        "".to_owned(),
        "{".to_owned(),
        format!("{DOCUMENT}{DOCUMENT}"),
    ] {
        assert_eq!(parse_trajectory(&input).unwrap_err().path, "$");
    }
}

#[test]
fn validates_timestamps_without_requiring_a_timezone_or_changing_the_input() {
    let mut document: Value = serde_json::from_str(DOCUMENT).unwrap();
    for timestamp in [
        Value::Null,
        json!("2024-02-29T12:34:56Z"),
        json!("2024-02-29T12:34:56.123456789012+05:30"),
        json!("2024-02-29T12:34:56"),
        json!("2024-02-29 12:34:56.123456"),
        json!("2024-02-29"),
        json!("20240229T123456+0530"),
        json!("2024-W09-4T12:34"),
    ] {
        document["steps"][0]["timestamp"] = timestamp.clone();
        let input = document.to_string();
        let parsed = parse_trajectory(&input).unwrap();
        assert_eq!(parsed.raw_json(), input);
        assert_eq!(
            parsed.trajectory.steps[0].additional_fields["timestamp"],
            timestamp
        );
    }
    for timestamp in [
        json!(""),
        json!("not-a-timestamp"),
        json!("2023-02-29T12:34:56Z"),
        json!("2024-04-31T12:34:56Z"),
        json!("2024-02-29T24:34:56Z"),
        json!("2024-02-29T12:60:56Z"),
        json!("2024-02-29T12:34:61Z"),
        json!("2024-02-29T12:34:56+24:00"),
        json!("2024-02-29T12:34:56+05:60"),
        json!("2024-02-29T12:34:56Ztrailing"),
        json!(42),
    ] {
        document["steps"][0]["timestamp"] = timestamp.clone();
        let error = parse_trajectory(&document.to_string()).unwrap_err();
        assert_eq!(error.path, "steps[0].timestamp", "{timestamp}");
    }
}

#[test]
fn enforces_agent_only_and_zero_llm_call_rules_with_nulls_treated_as_unset() {
    let fields = [
        ("model_name", json!("")),
        ("reasoning_effort", json!("high")),
        ("reasoning_content", json!("")),
        ("tool_calls", json!([])),
        ("metrics", json!({})),
    ];
    for source in ["system", "user", "agent"] {
        for (field, value) in &fields {
            for value in [Value::Null, value.clone()] {
                let mut document: Value = serde_json::from_str(DOCUMENT).unwrap();
                document["steps"][0]["source"] = json!(source);
                document["steps"][0][*field] = value.clone();
                let result = parse_trajectory(&document.to_string());
                if source == "agent" || value.is_null() {
                    result.unwrap();
                } else {
                    assert_eq!(result.unwrap_err().path, format!("steps[0].{field}"));
                }
            }
        }
    }
    for count in [Value::Null, json!(0), json!(1)] {
        for field in ["metrics", "reasoning_content"] {
            for value in [
                Value::Null,
                if field == "metrics" {
                    json!({})
                } else {
                    json!("")
                },
            ] {
                let mut document: Value = serde_json::from_str(DOCUMENT).unwrap();
                document["steps"][0]["source"] = json!("agent");
                document["steps"][0]["llm_call_count"] = count.clone();
                document["steps"][0][field] = value.clone();
                let result = parse_trajectory(&document.to_string());
                if count == json!(0) && !value.is_null() {
                    assert_eq!(result.unwrap_err().path, format!("steps[0].{field}"));
                } else {
                    result.unwrap();
                }
            }
        }
    }
    // Deterministic steps can still contain tool calls; system observations are allowed.
    for source in ["system", "user", "agent"] {
        let mut document: Value = serde_json::from_str(DOCUMENT).unwrap();
        document["steps"][0]["source"] = json!(source);
        document["steps"][0]["llm_call_count"] = json!(0);
        document["steps"][0]["observation"] = json!({"results": []});
        if source == "agent" {
            document["steps"][0]["tool_calls"] = json!([]);
            document["steps"][0]["model_name"] = json!("example");
        }
        parse_trajectory(&document.to_string()).unwrap();
    }
}
