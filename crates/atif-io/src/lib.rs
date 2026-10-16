mod error;
#[cfg(feature = "gzip")]
mod gzip;
mod reader;
mod storage;

pub use error::ReadError;
#[cfg(feature = "gzip")]
pub use gzip::GzipReader;
pub use reader::{InputFormat, TrajectoryReader};
