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
    if !opts.raw {
        // Second line of defence behind the sanitizer: the page may only
        // load what it embeds (plus remote images when they were asked for).
        let img = if opts.allow_remote_images {
            "data: http: https:"
        } else {
            "data:"
        };
        out.push_str(&format!(
            "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src {img}; style-src 'unsafe-inline'\">\n"
        ));
    }
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
    let too_nested = !opts.raw && body.html.as_deref().is_some_and(html_too_nested);
    if too_nested {
        out.push_str(&format!(
            "<p class=\"note\">{}</p>\n",
            escape_html(crate::i18n::html_too_nested())
        ));
    }
    if let Some(html) = body.html.as_ref().filter(|_| !too_nested) {
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

/// Deepest element nesting handed to the sanitizer.
///
/// Building and cleaning the tree costs time quadratic in the nesting depth:
/// 100 000 unclosed `<div>` (500 KB of mail) took 17 s, 2 MB over a minute,
/// with the TUI frozen meanwhile. Real mail nests a few dozen levels deep;
/// at this limit the sanitizer still answers in a fraction of a second.
const MAX_HTML_DEPTH: usize = 5_000;

/// Whether `html` may nest more than [`MAX_HTML_DEPTH`] elements deep.
///
/// A single linear pass that only ever over-estimates: every start tag
/// counts as one level deeper and every end tag as one level back, except
/// for elements that cannot nest — void ones (`<br>`, `<img>`…) and `<p>` /
/// `<li>`, which close the previous one of their kind and are routinely left
/// unclosed. `<div/>` is not self-closing in HTML and counts as a start tag.
pub(crate) fn html_too_nested(html: &str) -> bool {
    const FLAT: &[&[u8]] = &[
        b"br", b"img", b"hr", b"meta", b"input", b"link", b"col", b"area", b"base", b"embed",
        b"source", b"track", b"wbr", b"param", b"p", b"li",
    ];
    let bytes = html.as_bytes();
    let mut depth = 0usize;
    let mut from = 0;
    while let Some(found) = memchr::memchr(b'<', &bytes[from..]) {
        let at = from + found + 1;
        from = at;
        match bytes.get(at) {
            Some(b'/') => depth = depth.saturating_sub(1),
            Some(c) if c.is_ascii_alphabetic() => {
                let len = bytes[at..]
                    .iter()
                    .take(16)
                    .take_while(|b| b.is_ascii_alphanumeric())
                    .count();
                let name = &bytes[at..at + len];
                if !FLAT.iter().any(|flat| name.eq_ignore_ascii_case(flat)) {
                    depth += 1;
                    if depth > MAX_HTML_DEPTH {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
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

    // Callers show the text part instead; this is the backstop for one that
    // does not check, so the sanitizer never sees such markup.
    if html_too_nested(html) {
        return (format!("<pre>{}</pre>", escape_html(html)), 0);
    }

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
            // Without `allow_remote_images` the only image source that
            // survives is a `cid:` one resolved above. Matching "looks
            // remote" on the text was bypassable: `h&#9;ttp://…` has no
            // `http:` in it, yet a browser strips tab/CR/LF from a URL and
            // fetches it, and `\\host\share` or `/\host` reach the network
            // too. A relative path points at nothing in an exported page
            // anyway.
            let remote = !allow_remote_images
                && element == "img"
                && matches!(attribute, "src" | "srcset")
                && !v.trim().is_empty();
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
    let date = super::eml::filename_date_stamp(&entry.date);
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
        assert_eq!(blocked, 3);
        assert!(
            !clean.contains("track.example") && !clean.contains("cdn.example"),
            "{clean}"
        );
        assert!(clean.contains(r#"alt="logo""#), "alt text stays: {clean}");
        assert!(!clean.contains("local.png"), "{clean}");
        assert!(
            clean.contains("https://example.com"),
            "links are kept: {clean}"
        );

        let (kept, none) = sanitize_html_with(html, true);
        assert_eq!(none, 0);
        assert!(kept.contains("https://track.example/p.gif"));
    }

    #[test]
    fn test_remote_image_block_cannot_be_dodged_with_url_tricks() {
        // A browser strips tab/CR/LF from a URL and treats `\` as `/`, so
        // none of these contain `http:` or start with `//` yet all hit the
        // network when the page is opened.
        for src in [
            "h&#9;ttp://t.example/a.gif",
            "ht&#10;tp://t.example/b.gif",
            "ht&#13;tps://t.example/c.gif",
            "/&#9;/t.example/d.gif",
            "/\\t.example/e.gif",
            "\\\\t.example\\share\\f.gif",
            " HTTP://t.example/g.gif",
        ] {
            // (`srcset` never reaches the filter: ammonia drops it.)
            let html = format!(r#"<img src="{src}" alt="x"><img srcset="{src} 2x">"#);
            let (clean, blocked) = sanitize_html_with(&html, false);
            assert_eq!(blocked, 1, "{src}: {clean}");
            assert!(!clean.contains("t.example"), "{src}: {clean}");
        }
    }

    #[test]
    fn test_exported_page_carries_a_content_security_policy() {
        let dir = tempfile::tempdir().unwrap();
        let entry = sample_entry();
        let body = MailBody {
            text: None,
            html: Some("<p>x</p>".into()),
            raw_headers: String::new(),
            attachments: vec![],
        };
        let page = |opts: &HtmlOptions| {
            let path = export_html_with(&entry, &body, dir.path(), opts).unwrap();
            std::fs::read_to_string(path).unwrap()
        };
        let strict = page(&HtmlOptions::default());
        assert!(strict.contains("img-src data:;"), "{strict}");
        let remote = page(&HtmlOptions {
            allow_remote_images: true,
            ..Default::default()
        });
        assert!(remote.contains("img-src data: http: https:;"), "{remote}");
        let raw = page(&HtmlOptions {
            raw: true,
            ..Default::default()
        });
        assert!(!raw.contains("Content-Security-Policy"), "{raw}");
    }

    #[test]
    fn test_deeply_nested_html_is_not_handed_to_the_sanitizer() {
        // 200 000 unclosed <div>: the sanitizer needed over a minute.
        let hostile = "<div>".repeat(200_000);
        assert!(html_too_nested(&hostile));
        let closed = format!(
            "{}x{}",
            "<DIV class=a>".repeat(6_000),
            "</div>".repeat(6_000)
        );
        assert!(html_too_nested(&closed));

        let started = std::time::Instant::now();
        let (clean, _) = sanitize_html_with(&hostile, false);
        assert!(clean.starts_with("<pre>&lt;div&gt;"), "{}", &clean[..40]);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));

        // Ordinary mail is nowhere near the limit, however long it is.
        let newsletter =
            "<table><tr><td><p>text<br><img src=x><li>item</td></tr></table>".repeat(20_000);
        assert!(!html_too_nested(&newsletter));
        let unclosed_paragraphs = "<p>line<br>".repeat(100_000);
        assert!(!html_too_nested(&unclosed_paragraphs));
        // Just under the limit still goes through the sanitizer, quickly.
        let deep = "<div>".repeat(MAX_HTML_DEPTH);
        assert!(!html_too_nested(&deep));
        let started = std::time::Instant::now();
        let _ = sanitize_html_with(&deep, false);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn test_export_falls_back_to_text_for_deeply_nested_html() {
        let dir = tempfile::tempdir().unwrap();
        let body = MailBody {
            text: Some("the plain text".into()),
            html: Some("<div>".repeat(50_000)),
            raw_headers: String::new(),
            attachments: vec![],
        };
        let path = export_html(&sample_entry(), &body, dir.path()).unwrap();
        let page = std::fs::read_to_string(path).unwrap();
        assert!(page.contains("<pre>the plain text</pre>"), "{page}");
        assert!(page.contains("class=\"note\""));
        assert!(!page.contains("<div><div>"));
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
