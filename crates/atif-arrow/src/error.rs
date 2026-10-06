use std::fmt;

/// A JSON decoding or core ATIF validation error with its field location.
#[derive(Debug)]
pub struct ParseError {
    pub path: String,
    pub message: String,
}

impl ParseError {
    /// Attaches a diagnostic to its field path, using `$` for the document root.
    pub(crate) fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            path: if path.is_empty() || path == "." || path == "?" {
                "$".into()
            } else {
                path
            },
            message: message.into(),
        }
    }
}

impl fmt::Display for ParseError {
    /// Displays both the field location and the reason parsing failed.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for ParseError {}

/// A field mapping failure or an Arrow array construction error.
#[derive(Debug)]
pub enum ConversionError {
    Field(ParseError),
    Arrow(arrow_schema::ArrowError),
}

impl fmt::Display for ConversionError {
    /// Displays the input location or Arrow's construction diagnostic.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Field(error) => error.fmt(f),
            Self::Arrow(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ConversionError {
    /// Exposes the underlying parsing or Arrow error.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Field(error) => error,
            Self::Arrow(error) => error,
        })
    }
}

impl From<arrow_schema::ArrowError> for ConversionError {
    /// Propagates Arrow construction failures without losing their diagnostics.
    fn from(error: arrow_schema::ArrowError) -> Self {
        Self::Arrow(error)
    }
}
