//! Streaming MBOX parser.
//!
//! Reads MBOX files line-by-line with a 128 KB buffer.
//! Never loads the entire file into memory. Tolerant of malformed input.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use tracing::warn;

use crate::error::{MboxError, Result};

/// Size of the internal read buffer (1 MB for fast sequential reads on modern SSDs).
const READ_BUFFER_SIZE: usize = 1024 * 1024;

/// Default maximum message size in bytes (256 MB).
const MAX_MESSAGE_SIZE: usize = 256 * 1024 * 1024;

/// Streaming MBOX parser.
///
/// Reads through the file sequentially, invoking a caller-supplied callback for
/// every message boundary it finds. The parser is tolerant of:
///
/// - Mixed `\n` and `\r\n` line endings
/// - `From ` lines not preceded by a blank line (logs a warning)
/// - Truncated messages at EOF
/// - NUL bytes and other binary content in the body
/// - UTF-8 BOM at the start of the file
pub struct MboxParser {
    path: PathBuf,
    file_size: u64,
    max_message_size: usize,
}

impl MboxParser {
    /// Create a parser for the given MBOX file.
    ///
    /// Verifies that the file exists and is readable, but does NOT validate
    /// that it is actually an MBOX.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let metadata = std::fs::metadata(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                MboxError::FileNotFound(path.clone())
            } else {
                MboxError::io(&path, e)
            }
        })?;
        Ok(Self {
            path,
            file_size: metadata.len(),
            max_message_size: MAX_MESSAGE_SIZE,
        })
    }

    /// Total size of the underlying file in bytes.
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Path to the MBOX file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Parse the full MBOX, calling `message_callback` for each message found.
    ///
    /// The callback receives `(offset, raw_bytes)` and returns `true` to
    /// continue or `false` to abort early.
    ///
    /// Returns the number of messages found.
    pub fn parse(
        &self,
        message_callback: &mut dyn FnMut(u64, &[u8]) -> bool,
        progress_callback: Option<&dyn Fn(u64, u64)>,
    ) -> Result<u64> {
        if self.file_size == 0 {
            return Ok(0);
        }

        let file = File::open(&self.path).map_err(|e| MboxError::io(&self.path, e))?;
        let mut reader = BufReader::with_capacity(READ_BUFFER_SIZE, file);

        let mut count: u64 = 0;
        let mut current_offset: u64 = 0;
        let mut message_buf: Vec<u8> = Vec::with_capacity(64 * 1024);
        let mut message_start: u64 = 0;
        let mut bytes_read: u64 = 0;
        let mut prev_line_was_empty = true;
        let mut first_line = true;
        let mut last_progress: u64 = 0;

        // Reusable line buffer
        let mut line_buf: Vec<u8> = Vec::with_capacity(4096);
        const PROGRESS_INTERVAL: u64 = 4 * 1024 * 1024;

        loop {
            line_buf.clear();
            let line_len = {
                let buf = reader
                    .fill_buf()
                    .map_err(|e| MboxError::io(&self.path, e))?;
                if buf.is_empty() {
                    break; // EOF
                }
                let newline_pos = memchr_newline(buf);
                let consume_len = match newline_pos {
                    Some(pos) => pos + 1,
                    None => buf.len(),
                };
                line_buf.extend_from_slice(&buf[..consume_len]);
                reader.consume(consume_len);
                consume_len as u64
            };

            let is_from_line = is_mbox_separator(&line_buf);

            if is_from_line && (first_line || prev_line_was_empty) {
                if !message_buf.is_empty() {
                    if !message_callback(message_start, &message_buf) {
                        return Ok(count);
                    }
                    count += 1;
                }
                message_start = current_offset;
                message_buf.clear();
                message_buf.extend_from_slice(&line_buf);
            } else if is_from_line && !prev_line_was_empty && !first_line {
                warn!(
                    offset = current_offset,
                    "Found 'From ' separator without preceding blank line"
                );
                if !message_buf.is_empty() {
                    if !message_callback(message_start, &message_buf) {
                        return Ok(count);
                    }
                    count += 1;
                }
                message_start = current_offset;
                message_buf.clear();
                message_buf.extend_from_slice(&line_buf);
            } else if message_buf.len() + line_buf.len() <= self.max_message_size {
                message_buf.extend_from_slice(&line_buf);
            } else if message_buf.len() <= self.max_message_size {
                // First time exceeding the limit — log a warning once per message
                warn!(
                    offset = message_start,
                    max_size = self.max_message_size,
                    "Message exceeds maximum size, truncating body"
                );
            }

            prev_line_was_empty = is_blank_line(&line_buf);
            first_line = false;
            current_offset += line_len;
            bytes_read += line_len;

            if let Some(cb) = progress_callback {
                if bytes_read - last_progress >= PROGRESS_INTERVAL {
                    cb(bytes_read, self.file_size);
                    last_progress = bytes_read;
                }
            }
        }

        // Flush last message
        if !message_buf.is_empty() && message_callback(message_start, &message_buf) {
            count += 1;
        }

        if let Some(cb) = progress_callback {
            cb(self.file_size, self.file_size);
        }

        Ok(count)
    }

    /// Parse only the headers of each message (faster than full parsing).
    ///
    /// The callback receives `(offset, message_length, header_bytes)`.
    /// `message_length` includes both headers and body.
    ///
    /// Optimized for very large files (50 GB+): uses a reusable line buffer
    /// to minimize allocations in the hot loop, and reports progress every 4 MB.
    pub fn parse_headers_only(
        &self,
        header_callback: &mut dyn FnMut(u64, u64, &[u8]) -> bool,
        progress_callback: Option<&dyn Fn(u64, u64)>,
    ) -> Result<u64> {
        if self.file_size == 0 {
            return Ok(0);
        }

        let file = File::open(&self.path).map_err(|e| MboxError::io(&self.path, e))?;
        let mut reader = BufReader::with_capacity(READ_BUFFER_SIZE, file);

        let mut count: u64 = 0;
        let mut current_offset: u64 = 0;
        let mut header_buf: Vec<u8> = Vec::with_capacity(16 * 1024);
        let mut in_headers = false;
        let mut prev_line_was_empty = true;
        let mut first_line = true;
        let mut bytes_read: u64 = 0;
        let mut last_progress: u64 = 0;
        let mut prev_message_start: Option<u64> = None;
        let mut prev_headers: Option<Vec<u8>> = None;

        // Reusable line buffer — avoids allocation per line
        let mut line_buf: Vec<u8> = Vec::with_capacity(4096);

        // Progress every 4 MB (less overhead on large files)
        const PROGRESS_INTERVAL: u64 = 4 * 1024 * 1024;

        loop {
            // Read a line into the reusable buffer (zero-alloc in the common case)
            line_buf.clear();
            let line_len = {
                let buf = reader
                    .fill_buf()
                    .map_err(|e| MboxError::io(&self.path, e))?;
                if buf.is_empty() {
                    break; // EOF
                }
                let newline_pos = memchr_newline(buf);
                let consume_len = match newline_pos {
                    Some(pos) => pos + 1,
                    None => buf.len(),
                };
                line_buf.extend_from_slice(&buf[..consume_len]);
                reader.consume(consume_len);
                consume_len as u64
            };

            let is_from_line = is_mbox_separator(&line_buf);

            if is_from_line {
                if !first_line && !prev_line_was_empty {
                    warn!(
                        offset = current_offset,
                        "Found 'From ' separator without preceding blank line"
                    );
                }

                // Emit the *previous* message
                if let (Some(pstart), Some(pheaders)) = (prev_message_start, prev_headers.take()) {
                    let msg_length = current_offset - pstart;
                    if !header_callback(pstart, msg_length, &pheaders) {
                        return Ok(count);
                    }
                    count += 1;
                }

                header_buf.clear();
                header_buf.extend_from_slice(&line_buf);
                in_headers = true;
                prev_message_start = Some(current_offset);
            } else if in_headers {
                if is_blank_line(&line_buf) {
                    // End of headers — save without cloning (swap trick)
                    in_headers = false;
                    let mut saved = Vec::with_capacity(header_buf.len());
                    std::mem::swap(&mut saved, &mut header_buf);
                    prev_headers = Some(saved);
                } else {
                    header_buf.extend_from_slice(&line_buf);
                }
            }

            prev_line_was_empty = is_blank_line(&line_buf);
            first_line = false;
            current_offset += line_len;
            bytes_read += line_len;

            if let Some(cb) = progress_callback {
                if bytes_read - last_progress >= PROGRESS_INTERVAL {
                    cb(bytes_read, self.file_size);
                    last_progress = bytes_read;
                }
            }
        }

        // Flush last message
        if let Some(pstart) = prev_message_start {
            let hdrs = prev_headers.unwrap_or(header_buf);
            let msg_length = current_offset - pstart;
            if header_callback(pstart, msg_length, &hdrs) {
                count += 1;
            }
        }

        if let Some(cb) = progress_callback {
            cb(self.file_size, self.file_size);
        }

        Ok(count)
    }

    /// Read a single message at the given offset and length.
    ///
    /// Uses `seek` to jump directly to the message without scanning the file.
    pub fn read_message_at(path: impl AsRef<Path>, offset: u64, length: u64) -> Result<Vec<u8>> {
        let path = path.as_ref();
        let mut file = File::open(path).map_err(|e| MboxError::io(path, e))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| MboxError::io(path, e))?;
        let mut buffer = vec![0u8; length as usize];
        file.read_exact(&mut buffer)
            .map_err(|e| MboxError::io(path, e))?;
        Ok(buffer)
    }
}

/// Fast newline search (equivalent to memchr for `\n`).
#[inline]
fn memchr_newline(buf: &[u8]) -> Option<usize> {
    buf.iter().position(|&b| b == b'\n')
}

/// Check whether a line is an MBOX separator (`From ` at the start).
fn is_mbox_separator(line: &[u8]) -> bool {
    // Skip BOM if present at very start
    let line = if line.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &line[3..]
    } else {
        line
    };
    line.starts_with(b"From ")
}

/// Check whether a line is blank (empty or only whitespace / CR / LF).
fn is_blank_line(line: &[u8]) -> bool {
    line.iter()
        .all(|&b| b == b'\n' || b == b'\r' || b == b' ' || b == b'\t')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_mbox_separator() {
        assert!(is_mbox_separator(
            b"From user@example.com Thu Jan 01 00:00:00 2024\n"
        ));
        assert!(is_mbox_separator(
            b"From sender@example.com Mon Feb 12 10:00:00 2024\n"
        ));
        assert!(!is_mbox_separator(b"from user@example.com\n")); // lowercase
        assert!(!is_mbox_separator(b">From user@example.com\n")); // escaped
        assert!(!is_mbox_separator(b"Subject: From here\n"));
    }

    #[test]
    fn test_is_blank_line() {
        assert!(is_blank_line(b"\n"));
        assert!(is_blank_line(b"\r\n"));
        assert!(is_blank_line(b"  \n"));
        assert!(!is_blank_line(b"hello\n"));
    }

    #[test]
    fn test_is_mbox_separator_with_bom() {
        let mut line = vec![0xEF, 0xBB, 0xBF];
        line.extend_from_slice(b"From user@example.com Thu Jan 01 00:00:00 2024\n");
        assert!(is_mbox_separator(&line));
    }
}
