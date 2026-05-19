//! Export messages as individual `.eml` files.
//!
//! An `.eml` file is the raw RFC 5322 message bytes without the `From ` separator.

use std::path::{Path, PathBuf};

use crate::model::mail::MailEntry;
use crate::store::reader::MboxStore;

/// Export a single message as an `.eml` file.
///
/// Returns the path of the created file.
pub fn export_eml(
    store: &mut MboxStore,
    entry: &MailEntry,
    output_dir: &Path,
) -> anyhow::Result<PathBuf> {
    export_eml_opts(store, entry, output_dir, false)
}

/// Export a single message as an `.eml` file with options.
///
/// If `qp` is true, single-part text bodies with 8-bit content are
/// re-encoded as quoted-printable so the resulting file is pure 7-bit
/// ASCII (helps strict-UTF-8 tools like `eml-extractor`). Multipart
/// messages are written as-is (a future enhancement could walk the
/// MIME tree and re-encode each text part).
pub fn export_eml_opts(
    store: &mut MboxStore,
    entry: &MailEntry,
    output_dir: &Path,
    qp: bool,
) -> anyhow::Result<PathBuf> {
    let raw = store.get_raw_message(entry)?;
    let stripped = skip_from_line(&raw);
    let mut bytes = unescape_mboxrd(stripped);

    if qp {
        bytes = reencode_single_part_as_qp(bytes);
    }

    let filename = eml_filename(entry);
    let path = output_dir.join(&filename);

    std::fs::write(&path, &bytes)?;
    Ok(path)
}

/// Export multiple messages as `.eml` files.
///
/// The progress callback receives `(current, total)`.
pub fn export_multiple_eml(
    store: &mut MboxStore,
    entries: &[&MailEntry],
    output_dir: &Path,
    progress: &dyn Fn(usize, usize),
) -> anyhow::Result<Vec<PathBuf>> {
    export_multiple_eml_opts(store, entries, output_dir, false, progress)
}

/// Export multiple messages as `.eml` files with options.
pub fn export_multiple_eml_opts(
    store: &mut MboxStore,
    entries: &[&MailEntry],
    output_dir: &Path,
    qp: bool,
    progress: &dyn Fn(usize, usize),
) -> anyhow::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(output_dir)?;
    let mut paths = Vec::with_capacity(entries.len());
    let total = entries.len();

    for (i, entry) in entries.iter().enumerate() {
        progress(i, total);
        let path = export_eml_opts(store, entry, output_dir, qp)?;
        paths.push(path);
    }
    progress(total, total);

    Ok(paths)
}

/// Generate a sanitized filename for an EML export.
///
/// Format: `{date}_{from}_{subject}.eml`, truncated to 200 chars.
fn eml_filename(entry: &MailEntry) -> String {
    let date = entry.date.format("%Y%m%d_%H%M%S").to_string();
    let from = sanitize_filename_part(&entry.from.address, 30);
    let subject = sanitize_filename_part(&entry.subject, 80);

    let name = format!("{date}_{from}_{subject}.eml");
    if name.len() > 200 {
        format!("{}.eml", &name[..196])
    } else {
        name
    }
}

/// Strip the `From ` separator line from raw MBOX message bytes.
fn skip_from_line(raw: &[u8]) -> &[u8] {
    if raw.starts_with(b"From ") {
        // Find the end of the first line
        if let Some(pos) = raw.iter().position(|&b| b == b'\n') {
            return &raw[pos + 1..];
        }
    }
    raw
}

/// Reverse mboxrd `From `-line escaping for EML output.
///
/// In mboxrd, any body line starting with one or more `>` followed by `From `
/// was escaped by prepending an extra `>`. To produce a standards-compliant
/// RFC 5322 message we strip exactly one leading `>` from those lines.
/// Also trims a trailing blank line that MBOX adds as a message separator.
fn unescape_mboxrd(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut at_line_start = true;

    let mut i = 0;
    while i < body.len() {
        if at_line_start && body[i] == b'>' {
            // Count consecutive '>' followed by "From "
            let mut j = i;
            while j < body.len() && body[j] == b'>' {
                j += 1;
            }
            if body[j..].starts_with(b"From ") {
                // Drop exactly one '>'
                out.extend_from_slice(&body[i + 1..j]);
                out.extend_from_slice(b"From ");
                i = j + b"From ".len();
                at_line_start = false;
                continue;
            }
        }
        let b = body[i];
        out.push(b);
        at_line_start = b == b'\n';
        i += 1;
    }

    // Trim a single trailing blank line added by MBOX as separator
    while out.ends_with(b"\n\n") || out.ends_with(b"\r\n\r\n") {
        out.pop();
        if out.last() == Some(&b'\r') {
            out.pop();
        }
    }

    out
}

/// Re-encode a single-part text body as quoted-printable.
///
/// Operates on the raw EML bytes:
/// - Splits at the first blank line into headers and body.
/// - Bails out (returns input unchanged) if the message is multipart,
///   already quoted-printable/base64, or has no 8-bit bytes in the body.
/// - Otherwise rewrites/inserts `Content-Transfer-Encoding: quoted-printable`
///   and replaces the body with its QP-encoded form.
fn reencode_single_part_as_qp(eml: Vec<u8>) -> Vec<u8> {
    // Locate header/body boundary (first blank line).
    let split = match find_header_body_split(&eml) {
        Some(s) => s,
        None => return eml,
    };
    let headers_raw = &eml[..split.headers_end];
    let body = &eml[split.body_start..];

    // Bail out cases:
    // - multipart anything
    // - already non-8bit transfer encoding
    let headers_lower = headers_raw.to_ascii_lowercase();
    if window_contains(&headers_lower, b"content-type: multipart/")
        || window_contains(&headers_lower, b"content-type:multipart/")
    {
        return eml;
    }
    let cte = extract_header_value(&headers_lower, b"content-transfer-encoding");
    if let Some(v) = &cte {
        let v = v.trim().to_ascii_lowercase();
        if v == "quoted-printable" || v == "base64" {
            return eml;
        }
    }
    // Nothing to re-encode if body is already 7-bit ASCII.
    if body.iter().all(|&b| b < 128) {
        return eml;
    }

    // Encode body as QP.
    let encoded = quoted_printable::encode(body);

    // Build new headers: drop any existing CTE, then append the new one.
    let mut new_headers: Vec<u8> = Vec::with_capacity(headers_raw.len() + 64);
    for line in split_header_lines(headers_raw) {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with(b"content-transfer-encoding:") {
            continue;
        }
        new_headers.extend_from_slice(line);
    }
    // Ensure a trailing line ending before appending.
    if !new_headers.ends_with(b"\n") {
        new_headers.extend_from_slice(b"\r\n");
    }
    new_headers.extend_from_slice(b"Content-Transfer-Encoding: quoted-printable\r\n");

    // Reassemble: new_headers + blank line + encoded body
    let mut out = new_headers;
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&encoded);
    if !out.ends_with(b"\n") {
        out.extend_from_slice(b"\r\n");
    }
    out
}

struct HeaderBodySplit {
    headers_end: usize,
    body_start: usize,
}

fn find_header_body_split(bytes: &[u8]) -> Option<HeaderBodySplit> {
    // Look for \r\n\r\n first, then \n\n.
    if let Some(pos) = find_subsequence(bytes, b"\r\n\r\n") {
        return Some(HeaderBodySplit {
            headers_end: pos + 2,
            body_start: pos + 4,
        });
    }
    if let Some(pos) = find_subsequence(bytes, b"\n\n") {
        return Some(HeaderBodySplit {
            headers_end: pos + 1,
            body_start: pos + 2,
        });
    }
    None
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn window_contains(haystack: &[u8], needle: &[u8]) -> bool {
    find_subsequence(haystack, needle).is_some()
}

/// Iterate over header lines, including folded continuations as part of the
/// previous logical header. Each returned slice includes its terminating
/// newline(s) if any.
fn split_header_lines(headers: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < headers.len() {
        if headers[i] == b'\n' {
            let end = i + 1;
            // Check next line: if it starts with whitespace, it's a folded
            // continuation — include it in the same logical header.
            if end < headers.len() && (headers[end] == b' ' || headers[end] == b'\t') {
                i += 1;
                continue;
            }
            lines.push(&headers[start..end]);
            start = end;
        }
        i += 1;
    }
    if start < headers.len() {
        lines.push(&headers[start..]);
    }
    lines
}

/// Extract the value of `name:` from a lowercased header block. Returns the
/// first occurrence only, without folding handling (sufficient for CTE).
fn extract_header_value(headers_lower: &[u8], name_lower: &[u8]) -> Option<String> {
    for line in split_header_lines(headers_lower) {
        if line.starts_with(name_lower) && line.get(name_lower.len()) == Some(&b':') {
            let val = &line[name_lower.len() + 1..];
            return Some(String::from_utf8_lossy(val).trim().to_string());
        }
    }
    None
}

/// Sanitize a string for use in filenames.
///
/// Replaces invalid characters with `_`, strips path separators and `..`
/// sequences to prevent path traversal, and truncates to `max_len`.
pub fn sanitize_filename_part(s: &str, max_len: usize) -> String {
    let sanitized: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '.' || c == '_' || c == '@' {
                c
            } else {
                '_'
            }
        })
        .take(max_len)
        .collect();

    // Prevent path traversal: collapse `..` into `_`
    let sanitized = sanitized.replace("..", "_");

    // Strip leading dots (hidden files on Unix) and trailing dots (Windows issue)
    let sanitized = sanitized.trim_matches('.').to_string();

    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename_part("hello world", 20), "hello_world");
        assert_eq!(
            sanitize_filename_part("user@example.com", 30),
            "user@example.com"
        );
        assert_eq!(sanitize_filename_part("a/b\\c:d*e", 20), "a_b_c_d_e");
        assert_eq!(sanitize_filename_part("", 20), "unknown");
    }

    #[test]
    fn test_sanitize_filename_path_traversal() {
        // ".." sequences must be neutralized
        assert!(!sanitize_filename_part("../../etc/passwd", 50).contains(".."));
        assert!(!sanitize_filename_part("..%2f..%2fetc", 50).contains(".."));
        // Leading dots stripped (no hidden files)
        assert!(!sanitize_filename_part(".hidden", 50).starts_with('.'));
        assert!(!sanitize_filename_part("...triple", 50).starts_with('.'));
        // Trailing dots stripped
        assert!(!sanitize_filename_part("file...", 50).ends_with('.'));
    }

    #[test]
    fn test_skip_from_line() {
        let raw = b"From user@example.com Thu Jan 01\nSubject: Test\n\nBody";
        let result = skip_from_line(raw);
        assert!(result.starts_with(b"Subject:"));
    }

    #[test]
    fn test_skip_from_line_no_from() {
        let raw = b"Subject: Test\n\nBody";
        let result = skip_from_line(raw);
        assert_eq!(result, raw);
    }

    #[test]
    fn test_qp_reencode_skips_when_ascii() {
        let eml = b"Subject: Test\r\nContent-Type: text/plain\r\n\r\nHello world\r\n".to_vec();
        let out = reencode_single_part_as_qp(eml.clone());
        assert_eq!(out, eml, "pure ASCII bodies must be unchanged");
    }

    #[test]
    fn test_qp_reencode_skips_multipart() {
        let eml = b"Subject: Test\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nbody \xf1\r\n--x--\r\n".to_vec();
        let out = reencode_single_part_as_qp(eml.clone());
        assert_eq!(out, eml, "multipart messages must be left untouched");
    }

    #[test]
    fn test_qp_reencode_encodes_8bit_body() {
        // Body contains 0xf1 (ñ in ISO-8859-1)
        let eml = b"Subject: Test\r\nContent-Type: text/plain; charset=ISO-8859-1\r\nContent-Transfer-Encoding: 8bit\r\n\r\nHola caf\xf1n\r\n".to_vec();
        let out = reencode_single_part_as_qp(eml);
        // All bytes must be < 128 after re-encoding
        assert!(
            out.iter().all(|&b| b < 128),
            "QP output must be 7-bit ASCII"
        );
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Content-Transfer-Encoding: quoted-printable"));
        assert!(
            s.contains("=F1") || s.contains("=f1"),
            "0xf1 must be QP-encoded"
        );
        // Old 8bit header must be gone
        assert!(!s.contains("Content-Transfer-Encoding: 8bit"));
    }
}
