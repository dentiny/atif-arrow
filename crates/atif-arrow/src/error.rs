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

    pub(crate) fn with_prefix(mut self, prefix: &str) -> Self {
        if !prefix.is_empty() {
            self.path = if self.path == "$" {
                prefix.into()
            } else {
                format!("{prefix}.{}", self.path)
            };
        }
        self
    }
}

impl fmt::Display for ParseError {
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Field(error) => error.fmt(f),
            Self::Arrow(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ConversionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Field(error) => error,
            Self::Arrow(error) => error,
        })
    }
}

impl From<arrow_schema::ArrowError> for ConversionError {
    fn from(error: arrow_schema::ArrowError) -> Self {
        Self::Arrow(error)
    }
}
