mod error;
#[cfg(feature = "gzip")]
mod gzip;
mod media;
mod media_reader;
mod reader;
mod storage;

pub use error::ReadError;
#[cfg(feature = "gzip")]
pub use gzip::GzipReader;
pub use media::{MediaOptions, MediaResolver};
pub use media_reader::MediaReader;
pub use reader::{InputFormat, TrajectoryReader};
