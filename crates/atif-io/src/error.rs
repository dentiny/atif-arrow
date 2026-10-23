use std::fmt;

pub(crate) type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// A reader configuration error or a record failure with source provenance.
#[derive(Debug)]
pub enum ReadError {
    InvalidBatchSize,
    InvalidMediaOptions(String),
    Media {
        source_uri: Option<String>,
        record_index: u64,
        source_locator: String,
        source: BoxError,
    },
    Open {
        source_uri: String,
        source: opendal::Error,
    },
    Record {
        source_uri: Option<String>,
        record_index: u64,
        source: BoxError,
    },
}

impl ReadError {
    pub(crate) fn record(source_uri: &Option<String>, record_index: u64, source: BoxError) -> Self {
        Self::Record {
            source_uri: source_uri.clone(),
            record_index,
            source,
        }
    }
}

impl fmt::Display for ReadError {
    /// Displays the source, document index, and underlying diagnostic.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBatchSize => write!(f, "batch size must be greater than zero"),
            Self::InvalidMediaOptions(message) => {
                write!(f, "invalid media configuration: {message}")
            }
            Self::Media {
                source_uri,
                record_index,
                source_locator,
                source,
            } => write!(
                f,
                "{}: record {record_index}: media {source_locator}: {source}",
                source_uri.as_deref().unwrap_or("<input>")
            ),
            Self::Open { source_uri, source } => {
                write!(f, "{source_uri}: failed to open source: {source}")
            }
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
            Self::InvalidBatchSize | Self::InvalidMediaOptions(_) => None,
            Self::Media { source, .. } => Some(source.as_ref()),
            Self::Open { source, .. } => Some(source),
            Self::Record { source, .. } => Some(source.as_ref()),
        }
    }
}
