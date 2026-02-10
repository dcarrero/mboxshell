//! Export messages as plain text files.

use std::path::{Path, PathBuf};

use crate::model::mail::{MailBody, MailEntry};

use super::eml::sanitize_filename_part;

/// Export a single message as a plain text file with headers and body.
pub fn export_text(
    entry: &MailEntry,
    body: &MailBody,
    output_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let filename = text_filename(entry);
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
    }

    std::fs::write(&path, content)?;
    Ok(path)
}

/// Generate a filename for text export.
fn text_filename(entry: &MailEntry) -> String {
    let date = entry.date.format("%Y%m%d_%H%M%S").to_string();
    let subject = sanitize_filename_part(&entry.subject, 80);
    let name = format!("{date}_{subject}.txt");
    if name.len() > 200 {
        format!("{}.txt", &name[..196])
    } else {
        name
    }
}
