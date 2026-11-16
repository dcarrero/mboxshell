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
    assert!(!html.contains("src=\"data:"), "{html}");
    assert!(!html.contains("cid:"), "{html}");
}

/// The `File` column of a numbered CSV export names the `.html`/`.eml` file
/// that `export` writes for the same row under the same naming (issue #59),
/// including when only some messages are selected.
#[test]
fn test_csv_file_column_matches_exported_files() {
    use mboxshell::export::attachment::FolderNaming;

    let tmp = tempfile::tempdir().unwrap();
    let mbox_path = tmp.path().join("simple.mbox");
    std::fs::copy("tests/fixtures/simple.mbox", &mbox_path).unwrap();
    let entries = builder::build_index(&mbox_path, true, None).unwrap();
    assert!(entries.len() > 2);
    // Every other message, as `--query` would pick a subset; the numbers
    // still count the whole mailbox.
    let selected: Vec<_> = entries.iter().step_by(2).collect();
    let naming = FolderNaming::sequence(1, 3, entries.len());

    let out = tmp.path().join("out");
    let mut store = MboxStore::open(&mbox_path).unwrap();
    mboxshell::export::eml::export_multiple_eml_opts(
        &mut store,
        &selected,
        &out,
        false,
        Some(naming),
        &|_, _| {},
    )
    .unwrap();
    let opts = HtmlOptions {
        naming: Some(naming),
        ..HtmlOptions::default()
    };
    for entry in &selected {
        let body = store.get_message(entry).unwrap();
        export_html_with(entry, &body, &out, &opts).unwrap();
    }

    let csv_path = tmp.path().join("export.csv");
    mboxshell::export::csv::export_csv_named(&selected, &csv_path, Some(naming)).unwrap();
    let text = std::fs::read_to_string(&csv_path).unwrap();
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    assert!(lines.next().unwrap().ends_with(",Message_ID,File"));
    let stems: Vec<&str> = lines.map(|l| l.rsplit(',').next().unwrap()).collect();
    assert_eq!(stems.len(), selected.len());
    for (stem, entry) in stems.iter().zip(&selected) {
        assert_eq!(*stem, format!("{:03}", entry.sequence + 1));
        assert!(out.join(format!("{stem}.html")).is_file(), "{stem}.html");
        assert!(out.join(format!("{stem}.eml")).is_file(), "{stem}.eml");
    }

    // Date-and-subject names leave the CSV as it always was.
    mboxshell::export::csv::export_csv_named(&selected, &csv_path, Some(FolderNaming::DateSubject))
        .unwrap();
    let text = std::fs::read_to_string(&csv_path).unwrap();
    let header = text.trim_start_matches('\u{feff}').lines().next().unwrap();
    assert!(header.ends_with(",Message_ID"), "{header}");
}
