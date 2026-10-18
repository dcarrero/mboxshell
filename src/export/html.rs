//! Export messages as standalone HTML files.
//!
//! Produces a self-contained HTML page with the message headers in a
//! table and the original HTML body when present (falling back to
//! `<pre>`-wrapped plain text). Suitable for archival and for sharing
//! a message with anyone who has a browser.
//!
//! Every page opens with a visually hidden `<h1>` (`Email 0023 — date —
//! subject`) and an invisible `__EMAIL_START_0023__` marker, numbered by the
//! message's position in the mailbox from 1. Converted with wkhtmltopdf and
//! joined into one PDF, the heading becomes a bookmark and the marker shows
//! where to split it again (issue #36); the heading also gives screen
//! readers a title for the page.
//!
//! Inline images (`<img src="cid:…">`) are embedded as `data:` URIs when the
//! caller loads them with [`InlineImages::load`] (issue #49), so the page
//! shows them without any file next to it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;

use crate::model::mail::{MailBody, MailEntry};
use crate::store::reader::MboxStore;

use super::attachment::FolderNaming;
use super::eml::{sanitize_filename_part, truncate_at_char_boundary};

/// Export a single message as a standalone HTML file.
///
/// The HTML body is sanitized by default: `<script>`, `<style>`,
/// `<iframe>`, `<object>`, `on*` event handlers and `javascript:` URLs
/// are stripped, and remote images are blocked (see [`export_html_opts`]).
/// Use `export_html_opts` with `sanitize=false` to keep the original markup
/// (e.g. for archival of the exact source).
pub fn export_html(
    entry: &MailEntry,
    body: &MailBody,
    output_dir: &Path,
) -> anyhow::Result<PathBuf> {
    export_html_opts(entry, body, output_dir, true, false, None)
}

/// The inline images of one message, ready to replace its `cid:` references.
///
/// Only raster formats a browser renders without running anything are kept:
/// PNG, JPEG, GIF and WebP, recognised by their first bytes rather than by
/// the `Content-Type` the sender wrote, so an SVG (which can carry scripts)
/// or an HTML page labelled `image/png` is never embedded.
#[derive(Debug, Clone, Default)]
pub struct InlineImages(Arc<HashMap<String, String>>);

impl InlineImages {
    /// Decode the images the message's HTML refers to by `cid:`.
    ///
    /// Reads the message from the mailbox once, and not at all when the HTML
    /// has no `cid:` reference or no attachment has a `Content-ID`.
    pub fn load(
        store: &mut MboxStore,
        entry: &MailEntry,
        body: &MailBody,
    ) -> crate::error::Result<Self> {
        let refers_to_cid = body
            .html
            .as_deref()
            .is_some_and(|h| h.to_ascii_lowercase().contains("cid:"));
        let candidates: Vec<_> = body
            .attachments
            .iter()
            .filter(|a| a.content_id.is_some())
            .filter(|a| a.content_type.to_ascii_lowercase().starts_with("image/"))
            .collect();
        if !refers_to_cid || candidates.is_empty() {
            return Ok(Self::default());
        }
        let raw = store.get_raw_message(entry)?;
        let decoded = crate::parser::mime::extract_attachments(&raw, &candidates)?;
        Ok(Self::from_parts(candidates.iter().zip(decoded).filter_map(
            |(att, bytes)| Some((att.content_id.clone()?, bytes?)),
        )))
    }

    /// Build the set from `(Content-ID, decoded bytes)` pairs, dropping
    /// whatever is not a recognised raster image.
    pub fn from_parts(parts: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
        let map = parts
            .into_iter()
            .filter_map(|(cid, bytes)| {
                let mime = sniff_image(&bytes)?;
                let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                Some((cid.to_lowercase(), format!("data:{mime};base64,{data}")))
            })
            .collect();
        Self(Arc::new(map))
    }

    /// Number of images available.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `true` when there is no image to embed.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The `data:` URI for a `cid:` URL, if the message carries that image.
    fn resolve(&self, url: &str) -> Option<&str> {
        let id = url.trim().get(4..)?; // after `cid:`
        let id = percent_decode(id);
        let id = id.trim_start_matches('<').trim_end_matches('>');
        self.0.get(&id.to_lowercase()).map(String::as_str)
    }
}

/// The MIME type of a PNG, JPEG, GIF or WebP image, from its magic bytes.
fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Undo `%XX` escapes: RFC 2392 percent-encodes a Content-ID in a `cid:`
/// URL (`cid:logo%40example.com`). A malformed escape is kept as written.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(b) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Options for [`export_html_with`].
#[derive(Debug, Clone, Default)]
pub struct HtmlOptions {
    /// Keep the original markup instead of sanitizing it (`--raw-html`).
    pub raw: bool,
    /// Let remote images through the sanitizer.
    pub allow_remote_images: bool,
    /// Images to embed in place of `cid:` references (sanitized mode only).
    pub inline_images: InlineImages,
    /// Name the file and list the folder like `mboxshell attachments`.
    pub naming: Option<FolderNaming>,
}

/// Export a single message as a standalone HTML file with options.
///
/// With `sanitize`, remote images are blocked unless `allow_remote_images`:
/// opening the page would otherwise fetch them, and a tracking pixel tells
/// the sender when and from where the archive was read. A note at the top
/// of the page says how many were blocked.
///
/// With `naming`, the attachment list ends with the folder `mboxshell
/// attachments` extracts them into under that naming, and sequence naming
/// also names the file after it (`0007.html` for folder `0007/`).
pub fn export_html_opts(
    entry: &MailEntry,
    body: &MailBody,
    output_dir: &Path,
    sanitize: bool,
    allow_remote_images: bool,
    naming: Option<FolderNaming>,
) -> anyhow::Result<PathBuf> {
    export_html_with(
        entry,
        body,
        output_dir,
        &HtmlOptions {
            raw: !sanitize,
            allow_remote_images,
            inline_images: InlineImages::default(),
            naming,
        },
    )
}

/// [`export_html_opts`] with every option, including inline images.
pub fn export_html_with(
    entry: &MailEntry,
    body: &MailBody,
    output_dir: &Path,
    opts: &HtmlOptions,
) -> anyhow::Result<PathBuf> {
    let naming = opts.naming;
    let filename = match FolderNaming::file_stem(naming, entry) {
        Some(stem) => format!("{stem}.html"),
        None => html_filename(entry),
    };
    let path = output_dir.join(&filename);

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
         .attachments li{margin:.25em 0}\n\
         .note{background:#fff8dc;border:1px solid #e0c96a;padding:.5em .75em;color:#5a4a00}\n\
         .sr-only{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}\n\
         .marker{font-size:1px;line-height:1px;color:transparent}\n",
    );
    out.push_str("</style>\n</head>\n<body>\n");

    // Bookmark heading and split marker (issue #36)
    let number = entry.sequence + 1;
    out.push_str(&format!(
        "<h1 class=\"sr-only\">Email {number:04} \u{2014} {} \u{2014} {}</h1>\n",
        entry.date.format("%Y-%m-%d %H:%M"),
        escape_html(&entry.subject)
    ));
    out.push_str(&format!(
        "<div class=\"marker\" aria-hidden=\"true\">__EMAIL_START_{number:04}__</div>\n"
    ));

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
        if !opts.raw {
            let (clean, blocked) =
                sanitize_html_inline(html, opts.allow_remote_images, &opts.inline_images);
            if blocked > 0 {
                out.push_str(&format!(
                    "<p class=\"note\">{} ({blocked})</p>\n",
                    escape_html(crate::i18n::html_remote_images_blocked())
                ));
            }
            out.push_str(&clean);
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
        out.push_str("</ul>\n");
        if let Some(naming) = naming {
            out.push_str(&format!(
                "<p>Folder from <code>mboxshell attachments</code>: <code>{}/</code></p>\n",
                escape_html(&naming.folder_name(entry))
            ));
        }
        out.push_str("</div>\n");
    }

    out.push_str("</body>\n</html>\n");

    Ok(super::attachment::write_unique(&path, out.as_bytes())?)
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
/// `javascript:` URLs while keeping safe formatting and links. Remote images
/// are blocked.
pub(crate) fn sanitize_html(html: &str) -> String {
    sanitize_html_with(html, false).0
}

/// [`sanitize_html`], optionally letting remote images through. Returns the
/// clean HTML and how many image sources were removed.
pub(crate) fn sanitize_html_with(html: &str, allow_remote_images: bool) -> (String, usize) {
    sanitize_html_inline(html, allow_remote_images, &InlineImages::default())
}

/// [`sanitize_html_with`], also replacing each `<img src="cid:…">` with the
/// matching image from `inline` as a `data:` URI.
///
/// With ammonia's defaults the only thing that loads on its own is an
/// `<img src>` (inline `style` and `<link>` are already dropped); links in
/// `<a href>` stay, since they are only followed on a click.
///
/// `cid` is added to the allowed URL schemes only so that ammonia hands
/// those values to the attribute filter, which runs after its scheme check:
/// the filter swaps a `cid:` image source for the `data:` URI built in
/// [`InlineImages::from_parts`] from bytes checked to be an image, and drops
/// every other `cid:` value. Nothing from the message's markup can produce a
/// `data:` URI, so the `data` scheme itself stays blocked.
pub(crate) fn sanitize_html_inline(
    html: &str,
    allow_remote_images: bool,
    inline: &InlineImages,
) -> (String, usize) {
    use std::borrow::Cow;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let blocked = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&blocked);
    let inline = inline.clone();
    let clean = ammonia::Builder::default()
        .add_url_schemes(["cid"])
        .attribute_filter(move |element, attribute, value| {
            let v = value.trim_start().to_ascii_lowercase();
            if v.starts_with("cid:") {
                if element == "img" && attribute == "src" {
                    return inline
                        .resolve(value)
                        .map(|data| Cow::Owned(data.to_string()));
                }
                return None;
            }
            // http(s) and protocol-relative `//host` hit the network; a
            // relative path stays on disk, and other schemes (`data:`) are
            // removed by ammonia's scheme filter anyway.
            let remote = !allow_remote_images
                && element == "img"
                && matches!(attribute, "src" | "srcset")
                && (v.contains("http:") || v.contains("https:") || v.starts_with("//"));
            if remote {
                counter.fetch_add(1, Ordering::Relaxed);
                None
            } else {
                Some(value.into())
            }
        })
        .clean(html)
        .to_string();
    (clean, blocked.load(Ordering::Relaxed))
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
            thread_id: None,
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
    fn test_export_html_bookmark_heading_and_split_marker() {
        let mut entry = sample_entry();
        entry.sequence = 22; // the 23rd message of the mailbox
        let body = MailBody {
            text: Some("Hello".to_string()),
            html: None,
            raw_headers: String::new(),
            attachments: vec![],
        };
        let tmp = tempfile::tempdir().unwrap();
        let path = export_html(&entry, &body, tmp.path()).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains(
            "<h1 class=\"sr-only\">Email 0023 \u{2014} 2024-01-04 10:00 \u{2014} Test &lt;subject&gt; &amp; more</h1>"
        ));
        assert!(content.contains("__EMAIL_START_0023__"));
        // Both come before the message itself
        assert!(content.find("__EMAIL_START_0023__") < content.find("<table class=\"hdr\">"));
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
    fn test_remote_images_are_blocked_by_default() {
        let html = r#"<p>Hi</p><img src="https://track.example/p.gif" alt="logo"><img src="//cdn.example/x.png"><img src="local.png"><a href="https://example.com">link</a>"#;
        let (clean, blocked) = sanitize_html_with(html, false);
        assert_eq!(blocked, 2);
        assert!(
            !clean.contains("track.example") && !clean.contains("cdn.example"),
            "{clean}"
        );
        assert!(clean.contains(r#"alt="logo""#), "alt text stays: {clean}");
        assert!(
            clean.contains("local.png"),
            "relative images stay local: {clean}"
        );
        assert!(
            clean.contains("https://example.com"),
            "links are kept: {clean}"
        );

        let (kept, none) = sanitize_html_with(html, true);
        assert_eq!(none, 0);
        assert!(kept.contains("https://track.example/p.gif"));
    }

    /// A 1x1 transparent PNG.
    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0, 0, 0, 13, b'I', b'H', b'D', b'R',
    ];

    #[test]
    fn test_cid_image_becomes_data_uri() {
        let inline = InlineImages::from_parts([("Logo@Example.com".to_string(), PNG.to_vec())]);
        assert_eq!(inline.len(), 1);
        let html =
            r#"<img src="cid:logo%40example.com" alt="logo"><img src="cid:<logo@example.com>">"#;
        let (clean, blocked) = sanitize_html_inline(html, false, &inline);
        assert_eq!(blocked, 0);
        let expected = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(PNG)
        );
        assert_eq!(clean.matches(&expected).count(), 2, "{clean}");
        assert!(!clean.contains("cid:"), "{clean}");
    }

    #[test]
    fn test_cid_that_is_not_an_embeddable_image_is_dropped() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#;
        let inline = InlineImages::from_parts([
            ("svg@x".to_string(), svg.to_vec()),
            (
                "html@x".to_string(),
                b"<html><script>alert(1)</script>".to_vec(),
            ),
            ("png@x".to_string(), PNG.to_vec()),
        ]);
        assert_eq!(inline.len(), 1, "only the PNG is kept");
        let html = concat!(
            r#"<img src="cid:svg@x"><img src="cid:html@x"><img src="cid:missing@x">"#,
            r#"<a href="cid:png@x">link</a><img src="data:image/png;base64,AAAA">"#,
        );
        let (clean, _) = sanitize_html_inline(html, false, &inline);
        assert!(!clean.contains("cid:"), "{clean}");
        assert!(!clean.contains("data:"), "no data: URI but ours: {clean}");
        assert!(!clean.contains("script"), "{clean}");
    }

    #[test]
    fn test_cid_images_are_kept_with_remote_images_allowed() {
        let inline = InlineImages::from_parts([("a@x".to_string(), PNG.to_vec())]);
        let html = r#"<img src="cid:a@x"><img src="https://example.com/x.png">"#;
        let (clean, blocked) = sanitize_html_inline(html, true, &inline);
        assert_eq!(blocked, 0);
        assert!(clean.contains("data:image/png;base64,"), "{clean}");
        assert!(clean.contains("https://example.com/x.png"), "{clean}");
    }

    #[test]
    fn test_sniff_image_and_percent_decode() {
        assert_eq!(sniff_image(PNG), Some("image/png"));
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_image(b"GIF89a...."), Some("image/gif"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff_image(b"<svg"), None);
        assert_eq!(sniff_image(b""), None);
        assert_eq!(percent_decode("a%40b.c"), "a@b.c");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
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
        let path = export_html_opts(&entry, &body, tmp.path(), false, false, None).unwrap();
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
