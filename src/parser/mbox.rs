//! Streaming MBOX parser.
//!
//! Reads MBOX files line-by-line with a 1 MB buffer.
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

/// Maximum bytes retained for a single physical line during header scanning.
/// A line longer than this (an unwrapped multi-GB body run with no newline, or
/// a corrupt/truncated region) is still consumed from the file in full for
/// correct offset accounting, but only this many bytes are kept in memory.
const MAX_LINE_RETAIN: usize = 8 * 1024 * 1024;

/// Maximum bytes retained for one message's accumulated header block. A header
/// section larger than this (a malformed message with no header/body blank
/// line) stops accumulating; offset accounting is unaffected.
const MAX_HEADER_RETAIN: usize = 16 * 1024 * 1024;

/// Streaming MBOX parser.
///
/// Reads through the file sequentially, invoking a caller-supplied callback for
/// every message boundary it finds. The parser is tolerant of:
///
/// - Mixed `\n` and `\r\n` line endings
/// - `From ` lines not preceded by a blank line (logs a warning)
/// - `From `-prefixed lines inside message bodies: only lines shaped like a
///   real separator (`From <sender> <asctime date>`) split messages, and
///   git `format-patch` pseudo-separators (magic date `Mon Sep 17 00:00:00
///   2001`) are treated as content unless the file itself is a patch series
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
        let mut git_patch_mbox = false;
        let mut last_progress: u64 = 0;

        // Reusable line buffer
        let mut line_buf: Vec<u8> = Vec::with_capacity(4096);
        const PROGRESS_INTERVAL: u64 = 4 * 1024 * 1024;

        loop {
            line_buf.clear();
            let line_len = {
                // Read a full physical line, accumulating across buffer
                // boundaries. Using `fill_buf` + manual consume here would
                // split long lines (e.g. folded `Received:` headers) when they
                // straddle the read buffer, leaving a stray `\r\n` tail that
                // `is_blank_line` misreads as the end of the headers.
                let consumed = reader
                    .read_until(b'\n', &mut line_buf)
                    .map_err(|e| MboxError::io(&self.path, e))?;
                if consumed == 0 {
                    break; // EOF
                }
                consumed as u64
            };

            let kind = classify_from_line(&line_buf);
            if first_line && kind == FromLineKind::GitPatchMarker {
                git_patch_mbox = true;
            }
            let is_from_line = match kind {
                FromLineKind::Separator => true,
                FromLineKind::GitPatchMarker => git_patch_mbox,
                // The very first line of the file is a separator even if
                // malformed: there is no preceding content to mis-split and
                // some writers use nonstandard From lines.
                FromLineKind::Content => first_line && starts_with_from(&line_buf),
            };

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
        let mut git_patch_mbox = false;
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
                // Read a full physical line, accumulating across buffer
                // boundaries. Using `fill_buf` + manual consume here would
                // split long lines (e.g. folded `Received:` headers) when they
                // straddle the read buffer, leaving a stray `\r\n` tail that
                // `is_blank_line` misreads as the end of the headers.
                let consumed = reader
                    .read_until(b'\n', &mut line_buf)
                    .map_err(|e| MboxError::io(&self.path, e))?;
                if consumed == 0 {
                    break; // EOF
                }
                consumed as u64
            };
            // Cap what we RETAIN, never what we consumed: `line_len` above is
            // the true byte count read from the file, so offsets stay exact
            // while a pathological newline-free run can't blow up memory.
            if line_buf.len() > MAX_LINE_RETAIN {
                warn!(
                    offset = current_offset,
                    retained = MAX_LINE_RETAIN,
                    "Oversized line while indexing; truncating retained bytes"
                );
                line_buf.truncate(MAX_LINE_RETAIN);
            }

            let kind = classify_from_line(&line_buf);
            if first_line && kind == FromLineKind::GitPatchMarker {
                git_patch_mbox = true;
            }
            let is_from_line = match kind {
                FromLineKind::Separator => true,
                FromLineKind::GitPatchMarker => git_patch_mbox,
                // See `parse()`: lenient on the very first line of the file.
                FromLineKind::Content => first_line && starts_with_from(&line_buf),
            };

            if is_from_line {
                if !first_line && !prev_line_was_empty {
                    warn!(
                        offset = current_offset,
                        "Found 'From ' separator without preceding blank line"
                    );
                }

                // Emit the *previous* message. Use its saved headers, or fall
                // back to the still-accumulating buffer when the message had no
                // blank line before this separator — otherwise that message
                // (its headers never got swapped out) would be silently dropped.
                if let Some(pstart) = prev_message_start {
                    let pheaders = prev_headers
                        .take()
                        .unwrap_or_else(|| std::mem::take(&mut header_buf));
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
                } else if header_buf.len() < MAX_HEADER_RETAIN {
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
        // Convert with a checked cast instead of `as usize`, which would
        // truncate on a 32-bit target and under-allocate the buffer.
        let len = usize::try_from(length).map_err(|_| {
            MboxError::io(path, std::io::Error::from(std::io::ErrorKind::InvalidData))
        })?;
        let mut buffer = vec![0u8; len];
        file.read_exact(&mut buffer)
            .map_err(|e| MboxError::io(path, e))?;
        Ok(buffer)
    }
}

/// BOM-tolerant check for a `From `-prefixed line.
fn starts_with_from(line: &[u8]) -> bool {
    let line = if line.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &line[3..]
    } else {
        line
    };
    line.starts_with(b"From ")
}

/// Classification of a line with respect to MBOX message separation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FromLineKind {
    /// Regular content — including `From `-prefixed lines that do not have
    /// the structure of a real separator (e.g. quoted email headers inside
    /// a message body, issue #16).
    Content,
    /// A structurally valid `From <sender> <asctime date>` separator.
    Separator,
    /// A valid separator shape carrying git's `format-patch` magic
    /// timestamp (`Mon Sep 17 00:00:00 2001`). Only treated as a real
    /// separator when the file itself is a git patch series (i.e. it
    /// starts with one); inside a normal mailbox it is almost always a
    /// patch quoted verbatim in a message body (issue #16).
    GitPatchMarker,
}

/// Classify a line as MBOX separator, git-patch marker, or plain content.
///
/// A separator must look like `From <sender> <asctime date>` and end right
/// after the date (an optional timezone is allowed before or after the
/// year). Anything trailing the date — like the `<br>` of an HTML body that
/// quotes an email verbatim — disqualifies the line. A bare `From ` line
/// with nothing after it is also a separator: some writers (e.g.
/// Thunderbird exporting a Gmail account) emit exactly that (issue #16).
fn classify_from_line(line: &[u8]) -> FromLineKind {
    // Skip BOM if present at very start
    let line = if line.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &line[3..]
    } else {
        line
    };
    if !line.starts_with(b"From ") {
        return FromLineKind::Content;
    }
    let Ok(rest) = std::str::from_utf8(&line[5..]) else {
        return FromLineKind::Content;
    };
    let tokens: Vec<&str> = rest.split_ascii_whitespace().collect();

    // sender + "Www Mmm dd hh:mm:ss yyyy" (+ optional timezone)
    let date = match tokens.len() {
        // Bare "From " with nothing after it: Thunderbird-style separator.
        0 => return FromLineKind::Separator,
        6 => &tokens[1..6],
        7 if is_timezone(tokens[6]) => &tokens[1..6],
        7 if is_timezone(tokens[5]) && is_year(tokens[6]) => {
            // "Www Mmm dd hh:mm:ss TZ yyyy" (old ctime placement)
            &tokens[1..5] // year checked above, validate the rest below
        }
        _ => return FromLineKind::Content,
    };
    let valid = match date {
        [dow, mon, day, time, year] => {
            is_day_of_week(dow) && is_month(mon) && is_day(day) && is_time(time) && is_year(year)
        }
        [dow, mon, day, time] => {
            is_day_of_week(dow) && is_month(mon) && is_day(day) && is_time(time)
        }
        _ => false,
    };
    if !valid {
        return FromLineKind::Content;
    }

    // `git format-patch` always stamps its pseudo-mbox From line with this
    // fixed magic date (chosen as a marker; it predates git itself).
    if rest.trim_end().ends_with("Mon Sep 17 00:00:00 2001") {
        return FromLineKind::GitPatchMarker;
    }
    FromLineKind::Separator
}

fn is_day_of_week(s: &str) -> bool {
    matches!(s, "Mon" | "Tue" | "Wed" | "Thu" | "Fri" | "Sat" | "Sun")
}

fn is_month(s: &str) -> bool {
    matches!(
        s,
        "Jan"
            | "Feb"
            | "Mar"
            | "Apr"
            | "May"
            | "Jun"
            | "Jul"
            | "Aug"
            | "Sep"
            | "Oct"
            | "Nov"
            | "Dec"
    )
}

fn is_day(s: &str) -> bool {
    (1..=2).contains(&s.len()) && s.parse::<u8>().is_ok_and(|d| (1..=31).contains(&d))
}

fn is_time(s: &str) -> bool {
    let mut parts = s.split(':');
    let (Some(h), Some(m), Some(sec), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let ok =
        |t: &str, max: u8| (1..=2).contains(&t.len()) && t.parse::<u8>().is_ok_and(|v| v <= max);
    ok(h, 23) && ok(m, 59) && ok(sec, 61)
}

fn is_year(s: &str) -> bool {
    s.len() == 4 && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_timezone(s: &str) -> bool {
    let numeric = (s.starts_with('+') || s.starts_with('-'))
        && s.len() == 5
        && s[1..].bytes().all(|b| b.is_ascii_digit());
    let named = (1..=5).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_uppercase());
    numeric || named
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
    fn test_classify_from_line_separators() {
        use FromLineKind::*;
        // Bare "From " separator (Thunderbird-style, issue #16)
        assert_eq!(classify_from_line(b"From \n"), Separator);
        assert_eq!(classify_from_line(b"From \r\n"), Separator);
        assert_eq!(classify_from_line(b"From  \n"), Separator);
        assert_eq!(
            classify_from_line(b"From user@example.com Thu Jan 01 00:00:00 2024\n"),
            Separator
        );
        assert_eq!(
            classify_from_line(b"From sender@example.com Mon Feb 12 10:00:00 2024\n"),
            Separator
        );
        // Thunderbird-style "-" sender, CRLF ending
        assert_eq!(
            classify_from_line(b"From - Thu Jan 01 00:00:00 2024\r\n"),
            Separator
        );
        // asctime day padding collapses to a single-digit day token
        assert_eq!(
            classify_from_line(b"From MAILER-DAEMON Fri Jul  8 12:08:34 2011\n"),
            Separator
        );
        // Timezone after the year, and old ctime placement before the year
        assert_eq!(
            classify_from_line(b"From user@example.com Mon Sep 18 00:00:00 2023 +0200\n"),
            Separator
        );
        assert_eq!(
            classify_from_line(b"From user@example.com Fri Jul  8 12:08:34 EDT 2011\n"),
            Separator
        );
    }

    #[test]
    fn test_classify_from_line_content() {
        use FromLineKind::*;
        assert_eq!(classify_from_line(b"from user@example.com\n"), Content); // lowercase
        assert_eq!(classify_from_line(b">From user@example.com\n"), Content); // escaped
        assert_eq!(classify_from_line(b"Subject: From here\n"), Content);
        // Body prose starting with "From "
        assert_eq!(classify_from_line(b"From here on, all is well\n"), Content);
        // No date at all (but see the bare "From " separator case above)
        assert_eq!(classify_from_line(b"From user@example.com\n"), Content);
        assert_eq!(classify_from_line(b"From -\n"), Content);
        // Trailing junk after the date — the issue #16 HTML case
        assert_eq!(
            classify_from_line(b"From abc123 Mon Sep 17 00:00:00 2001<br>\n"),
            Content
        );
        // Bogus date fields
        assert_eq!(
            classify_from_line(b"From u@e.com Xxx Jan 01 00:00:00 2024\n"),
            Content
        );
        assert_eq!(
            classify_from_line(b"From u@e.com Thu Jan 32 00:00:00 2024\n"),
            Content
        );
        assert_eq!(
            classify_from_line(b"From u@e.com Thu Jan 01 25:00:00 2024\n"),
            Content
        );
    }

    #[test]
    fn test_classify_from_line_git_magic_date() {
        // `git format-patch` pseudo-separator: structurally valid, but
        // stamped with git's fixed magic date.
        assert_eq!(
            classify_from_line(b"From 8f3b1c4d5e6f Mon Sep 17 00:00:00 2001\n"),
            FromLineKind::GitPatchMarker
        );
        // Same date with a different shape is still just a separator
        assert_eq!(
            classify_from_line(b"From u@e.com Mon Sep 17 00:00:01 2001\n"),
            FromLineKind::Separator
        );
    }

    #[test]
    fn test_is_blank_line() {
        assert!(is_blank_line(b"\n"));
        assert!(is_blank_line(b"\r\n"));
        assert!(is_blank_line(b"  \n"));
        assert!(!is_blank_line(b"hello\n"));
    }

    #[test]
    fn test_classify_from_line_with_bom() {
        let mut line = vec![0xEF, 0xBB, 0xBF];
        line.extend_from_slice(b"From user@example.com Thu Jan 01 00:00:00 2024\n");
        assert_eq!(classify_from_line(&line), FromLineKind::Separator);
    }

    /// Regression test for issue #16: an email quoted verbatim inside a
    /// message body (a `git format-patch` mail, with and without an HTML
    /// `<br>` tail) must not split the containing message.
    #[test]
    fn test_embedded_git_patch_does_not_split_message() {
        use std::io::Write;

        let mut data = Vec::new();
        data.extend_from_slice(b"From a@example.com Thu Jan 01 00:00:00 2024\n");
        data.extend_from_slice(b"Subject: Patch review\n");
        data.extend_from_slice(b"Message-ID: <1@example.com>\n");
        data.extend_from_slice(b"\n");
        data.extend_from_slice(b"Thanks,<br>\n");
        data.extend_from_slice(b"<br>\n");
        // HTML part: trailing <br> after the date
        data.extend_from_slice(b"From abc123 Mon Sep 17 00:00:00 2001<br>\n");
        data.extend_from_slice(b"From: Jakov <j@example.com><br>\n");
        data.extend_from_slice(b"Subject: [PATCH] fix the thing<br>\n");
        data.extend_from_slice(b"\n");
        // Plain-text part: bare git pseudo-separator, blank line before it
        data.extend_from_slice(b"From abc123 Mon Sep 17 00:00:00 2001\n");
        data.extend_from_slice(b"From: Jakov <j@example.com>\n");
        data.extend_from_slice(b"\n");
        data.extend_from_slice(b"From b@example.com Thu Jan 02 00:00:00 2024\n");
        data.extend_from_slice(b"Subject: Second real message\n");
        data.extend_from_slice(b"Message-ID: <2@example.com>\n");
        data.extend_from_slice(b"\n");
        data.extend_from_slice(b"Body\n");

        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&data).unwrap();
        file.flush().unwrap();

        let parser = MboxParser::new(file.path()).unwrap();
        let mut subjects: Vec<String> = Vec::new();
        let count = parser
            .parse_headers_only(
                &mut |_off, _len, headers| {
                    let h = String::from_utf8_lossy(headers);
                    let subj = h
                        .lines()
                        .find(|l| l.starts_with("Subject: "))
                        .unwrap_or("")
                        .to_string();
                    subjects.push(subj);
                    true
                },
                None,
            )
            .unwrap();

        assert_eq!(count, 2, "embedded quoted email must not split the message");
        assert_eq!(subjects[0], "Subject: Patch review");
        assert_eq!(subjects[1], "Subject: Second real message");
    }

    /// Regression test for issue #16 (second report): Thunderbird writes bare
    /// `From ` separators with no sender/date and no blank line between
    /// messages. They must keep splitting, while an embedded quoted git
    /// patch in a body must not.
    #[test]
    fn test_bare_from_separators_still_split() {
        use std::io::Write;

        let mut data = Vec::new();
        data.extend_from_slice(b"From \n");
        data.extend_from_slice(b"Subject: First\n");
        data.extend_from_slice(b"Message-ID: <1@example.com>\n");
        data.extend_from_slice(b"\n");
        data.extend_from_slice(b"Thanks,<br>\n");
        data.extend_from_slice(b"From abc123 Mon Sep 17 00:00:00 2001<br>\n");
        data.extend_from_slice(b"From: Jakov <j@example.com><br>\n");
        data.extend_from_slice(b"Body of first message\n");
        // No blank line before the next bare separator (Thunderbird does this too)
        data.extend_from_slice(b"From \n");
        data.extend_from_slice(b"Subject: Second\n");
        data.extend_from_slice(b"Message-ID: <2@example.com>\n");
        data.extend_from_slice(b"\n");
        data.extend_from_slice(b"Body\n");

        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&data).unwrap();
        file.flush().unwrap();

        let parser = MboxParser::new(file.path()).unwrap();
        let count = parser
            .parse_headers_only(&mut |_, _, _| true, None)
            .unwrap();
        assert_eq!(count, 2, "bare 'From ' lines must separate messages");
    }

    /// A file produced by `git format-patch` IS a valid mbox whose
    /// separators all carry the magic date; it must still split normally.
    #[test]
    fn test_git_patch_series_file_still_splits() {
        use std::io::Write;

        let mut data = Vec::new();
        for i in 1..=3 {
            data.extend_from_slice(b"From 8f3b1c4d5e6f Mon Sep 17 00:00:00 2001\n");
            data.extend_from_slice(format!("Subject: [PATCH {i}/3] change\n").as_bytes());
            data.extend_from_slice(b"\n");
            data.extend_from_slice(b"diff --git a/x b/x\n");
            data.extend_from_slice(b"\n");
        }

        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&data).unwrap();
        file.flush().unwrap();

        let parser = MboxParser::new(file.path()).unwrap();
        let count = parser
            .parse_headers_only(&mut |_, _, _| true, None)
            .unwrap();
        assert_eq!(count, 3, "a patch-series mbox must split on every patch");
    }

    /// Regression test for issue #15: a header line whose trailing CRLF lands
    /// exactly on the read-buffer boundary used to be split into a separate
    /// blank line, prematurely ending the header section and dropping every
    /// header after it (Subject, Date, …).
    #[test]
    fn test_header_line_straddling_buffer_boundary() {
        use std::io::Write;

        let from_line = b"From sender@example.com Thu Jan 01 00:00:00 2024\n";
        let prefix = b"X-Pad: ";
        // Pad a header line so its content fills the read buffer exactly,
        // leaving its `\r\n` to start right at the boundary.
        let pad = READ_BUFFER_SIZE - from_line.len() - prefix.len();

        let mut data = Vec::new();
        data.extend_from_slice(from_line);
        data.extend_from_slice(prefix);
        data.extend(std::iter::repeat_n(b'a', pad));
        assert_eq!(data.len(), READ_BUFFER_SIZE);
        data.extend_from_slice(b"\r\n");
        data.extend_from_slice(b"Subject: Boundary Bug\r\n");
        data.extend_from_slice(b"Date: Wed, 22 Apr 2020 14:32:19 -0700\r\n");
        data.extend_from_slice(b"\r\n");
        data.extend_from_slice(b"Body here\r\n");

        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&data).unwrap();
        file.flush().unwrap();

        let parser = MboxParser::new(file.path()).unwrap();
        let mut captured: Vec<u8> = Vec::new();
        let count = parser
            .parse_headers_only(
                &mut |_off, _len, headers| {
                    captured = headers.to_vec();
                    true
                },
                None,
            )
            .unwrap();

        assert_eq!(count, 1);
        let headers = String::from_utf8_lossy(&captured);
        assert!(
            headers.contains("Subject: Boundary Bug"),
            "Subject header was dropped at the buffer boundary"
        );
        assert!(
            headers.contains("Date: Wed, 22 Apr 2020"),
            "Date header was dropped at the buffer boundary"
        );
    }

    #[test]
    fn test_oversized_line_offsets_stay_exact() {
        use std::io::Write;

        // Message 1 has an oversized body line (no newline until its end) that
        // exceeds MAX_LINE_RETAIN, so it is truncated in memory. `current_offset`
        // must still advance by the full consumed byte count, so message 2's
        // reported offset is byte-exact.
        let big_len = MAX_LINE_RETAIN + 1024;
        let mut data = Vec::new();
        data.extend_from_slice(b"From a@b.com Thu Jan 01 00:00:00 2024\n");
        data.extend_from_slice(b"Subject: First\n\n");
        data.extend(std::iter::repeat_n(b'x', big_len));
        data.push(b'\n');
        let second_start = data.len() as u64;
        data.extend_from_slice(b"From c@d.com Fri Jan 02 00:00:00 2024\n");
        data.extend_from_slice(b"Subject: Second\n\n");
        data.extend_from_slice(b"Body\n");

        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&data).unwrap();
        file.flush().unwrap();

        let parser = MboxParser::new(file.path()).unwrap();
        let mut offsets: Vec<u64> = Vec::new();
        let mut second_headers: Vec<u8> = Vec::new();
        let count = parser
            .parse_headers_only(
                &mut |off, _len, headers| {
                    if offsets.len() == 1 {
                        second_headers = headers.to_vec();
                    }
                    offsets.push(off);
                    true
                },
                None,
            )
            .unwrap();

        assert_eq!(count, 2, "both messages must be found across the huge line");
        assert_eq!(offsets.len(), 2);
        assert_eq!(
            offsets[1], second_start,
            "second message offset must stay byte-exact despite the truncated line"
        );
        assert!(String::from_utf8_lossy(&second_headers).contains("Subject: Second"));
    }

    #[test]
    fn test_header_cap_preserves_normal_multiheader_messages() {
        use std::io::Write;

        // Two well-formed messages with several headers each. The header
        // retention cap (MAX_HEADER_RETAIN) must never drop a legitimate header
        // of a normally-sized message.
        let mut data = Vec::new();
        data.extend_from_slice(b"From a@b.com Thu Jan 01 00:00:00 2024\n");
        data.extend_from_slice(b"Subject: First\n");
        data.extend_from_slice(b"X-Foo: bar\n");
        data.extend_from_slice(b"Date: Wed, 22 Apr 2020 14:32:19 -0700\n\n");
        data.extend_from_slice(b"Body one\n");
        data.extend_from_slice(b"From c@d.com Fri Jan 02 00:00:00 2024\n");
        data.extend_from_slice(b"Subject: Second\n\n");
        data.extend_from_slice(b"Body two\n");

        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&data).unwrap();
        file.flush().unwrap();

        let parser = MboxParser::new(file.path()).unwrap();
        let mut all_headers: Vec<String> = Vec::new();
        let count = parser
            .parse_headers_only(
                &mut |_off, _len, headers| {
                    all_headers.push(String::from_utf8_lossy(headers).into_owned());
                    true
                },
                None,
            )
            .unwrap();

        assert_eq!(count, 2);
        assert!(all_headers[0].contains("Subject: First"));
        assert!(all_headers[0].contains("X-Foo: bar"));
        assert!(all_headers[0].contains("Date: Wed, 22 Apr 2020"));
        assert!(all_headers[1].contains("Subject: Second"));
    }

    #[test]
    fn test_intermediate_message_without_blank_line_is_not_dropped() {
        use std::io::Write;
        // The first message has no blank line before the next `From ` separator.
        // Its headers were never swapped into `prev_headers`, so before the fix
        // it was silently dropped; now it is emitted from the accumulating buffer.
        let mut data = Vec::new();
        data.extend_from_slice(b"From a@b.com Thu Jan 01 00:00:00 2024\n");
        data.extend_from_slice(b"Subject: First\n");
        data.extend_from_slice(b"From c@d.com Fri Jan 02 00:00:00 2024\n");
        data.extend_from_slice(b"Subject: Second\n\n");
        data.extend_from_slice(b"Body\n");

        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&data).unwrap();
        file.flush().unwrap();

        let parser = MboxParser::new(file.path()).unwrap();
        let mut headers: Vec<String> = Vec::new();
        let count = parser
            .parse_headers_only(
                &mut |_off, _len, h| {
                    headers.push(String::from_utf8_lossy(h).into_owned());
                    true
                },
                None,
            )
            .unwrap();

        assert_eq!(
            count, 2,
            "the blank-line-less first message must not be dropped"
        );
        assert!(headers[0].contains("Subject: First"));
        assert!(headers[1].contains("Subject: Second"));
    }
}
