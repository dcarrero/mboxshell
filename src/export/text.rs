//! Export messages as plain text files.

use std::path::{Path, PathBuf};

use crate::model::mail::{MailBody, MailEntry};

use super::attachment::FolderNaming;
use super::eml::{sanitize_filename_part, truncate_at_char_boundary};

/// Export a single message as a plain text file with headers and body.
pub fn export_text(
    entry: &MailEntry,
    body: &MailBody,
    output_dir: &Path,
) -> anyhow::Result<PathBuf> {
    export_text_opts(entry, body, output_dir, None)
}

/// Export a single message as a plain text file, naming it and its
/// attachments' folder as `naming` says.
///
/// With `naming`, the attachment list ends with the folder `mboxshell
/// attachments` extracts them into under that naming, and sequence naming
/// also names the file after it (`0007.txt` for folder `0007/`).
pub fn export_text_opts(
    entry: &MailEntry,
    body: &MailBody,
    output_dir: &Path,
    naming: Option<FolderNaming>,
) -> anyhow::Result<PathBuf> {
    let filename = match FolderNaming::file_stem(naming, entry) {
        Some(stem) => format!("{stem}.txt"),
        None => text_filename(entry),
    };
    let path = output_dir.join(&filename);

    let mut content = String::new();

    // Headers
    content.push_str(&format!(
        "Date:    {}\n",
        entry.date.format("%a, %d %b %Y %H:%M:%S %z")
    ));
    content.push_str(&format!("From:    {}\n", entry.from.display()));

    if !entry.to.is_empty() {
        let to_str = entry
            .to
            .iter()
            .map(|a| a.display())
            .collect::<Vec<_>>()
            .join(", ");
        content.push_str(&format!("To:      {to_str}\n"));
    }

    if !entry.cc.is_empty() {
        let cc_str = entry
            .cc
            .iter()
            .map(|a| a.display())
            .collect::<Vec<_>>()
            .join(", ");
        content.push_str(&format!("Cc:      {cc_str}\n"));
    }

    content.push_str(&format!("Subject: {}\n", entry.subject));
    content.push_str(&format!("\n{}\n", "-".repeat(72)));

    // Body
    if let Some(text) = &body.text {
        content.push('\n');
        content.push_str(text);
        content.push('\n');
    }

    // Attachments list
    if !body.attachments.is_empty() {
        content.push_str(&format!(
            "\n[Attachments: {} file(s)]\n",
            body.attachments.len()
        ));
        for att in &body.attachments {
            let size = humansize::format_size(att.size, humansize::BINARY);
            content.push_str(&format!(
                "  - {} ({}, {})\n",
                att.filename, att.content_type, size
            ));
        }
        if let Some(naming) = naming {
            content.push_str(&format!(
                "  Folder from `mboxshell attachments`: {}/\n",
                naming.folder_name(entry)
            ));
        }
    }

    Ok(super::attachment::write_unique(&path, content.as_bytes())?)
}

/// Generate a filename for text export.
fn text_filename(entry: &MailEntry) -> String {
    let date = entry.date.format("%Y%m%d_%H%M%S").to_string();
    let subject = sanitize_filename_part(&entry.subject, 80);
    let name = format!("{date}_{subject}.txt");
    if name.len() > 200 {
        format!("{}.txt", truncate_at_char_boundary(&name, 196))
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::attachment::export_bulk_attachments;
    use crate::store::reader::MboxStore;

    /// `export` and `attachments` under the same naming must agree on the
    /// message's folder (issue #39).
    #[test]
    fn test_export_names_match_attachment_folders() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("in.mbox");
        std::fs::copy("tests/fixtures/cloud_infra.mbox", &src).unwrap();
        let entries = crate::index::builder::build_index(&src, true, None).unwrap();
        let entry = entries.iter().find(|e| e.has_attachments).unwrap();
        let mut store = MboxStore::open(&src).unwrap();
        let body = store.get_message(entry).unwrap();

        let naming = FolderNaming::sequence(0, 0, entries.len());
        let atts = dir.path().join("atts");
        export_bulk_attachments(&mut store, &[entry], &atts, naming, &|_, _| {}).unwrap();
        let folder = std::fs::read_dir(&atts)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name()
            .into_string()
            .unwrap();
        assert_eq!(folder, format!("{:02}", entry.sequence));

        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let txt = export_text_opts(entry, &body, &out, Some(naming)).unwrap();
        assert_eq!(
            txt.file_name().unwrap().to_str(),
            Some(&*format!("{folder}.txt"))
        );
        let content = std::fs::read_to_string(&txt).unwrap();
        assert!(
            content.contains(&format!("Folder from `mboxshell attachments`: {folder}/")),
            "{content}"
        );

        let html =
            crate::export::html::export_html_opts(entry, &body, &out, true, false, Some(naming))
                .unwrap();
        assert_eq!(
            html.file_name().unwrap().to_str(),
            Some(&*format!("{folder}.html"))
        );
        let content = std::fs::read_to_string(&html).unwrap();
        assert!(
            content.contains(&format!("<code>{folder}/</code>")),
            "{content}"
        );

        // Date-and-subject naming keeps the usual file name but still
        // points at the folder.
        let dts = FolderNaming::DateSubject;
        let txt = export_text_opts(entry, &body, &out, Some(dts)).unwrap();
        assert_eq!(
            txt.file_name().unwrap().to_str(),
            Some(&*text_filename(entry))
        );
        let content = std::fs::read_to_string(&txt).unwrap();
        assert!(content.contains(&format!(": {}/", dts.folder_name(entry))));

        // Without naming (the TUI), no folder line.
        let plain = export_text(entry, &body, &out).unwrap();
        assert!(!std::fs::read_to_string(plain)
            .unwrap()
            .contains("Folder from"));
    }
}
