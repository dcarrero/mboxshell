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

    // Never overwrite: a counter is appended while the name is taken.
    Ok(write_unique(&path, &data)?)
}

/// Extract all attachments from a single message.
pub fn export_all_attachments(
    store: &mut MboxStore,
    entry: &MailEntry,
    output_dir: &Path,
) -> anyhow::Result<Vec<PathBuf>> {
    let body = store.get_message(entry)?;
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

        let body = store.get_message(entry)?;
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

/// Write `data` to `path`, or to `stem_1.ext`, `stem_2.ext`… when that name
/// is taken, and return where it went.
///
/// Each candidate is opened with `create_new`, so an existing file is never
/// overwritten — not even one created between the check and the write — and
/// a planted symlink is never followed. There is no cap: the old fixed limit
/// of 999 then fell back to one shared `_dup` name that every further copy
/// silently overwrote.
pub(crate) fn write_unique(path: &Path, data: &[u8]) -> std::io::Result<PathBuf> {
    use std::io::Write;

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let parent = path.parent().unwrap_or(Path::new("."));

    let mut i: u64 = 0;
    loop {
        let candidate = match (i, ext.is_empty()) {
            (0, _) => path.to_path_buf(),
            (_, true) => parent.join(format!("{stem}_{i}")),
            (_, false) => parent.join(format!("{stem}_{i}.{ext}")),
        };
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                file.write_all(data)?;
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => i += 1,
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_unique_never_overwrites_past_a_thousand_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.png");
        for i in 0..1005u32 {
            write_unique(&target, &i.to_le_bytes()).unwrap();
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1005);
        assert_eq!(std::fs::read(&target).unwrap(), 0u32.to_le_bytes());
        assert_eq!(
            std::fs::read(dir.path().join("a_1004.png")).unwrap(),
            1004u32.to_le_bytes()
        );
    }
}
