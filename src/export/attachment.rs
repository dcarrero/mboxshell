//! Extract attachments from messages.

use std::path::{Path, PathBuf};

use crate::model::attachment::AttachmentMeta;
use crate::model::mail::MailEntry;
use crate::store::reader::MboxStore;

use super::eml::{filename_date_stamp, sanitize_attachment_filename, sanitize_filename_part};

/// Most digits a sequence folder name is padded to. More than any message
/// count needs; a larger `--seq-width` only produced names too long for the
/// file system, and past 65535 a panic in the formatter.
pub const MAX_SEQ_WIDTH: usize = 20;

/// What a bulk attachment extraction did.
#[derive(Debug, Default)]
pub struct AttachmentExport {
    /// Every file written.
    pub paths: Vec<PathBuf>,
    /// Attachments that could not be written (name too long for the file
    /// system, disk full, folder not creatable…). Each one is logged.
    pub failed: usize,
}

/// Export a single decoded attachment to disk.
pub fn export_attachment(
    store: &mut MboxStore,
    entry: &MailEntry,
    attachment: &AttachmentMeta,
    output_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let data = store.get_attachment(entry, attachment)?;
    Ok(write_attachment(&attachment.filename, &data, output_dir)?)
}

/// Write one attachment's bytes under its sanitized name.
fn write_attachment(filename: &str, data: &[u8], output_dir: &Path) -> std::io::Result<PathBuf> {
    let filename = sanitize_attachment_filename(filename, 150);
    // Never overwrite: a counter is appended while the name is taken.
    write_unique(&output_dir.join(filename), data)
}

/// Extract all attachments from a single message.
pub fn export_all_attachments(
    store: &mut MboxStore,
    entry: &MailEntry,
    output_dir: &Path,
) -> anyhow::Result<Vec<PathBuf>> {
    let body = store.get_message(entry)?;
    if body.attachments.is_empty() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    let mut first_error = None;
    write_message_attachments(
        store,
        entry,
        &body.attachments,
        output_dir,
        |result| match result {
            Ok(path) => paths.push(path),
            Err(e) => {
                first_error.get_or_insert(e);
            }
        },
    )?;
    match first_error {
        Some(e) => Err(e.into()),
        None => Ok(paths),
    }
}

/// Decode a message once and write each of its attachments into `dir`,
/// reporting every outcome to `done`.
fn write_message_attachments(
    store: &mut MboxStore,
    entry: &MailEntry,
    attachments: &[AttachmentMeta],
    dir: &Path,
    mut done: impl FnMut(std::io::Result<PathBuf>),
) -> anyhow::Result<()> {
    let raw = store.get_raw_message(entry)?;
    let mut seen = vec![false; attachments.len()];
    crate::parser::mime::visit_attachments(&raw, |part_index, data| {
        for (att, seen) in attachments.iter().zip(seen.iter_mut()) {
            if att.part_index == part_index && !*seen {
                *seen = true;
                let result = write_attachment(&att.filename, data, dir);
                if let Err(e) = &result {
                    // `?`: Debug-escaped. The name comes from the message and
                    // could carry terminal escape sequences into the log.
                    tracing::warn!(
                        filename = ?att.filename,
                        error = %e,
                        "Failed to export attachment"
                    );
                }
                done(result);
            }
        }
    })?;
    for (att, seen) in attachments.iter().zip(&seen) {
        if !seen {
            tracing::warn!(
                filename = ?att.filename,
                part = att.part_index,
                "Attachment not found in message"
            );
            done(Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("attachment part {} not found in message", att.part_index),
            )));
        }
    }
    Ok(())
}

/// How `export_bulk_attachments` names each message's subfolder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderNaming {
    /// `{date}_{subject}` (the default).
    DateSubject,
    /// The message's position in the mailbox, zero-padded to `width`
    /// digits: `start` is the number of the first message (1 or 0). Easy to
    /// match back to a message, unlike a date shifted by time zones or a
    /// subject cut short (issue #32).
    Sequence {
        /// Number given to the first message of the mailbox.
        start: u64,
        /// Minimum number of digits.
        width: usize,
    },
}

impl FolderNaming {
    /// Sequence naming for a mailbox of `total` messages.
    ///
    /// A `width` of 0 means as many digits as the highest number needs, so
    /// the names sort in order. Always pass the whole mailbox's count, not
    /// a filtered selection's: `attachments` and `export` must agree.
    pub fn sequence(start: u64, width: usize, total: usize) -> Self {
        let last = (total as u64).saturating_sub(1) + start;
        let width = match width {
            0 => last.to_string().len(),
            w => w.min(MAX_SEQ_WIDTH),
        };
        FolderNaming::Sequence { start, width }
    }

    /// Name of `entry`'s attachment folder, as `export_bulk_attachments`
    /// creates it. Exports print it so a message can be matched to its
    /// attachments (issue #39).
    pub fn folder_name(self, entry: &MailEntry) -> String {
        message_folder_name(entry, self)
    }

    /// File name stem for an exported message: the folder's number under
    /// sequence naming, so `0007.html` goes with folder `0007/`. `None`
    /// keeps each exporter's own date-and-subject name.
    pub(crate) fn file_stem(naming: Option<Self>, entry: &MailEntry) -> Option<String> {
        match naming {
            Some(n @ FolderNaming::Sequence { .. }) => Some(n.folder_name(entry)),
            _ => None,
        }
    }
}

/// Extract all attachments from multiple messages.
///
/// Creates a subfolder per message, named as `naming` says:
/// `{output_dir}/{date}_{subject}/` or `{output_dir}/{number}/`.
///
/// `entries` may be the whole mailbox: messages without attachments are
/// skipped. One attachment (or one message) that cannot be written does not
/// stop the rest; it is counted in [`AttachmentExport::failed`].
pub fn export_bulk_attachments(
    store: &mut MboxStore,
    entries: &[&MailEntry],
    output_dir: &Path,
    naming: FolderNaming,
    progress: &dyn Fn(usize, usize),
) -> anyhow::Result<AttachmentExport> {
    std::fs::create_dir_all(output_dir)?;
    let mut report = AttachmentExport::default();
    let total = entries.len();

    for (i, entry) in entries.iter().enumerate() {
        progress(i, total);

        // One read and one MIME parse per message: the same pass finds the
        // attachments and hands over their bytes.
        let raw = store.get_raw_message(entry)?;
        // Created when the first attachment turns up; `Err` once it failed.
        let mut subfolder: Option<Result<PathBuf, ()>> = None;
        crate::parser::mime::visit_named_attachments(&raw, |_, filename, data| {
            let dir = subfolder.get_or_insert_with(|| {
                let name = message_folder_name(entry, naming);
                let dir = output_dir.join(&name);
                crate::fsutil::create_dir_no_follow(&dir)
                    .map(|()| dir)
                    .map_err(|e| {
                        tracing::warn!(folder = ?name, error = %e, "Cannot create attachment folder");
                    })
            });
            let written = match dir {
                Ok(dir) => write_attachment(filename, data, dir).map_err(|e| {
                    // `?`: Debug-escaped. The name comes from the message
                    // and could carry terminal escape sequences into the log.
                    tracing::warn!(filename = ?filename, error = %e, "Failed to export attachment");
                }),
                Err(()) => Err(()),
            };
            match written {
                Ok(path) => report.paths.push(path),
                Err(()) => report.failed += 1,
            }
        });
    }
    progress(total, total);

    Ok(report)
}

/// Generate a folder name for a message's attachments.
fn message_folder_name(entry: &MailEntry, naming: FolderNaming) -> String {
    match naming {
        FolderNaming::DateSubject => {
            let date = filename_date_stamp(&entry.date);
            let subject = sanitize_filename_part(&entry.subject, 60);
            format!("{date}_{subject}")
        }
        FolderNaming::Sequence { start, width } => {
            // Clamped here too: the variant can be built directly.
            let width = width.min(MAX_SEQ_WIDTH);
            format!("{:0width$}", entry.sequence.saturating_add(start))
        }
    }
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

    let (created, mut file) = crate::fsutil::create_unique(path)?;
    if let Err(e) = file.write_all(data).and_then(|()| file.flush()) {
        // A full disk used to leave an empty or cut-short file behind under
        // its final name, looking like a finished export.
        drop(file);
        let _ = std::fs::remove_file(&created);
        return Err(e);
    }
    Ok(created)
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

    #[test]
    fn test_message_folder_name_sequence() {
        use crate::model::address::EmailAddress;
        use chrono::TimeZone;

        let entry = MailEntry {
            offset: 0,
            length: 500,
            date: chrono::Utc.with_ymd_and_hms(2024, 3, 5, 9, 8, 7).unwrap(),
            from: EmailAddress {
                display_name: String::new(),
                address: "a@example.com".to_string(),
            },
            to: Vec::new(),
            cc: Vec::new(),
            subject: "Invoice / March".to_string(),
            message_id: String::new(),
            in_reply_to: None,
            references: Vec::new(),
            has_attachments: true,
            content_type: "multipart/mixed".to_string(),
            text_size: 100,
            labels: Vec::new(),
            thread_id: None,
            sequence: 9, // the 10th message of the mailbox
        };
        let seq = |start, width| FolderNaming::Sequence { start, width };
        assert_eq!(message_folder_name(&entry, seq(1, 4)), "0010");
        assert_eq!(message_folder_name(&entry, seq(0, 4)), "0009");
        // The width is a minimum, never a cut
        assert_eq!(message_folder_name(&entry, seq(1, 1)), "10");
        // An absurd width is clamped instead of panicking in the formatter.
        assert_eq!(
            message_folder_name(&entry, seq(1, 70_000)).len(),
            MAX_SEQ_WIDTH
        );
        assert_eq!(
            FolderNaming::sequence(1, 70_000, 10),
            FolderNaming::Sequence {
                start: 1,
                width: MAX_SEQ_WIDTH
            }
        );
        assert!(
            message_folder_name(&entry, FolderNaming::DateSubject).starts_with("20240305_090807_")
        );
    }

    #[test]
    fn test_sequence_auto_width() {
        // 1000 messages: seq-no ends at 1000 (4 digits), seq-no0 at 999 (3)
        assert_eq!(
            FolderNaming::sequence(1, 0, 1000),
            FolderNaming::Sequence { start: 1, width: 4 }
        );
        assert_eq!(
            FolderNaming::sequence(0, 0, 1000),
            FolderNaming::Sequence { start: 0, width: 3 }
        );
        assert_eq!(
            FolderNaming::sequence(0, 6, 1000),
            FolderNaming::Sequence { start: 0, width: 6 }
        );
        assert_eq!(
            FolderNaming::sequence(1, 0, 0),
            FolderNaming::Sequence { start: 1, width: 1 }
        );
    }
}
