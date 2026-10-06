use std::fmt;

/// A reader configuration error or a record failure with source provenance.
#[derive(Debug)]
pub enum ReadError {
    InvalidBatchSize,
    Record {
        source_uri: Option<String>,
        record_index: u64,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl fmt::Display for ReadError {
    /// Displays the source, document index, and underlying diagnostic.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBatchSize => write!(f, "batch size must be greater than zero"),
            Self::Record {
                source_uri,
                record_index,
                source,
            } => write!(
                f,
                "{}: record {record_index}: {source}",
                source_uri.as_deref().unwrap_or("<input>")
            ),
        }
    }
}

impl std::error::Error for ReadError {
    /// Exposes the underlying I/O, parsing, or conversion error.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidBatchSize => None,
            Self::Record { source, .. } => Some(source.as_ref()),
        }
    }
}
