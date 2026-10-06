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
