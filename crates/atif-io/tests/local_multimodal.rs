use std::{fs, io::Write};

use arrow_array::{Array, LargeBinaryArray, ListArray, StringArray, StructArray, UInt64Array};
use atif_arrow::trajectory_schema_with_media;
use atif_io::{InputFormat, MediaOptions, MediaResolver, TrajectoryReader};
use flate2::{write::ZlibEncoder, Compression};
use opendal::{services, Operator};
use serde_json::{json, Value};

/// Generate an RGB PNG with filter-0 scanlines and valid chunk CRCs. Uses the
/// standard PNG layout: https://www.w3.org/TR/png/#5DataRep
fn generate_png() -> Vec<u8> {
    let (width, height) = (64_u32, 48_u32);
    let mut pixels = Vec::new();
    for y in 0..height {
        pixels.push(0); // No filter for this scanline.
        for x in 0..width {
            pixels.extend_from_slice(&[(x * 4) as u8, (y * 5) as u8, ((x + y) * 2) as u8]);
        }
    }
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&pixels).unwrap();

    fn chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        output.extend_from_slice(&(data.len() as u32).to_be_bytes());
        output.extend_from_slice(kind);
        output.extend_from_slice(data);
        let mut crc = crc32fast::Hasher::new();
        crc.update(kind);
        crc.update(data);
        output.extend_from_slice(&crc.finalize().to_be_bytes());
    }

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB, deflate, no interlacing.
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"IDAT", &encoder.finish().unwrap());
    chunk(&mut png, b"IEND", &[]);
    png
}

/// Generate 125 ms of 440 Hz mono audio as a 16-bit PCM RIFF/WAVE file.
fn generate_wav() -> Vec<u8> {
    let sample_rate = 8_000_u32;
    let samples = 1_000_u32;
    let data_size = samples * 2;
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes()); // PCM format chunk size.
    wav.extend_from_slice(&1_u16.to_le_bytes()); // PCM format tag.
    wav.extend_from_slice(&1_u16.to_le_bytes()); // One channel.
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // Bytes per second.
    wav.extend_from_slice(&2_u16.to_le_bytes()); // Bytes per frame.
    wav.extend_from_slice(&16_u16.to_le_bytes()); // Bits per sample.
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    for index in 0..samples {
        let phase = std::f64::consts::TAU * 440.0 * f64::from(index) / f64::from(sample_rate);
        wav.extend_from_slice(&((phase.sin() * 12_000.0) as i16).to_le_bytes());
    }
    wav
}

#[tokio::test]
async fn reads_locally_generated_atif_with_real_png_and_wav_into_owned_arrow_payloads() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_path_buf();
    let media_dir = root.join("trial/agent/media");
    fs::create_dir_all(&media_dir).unwrap();
    let png = generate_png();
    let wav = generate_wav();
    fs::write(media_dir.join("image.png"), &png).unwrap();
    fs::write(media_dir.join("audio.wav"), &wav).unwrap();

    let trajectory = json!({
        "schema_version": "ATIF-v1.8",
        "trajectory_id": "local-multimodal-test",
        "agent": {"name": "fixture-agent", "version": "1.0"},
        "steps": [
            {
                "step_id": 1,
                "source": "user",
                "message": [
                    {"type": "text", "text": "Describe this image and audio clip."},
                    {"type": "image", "source": {"media_type": "image/png", "path": "media/image.png"}},
                    {"type": "audio", "source": {"media_type": "audio/wav", "path": "media/audio.wav", "duration_sec": 0.125}}
                ]
            },
            {"step_id": 2, "source": "agent", "message": "A color gradient and a short tone."}
        ]
    });
    let raw_json = format!("{}\n", serde_json::to_string_pretty(&trajectory).unwrap());
    let path = "trial/agent/trajectory.json";
    fs::write(root.join(path), &raw_json).unwrap();

    // Read files created by the producer directly, without a mock backend or data URI.
    let operator = Operator::new(services::Fs::default().root(root.to_str().unwrap())).unwrap();
    let resolver = MediaResolver::new(&operator, path, MediaOptions::default()).unwrap();
    let mut reader = TrajectoryReader::open(&operator, path, InputFormat::Json, 1, None)
        .await
        .unwrap()
        .with_media(resolver);
    let batch = reader.next_batch().await.unwrap().unwrap();
    assert_eq!(batch.schema(), trajectory_schema_with_media());
    assert_eq!(batch.num_rows(), 1);
    assert_eq!(batch.num_columns(), 14);
    let raw = batch
        .column_by_name("raw_json")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    assert_eq!(raw.value(0), raw_json);
    let source = batch
        .column_by_name("source_uri")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    assert_eq!(source.value(0), path);
    let index = batch
        .column_by_name("source_record_index")
        .unwrap()
        .as_any()
        .downcast_ref::<UInt64Array>()
        .unwrap();
    assert_eq!(index.value(0), 1);

    let list = batch
        .column_by_name("media")
        .unwrap()
        .as_any()
        .downcast_ref::<ListArray>()
        .unwrap();
    assert!(!list.is_null(0));
    let value = list.value(0);
    let media = value.as_any().downcast_ref::<StructArray>().unwrap();
    let strings = |name| {
        media
            .column_by_name(name)
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
    };
    let payloads = media
        .column_by_name("payload")
        .unwrap()
        .as_any()
        .downcast_ref::<LargeBinaryArray>()
        .unwrap();
    let sizes = media
        .column_by_name("byte_size")
        .unwrap()
        .as_any()
        .downcast_ref::<UInt64Array>()
        .unwrap();
    assert_eq!(media.len(), 2);
    for (row, (path, mime, locator, expected)) in [
        (
            "media/image.png",
            "image/png",
            "/steps/0/message/1",
            png.as_slice(),
        ),
        (
            "media/audio.wav",
            "audio/wav",
            "/steps/0/message/2",
            wav.as_slice(),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(!payloads.is_null(row));
        assert_eq!(payloads.value(row), expected);
        assert_eq!(sizes.value(row), expected.len() as u64);
        assert_eq!(strings("path").value(row), path);
        assert_eq!(strings("media_type").value(row), mime);
        assert_eq!(strings("source_locator").value(row), locator);
        assert_eq!(
            strings("resolved_uri").value(row),
            format!("trial/agent/{path}")
        );
        assert_eq!(strings("resolution_status").value(row), "embedded");
        let part: &Value = trajectory.pointer(locator).unwrap();
        assert_eq!(part["source"]["path"], path);
        assert_eq!(part["source"]["media_type"], mime);
    }
    assert!(reader.next_batch().await.unwrap().is_none());

    // The Arrow values own their bytes independently of the original files.
    drop(reader);
    drop(operator);
    directory.close().unwrap();
    assert!(!root.exists());
    assert_eq!(payloads.value(0), png.as_slice());
    assert_eq!(payloads.value(1), wav.as_slice());
}
