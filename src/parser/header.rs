//! RFC 5322 header parsing: folding, encoded-words (RFC 2047), and date parsing.

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use tracing::warn;

use crate::model::address::EmailAddress;
use crate::model::mail::MailEntry;

/// Build a [`MailEntry`] from raw header bytes.
///
/// Only the headers we need for the index are extracted. The body is not read.
pub fn parse_headers_to_entry(
    raw_headers: &[u8],
    offset: u64,
    message_length: u64,
    sequence: u64,
) -> crate::error::Result<MailEntry> {
    let text = decode_header_bytes(raw_headers);
    let headers = unfold_headers(&text);

    let date_str = get_header(&headers, "date").unwrap_or_default();
    let date = parse_date(&date_str).unwrap_or(DateTime::UNIX_EPOCH);

    let from_raw = get_header(&headers, "from").unwrap_or_default();
    let from = EmailAddress::parse(&decode_encoded_words(&from_raw));

    let to_raw = get_header(&headers, "to").unwrap_or_default();
    let mut to = EmailAddress::parse_list(&decode_encoded_words(&to_raw));
    to.truncate(5);

    let cc_raw = get_header(&headers, "cc").unwrap_or_default();
    let mut cc = EmailAddress::parse_list(&decode_encoded_words(&cc_raw));
    cc.truncate(5);

    let subject_raw = get_header(&headers, "subject").unwrap_or_default();
    let subject = decode_encoded_words(&subject_raw);

    let message_id = get_header(&headers, "message-id")
        .map(|s| extract_angle_bracket(&s))
        .unwrap_or_default();

    let in_reply_to = get_header(&headers, "in-reply-to").map(|s| extract_angle_bracket(&s));

    let references_raw = get_header(&headers, "references").unwrap_or_default();
    let references = extract_all_angle_brackets(&references_raw);

    let content_type = get_header(&headers, "content-type")
        .map(|ct| ct.split(';').next().unwrap_or("").trim().to_lowercase())
        .unwrap_or_else(|| "text/plain".to_string());

    let has_attachments = content_type.starts_with("multipart/mixed")
        || content_type.starts_with("multipart/related")
        || headers
            .iter()
            .any(|(k, v)| k == "content-disposition" && v.to_lowercase().contains("attachment"));

    let gmail_labels = get_header(&headers, "x-gmail-labels")
        .map(|s| {
            let decoded = decode_encoded_words(&s);
            decoded
                .split(',')
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default();

    Ok(MailEntry {
        offset,
        length: message_length,
        date,
        from,
        to,
        cc,
        subject,
        message_id,
        in_reply_to,
        references,
        has_attachments,
        content_type,
        text_size: 0,
        labels: gmail_labels,
        sequence,
    })
}

/// Decode raw header bytes to a string.
///
/// Tries UTF-8 first, then falls back to ISO-8859-1 (which accepts every byte).
fn decode_header_bytes(bytes: &[u8]) -> String {
    // Strip BOM if present
    let bytes = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    };

    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let (decoded, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
            decoded.into_owned()
        }
    }
}

/// Unfold headers: join continuation lines (starting with space or tab) with the previous header.
///
/// Returns a list of `(lowercase_name, raw_value)` pairs.
fn unfold_headers(text: &str) -> Vec<(String, String)> {
    let mut result: Vec<(String, String)> = Vec::new();

    for line in text.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation line
            if let Some(last) = result.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
        } else if let Some(colon_pos) = line.find(':') {
            let name = line[..colon_pos].trim().to_lowercase();
            let value = line[colon_pos + 1..].trim().to_string();
            result.push((name, value));
        }
        // Lines without a colon and not a continuation are silently skipped
    }

    result
}

/// Get the first value for a header name (case-insensitive).
fn get_header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.clone())
}

/// Decode RFC 2047 encoded-words in a header value.
///
/// Example: `"=?UTF-8?B?SG9sYQ==?= =?UTF-8?B?IG11bmRv?="` → `"Hola mundo"`
///
/// If decoding fails for any token, the original text is preserved.
pub fn decode_encoded_words(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut remaining = input;
    let mut last_was_encoded = false;

    while let Some(start) = remaining.find("=?") {
        let before = &remaining[..start];
        // If the gap between two encoded words is only whitespace, skip it (RFC 2047 §6.2)
        if !last_was_encoded || !before.trim().is_empty() {
            result.push_str(before);
        }

        let after_start = &remaining[start + 2..];

        if let Some(decoded) = try_decode_one_word(after_start) {
            result.push_str(&decoded.text);
            remaining = &remaining[start + 2 + decoded.consumed..];
            last_was_encoded = true;
        } else {
            result.push_str("=?");
            remaining = after_start;
            last_was_encoded = false;
        }
    }

    result.push_str(remaining);
    result
}

struct DecodedWord {
    text: String,
    consumed: usize, // bytes consumed from the string *after* the initial "=?"
}

fn try_decode_one_word(s: &str) -> Option<DecodedWord> {
    // Format: charset?encoding?encoded_text?=
    let first_q = s.find('?')?;
    let charset = &s[..first_q];

    let rest = &s[first_q + 1..];
    let second_q = rest.find('?')?;
    let encoding = &rest[..second_q];

    let rest2 = &rest[second_q + 1..];
    let end = rest2.find("?=")?;
    let encoded_text = &rest2[..end];

    let total_consumed = first_q + 1 + second_q + 1 + end + 2;

    let bytes = match encoding.to_uppercase().as_str() {
        "B" => {
            use std::io::Read;
            let mut decoder = base64_decode_reader(encoded_text.as_bytes());
            let mut buf = Vec::new();
            decoder.read_to_end(&mut buf).ok()?;
            buf
        }
        "Q" => decode_q_encoding(encoded_text),
        _ => return None,
    };

    let text = decode_charset(charset, &bytes);

    Some(DecodedWord {
        text,
        consumed: total_consumed,
    })
}

/// Minimal base64 decoder (reads from a byte slice).
fn base64_decode_reader(input: &[u8]) -> impl std::io::Read + '_ {
    struct Base64Reader<'a> {
        input: &'a [u8],
        pos: usize,
        buf: [u8; 3],
        buf_len: usize,
        buf_pos: usize,
    }

    impl<'a> std::io::Read for Base64Reader<'a> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let mut written = 0;
            while written < out.len() {
                if self.buf_pos < self.buf_len {
                    out[written] = self.buf[self.buf_pos];
                    self.buf_pos += 1;
                    written += 1;
                    continue;
                }
                // Decode next 4-char block
                let mut quad = [0u8; 4];
                let mut qi = 0;
                while qi < 4 {
                    if self.pos >= self.input.len() {
                        if qi == 0 {
                            return Ok(written);
                        }
                        // Pad remaining
                        while qi < 4 {
                            quad[qi] = b'=';
                            qi += 1;
                        }
                        break;
                    }
                    let b = self.input[self.pos];
                    self.pos += 1;
                    if b == b' ' || b == b'\n' || b == b'\r' || b == b'\t' {
                        continue;
                    }
                    quad[qi] = b;
                    qi += 1;
                }
                let vals: [u8; 4] = quad.map(b64val);
                self.buf[0] = (vals[0] << 2) | (vals[1] >> 4);
                self.buf[1] = (vals[1] << 4) | (vals[2] >> 2);
                self.buf[2] = (vals[2] << 6) | vals[3];
                self.buf_len = if quad[3] == b'=' {
                    if quad[2] == b'=' {
                        1
                    } else {
                        2
                    }
                } else {
                    3
                };
                self.buf_pos = 0;
            }
            Ok(written)
        }
    }

    fn b64val(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        }
    }

    Base64Reader {
        input,
        pos: 0,
        buf: [0; 3],
        buf_len: 0,
        buf_pos: 0,
    }
}

/// Decode Q-encoding (RFC 2047): underscores → spaces, `=XX` → byte.
fn decode_q_encoding(input: &str) -> Vec<u8> {
    let mut result = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'_' => {
                result.push(b' ');
                i += 1;
            }
            b'=' if i + 2 < bytes.len() => {
                if let Ok(byte) = u8::from_str_radix(
                    std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00"),
                    16,
                ) {
                    result.push(byte);
                    i += 3;
                } else {
                    result.push(b'=');
                    i += 1;
                }
            }
            b => {
                result.push(b);
                i += 1;
            }
        }
    }
    result
}

/// Decode bytes using a named charset.
fn decode_charset(charset: &str, bytes: &[u8]) -> String {
    let charset_lower = charset.to_lowercase();
    match charset_lower.as_str() {
        "utf-8" | "utf8" => String::from_utf8_lossy(bytes).into_owned(),
        _ => {
            if let Some(encoding) = encoding_rs::Encoding::for_label(charset.as_bytes()) {
                let (decoded, _, _) = encoding.decode(bytes);
                decoded.into_owned()
            } else {
                warn!(
                    charset = charset,
                    "Unknown charset, falling back to UTF-8 lossy"
                );
                String::from_utf8_lossy(bytes).into_owned()
            }
        }
    }
}

/// Extract content between `<` and `>` (for Message-ID, In-Reply-To).
fn extract_angle_bracket(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(start) = trimmed.find('<') {
        if let Some(end) = trimmed[start..].find('>') {
            return trimmed[start..start + end + 1].to_string();
        }
    }
    trimmed.to_string()
}

/// Extract all `<…>` tokens from a string (for References header).
fn extract_all_angle_brackets(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut remaining = s;
    while let Some(start) = remaining.find('<') {
        if let Some(end) = remaining[start..].find('>') {
            result.push(remaining[start..start + end + 1].to_string());
            remaining = &remaining[start + end + 1..];
        } else {
            break;
        }
    }
    result
}

/// Parse an email date string in various common formats.
///
/// Supports RFC 2822, ISO 8601, and many broken real-world variants.
pub fn parse_date(date_str: &str) -> Option<DateTime<Utc>> {
    let trimmed = date_str.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Try chrono's RFC 2822
    if let Ok(dt) = DateTime::parse_from_rfc2822(trimmed) {
        return Some(dt.with_timezone(&Utc));
    }

    // Try ISO 8601 / RFC 3339
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.with_timezone(&Utc));
    }

    // Remove leading day-of-week: "Thu, " or "Thu "
    let no_dow = strip_day_of_week(trimmed);

    // IMAP-style: "16-JUL-2025 03:01:03" → normalize to "16 Jul 2025 03:01:03"
    let no_dow_normalized = normalize_imap_date(&no_dow);

    let formats = [
        "%d %b %Y %H:%M:%S %z",
        "%d %b %Y %H:%M:%S %Z",
        "%d %b %Y %H:%M:%S",
        "%b %d %H:%M:%S %Y",
        "%Y-%m-%dT%H:%M:%S%z",
        "%Y-%m-%dT%H:%M:%SZ",
        "%Y-%m-%d %H:%M:%S %z",
        "%Y-%m-%d %H:%M:%S",
        "%d/%m/%Y %H:%M:%S",
        "%m/%d/%Y %H:%M:%S",
    ];

    // Try both the original (stripped DOW) and the IMAP-normalized variant
    for candidate in [&no_dow, &no_dow_normalized] {
        for fmt in &formats {
            if let Ok(dt) = DateTime::parse_from_str(candidate, fmt) {
                return Some(dt.with_timezone(&Utc));
            }
            if let Ok(ndt) = NaiveDateTime::parse_from_str(candidate, fmt) {
                return Some(Utc.from_utc_datetime(&ndt));
            }
        }
    }

    // Replace named timezones with offsets and try again
    for candidate in [&no_dow, &no_dow_normalized] {
        let replaced = replace_named_tz(candidate);
        for fmt in &formats {
            if let Ok(dt) = DateTime::parse_from_str(&replaced, fmt) {
                return Some(dt.with_timezone(&Utc));
            }
        }
    }

    // Try using mail-parser's date parsing as last resort
    if let Some(dt) = mail_parser_date(trimmed) {
        return Some(dt);
    }

    warn!(date = trimmed, "Could not parse date");
    None
}

/// Attempt to parse a date using `mail-parser`'s built-in parser.
fn mail_parser_date(input: &str) -> Option<DateTime<Utc>> {
    use mail_parser::MessageParser;

    // Wrap input in a minimal RFC 5322 message so mail-parser can parse it
    let fake_msg = format!("Date: {input}\n\n");
    let parser = MessageParser::default();
    let parsed = parser.parse(fake_msg.as_bytes())?;
    let dt = parsed.date()?.to_rfc3339();
    DateTime::parse_from_rfc3339(&dt)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

/// Normalize IMAP-style dates: `"16-JUL-2025 03:01:03"` → `"16 Jul 2025 03:01:03"`.
///
/// IMAP INTERNALDATE and some mail servers use `DD-MMM-YYYY` with uppercase months
/// and hyphens instead of spaces. chrono's `%b` expects title-case months with spaces.
fn normalize_imap_date(s: &str) -> String {
    // Quick check: must contain at least one hyphen between a digit and a letter
    if !s.contains('-') {
        return s.to_string();
    }

    let months = [
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ];
    let title_months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    let mut result = s.to_string();

    // Replace hyphens between date components with spaces
    // Pattern: DD-MMM-YYYY (e.g. "16-JUL-2025")
    for (i, month) in months.iter().enumerate() {
        let uc_pattern = format!("-{month}-");
        if result.contains(&uc_pattern) {
            result = result.replacen(&uc_pattern, &format!(" {} ", title_months[i]), 1);
            return result;
        }
        // Also try lowercase
        let lc_month = month.to_lowercase();
        let lc_pattern = format!("-{lc_month}-");
        if result.contains(&lc_pattern) {
            result = result.replacen(&lc_pattern, &format!(" {} ", title_months[i]), 1);
            return result;
        }
    }

    result
}

/// Strip leading day-of-week prefix (e.g. "Thu, " or "Thu ").
fn strip_day_of_week(s: &str) -> String {
    let days = [
        "Mon,", "Tue,", "Wed,", "Thu,", "Fri,", "Sat,", "Sun,", "Mon ", "Tue ", "Wed ", "Thu ",
        "Fri ", "Sat ", "Sun ",
    ];
    for day in &days {
        if let Some(rest) = s.strip_prefix(day) {
            return rest.trim().to_string();
        }
    }
    s.to_string()
}

/// Replace well-known timezone abbreviations with numeric offsets.
fn replace_named_tz(s: &str) -> String {
    let tzs = [
        ("EST", "-0500"),
        ("EDT", "-0400"),
        ("CST", "-0600"),
        ("CDT", "-0500"),
        ("MST", "-0700"),
        ("MDT", "-0600"),
        ("PST", "-0800"),
        ("PDT", "-0700"),
        ("GMT", "+0000"),
        ("UTC", "+0000"),
        ("CET", "+0100"),
        ("CEST", "+0200"),
        ("JST", "+0900"),
    ];
    let mut result = s.to_string();
    for (name, offset) in &tzs {
        if result.ends_with(name) {
            let pos = result.len() - name.len();
            result.replace_range(pos.., offset);
            return result;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_base64_encoded_word() {
        let input = "=?UTF-8?B?SG9sYSBtdW5kbw==?=";
        assert_eq!(decode_encoded_words(input), "Hola mundo");
    }

    #[test]
    fn test_decode_q_encoded_word() {
        let input = "=?ISO-8859-1?Q?caf=E9?=";
        assert_eq!(decode_encoded_words(input), "café");
    }

    #[test]
    fn test_decode_multiple_encoded_words() {
        let input = "=?UTF-8?B?SG9sYQ==?= =?UTF-8?B?IG11bmRv?=";
        assert_eq!(decode_encoded_words(input), "Hola mundo");
    }

    #[test]
    fn test_decode_mixed_plain_and_encoded() {
        let input = "Re: =?UTF-8?B?SG9sYQ==?= there";
        assert_eq!(decode_encoded_words(input), "Re: Hola there");
    }

    #[test]
    fn test_unfold_headers() {
        let text = "Subject: This is a long\n\tsubject line\nFrom: user@example.com\n";
        let headers = unfold_headers(text);
        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].0, "subject");
        assert_eq!(headers[0].1, "This is a long subject line");
    }

    #[test]
    fn test_parse_date_rfc2822() {
        let dt = parse_date("Thu, 04 Jan 2024 10:00:00 +0000");
        assert!(dt.is_some());
        let dt = dt.unwrap();
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2024-01-04");
    }

    #[test]
    fn test_parse_date_without_dow() {
        let dt = parse_date("04 Jan 2024 10:00:00 +0000");
        assert!(dt.is_some());
    }

    #[test]
    fn test_parse_date_named_tz() {
        let dt = parse_date("Thu, 04 Jan 2024 10:00:00 EST");
        assert!(dt.is_some());
    }

    #[test]
    fn test_parse_date_iso8601() {
        let dt = parse_date("2024-01-04T10:00:00Z");
        assert!(dt.is_some());
    }

    #[test]
    fn test_parse_date_imap_style() {
        // IMAP INTERNALDATE format: DD-MMM-YYYY HH:MM:SS
        let dt = parse_date("16-JUL-2025 03:01:03");
        assert!(dt.is_some(), "Failed to parse IMAP date");
        let dt = dt.unwrap();
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2025-07-16");
    }

    #[test]
    fn test_parse_date_imap_with_tz() {
        let dt = parse_date("14-AUG-2025 02:01:35 +0000");
        assert!(dt.is_some(), "Failed to parse IMAP date with tz");
    }

    #[test]
    fn test_normalize_imap_date() {
        assert_eq!(
            normalize_imap_date("16-JUL-2025 03:01:03"),
            "16 Jul 2025 03:01:03"
        );
        assert_eq!(
            normalize_imap_date("10-MAR-2025 06:00:42"),
            "10 Mar 2025 06:00:42"
        );
        // Non-IMAP dates pass through unchanged
        assert_eq!(
            normalize_imap_date("04 Jan 2024 10:00:00"),
            "04 Jan 2024 10:00:00"
        );
    }

    #[test]
    fn test_extract_angle_brackets() {
        assert_eq!(
            extract_angle_bracket(" <msg001@example.com> "),
            "<msg001@example.com>"
        );
    }

    #[test]
    fn test_extract_all_angle_brackets() {
        let refs = extract_all_angle_brackets("<a@b.com> <c@d.com> <e@f.com>");
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0], "<a@b.com>");
    }

    #[test]
    fn test_decode_iso8859_encoded_word() {
        let input = "=?ISO-8859-1?Q?R=E9sum=E9_du_projet?=";
        assert_eq!(decode_encoded_words(input), "Résumé du projet");
    }

    #[test]
    fn test_decode_utf8_base64_japanese() {
        // 山田太郎
        let input = "=?UTF-8?B?5bGx55Sw5aSq6YOO?=";
        let decoded = decode_encoded_words(input);
        assert_eq!(decoded, "山田太郎");
    }

    #[test]
    fn test_decode_windows1252_encoded_word() {
        // Müller
        let input = "=?Windows-1252?Q?M=FCller?=";
        let decoded = decode_encoded_words(input);
        assert_eq!(decoded, "Müller");
    }
}
