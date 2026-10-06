use arrow_array::{cast::AsArray, types::UInt64Type};
use atif_arrow::trajectory_schema;
use atif_io::{InputFormat, ReadError, TrajectoryReader};
use futures::TryStreamExt;
use opendal::{services, ErrorKind, Operator};

const DOCUMENT: &str = r#"{"schema_version":"ATIF-v1.8","agent":{"name":"test","version":"1"},"steps":[{"step_id":1,"source":"user","message":"hello"}]}"#;

#[tokio::test]
async fn reads_memory_and_filesystem_objects_in_batches() {
    let directory = tempfile::tempdir().unwrap();
    let memory = Operator::new(services::Memory::default()).unwrap();
    let filesystem =
        Operator::new(services::Fs::default().root(directory.path().to_str().unwrap())).unwrap();
    let line = format!("{DOCUMENT}\r\n");
    let jsonl = format!("\n{line} \t\n{line}{DOCUMENT}");
    let pretty =
        serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(DOCUMENT).unwrap())
            .unwrap();
    for operator in [memory, filesystem] {
        for (path, input, format, expected_raw, expected_sizes) in [
            (
                "trajectory.json",
                pretty.as_str(),
                InputFormat::Json,
                vec![pretty.as_str()],
                vec![1],
            ),
            (
                "trajectories.jsonl",
                jsonl.as_str(),
                InputFormat::JsonLines,
                vec![line.as_str(), line.as_str(), DOCUMENT],
                vec![2, 1],
            ),
        ] {
            operator.write(path, input.to_owned()).await.unwrap();
            let batches = TrajectoryReader::open(&operator, path, format, 2, None)
                .await
                .unwrap()
                .into_stream()
                .try_collect::<Vec<_>>()
                .await
                .unwrap();
            assert_eq!(
                batches.iter().map(|b| b.num_rows()).collect::<Vec<_>>(),
                expected_sizes
            );
            let mut index = 0;
            for batch in batches {
                assert_eq!(batch.schema(), trajectory_schema());
                let raw = batch.column_by_name("raw_json").unwrap().as_string::<i32>();
                let indices = batch
                    .column_by_name("source_record_index")
                    .unwrap()
                    .as_primitive::<UInt64Type>();
                let sources = batch
                    .column_by_name("source_uri")
                    .unwrap()
                    .as_string::<i32>();
                for row in 0..batch.num_rows() {
                    assert_eq!(raw.value(row), expected_raw[index]);
                    assert_eq!(indices.value(row), index as u64 + 1);
                    assert_eq!(sources.value(row), path);
                    index += 1;
                }
            }
        }
    }
}

#[tokio::test]
async fn preserves_storage_errors_and_custom_source_identity() {
    let operator = Operator::new(services::Memory::default()).unwrap();
    let uri = "memory://test/missing.json";
    let mut reader = TrajectoryReader::open(
        &operator,
        "missing.json",
        InputFormat::Json,
        1,
        Some(uri.into()),
    )
    .await
    .unwrap();
    let error = reader.next_batch().await.unwrap_err();
    match error {
        ReadError::Record {
            source_uri: Some(source_uri),
            record_index: 1,
            source,
        } => {
            assert_eq!(source_uri, uri);
            let error = source.downcast_ref::<std::io::Error>().unwrap();
            let storage = error
                .get_ref()
                .unwrap()
                .downcast_ref::<opendal::Error>()
                .unwrap();
            assert_eq!(storage.kind(), ErrorKind::NotFound);
        }
        error => panic!("unexpected missing-object error: {error}"),
    }
    assert!(reader.next_batch().await.unwrap().is_none());
    assert!(matches!(
        TrajectoryReader::open(&operator, "folder/", InputFormat::Json, 1, None).await,
        Err(ReadError::Open { source, .. }) if source.kind() == ErrorKind::IsADirectory
    ));
    assert!(matches!(
        TrajectoryReader::open(&operator, "missing.json", InputFormat::Json, 0, None).await,
        Err(ReadError::InvalidBatchSize)
    ));

    operator
        .write("bad.jsonl", format!("{DOCUMENT}\n{{\n"))
        .await
        .unwrap();
    let uri = "memory://test/bad.jsonl";
    let mut reader = TrajectoryReader::open(
        &operator,
        "bad.jsonl",
        InputFormat::JsonLines,
        1,
        Some(uri.into()),
    )
    .await
    .unwrap();
    let batch = reader.next_batch().await.unwrap().unwrap();
    let sources = batch
        .column_by_name("source_uri")
        .unwrap()
        .as_string::<i32>();
    assert_eq!(sources.value(0), uri);
    let error = reader.next_batch().await.unwrap_err();
    assert!(
        matches!(&error, ReadError::Record { source_uri: Some(source), record_index: 2, .. } if source == uri)
    );
    assert!(error.to_string().contains(uri));
    assert!(reader.next_batch().await.unwrap().is_none());
}
