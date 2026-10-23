use arrow_array::{Array, ListArray, StructArray, UInt64Array};
use atif_io::{InputFormat, MediaOptions, MediaResolver, TrajectoryReader};
use opendal::{services, Operator};

/// Run with: cargo run -p atif-io --example multimodal -- /root runs/trajectory.json
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = args
        .next()
        .ok_or("usage: multimodal <storage-root> <trajectory.json>")?;
    let path = args
        .next()
        .ok_or("usage: multimodal <storage-root> <trajectory.json>")?;
    opendal::install_default();
    let operator = Operator::new(services::Fs::default().root(&root))?;
    let resolver = MediaResolver::new(&operator, &path, MediaOptions::default())?;
    let mut reader = TrajectoryReader::open(&operator, &path, InputFormat::Json, 1, None)
        .await?
        .with_media(resolver);
    while let Some(batch) = reader.next_batch().await? {
        let list = batch
            .column_by_name("media")
            .unwrap()
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap();
        for row in 0..batch.num_rows() {
            let value = list.value(row);
            let media = value.as_any().downcast_ref::<StructArray>().unwrap();
            let sizes = media
                .column_by_name("byte_size")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap();
            println!(
                "{} columns, {} media, {} payload bytes",
                batch.num_columns(),
                media.len(),
                sizes.values().iter().sum::<u64>()
            );
        }
    }
    Ok(())
}
