//! Export messages as standalone HTML files.
//!
//! Produces a self-contained HTML page with the message headers in a
//! table and the original HTML body when present (falling back to
//! `<pre>`-wrapped plain text). Suitable for archival and for sharing
//! a message with anyone who has a browser.

use std::path::{Path, PathBuf};

use crate::model::mail::{MailBody, MailEntry};

use super::eml::{sanitize_filename_part, truncate_at_char_boundary};

/// Export a single message as a standalone HTML file.
///
/// The HTML body is sanitized by default: `<script>`, `<style>`,
/// `<iframe>`, `<object>`, `on*` event handlers and `javascript:` URLs
/// are stripped. Use `export_html_opts` with `sanitize=false` to keep
/// the original markup (e.g. for archival of the exact source).
pub fn export_html(
    entry: &MailEntry,
    body: &MailBody,
    output_dir: &Path,
) -> anyhow::Result<PathBuf> {
    export_html_opts(entry, body, output_dir, true)
}

/// Export a single message as a standalone HTML file with options.
pub fn export_html_opts(
    entry: &MailEntry,
    body: &MailBody,
    output_dir: &Path,
    sanitize: bool,
) -> anyhow::Result<PathBuf> {
    let filename = html_filename(entry);
    let path = output_dir.join(&filename);
    let path = super::attachment::unique_path(&path);

    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"UTF-8\">\n");
    out.push_str(&format!("<title>{}</title>\n", escape_html(&entry.subject)));
    out.push_str("<style>\n");
    out.push_str(
        "body{font-family:-apple-system,Segoe UI,Roboto,sans-serif;max-width:900px;margin:2em auto;padding:0 1em;color:#222}\n\
         .hdr{border-collapse:collapse;margin-bottom:1.5em;width:100%}\n\
         .hdr th{text-align:right;padding:.25em .75em .25em 0;vertical-align:top;color:#555;font-weight:600;white-space:nowrap;width:8em}\n\
         .hdr td{padding:.25em 0;word-break:break-word}\n\
         .body{border-top:1px solid #ddd;padding-top:1em}\n\
         pre{white-space:pre-wrap;word-wrap:break-word;font-family:ui-monospace,Menlo,Consolas,monospace}\n\
         .attachments{margin-top:2em;padding-top:1em;border-top:1px solid #ddd;color:#555}\n\
         .attachments li{margin:.25em 0}\n",
    );
    out.push_str("</style>\n</head>\n<body>\n");

    // Headers
    out.push_str("<table class=\"hdr\">\n");
    push_header(
        &mut out,
        "Date",
        &entry.date.format("%a, %d %b %Y %H:%M:%S %z").to_string(),
    );
    push_header(&mut out, "From", &entry.from.display());
    if !entry.to.is_empty() {
        push_header(&mut out, "To", &join_addresses(&entry.to));
    }
    if !entry.cc.is_empty() {
        push_header(&mut out, "Cc", &join_addresses(&entry.cc));
    }
    push_header(&mut out, "Subject", &entry.subject);
    out.push_str("</table>\n");

    // Body
    out.push_str("<div class=\"body\">\n");
    if let Some(html) = &body.html {
        if sanitize {
            out.push_str(&sanitize_html(html));
        } else {
            // Raw mode: insert the original markup as-is. Only safe for
            // local archival — DO NOT serve unsanitized export to a browser.
            out.push_str(html);
        }
    } else if let Some(text) = &body.text {
        out.push_str("<pre>");
        out.push_str(&escape_html(text));
        out.push_str("</pre>");
    }
    out.push_str("\n</div>\n");

    // Attachments
    if !body.attachments.is_empty() {
        out.push_str("<div class=\"attachments\">\n");
        out.push_str(&format!(
            "<strong>Attachments ({}):</strong>\n<ul>\n",
            body.attachments.len()
        ));
        for att in &body.attachments {
            let size = humansize::format_size(att.size, humansize::BINARY);
            out.push_str(&format!(
                "  <li>{} <span style=\"color:#888\">({}, {})</span></li>\n",
                escape_html(&att.filename),
                escape_html(&att.content_type),
                size
            ));
        }
        out.push_str("</ul>\n</div>\n");
    }

    out.push_str("</body>\n</html>\n");

    std::fs::write(&path, out)?;
    Ok(path)
}

fn push_header(out: &mut String, label: &str, value: &str) {
    out.push_str(&format!(
        "<tr><th>{}:</th><td>{}</td></tr>\n",
        escape_html(label),
        escape_html(value)
    ));
}

fn join_addresses(addrs: &[crate::model::address::EmailAddress]) -> String {
    addrs
        .iter()
        .map(|a| a.display())
        .collect::<Vec<_>>()
        .join(", ")
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Sanitize an HTML fragment using `ammonia` with defaults that strip
/// scripts, styles, iframes, objects, embeds, `on*` event handlers and
/// `javascript:` URLs while keeping safe formatting and links.
pub(crate) fn sanitize_html(html: &str) -> String {
    ammonia::clean(html)
}

fn html_filename(entry: &MailEntry) -> String {
    let date = entry.date.format("%Y%m%d_%H%M%S").to_string();
    let subject = sanitize_filename_part(&entry.subject, 80);
    let name = format!("{date}_{subject}.html");
    if name.len() > 200 {
        format!("{}.html", truncate_at_char_boundary(&name, 196))
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::address::EmailAddress;
    use chrono::TimeZone;

    fn sample_entry() -> MailEntry {
        MailEntry {
            offset: 0,
            length: 0,
            date: chrono::Utc.with_ymd_and_hms(2024, 1, 4, 10, 0, 0).unwrap(),
            from: EmailAddress {
                display_name: "Alice".to_string(),
                address: "alice@example.com".to_string(),
            },
            to: vec![EmailAddress {
                display_name: String::new(),
                address: "bob@example.com".to_string(),
            }],
            cc: vec![],
            subject: "Test <subject> & more".to_string(),
            message_id: "<msg@example.com>".to_string(),
            in_reply_to: None,
            references: vec![],
            has_attachments: false,
            content_type: "text/html".to_string(),
            text_size: 0,
            labels: vec![],
            sequence: 0,
        }
    }

    #[test]
    fn test_export_html_escapes_subject() {
        let entry = sample_entry();
        let body = MailBody {
            text: Some("Hello".to_string()),
            html: None,
            raw_headers: String::new(),
            attachments: vec![],
        };
        let tmp = tempfile::tempdir().unwrap();
        let path = export_html(&entry, &body, tmp.path()).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("Test &lt;subject&gt; &amp; more"));
        assert!(content.contains("alice@example.com"));
        assert!(content.contains("<pre>Hello</pre>"));
    }

    #[test]
    fn test_export_html_keeps_html_body() {
        let entry = sample_entry();
        let body = MailBody {
            text: None,
            html: Some("<p>Hello <b>world</b></p>".to_string()),
            raw_headers: String::new(),
            attachments: vec![],
        };
        let tmp = tempfile::tempdir().unwrap();
        let path = export_html(&entry, &body, tmp.path()).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        // ammonia preserves p/b
        assert!(content.contains("Hello"));
        assert!(content.contains("<b>world</b>") || content.contains("<b>world</b>"));
    }

    #[test]
    fn test_export_html_sanitizes_scripts() {
        let entry = sample_entry();
        let body = MailBody {
            text: None,
            html: Some(
                "<p>safe</p><script>alert('xss')</script><img src=x onerror=alert(1)>".to_string(),
            ),
            raw_headers: String::new(),
            attachments: vec![],
        };
        let tmp = tempfile::tempdir().unwrap();
        let path = export_html(&entry, &body, tmp.path()).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("<script"));
        assert!(!content.contains("alert("));
        assert!(!content.contains("onerror"));
        assert!(content.contains("safe"));
    }

    #[test]
    fn test_export_html_raw_keeps_scripts() {
        let entry = sample_entry();
        let body = MailBody {
            text: None,
            html: Some("<script>x</script><p>p</p>".to_string()),
            raw_headers: String::new(),
            attachments: vec![],
        };
        let tmp = tempfile::tempdir().unwrap();
        let path = export_html_opts(&entry, &body, tmp.path(), false).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("<script>x</script>"));
    }

    #[test]
    fn test_html_export_unique_on_collision() {
        // Two messages with identical date and subject must not overwrite each
        // other — the second export gets a `_1` suffix.
        let entry = sample_entry();
        let body = MailBody {
            text: Some("x".to_string()),
            html: None,
            raw_headers: String::new(),
            attachments: vec![],
        };
        let tmp = tempfile::tempdir().unwrap();
        let p1 = export_html(&entry, &body, tmp.path()).unwrap();
        let p2 = export_html(&entry, &body, tmp.path()).unwrap();
        assert_ne!(p1, p2, "second export must not overwrite the first");
        assert!(p1.exists() && p2.exists());
    }
}
