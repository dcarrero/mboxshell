//! Centralized error types for mboxShell.

use std::path::PathBuf;
use thiserror::Error;

/// All errors produced by the mboxShell library.
#[derive(Error, Debug)]
pub enum MboxError {
    /// I/O error with the associated file path.
    #[error("I/O error reading '{path}': {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    /// The specified file does not exist.
    #[error("MBOX file not found: {0}")]
    FileNotFound(PathBuf),

    /// The file does not appear to be a valid MBOX.
    #[error("File does not appear to be a valid MBOX: {0}")]
    InvalidMbox(PathBuf),

    /// The index file is corrupt or was built with an incompatible version.
    #[error("Corrupt or incompatible index for '{path}': {reason}")]
    InvalidIndex { path: PathBuf, reason: String },

    /// A parsing error occurred at a specific byte offset.
    #[error("Parse error at offset {offset}: {reason}")]
    ParseError { offset: u64, reason: String },

    /// The character encoding is not supported.
    #[error("Unsupported encoding: {0}")]
    UnsupportedEncoding(String),

    /// A MIME decoding error.
    #[error("MIME decoding error: {0}")]
    MimeError(String),

    /// The user cancelled the operation.
    #[error("Operation cancelled by user")]
    Cancelled,

    /// The MBOX file has changed since the index was built.
    #[error("File has changed since last indexing")]
    FileModified,

    /// An export operation failed.
    #[error("Export error: {0}")]
    ExportError(String),

    /// An invalid path was provided.
    #[error("Invalid path: {0}")]
    InvalidPath(String),
}

/// Convenience alias for `Result<T, MboxError>`.
pub type Result<T> = std::result::Result<T, MboxError>;

/// Helper to convert a bare `std::io::Error` together with a path.
impl MboxError {
    /// Create an `Io` variant from a path and an `io::Error`.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Allow `?` on `std::io::Error` inside functions returning `MboxError`
/// when no path context is available (rare — prefer `MboxError::io`).
impl From<std::io::Error> for MboxError {
    fn from(source: std::io::Error) -> Self {
        Self::Io {
            path: PathBuf::from("<unknown>"),
            source,
        }
    }
}
