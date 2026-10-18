//! Integration tests for exports that read back from a real mailbox.

use mboxshell::export::html::{export_html_with, HtmlOptions, InlineImages};
use mboxshell::index::builder;
use mboxshell::store::reader::MboxStore;

/// A message whose HTML shows a PNG by `cid:`, plus an SVG that claims to be
/// a PNG and must not be embedded.
const MBOX: &str = "From a@example.com Thu Jan 04 10:00:00 2024\n\
From: Alice <a@example.com>\n\
To: b@example.com\n\
Subject: Inline\n\
Date: Thu, 04 Jan 2024 10:00:00 +0000\n\
Message-ID: <inline@example.com>\n\
MIME-Version: 1.0\n\
Content-Type: multipart/related; boundary=\"b\"\n\
\n\
--b\n\
Content-Type: text/html; charset=utf-8\n\
\n\
<p>Hi</p><img src=\"cid:logo@example.com\" alt=\"logo\"><img src=\"cid:fake@example.com\">\n\
--b\n\
Content-Type: image/png\n\
Content-Transfer-Encoding: base64\n\
Content-ID: <logo@example.com>\n\
Content-Disposition: inline; filename=\"logo.png\"\n\
\n\
iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==\n\
--b\n\
Content-Type: image/png\n\
Content-ID: <fake@example.com>\n\
Content-Disposition: inline; filename=\"fake.png\"\n\
\n\
<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>\n\
--b--\n\
\n";

#[test]
fn test_html_export_embeds_cid_images() {
    let tmp = tempfile::tempdir().unwrap();
    let mbox_path = tmp.path().join("inline.mbox");
    std::fs::write(&mbox_path, MBOX).unwrap();
    let entries = builder::build_index(&mbox_path, true, None).unwrap();
    assert_eq!(entries.len(), 1);

    let mut store = MboxStore::open(&mbox_path).unwrap();
    let body = store.get_message(&entries[0]).unwrap();
    let inline = InlineImages::load(&mut store, &entries[0], &body).unwrap();
    assert_eq!(inline.len(), 1, "the PNG, not the SVG labelled image/png");

    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let opts = HtmlOptions {
        inline_images: inline,
        ..HtmlOptions::default()
    };
    let path = export_html_with(&entries[0], &body, &out, &opts).unwrap();
    let html = std::fs::read_to_string(path).unwrap();
    assert!(
        html.contains("src=\"data:image/png;base64,iVBORw0KGgo"),
        "{html}"
    );
    assert!(!html.contains("cid:"), "{html}");
    assert!(!html.contains("<script"), "{html}");

    // Without loading the images the references are removed, as before.
    let path = export_html_with(&entries[0], &body, &out, &HtmlOptions::default()).unwrap();
    let html = std::fs::read_to_string(path).unwrap();
    assert!(!html.contains("data:"), "{html}");
    assert!(!html.contains("cid:"), "{html}");
}
