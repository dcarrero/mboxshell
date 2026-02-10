//! Parser for individual `.eml` files (RFC 5322 messages without MBOX framing).

use std::path::Path;

use crate::error::{MboxError, Result};
use crate::model::mail::MailEntry;
use crate::parser::header;

/// Parse a single `.eml` file and return its [`MailEntry`].
///
/// An EML file is a bare RFC 5322 message (no `From ` separator).
pub fn parse_eml(path: impl AsRef<Path>, sequence: u64) -> Result<MailEntry> {
    let path = path.as_ref();
    let data = std::fs::read(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            MboxError::FileNotFound(path.to_path_buf())
        } else {
            MboxError::io(path, e)
        }
    })?;

    let length = data.len() as u64;

    // Find end of headers (first blank line)
    let header_end = find_header_end(&data).unwrap_or(data.len());
    let header_bytes = &data[..header_end];

    header::parse_headers_to_entry(header_bytes, 0, length, sequence)
}

/// Find the byte offset where headers end (position of the first blank line).
fn find_header_end(data: &[u8]) -> Option<usize> {
    // Look for \n\n or \r\n\r\n
    for i in 0..data.len().saturating_sub(1) {
        if data[i] == b'\n' && data[i + 1] == b'\n' {
            return Some(i);
        }
        if i + 3 < data.len()
            && data[i] == b'\r'
            && data[i + 1] == b'\n'
            && data[i + 2] == b'\r'
            && data[i + 3] == b'\n'
        {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_header_end() {
        // "From: a@b.com\n" = 14 bytes, "Subject: Hi\n" = 12 bytes
        // The \n\n starts at offset 25 (the \n ending "Subject: Hi")
        let data = b"From: a@b.com\nSubject: Hi\n\nBody\n";
        assert_eq!(find_header_end(data), Some(25));
    }

    #[test]
    fn test_find_header_end_crlf() {
        // "From: a@b.com\r\n" = 15 bytes, "Subject: Hi\r\n" = 13 bytes
        // The \r\n\r\n starts at offset 26
        let data = b"From: a@b.com\r\nSubject: Hi\r\n\r\nBody\r\n";
        assert_eq!(find_header_end(data), Some(26));
    }
}
