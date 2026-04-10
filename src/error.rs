use std::fmt;
use std::io;

use rusqlite::Error as SqliteError;

/// Crate-wide result type.
pub type Result<T> = std::result::Result<T, ClipError>;

/// Errors returned while reading or parsing a `.clip` file.
#[derive(Debug)]
pub enum ClipError {
    /// Wraps I/O failures from the underlying reader or filesystem.
    Io(io::Error),
    /// Wraps SQLite failures while reading the embedded database.
    Sqlite(SqliteError),
    /// The input does not match the expected `.clip` file structure.
    InvalidFormat(&'static str),
    /// The file looks valid, but no preview payload was found.
    PreviewNotFound,
    /// The file uses a variation this crate does not support yet.
    Unsupported(&'static str),
}

impl fmt::Display for ClipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(source) => write!(f, "I/O error: {source}"),
            Self::Sqlite(source) => write!(f, "SQLite error: {source}"),
            Self::InvalidFormat(message) => write!(f, "invalid .clip file: {message}"),
            Self::PreviewNotFound => write!(f, "preview image not found"),
            Self::Unsupported(message) => write!(f, "unsupported .clip file: {message}"),
        }
    }
}

impl std::error::Error for ClipError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(source) => Some(source),
            Self::Sqlite(source) => Some(source),
            Self::InvalidFormat(_) | Self::PreviewNotFound | Self::Unsupported(_) => None,
        }
    }
}

impl From<io::Error> for ClipError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<SqliteError> for ClipError {
    fn from(value: SqliteError) -> Self {
        Self::Sqlite(value)
    }
}
