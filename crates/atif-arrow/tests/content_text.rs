use arrow_array::{cast::AsArray, Array};
use atif_arrow::{parse_trajectory, to_record_batch};
use serde_json::json;

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
            let document = json!({
                "schema_version": "ATIF-v1.8",
                "agent": {"name": "test", "version": "1"},
                "steps": [
                    {"step_id": 1, "source": "user", "message": content},
                    {"step_id": 2, "source": "agent", "message": "reply",
                     "observation": {"results": [{"content": content}]}}
                ]
            });
            let batch = to_record_batch(&parse_trajectory(&document.to_string()).unwrap(), None, 1)
                .unwrap();
            let steps = batch["steps"].as_list::<i32>().value(0);
            let steps = steps.as_struct();
            let message = steps["message"].as_list::<i32>().value(0);
            let results = steps["observation"].as_struct()["results"]
                .as_list::<i32>()
                .value(1);
            let observation = results.as_struct()["content"].as_list::<i32>().value(0);
            for parts in [message, observation] {
                let text = parts.as_struct()["text"].as_binary::<i64>();
                assert!(!text.is_null(0));
                assert_eq!(text.value(0), input.as_bytes());
            }
        }
    }
}
