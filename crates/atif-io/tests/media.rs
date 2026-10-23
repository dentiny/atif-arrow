use arrow_array::{
    Array, LargeBinaryArray, ListArray, RecordBatch, StringArray, StructArray, UInt64Array,
};
use atif_arrow::{
    media_field, parse_trajectory, to_record_batch, trajectory_schema, trajectory_schema_with_media,
};
use atif_io::{InputFormat, MediaOptions, MediaResolver, ReadError, TrajectoryReader};
use futures::io::Cursor;
#[cfg(feature = "gzip")]
use futures::TryStreamExt;
use opendal::{services, Operator};
use serde_json::{json, Value};

fn document(message: Value) -> Value {
    json!({"schema_version":"ATIF-v1.8", "trajectory_id":"parent",
        "agent":{"name":"test","version":"1"},
        "steps":[{"step_id":1,"source":"agent","message":message}]})
}

fn image(path: &str) -> Value {
    json!({"type":"image","source":{"media_type":"image/png","path":path}})
}

fn native(value: &Value, index: u64) -> RecordBatch {
    to_record_batch(
        &parse_trajectory(&value.to_string()).unwrap(),
        Some("display://source"),
        index,
    )
    .unwrap()
}

fn column<'a, T: 'static>(batch: &'a RecordBatch, name: &str) -> &'a T {
    batch
        .column_by_name(name)
        .unwrap()
        .as_any()
        .downcast_ref()
        .unwrap()
}

fn items(batch: &RecordBatch, row: usize) -> StructArray {
    column::<ListArray>(batch, "media")
        .value(row)
        .as_any()
        .downcast_ref::<StructArray>()
        .unwrap()
        .clone()
}

fn field<'a, T: 'static>(items: &'a StructArray, name: &str) -> &'a T {
    items
        .column_by_name(name)
        .unwrap()
        .as_any()
        .downcast_ref()
        .unwrap()
}

#[tokio::test]
async fn embeds_large_images_audio_observations_and_recursive_children() {
    let directory = tempfile::tempdir().unwrap();
    let operator =
        Operator::new(services::Fs::default().root(directory.path().to_str().unwrap())).unwrap();
    let mut png = vec![0_u8; 2 * 1024 * 1024 + 5];
    png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    png[10] = 255;
    operator
        .write("runs/images/one.png", png.clone())
        .await
        .unwrap();
    let audio = json!({"type":"audio", "source":{
        "media_type":"audio/wav", "path":"data:audio/wav;base64,UklGRgD%2F", "duration_sec":1.5
    }});
    let mut value =
        document(json!([{"type":"text","text":"hello"}, image("images/one.png"), audio]));
    value["steps"][0]["observation"] = json!({"results":[{"content":[image("./images/one.png")]}]});
    let mut child = document(json!([image("images/../images/one.png")]));
    child["trajectory_id"] = json!("child");
    let mut grandchild = document(json!([image("images/one.png")]));
    grandchild["trajectory_id"] = json!("grandchild");
    child["subagent_trajectories"] = json!([grandchild]);
    value["subagent_trajectories"] = json!([child]);
    let raw = serde_json::to_string_pretty(&value).unwrap();
    operator
        .write("runs/trajectory.json", raw.clone())
        .await
        .unwrap();
    let resolver =
        MediaResolver::new(&operator, "runs/trajectory.json", MediaOptions::default()).unwrap();
    let mut reader = TrajectoryReader::open(
        &operator,
        "runs/trajectory.json",
        InputFormat::Json,
        1,
        Some("harbor://display-only".into()),
    )
    .await
    .unwrap()
    .with_media(resolver);
    assert_eq!(reader.schema(), trajectory_schema_with_media());
    let batch = reader.next_batch().await.unwrap().unwrap();
    assert_eq!(batch.num_columns(), 14);
    assert_eq!(batch.schema().field(13), &media_field());
    assert_eq!(column::<StringArray>(&batch, "raw_json").value(0), raw);
    assert_eq!(
        column::<StringArray>(&batch, "source_uri").value(0),
        "harbor://display-only"
    );
    let media = items(&batch, 0);
    let payloads = field::<LargeBinaryArray>(&media, "payload");
    let locators = field::<StringArray>(&media, "source_locator");
    let expected = [
        "/steps/0/message/1",
        "/steps/0/message/2",
        "/steps/0/observation/results/0/content/0",
        "/subagent_trajectories/0/steps/0/message/0",
        "/subagent_trajectories/0/subagent_trajectories/0/steps/0/message/0",
    ];
    assert_eq!(media.len(), expected.len());
    for (index, locator) in expected.iter().enumerate() {
        assert_eq!(locators.value(index), *locator);
        assert!(value.pointer(locator).is_some());
        let bytes = if index == 1 {
            b"RIFF\0\xff".as_slice()
        } else {
            png.as_slice()
        };
        assert_eq!(payloads.value(index), bytes);
        assert_eq!(
            field::<UInt64Array>(&media, "byte_size").value(index),
            bytes.len() as u64
        );
        assert_eq!(
            field::<StringArray>(&media, "resolution_status").value(index),
            "embedded"
        );
    }
    assert!(field::<StringArray>(&media, "resolved_uri").is_null(1));
    assert!(reader.next_batch().await.unwrap().is_none());
    operator.delete("runs/images/one.png").await.unwrap();
    assert_eq!(payloads.value(0), png.as_slice());
}

#[tokio::test]
async fn missing_media_is_a_provenanced_error_and_stops_without_a_partial_batch() {
    let operator = Operator::new(services::Memory::default()).unwrap();
    let valid = document(json!("text"));
    let missing = document(json!([image("missing.png")]));
    let input = format!("{valid}\n{missing}\n{valid}\n");
    let resolver =
        MediaResolver::new(&operator, "runs/input.jsonl", MediaOptions::default()).unwrap();
    let mut reader = TrajectoryReader::new(
        Cursor::new(input),
        InputFormat::JsonLines,
        2,
        Some("harbor://trial/input.jsonl".into()),
    )
    .unwrap()
    .with_media(resolver);
    let error = reader.next_batch().await.unwrap_err();
    let ReadError::Media {
        source_uri,
        record_index,
        source_locator,
        source,
    } = error
    else {
        panic!("unexpected error: {error}");
    };
    assert_eq!(source_uri.as_deref(), Some("harbor://trial/input.jsonl"));
    assert_eq!(record_index, 2);
    assert_eq!(source_locator, "/steps/0/message/0");
    assert_eq!(
        source.downcast_ref::<opendal::Error>().unwrap().kind(),
        opendal::ErrorKind::NotFound
    );
    assert!(reader.next_batch().await.unwrap().is_none());
}

#[tokio::test]
async fn reference_only_mode_does_no_io_and_empty_bytes_are_distinct_from_null() {
    let operator = Operator::new(services::Memory::default()).unwrap();
    let value = document(json!([image("https://untrusted.example/missing.png")]));
    let references = MediaResolver::new(
        &operator,
        "input.json",
        MediaOptions {
            resolve_media: false,
            ..MediaOptions::default()
        },
    )
    .unwrap()
    .resolve_batch(native(&value, 7))
    .await
    .unwrap();
    let media = items(&references, 0);
    assert!(field::<LargeBinaryArray>(&media, "payload").is_null(0));
    assert!(field::<UInt64Array>(&media, "byte_size").is_null(0));
    assert!(field::<StringArray>(&media, "resolved_uri").is_null(0));
    assert_eq!(
        field::<StringArray>(&media, "resolution_status").value(0),
        "not_resolved"
    );
    operator.write("empty.png", Vec::<u8>::new()).await.unwrap();
    let empty = MediaResolver::new(&operator, "input.json", MediaOptions::default())
        .unwrap()
        .resolve_batch(native(&document(json!([image("empty.png")])), 1))
        .await
        .unwrap();
    let media = items(&empty, 0);
    assert!(!field::<LargeBinaryArray>(&media, "payload").is_null(0));
    assert_eq!(field::<LargeBinaryArray>(&media, "payload").value(0), b"");
    assert_eq!(field::<UInt64Array>(&media, "byte_size").value(0), 0);
    let text = MediaResolver::new(&operator, "input.json", MediaOptions::default())
        .unwrap()
        .resolve_batch(native(&document(json!("text only")), 1))
        .await
        .unwrap();
    let list = column::<ListArray>(&text, "media");
    assert!(!list.is_null(0));
    assert_eq!(list.value_length(0), 0);
}

#[tokio::test]
async fn enforces_per_file_trajectory_and_batch_limits_and_resets_between_trajectories() {
    let operator = Operator::new(services::Memory::default()).unwrap();
    operator.write("one.png", vec![1_u8; 6]).await.unwrap();
    let single = document(json!([image("one.png")]));
    let double = document(json!([image("one.png"), image("one.png")]));
    for (value, options) in [
        (
            &single,
            MediaOptions {
                max_media_bytes: 5,
                ..MediaOptions::default()
            },
        ),
        (
            &double,
            MediaOptions {
                max_trajectory_media_bytes: 11,
                ..MediaOptions::default()
            },
        ),
        (
            &double,
            MediaOptions {
                max_batch_media_bytes: 11,
                ..MediaOptions::default()
            },
        ),
        (
            &document(json!([image("data:image/png;base64,MTIzNDU2")])),
            MediaOptions {
                max_media_bytes: 5,
                ..MediaOptions::default()
            },
        ),
    ] {
        let error = MediaResolver::new(&operator, "input.json", options)
            .unwrap()
            .resolve_batch(native(value, 8))
            .await
            .unwrap_err();
        assert!(matches!(
            &error,
            ReadError::Media {
                record_index: 8,
                ..
            }
        ));
        assert!(error.to_string().contains("byte limit"), "{error}");
    }
    let input = format!("{single}\n{single}\n");
    for (batch_limit, succeeds) in [(12, true), (11, false)] {
        let resolver = MediaResolver::new(
            &operator,
            "input.jsonl",
            MediaOptions {
                max_media_bytes: 6,
                max_trajectory_media_bytes: 6,
                max_batch_media_bytes: batch_limit,
                ..MediaOptions::default()
            },
        )
        .unwrap();
        let mut reader =
            TrajectoryReader::new(Cursor::new(input.clone()), InputFormat::JsonLines, 2, None)
                .unwrap()
                .with_media(resolver);
        let result = reader.next_batch().await;
        assert_eq!(result.is_ok(), succeeds);
        if succeeds {
            assert_eq!(result.unwrap().unwrap().num_rows(), 2);
        }
        assert!(reader.next_batch().await.unwrap().is_none());
    }
}

#[tokio::test]
async fn binds_absolute_uris_to_one_operator_scope_and_rejects_escapes() {
    let operator = Operator::new(services::Memory::default()).unwrap();
    operator
        .write("runs/image #%.png", b"data".to_vec())
        .await
        .unwrap();
    let resolver = MediaResolver::new(&operator, "runs/input.json", MediaOptions::default())
        .unwrap()
        .with_uri_prefix("s3://bucket/root/")
        .unwrap();
    for path in [
        "s3://bucket/root/runs/image%20%23%25.png",
        "image #%.png",
        "../runs/image #%.png",
    ] {
        let batch = resolver
            .resolve_batch(native(&document(json!([image(path)])), 1))
            .await
            .unwrap();
        assert_eq!(
            field::<LargeBinaryArray>(&items(&batch, 0), "payload").value(0),
            b"data"
        );
    }
    for path in [
        "s3://other/root/runs/image.png",
        "https://bucket/root/runs/image.png",
        "s3://bucket/outside/image.png",
        "s3://bucket/root/%2e%2e/outside.png",
        "s3://user:secret@bucket/root/a.png",
        "s3://bucket/root/a.png?token=x",
        "s3://bucket/root/a.png#fragment",
        "../../outside.png",
        "a\\image.png",
        "s3://bucket/root/%00.png",
        "s3://bucket/root/%ZZ.png",
    ] {
        let error = resolver
            .resolve_batch(native(&document(json!([image(path)])), 2))
            .await
            .unwrap_err();
        assert!(matches!(error, ReadError::Media { .. }), "{path}: {error}");
    }
    let no_prefix = MediaResolver::new(&operator, "input.json", MediaOptions::default()).unwrap();
    let error = no_prefix
        .resolve_batch(native(
            &document(json!([image("http://example.com/a.png")])),
            1,
        ))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("with_uri_prefix"));
}

#[tokio::test]
async fn filesystem_absolute_paths_and_file_uris_stay_under_the_operator_root() {
    let directory = tempfile::tempdir().unwrap();
    let operator =
        Operator::new(services::Fs::default().root(directory.path().to_str().unwrap())).unwrap();
    operator.write("one.png", b"png".to_vec()).await.unwrap();
    let resolver = MediaResolver::new(&operator, "input.json", MediaOptions::default()).unwrap();
    let absolute = directory.path().join("one.png");
    for path in [
        absolute.to_str().unwrap().to_owned(),
        url::Url::from_file_path(&absolute).unwrap().to_string(),
    ] {
        let batch = resolver
            .resolve_batch(native(&document(json!([image(&path)])), 1))
            .await
            .unwrap();
        assert_eq!(
            field::<LargeBinaryArray>(&items(&batch, 0), "payload").value(0),
            b"png"
        );
    }
    let outside = tempfile::NamedTempFile::new().unwrap();
    let error = resolver
        .resolve_batch(native(
            &document(json!([image(outside.path().to_str().unwrap())])),
            1,
        ))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("storage root"));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outside.path(), directory.path().join("outside.png")).unwrap();
        let error = resolver
            .resolve_batch(native(&document(json!([image("outside.png")])), 1))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("storage root"));
    }
}

#[tokio::test]
async fn decodes_percent_and_base64_data_uris_and_rejects_malformed_input() {
    let operator = Operator::new(services::Memory::default()).unwrap();
    let resolver = MediaResolver::new(&operator, "input.json", MediaOptions::default()).unwrap();
    for (path, expected) in [
        ("data:image/png,%00%ff+", &[0, 255, b'+'][..]),
        ("data:image/png;base64,AP8r", &[0, 255, b'+'][..]),
        ("data:image/png;base64,AP8%3D", &[0, 255][..]),
    ] {
        let result = resolver
            .resolve_batch(native(&document(json!([image(path)])), 1))
            .await;
        assert_eq!(
            field::<LargeBinaryArray>(&items(&result.unwrap(), 0), "payload").value(0),
            expected
        );
    }
    for path in [
        "data:image/png",
        "data:image/png,%",
        "data:image/png,%GG",
        "data:image/png;base64,????",
    ] {
        assert!(resolver
            .resolve_batch(native(&document(json!([image(path)])), 1))
            .await
            .is_err());
    }
}

#[cfg(feature = "gzip")]
#[tokio::test]
async fn reads_gzipped_jsonl_with_media_and_preserves_document_order() {
    use std::io::Write;
    let operator = Operator::new(services::Memory::default()).unwrap();
    operator
        .write("runs/one.png", b"png".to_vec())
        .await
        .unwrap();
    let first = document(json!([image("one.png")]));
    let second = document(json!("text"));
    let input = format!("{first}\n\n{second}\n");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(input.as_bytes()).unwrap();
    operator
        .write("runs/input.jsonl.gz", encoder.finish().unwrap())
        .await
        .unwrap();
    let resolver =
        MediaResolver::new(&operator, "runs/input.jsonl.gz", MediaOptions::default()).unwrap();
    let batches = TrajectoryReader::open_gzip(
        &operator,
        "runs/input.jsonl.gz",
        InputFormat::JsonLines,
        1,
        None,
    )
    .await
    .unwrap()
    .with_media(resolver)
    .into_stream()
    .try_collect::<Vec<_>>()
    .await
    .unwrap();
    assert_eq!(batches.len(), 2);
    for (index, batch) in batches.iter().enumerate() {
        assert_eq!(batch.schema(), trajectory_schema_with_media());
        assert_eq!(
            column::<UInt64Array>(batch, "source_record_index").value(0),
            index as u64 + 1
        );
        assert_eq!(items(batch, 0).len(), usize::from(index == 0));
    }
}

#[tokio::test]
async fn configuration_and_input_schema_fail_early_without_io() {
    let operator = Operator::new(services::Memory::default()).unwrap();
    for options in [
        MediaOptions {
            max_media_bytes: 0,
            ..MediaOptions::default()
        },
        MediaOptions {
            max_trajectory_media_bytes: 0,
            ..MediaOptions::default()
        },
        MediaOptions {
            max_batch_media_bytes: 0,
            ..MediaOptions::default()
        },
    ] {
        assert!(matches!(
            MediaResolver::new(&operator, "input.json", options),
            Err(ReadError::InvalidMediaOptions(_))
        ));
    }
    for path in [
        "",
        "../input.json",
        "https://example.com/input.json",
        "folder/",
    ] {
        assert!(MediaResolver::new(&operator, path, MediaOptions::default()).is_err());
    }
    for prefix in [
        "not a URI",
        "https://example.com/root",
        "https://user:pass@example.com/",
        "https://example.com/?key=x",
    ] {
        assert!(
            MediaResolver::new(&operator, "input.json", MediaOptions::default())
                .unwrap()
                .with_uri_prefix(prefix)
                .is_err()
        );
    }
    let resolver = MediaResolver::new(&operator, "input.json", MediaOptions::default()).unwrap();
    let batch = native(&document(json!("text")), 1);
    assert_eq!(batch.schema(), trajectory_schema());
    assert!(resolver
        .resolve_batch(batch.project(&[0]).unwrap())
        .await
        .is_err());
}
