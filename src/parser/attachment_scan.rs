//! Streaming attachment detection for the index (issue #51).
//!
//! The top-level `Content-Type: multipart/mixed` says nothing about whether a
//! message carries an attachment: Alpine, for one, wraps plain-text mails in
//! `multipart/mixed` with a single `text/plain` part. Marking every such
//! message with a paperclip made the list, `stats` and `has:attachment`
//! disagree with the attachment popup, which lists real MIME parts.
//!
//! [`AttachmentScanner`] is fed the body lines the indexer already reads and
//! looks only at the headers of MIME parts (the lines right after a boundary
//! line), so it costs a couple of byte comparisons per line and never decodes
//! anything.

/// Detects, line by line, whether a multipart body has an attachment-like part.
#[derive(Debug, Default)]
pub struct AttachmentScanner {
    in_part_headers: bool,
    found: bool,
}

impl AttachmentScanner {
    /// Create a scanner for a new message body.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether an attachment-like part has been seen so far.
    pub fn found(&self) -> bool {
        self.found
    }

    /// Feed one raw body line (with or without its line ending).
    pub fn feed(&mut self, line: &[u8]) {
        if self.found {
            return;
        }
        if is_boundary_line(line) {
            self.in_part_headers = true;
            return;
        }
        if !self.in_part_headers {
            return;
        }
        if is_blank(line) {
            self.in_part_headers = false;
            return;
        }
        if part_header_marks_attachment(line) {
            self.found = true;
        }
    }

    /// Feed a whole body (used for single `.eml` files).
    pub fn scan(body: &[u8]) -> bool {
        let mut scanner = Self::new();
        for line in body.split(|&b| b == b'\n') {
            scanner.feed(line);
            if scanner.found {
                break;
            }
        }
        scanner.found
    }
}

/// A MIME boundary delimiter: `--` followed directly by the boundary token.
///
/// `-- ` (the signature separator) is excluded by requiring a non-blank byte.
fn is_boundary_line(line: &[u8]) -> bool {
    line.len() > 2 && line.starts_with(b"--") && !line[2].is_ascii_whitespace()
}

fn is_blank(line: &[u8]) -> bool {
    line.iter().all(|&b| b == b'\r' || b == b'\n')
}

/// Whether one header line of a MIME part means "this part is an attachment".
///
/// Matches what the attachment popup lists: any part with a filename, an
/// explicit `attachment` disposition, or a non-text, non-multipart type
/// (images, PDFs, forwarded `message/rfc822`, …).
fn part_header_marks_attachment(line: &[u8]) -> bool {
    // Bound the work on pathological lines; part headers are short.
    let line = &line[..line.len().min(1024)];
    let lower = line.to_ascii_lowercase();

    if lower.starts_with(b" ") || lower.starts_with(b"\t") {
        // Folded continuation of a part header: a filename often lands here.
        return has_name_param(&lower);
    }
    if let Some(value) = lower.strip_prefix(b"content-disposition:") {
        return contains(value, b"attachment") || has_name_param(value);
    }
    if let Some(value) = lower.strip_prefix(b"content-type:") {
        let value = trim_start(value);
        let is_text_or_multipart = value.starts_with(b"text/") || value.starts_with(b"multipart/");
        return !is_text_or_multipart || has_name_param(value);
    }
    false
}

/// `name=` / `filename=` (and their RFC 2231 `*=` forms) as a parameter.
fn has_name_param(value: &[u8]) -> bool {
    [b"name=".as_slice(), b"name*="].iter().any(|needle| {
        let mut start = 0;
        while let Some(pos) = find(&value[start..], needle) {
            let at = start + pos;
            // Must be a parameter name, not the tail of e.g. `xname=`.
            let prev = at.checked_sub(1).map(|i| value[i]);
            let ok = match prev {
                None => true,
                Some(b) => b == b';' || b.is_ascii_whitespace() || value[..at].ends_with(b"file"),
            };
            if ok {
                return true;
            }
            start = at + needle.len();
        }
        false
    })
}

fn trim_start(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|b| b.is_ascii_whitespace()).count();
    &s[n..]
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find(haystack, needle).is_some()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::mime::list_attachments;

    fn body_of(raw: &[u8]) -> &[u8] {
        let pos = raw
            .windows(2)
            .position(|w| w == b"\n\n")
            .expect("test message has a header/body blank line");
        &raw[pos + 2..]
    }

    /// The scanner must agree with what the attachment popup lists.
    fn assert_agrees(raw: &[u8], expected: bool) {
        let listed = !list_attachments(raw).unwrap().is_empty();
        assert_eq!(
            listed, expected,
            "mail-parser disagrees with the test expectation"
        );
        assert_eq!(AttachmentScanner::scan(body_of(raw)), expected);
    }

    #[test]
    fn alpine_text_only_multipart_mixed_has_no_attachment() {
        // Shape from issue #51 (with the header/body blank line in place).
        let raw = b"From: a@b.c\nSubject: s\n\
Content-Type: multipart/mixed; boundary=\"464285959-821626569-1772377490=:3751081\"\n\
\n\
--464285959-821626569-1772377490=:3751081\n\
Content-Type: text/plain; format=flowed; charset=UTF-8\n\
Content-Transfer-Encoding: 8BIT\n\
\n\
Lorem ipsum.\n\
\n\
--464285959-821626569-1772377490=:3751081--\n";
        assert_agrees(raw, false);
    }

    #[test]
    fn pdf_part_is_an_attachment() {
        let raw = b"From: a@b.c\n\
Content-Type: multipart/mixed; boundary=\"B\"\n\
\n\
--B\n\
Content-Type: text/plain\n\
\n\
hi\n\
--B\n\
Content-Type: application/pdf\n\
Content-Disposition: attachment;\n\
 filename=\"doc.pdf\"\n\
Content-Transfer-Encoding: base64\n\
\n\
JVBERi0=\n\
--B--\n";
        assert_agrees(raw, true);
    }

    #[test]
    fn text_file_attachment_is_detected() {
        let raw = b"From: a@b.c\n\
Content-Type: multipart/mixed; boundary=\"B\"\n\
\n\
--B\n\
Content-Type: text/plain\n\
\n\
hi\n\
--B\n\
Content-Type: text/plain; charset=us-ascii\n\
Content-Disposition: attachment; filename=\"notes.txt\"\n\
\n\
notes\n\
--B--\n";
        assert_agrees(raw, true);
    }

    #[test]
    fn alternative_text_and_html_has_no_attachment() {
        let raw = b"From: a@b.c\n\
Content-Type: multipart/mixed; boundary=\"O\"\n\
\n\
--O\n\
Content-Type: multipart/alternative; boundary=\"I\"\n\
\n\
--I\n\
Content-Type: text/plain; charset=utf-8\n\
\n\
hi\n\
--I\n\
Content-Type: text/html; charset=utf-8\n\
\n\
<p>hi</p>\n\
--I--\n\
--O--\n";
        assert_agrees(raw, false);
    }

    #[test]
    fn content_lines_outside_part_headers_are_ignored() {
        // Prose that quotes a part header must not count.
        let body = b"--B\nContent-Type: text/plain\n\nSee: Content-Type: image/png\n\
Content-Disposition: attachment; filename=x\n--B--\n";
        assert!(!AttachmentScanner::scan(body));
    }

    #[test]
    fn signature_separator_is_not_a_boundary() {
        let body = b"hello\n-- \nContent-Type: image/png\n";
        assert!(!AttachmentScanner::scan(body));
    }

    #[test]
    fn charset_is_not_a_name_param() {
        assert!(!has_name_param(b" text/plain; charset=utf-8"));
        assert!(has_name_param(b" text/plain; name=\"a.txt\""));
        assert!(has_name_param(b"attachment; filename*=utf-8''a.txt"));
    }
}
