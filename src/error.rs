use std::fmt;

/// A problem a caller can fix by changing a note, an id, or the store path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteError {
    pub message: String,
}

impl NoteError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for NoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for NoteError {}

pub type NoteResult<T> = Result<T, NoteError>;
