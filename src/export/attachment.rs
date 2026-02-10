//! Extract attachments from messages.

use std::path::{Path, PathBuf};

use crate::model::attachment::AttachmentMeta;
use crate::model::mail::MailEntry;
use crate::store::reader::MboxStore;

use super::eml::sanitize_filename_part;

/// Export a single decoded attachment to disk.
pub fn export_attachment(
    store: &mut MboxStore,
    entry: &MailEntry,
    attachment: &AttachmentMeta,
    output_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let data = store.get_attachment(entry, attachment)?;
    let filename = sanitize_filename_part(&attachment.filename, 150);
    let path = output_dir.join(&filename);

    // Avoid overwriting — append a counter if needed
    let path = unique_path(&path);
    std::fs::write(&path, &data)?;
    Ok(path)
}

/// Extract all attachments from a single message.
pub fn export_all_attachments(
    store: &mut MboxStore,
    entry: &MailEntry,
    output_dir: &Path,
) -> anyhow::Result<Vec<PathBuf>> {
    let body = store.get_message(entry)?.clone();
    let mut paths = Vec::new();

    for att in &body.attachments {
        let path = export_attachment(store, entry, att, output_dir)?;
        paths.push(path);
    }

    Ok(paths)
}

/// Extract all attachments from multiple messages.
///
/// Creates a subfolder per message: `{output_dir}/{date}_{subject}/`
pub fn export_bulk_attachments(
    store: &mut MboxStore,
    entries: &[&MailEntry],
    output_dir: &Path,
    progress: &dyn Fn(usize, usize),
) -> anyhow::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(output_dir)?;
    let mut all_paths = Vec::new();
    let total = entries.len();

    for (i, entry) in entries.iter().enumerate() {
        progress(i, total);

        let body = store.get_message(entry)?.clone();
        if body.attachments.is_empty() {
            continue;
        }

        let subfolder_name = message_folder_name(entry);
        let subfolder = output_dir.join(&subfolder_name);
        std::fs::create_dir_all(&subfolder)?;

        for att in &body.attachments {
            match export_attachment(store, entry, att, &subfolder) {
                Ok(path) => all_paths.push(path),
                Err(e) => {
                    tracing::warn!(
                        filename = %att.filename,
                        error = %e,
                        "Failed to export attachment"
                    );
                }
            }
        }
    }
    progress(total, total);

    Ok(all_paths)
}

/// Generate a folder name for a message's attachments.
fn message_folder_name(entry: &MailEntry) -> String {
    let date = entry.date.format("%Y%m%d_%H%M%S").to_string();
    let subject = sanitize_filename_part(&entry.subject, 60);
    format!("{date}_{subject}")
}

/// If `path` already exists, append a counter to make it unique.
fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parent = path.parent().unwrap_or(Path::new("."));

    for i in 1..1000 {
        let candidate = if ext.is_empty() {
            parent.join(format!("{stem}_{i}"))
        } else {
            parent.join(format!("{stem}_{i}.{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
    }

    // Fallback — very unlikely
    parent.join(format!("{stem}_dup.{ext}"))
}
