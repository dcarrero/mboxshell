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
/// If `qp` is true, every text/* part with 8-bit content is re-encoded
/// as quoted-printable so the resulting file is pure 7-bit ASCII. Works
/// for both single-part and multipart messages (the MIME tree is walked
/// recursively and each leaf is re-encoded in place). Helps strict-UTF-8
/// tools like `eml-extractor` and `emlAnalyzer`.
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
        bytes = reencode_message_as_qp(bytes);
    }

    let filename = eml_filename(entry);
    let path = output_dir.join(&filename);
    let path = super::attachment::unique_path(&path);

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
        format!("{}.eml", truncate_at_char_boundary(&name, 196))
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

/// Re-encode every text/* part of a message as quoted-printable.
///
/// Works for both single-part and multipart messages. For multipart,
/// recursively walks the MIME tree by splitting on the declared
/// boundary and re-encoding each leaf part in turn. Non-text parts and
/// parts already encoded as quoted-printable/base64 are left untouched.
///
/// The result is a message whose text bodies are pure 7-bit ASCII,
/// which makes it accepted by strict-UTF-8 tooling like `eml-extractor`
/// and `emlAnalyzer`.
fn reencode_message_as_qp(eml: Vec<u8>) -> Vec<u8> {
    let split = match find_header_body_split(&eml) {
        Some(s) => s,
        None => return eml,
    };
    let headers_raw = &eml[..split.headers_end];
    let body = &eml[split.body_start..];

    let headers_lower = headers_raw.to_ascii_lowercase();
    let content_type_lc = extract_header_value(&headers_lower, b"content-type").unwrap_or_default();
    // Preserve original case for parameters like boundary= which is case-sensitive.
    let content_type_orig =
        extract_header_value_original_case(headers_raw, b"content-type").unwrap_or_default();

    // Multipart: split by boundary, recurse, reassemble.
    if content_type_lc.starts_with("multipart/") {
        if let Some(boundary) = extract_boundary(&content_type_orig) {
            let new_body = reencode_multipart_body(body, &boundary);
            let mut out = Vec::with_capacity(headers_raw.len() + new_body.len() + 2);
            out.extend_from_slice(headers_raw);
            if !out.ends_with(b"\n") {
                out.extend_from_slice(b"\r\n");
            }
            // Preserve the blank line that separates headers from body
            let sep_len = split.body_start - split.headers_end;
            if sep_len >= 2 {
                out.extend_from_slice(&eml[split.headers_end..split.body_start]);
            } else {
                out.extend_from_slice(b"\r\n");
            }
            out.extend_from_slice(&new_body);
            return out;
        }
        // No boundary parameter — give up and return as-is
        return eml;
    }

    // Non-multipart: single leaf part. Same logic as before.
    reencode_leaf_part(headers_raw, body, &headers_lower)
}

/// Re-encode a single leaf MIME part (headers + body).
///
/// Returns the headers + blank line + body, with quoted-printable
/// encoding applied iff the part is text/* with 8-bit content.
fn reencode_leaf_part(headers_raw: &[u8], body: &[u8], headers_lower: &[u8]) -> Vec<u8> {
    let content_type = extract_header_value(headers_lower, b"content-type").unwrap_or_default();
    let cte = extract_header_value(headers_lower, b"content-transfer-encoding").unwrap_or_default();

    // Only re-encode text/* parts.
    let is_text = content_type.starts_with("text/") || content_type.is_empty();
    let already_safe = cte == "quoted-printable" || cte == "base64";
    let has_8bit = body.iter().any(|&b| b >= 128);

    if !is_text || already_safe || !has_8bit {
        // Reassemble unchanged.
        return reassemble(headers_raw, body);
    }

    let encoded = quoted_printable::encode(body);
    let mut new_headers: Vec<u8> = Vec::with_capacity(headers_raw.len() + 64);
    for line in split_header_lines(headers_raw) {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with(b"content-transfer-encoding:") {
            continue;
        }
        new_headers.extend_from_slice(line);
    }
    if !new_headers.ends_with(b"\n") {
        new_headers.extend_from_slice(b"\r\n");
    }
    new_headers.extend_from_slice(b"Content-Transfer-Encoding: quoted-printable\r\n");

    let mut out = new_headers;
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&encoded);
    if !out.ends_with(b"\n") {
        out.extend_from_slice(b"\r\n");
    }
    out
}

fn reassemble(headers_raw: &[u8], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(headers_raw.len() + body.len() + 2);
    out.extend_from_slice(headers_raw);
    if !out.ends_with(b"\n") {
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

/// Split a multipart body by its boundary and re-encode each part.
///
/// MIME multipart bodies look like:
///   <preamble — ignored by clients>
///   --BOUNDARY
///   <part 1 headers + blank line + part 1 body>
///   --BOUNDARY
///   <part 2 headers + blank line + part 2 body>
///   --BOUNDARY--
///   <epilogue — ignored>
fn reencode_multipart_body(body: &[u8], boundary: &str) -> Vec<u8> {
    let delim = format!("--{boundary}");
    let delim_bytes = delim.as_bytes();
    let close = format!("--{boundary}--");
    let close_bytes = close.as_bytes();

    // Find all delimiter positions (must be at line start).
    let mut delim_positions: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let at_line_start = i == 0 || body[i - 1] == b'\n';
        if at_line_start && body[i..].starts_with(delim_bytes) {
            delim_positions.push(i);
            i += delim_bytes.len();
        } else {
            i += 1;
        }
    }

    if delim_positions.is_empty() {
        return body.to_vec();
    }

    let mut out = Vec::with_capacity(body.len() + 64);
    // Preamble before the first delimiter (rare, kept as-is)
    out.extend_from_slice(&body[..delim_positions[0]]);

    for win in delim_positions.windows(2) {
        let start = win[0];
        let end = win[1];
        // Each segment is: delimiter line + CRLF + part content
        // Find end of delimiter line
        let after_delim = body[start..end]
            .iter()
            .position(|&b| b == b'\n')
            .map(|p| start + p + 1)
            .unwrap_or(start + delim_bytes.len());
        // Closing delimiter (--BOUNDARY--) marks end of multipart
        let is_close = body[start..end].starts_with(close_bytes);

        // Write delimiter line as-is
        out.extend_from_slice(&body[start..after_delim]);
        if is_close {
            // Everything after a close delimiter is epilogue
            out.extend_from_slice(&body[after_delim..end]);
            continue;
        }
        // Re-encode the part. The "part" runs from after_delim up to end,
        // but we need to leave the CRLF that precedes the next boundary
        // line in place (RFC 2046 §5.1.1).
        let part_end = trim_trailing_crlf_before_boundary(&body[after_delim..end]);
        let part_bytes = &body[after_delim..after_delim + part_end];
        let trailing = &body[after_delim + part_end..end];

        let reencoded = reencode_message_as_qp(part_bytes.to_vec());
        out.extend_from_slice(&reencoded);
        out.extend_from_slice(trailing);
    }

    // Last delimiter segment: from last delim_position to end of body
    let last = *delim_positions.last().unwrap();
    out.extend_from_slice(&body[last..]);
    out
}

/// Strip the CRLF (or LF) that precedes a MIME boundary line, returning
/// the byte index where the part actually ends.
fn trim_trailing_crlf_before_boundary(segment: &[u8]) -> usize {
    let mut end = segment.len();
    if end >= 2 && &segment[end - 2..end] == b"\r\n" {
        end -= 2;
    } else if end >= 1 && segment[end - 1] == b'\n' {
        end -= 1;
    }
    end
}

/// Extract the `boundary=…` parameter from a Content-Type value.
/// Handles both quoted (`boundary="abc"`) and unquoted forms.
fn extract_boundary(content_type: &str) -> Option<String> {
    let lower = content_type.to_ascii_lowercase();
    let idx = lower.find("boundary")?;
    let after = &content_type[idx..];
    // Skip "boundary" then optional whitespace and '='
    let eq = after.find('=')?;
    let mut rest = after[eq + 1..].trim_start();
    if rest.starts_with('"') {
        rest = &rest[1..];
        let end = rest.find('"')?;
        return Some(rest[..end].to_string());
    }
    // Unquoted: stop at ';' or whitespace
    let end = rest
        .find(|c: char| c == ';' || c.is_whitespace())
        .unwrap_or(rest.len());
    Some(rest[..end].to_string())
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

/// Like `extract_header_value` but case-insensitive in the name match and
/// returning the value in its original case (needed for `boundary=…` which is
/// case-sensitive).
fn extract_header_value_original_case(headers_raw: &[u8], name_lower: &[u8]) -> Option<String> {
    for line in split_header_lines(headers_raw) {
        if line.len() <= name_lower.len() || line.get(name_lower.len()) != Some(&b':') {
            continue;
        }
        let name_slice = &line[..name_lower.len()];
        if name_slice.eq_ignore_ascii_case(name_lower) {
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

/// Truncate `s` to at most `max_bytes` bytes without splitting a UTF-8
/// character. Returns the longest prefix whose byte length is `<= max_bytes`.
pub(crate) fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
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
    fn test_truncate_at_char_boundary() {
        // Fits: returned unchanged.
        assert_eq!(truncate_at_char_boundary("hello", 10), "hello");
        assert_eq!(truncate_at_char_boundary("hello", 5), "hello");
        // Empty and zero.
        assert_eq!(truncate_at_char_boundary("", 4), "");
        assert_eq!(truncate_at_char_boundary("héllo", 0), "");
        // Multibyte: "é" is 2 bytes. max_bytes=2 must not split it.
        assert_eq!(truncate_at_char_boundary("é", 1), "");
        assert_eq!(truncate_at_char_boundary("aé", 2), "a");
        // Never panics and never yields invalid UTF-8 for a CJK string.
        let cjk = "日本語のテスト";
        for max in 0..cjk.len() + 2 {
            let t = truncate_at_char_boundary(cjk, max);
            assert!(t.len() <= max.min(cjk.len()));
        }
    }

    fn entry_with_subject(subject: &str) -> MailEntry {
        use crate::model::address::EmailAddress;
        use chrono::TimeZone;
        MailEntry {
            offset: 0,
            length: 0,
            date: chrono::Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            from: EmailAddress {
                display_name: String::new(),
                address: "sender@example.com".to_string(),
            },
            to: vec![],
            cc: vec![],
            subject: subject.to_string(),
            message_id: "<id@example.com>".to_string(),
            in_reply_to: None,
            references: vec![],
            has_attachments: false,
            content_type: "text/plain".to_string(),
            text_size: 0,
            labels: vec![],
            sequence: 0,
        }
    }

    #[test]
    fn test_eml_filename_long_multibyte_subject_no_panic() {
        // 80 CJK chars (each 3 bytes) = 240 bytes of subject; combined with the
        // date and sender the name exceeds 200 bytes and used to be sliced at a
        // fixed byte index, panicking mid-character.
        let subject: String = "日".repeat(80);
        let name = eml_filename(&entry_with_subject(&subject));
        assert!(name.ends_with(".eml"));
        assert!(name.len() <= 200, "name was {} bytes", name.len());
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
        let out = reencode_message_as_qp(eml.clone());
        assert_eq!(out, eml, "pure ASCII bodies must be unchanged");
    }

    #[test]
    fn test_qp_reencode_encodes_8bit_body() {
        // Body contains 0xf1 (ñ in ISO-8859-1)
        let eml = b"Subject: Test\r\nContent-Type: text/plain; charset=ISO-8859-1\r\nContent-Transfer-Encoding: 8bit\r\n\r\nHola caf\xf1n\r\n".to_vec();
        let out = reencode_message_as_qp(eml);
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
        assert!(!s.contains("Content-Transfer-Encoding: 8bit"));
    }

    #[test]
    fn test_qp_reencode_multipart_alternative() {
        let eml = b"Subject: Test\r\n\
            Content-Type: multipart/alternative; boundary=\"BNDRY\"\r\n\
            \r\n\
            --BNDRY\r\n\
            Content-Type: text/plain; charset=ISO-8859-1\r\n\
            Content-Transfer-Encoding: 8bit\r\n\
            \r\n\
            Hola caf\xf1n\r\n\
            --BNDRY\r\n\
            Content-Type: text/html; charset=UTF-8\r\n\
            Content-Transfer-Encoding: 8bit\r\n\
            \r\n\
            <p>Hola caf\xc3\xa9</p>\r\n\
            --BNDRY--\r\n"
            .to_vec();
        let out = reencode_message_as_qp(eml);
        assert!(
            out.iter().all(|&b| b < 128),
            "every byte of a multipart QP-encoded message must be 7-bit"
        );
        let s = String::from_utf8(out).unwrap();
        // Both inner parts must have CTE rewritten
        let cte_count = s
            .matches("Content-Transfer-Encoding: quoted-printable")
            .count();
        assert_eq!(cte_count, 2, "both text parts must declare QP");
        // Outer multipart Content-Type must be preserved
        assert!(s.contains("multipart/alternative"));
        assert!(s.contains("--BNDRY"));
        assert!(s.contains("--BNDRY--"));
    }

    #[test]
    fn test_qp_reencode_multipart_skips_binary_part() {
        // multipart/mixed with a text part + binary part; binary must stay
        let eml = b"Content-Type: multipart/mixed; boundary=BB\r\n\
            \r\n\
            --BB\r\n\
            Content-Type: text/plain; charset=ISO-8859-1\r\n\
            Content-Transfer-Encoding: 8bit\r\n\
            \r\n\
            ca\xf1a\r\n\
            --BB\r\n\
            Content-Type: application/octet-stream\r\n\
            Content-Transfer-Encoding: base64\r\n\
            \r\n\
            SGVsbG8gd29ybGQ=\r\n\
            --BB--\r\n"
            .to_vec();
        let out = reencode_message_as_qp(eml);
        let s = String::from_utf8_lossy(&out);
        // Text part is now QP
        assert!(s.contains("Content-Transfer-Encoding: quoted-printable"));
        // Binary part still says base64
        assert!(s.contains("Content-Transfer-Encoding: base64"));
        assert!(s.contains("SGVsbG8gd29ybGQ="));
    }

    #[test]
    fn test_extract_boundary_quoted() {
        assert_eq!(
            extract_boundary("multipart/mixed; boundary=\"abc def\""),
            Some("abc def".to_string())
        );
    }

    #[test]
    fn test_extract_boundary_unquoted() {
        assert_eq!(
            extract_boundary("multipart/mixed; boundary=abc; charset=utf-8"),
            Some("abc".to_string())
        );
    }
}
