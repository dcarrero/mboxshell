//! Centralized error types for mboxShell.

use std::path::PathBuf;
use thiserror::Error;

/// All errors produced by the mboxShell library.
#[derive(Error, Debug)]
pub enum MboxError {
    /// I/O error with the associated file path.
    ///
    /// The OS error is part of the message rather than a `#[source]`: as a
    /// source, `anyhow` printed it a second time under "Caused by:".
    #[error("{}: '{}': {err}", crate::i18n::err_io(), .path.display())]
    Io { path: PathBuf, err: std::io::Error },

    /// The specified file does not exist.
    #[error("{}: {}", crate::i18n::err_file_not_found(), .0.display())]
    FileNotFound(PathBuf),

    /// The file does not appear to be a valid MBOX.
    #[error("{}: {}", crate::i18n::err_not_mbox(), .0.display())]
    InvalidMbox(PathBuf),

    /// A search query holds filters that could not be understood (e.g. a
    /// malformed date). Running it anyway would silently widen the results.
    #[error("{}: {}", crate::i18n::err_invalid_query(), .0.join(" "))]
    InvalidQuery(Vec<String>),

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
            err: source,
        }
    }
}

/// Allow `?` on `std::io::Error` inside functions returning `MboxError`
/// when no path context is available (rare — prefer `MboxError::io`).
impl From<std::io::Error> for MboxError {
    fn from(source: std::io::Error) -> Self {
        Self::Io {
            path: PathBuf::from("<unknown>"),
            err: source,
        }
    }
}
