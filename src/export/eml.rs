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
    let raw = store.get_raw_message(entry)?;
    let body = skip_from_line(&raw);

    let filename = eml_filename(entry);
    let path = output_dir.join(&filename);

    std::fs::write(&path, body)?;
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
    std::fs::create_dir_all(output_dir)?;
    let mut paths = Vec::with_capacity(entries.len());
    let total = entries.len();

    for (i, entry) in entries.iter().enumerate() {
        progress(i, total);
        let path = export_eml(store, entry, output_dir)?;
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

/// Sanitize a string for use in filenames.
///
/// Replaces invalid characters with `_` and truncates to `max_len`.
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
}
