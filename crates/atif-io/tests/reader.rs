use std::error::Error;

use futures::{io::Cursor, TryStreamExt};

use arrow_json::{writer::LineDelimited, WriterBuilder};
use atif_arrow::trajectory_schema;
use atif_io::{InputFormat, ReadError, TrajectoryReader};
use serde_json::{json, Value};

const DOCUMENT: &str = r#"{"schema_version":"ATIF-v1.8","agent":{"name":"test","version":"1"},"steps":[{"step_id":1,"source":"user","message":"hello"}]}"#;

#[tokio::test]
async fn batches_jsonl_in_order_with_nested_values_and_provenance() {
    let mut document: Value = serde_json::from_str(DOCUMENT).unwrap();
    document["steps"][0]["message"] = json!([{"type":"image","source":{
        "media_type":"image/png","path":"image.png"}}]);
    document["steps"][0]["metrics"] =
        json!({"prompt_tokens": i64::MAX, "completion_token_ids":[1,2]});
    document["extra"] = serde_json::from_str(r#"{"id":123456789012345678901234567890}"#).unwrap();
    let second = document.to_string();
    let input = format!("\n{DOCUMENT}\r\n \t\n{second}\n{DOCUMENT}");
    let batches: Vec<_> = TrajectoryReader::new(
        Cursor::new(input),
        InputFormat::JsonLines,
        2,
        Some("input.jsonl".into()),
    )
    .unwrap()
    .into_stream()
    .try_collect()
    .await
    .unwrap();
    assert_eq!(
        batches.iter().map(|b| b.num_rows()).collect::<Vec<_>>(),
        [2, 1]
    );
    let mut writer = WriterBuilder::new()
        .with_explicit_nulls(true)
        .build::<_, LineDelimited>(Vec::new());
    for batch in &batches {
        assert_eq!(batch.schema(), trajectory_schema());
        writer.write(batch).unwrap();
    }
    writer.finish().unwrap();
    let output = String::from_utf8(writer.into_inner()).unwrap();
    let rows: Vec<Value> = output
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row["source_record_index"], index + 1);
        assert_eq!(row["source_uri"], "input.jsonl");
    }
    assert_eq!(rows[0]["raw_json"], format!("{DOCUMENT}\r\n"));
    assert_eq!(rows[1]["raw_json"], format!("{second}\n"));
    assert_eq!(rows[2]["raw_json"], DOCUMENT);
    assert_eq!(rows[0]["steps"][0]["metrics"], Value::Null);
    assert_eq!(
        rows[1]["steps"][0]["metrics"]["completion_token_ids"],
        json!([1, 2])
    );
    assert_eq!(
        rows[1]["steps"][0]["message"][0]["source"]["path"],
        "image.png"
    );
    assert_eq!(
        rows[1]["steps"][0]["metrics"]["prompt_tokens"],
        json!(i64::MAX)
    );
    let extra: Value = serde_json::from_str(rows[1]["extra_json"].as_str().unwrap()).unwrap();
    assert_eq!(extra["id"].to_string(), "123456789012345678901234567890");
    assert_eq!(rows[2]["steps"][0]["message"][0]["text"], "hello");
}

#[tokio::test]
async fn reads_whole_json_and_handles_empty_jsonl() {
    let pretty =
        serde_json::to_string_pretty(&serde_json::from_str::<Value>(DOCUMENT).unwrap()).unwrap();
    let mut reader =
        TrajectoryReader::new(Cursor::new(&pretty), InputFormat::Json, 10, None).unwrap();
    let batch = reader.next_batch().await.unwrap().unwrap();
    assert_eq!(batch.num_rows(), 1);
    let raw = batch
        .column_by_name("raw_json")
        .unwrap()
        .as_any()
        .downcast_ref::<arrow_array::StringArray>()
        .unwrap();
    assert_eq!(raw.value(0), pretty);
    assert!(reader.next_batch().await.unwrap().is_none());
    for input in ["", "\n \t\r\n"] {
        let mut reader =
            TrajectoryReader::new(Cursor::new(input), InputFormat::JsonLines, 2, None).unwrap();
        assert!(reader.next_batch().await.unwrap().is_none());
        assert!(reader.next_batch().await.unwrap().is_none());
    }
    assert!(matches!(
        TrajectoryReader::new(Cursor::new(""), InputFormat::Json, 0, None),
        Err(ReadError::InvalidBatchSize)
    ));
}

#[tokio::test]
async fn reports_record_errors_and_stops_without_yielding_a_partial_batch() {
    let invalid_field = DOCUMENT.replace("\"hello\"", "42");
    let invalid_conversion = DOCUMENT.replace("\"hello\"", "[{\"type\":\"video\"}]");
    for invalid in [
        b"{".as_slice(),
        invalid_field.as_bytes(),
        invalid_conversion.as_bytes(),
        &[0xff],
    ] {
        for batch_size in [1, 2] {
            let input = [
                DOCUMENT.as_bytes(),
                b"\n\n",
                invalid,
                b"\n",
                DOCUMENT.as_bytes(),
            ]
            .concat();
            let mut reader = TrajectoryReader::new(
                Cursor::new(input),
                InputFormat::JsonLines,
                batch_size,
                Some("bad.jsonl".into()),
            )
            .unwrap();
            if batch_size == 1 {
                assert_eq!(reader.next_batch().await.unwrap().unwrap().num_rows(), 1);
            }
            let error = reader.next_batch().await.unwrap_err();
            assert!(
                matches!(&error, ReadError::Record { record_index: 2, source_uri: Some(uri), .. } if uri == "bad.jsonl")
            );
            assert!(error.source().is_some());
            assert!(error.to_string().starts_with("bad.jsonl: record 2:"));
            assert!(reader.next_batch().await.unwrap().is_none());
        }
    }
    for input in [String::new(), format!("{DOCUMENT}\n{DOCUMENT}")] {
        let mut reader =
            TrajectoryReader::new(Cursor::new(input), InputFormat::Json, 2, None).unwrap();
        assert!(matches!(
            reader.next_batch().await,
            Err(ReadError::Record {
                record_index: 1,
                ..
            })
        ));
        assert!(reader.next_batch().await.unwrap().is_none());
    }
}

#[tokio::test]
async fn resumes_cancelled_reads_without_losing_partial_documents_or_rows() {
    use futures::{channel::mpsc, FutureExt};

    let document = DOCUMENT.replace("hello", "你好");
    let split = document.find("你好").unwrap() + 1;
    for format in [InputFormat::Json, InputFormat::JsonLines] {
        let (sender, input) = mpsc::unbounded::<Result<Vec<u8>, std::io::Error>>();
        let prefix = if matches!(format, InputFormat::JsonLines) {
            [DOCUMENT.as_bytes(), b"\n", &document.as_bytes()[..split]].concat()
        } else {
            document.as_bytes()[..split].to_vec()
        };
        sender.unbounded_send(Ok(prefix)).unwrap();
        let mut reader = TrajectoryReader::new(input.into_async_read(), format, 2, None).unwrap();
        assert!(reader.next_batch().now_or_never().is_none());
        sender
            .unbounded_send(Ok(document.as_bytes()[split..].to_vec()))
            .unwrap();
        drop(sender);
        let batch = reader.next_batch().await.unwrap().unwrap();
        let expected = if matches!(format, InputFormat::JsonLines) {
            2
        } else {
            1
        };
        assert_eq!(batch.num_rows(), expected);
        let indices = batch
            .column_by_name("source_record_index")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow_array::UInt64Array>()
            .unwrap();
        assert_eq!(indices.value(expected - 1), expected as u64);
        let raw = batch
            .column_by_name("raw_json")
            .unwrap()
            .as_any()
            .downcast_ref::<arrow_array::StringArray>()
            .unwrap();
        assert_eq!(raw.value(expected - 1), document);
        if expected == 2 {
            assert_eq!(raw.value(0), format!("{DOCUMENT}\n"));
        }
        assert!(reader.next_batch().await.unwrap().is_none());
    }
}
