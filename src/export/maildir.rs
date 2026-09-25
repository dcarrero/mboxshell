//! Export a selection as a Maildir: one file per message in `cur/`.
//!
//! Maildir (<https://cr.yp.to/proto/maildir.html>) keeps each message as a
//! plain RFC 5322 file, so the mbox envelope line is dropped and `>From `
//! quoting is undone. Read/replied/flagged state is carried in the file name
//! (`:2,FRS`), taken from the `Status:` / `X-Status:` headers mail clients
//! write into mbox files, and from Gmail's system labels in Takeout exports.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::export::eml::{skip_from_line, unescape_mboxrd};
use crate::model::mail::MailEntry;
use crate::store::reader::MboxStore;

/// Separator between a Maildir unique name and its info (`:2,FLAGS`).
///
/// `:` is not allowed in Windows file names; `;` is what Windows Maildir
/// tools (mbsync among them) use instead.
#[cfg(not(windows))]
const INFO_SEPARATOR: char = ':';
#[cfg(windows)]
const INFO_SEPARATOR: char = ';';

/// Write `entries` into the Maildir at `dir`, creating `cur/`, `new/` and
/// `tmp/` as needed. An existing Maildir is added to, never overwritten.
///
/// Each message is written to `tmp/` and then renamed into `cur/`, the
/// Maildir delivery protocol, so a reader never sees a half-written file.
/// The progress callback receives `(current, total)`; returns how many
/// messages were written.
pub fn export_maildir(
    store: &mut MboxStore,
    entries: &[&MailEntry],
    dir: &Path,
    progress: &dyn Fn(usize, usize),
) -> anyhow::Result<usize> {
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    let pid = std::process::id();
    // When this export started, in microseconds: two exports into the same
    // Maildir (same message dates, maybe the same pid) never share a name.
    let run = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0);

    let total = entries.len();
    for (i, entry) in entries.iter().enumerate() {
        progress(i, total);
        let raw = store.get_raw_message(entry)?;
        let message = unescape_mboxrd(skip_from_line(&raw));
        let flags = maildir_flags(&message, &entry.labels);

        // `time.MusecPpidQn.host`: the message date keeps a name-sorted
        // listing in date order; run time + pid + sequence make it unique.
        let unique = format!(
            "{}.M{run}P{pid}Q{i}.mboxshell",
            entry.date.timestamp().max(0)
        );
        let (tmp_path, mut file) = create_unique(dir, &unique, &flags)?;
        file.write_all(&message)?;
        // Many clients sort by file time; make it the message date.
        let _ = file.set_modified(std::time::SystemTime::from(entry.date));
        drop(file);

        std::fs::rename(&tmp_path, final_path(dir, &tmp_path, &flags))?;
    }
    progress(total, total);
    Ok(total)
}

/// Where a message written at `tmp_path` ends up: `cur/<name>:2,<flags>`.
fn final_path(maildir: &Path, tmp_path: &Path, flags: &str) -> PathBuf {
    let name = tmp_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    maildir
        .join("cur")
        .join(format!("{name}{INFO_SEPARATOR}2,{flags}"))
}

/// Create `tmp/<name>` with `create_new`, adding a suffix while that name is
/// taken in `tmp/` or its final name already exists in `cur/` (the rename
/// into `cur/` would otherwise replace an existing message).
fn create_unique(
    maildir: &Path,
    name: &str,
    flags: &str,
) -> std::io::Result<(PathBuf, std::fs::File)> {
    let mut attempt = 0u32;
    loop {
        let candidate = if attempt == 0 {
            maildir.join("tmp").join(name)
        } else {
            maildir.join("tmp").join(format!("{name}_{attempt}"))
        };
        attempt += 1;
        if final_path(maildir, &candidate, flags).exists() {
            continue;
        }
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(f) => return Ok((candidate, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

/// Maildir info flags for a message, in the ASCII order the spec requires.
///
/// - `Status: R` (read) → `S` (seen)
/// - `X-Status: A` (answered) → `R` (replied)
/// - `X-Status: F` (flagged) → `F`
/// - `X-Status: D` (deleted) → `T` (trashed)
/// - `X-Status: T` (draft) → `D`
///
/// Gmail/Takeout mailboxes carry no `Status:` but label messages
/// `Opened`/`Unread` and `Starred`; those map to `S` and `F`.
fn maildir_flags(message: &[u8], labels: &[String]) -> String {
    let status = header_value(message, "status").unwrap_or_default();
    let x_status = header_value(message, "x-status").unwrap_or_default();
    let has_label = |name: &str| labels.iter().any(|l| l.eq_ignore_ascii_case(name));

    let draft = x_status.contains('T');
    let flagged = x_status.contains('F') || has_label("Starred");
    let replied = x_status.contains('A');
    let seen = status.contains('R') || (has_label("Opened") && !has_label("Unread"));
    let trashed = x_status.contains('D');

    [
        (draft, 'D'),
        (flagged, 'F'),
        (replied, 'R'),
        (seen, 'S'),
        (trashed, 'T'),
    ]
    .into_iter()
    .filter_map(|(on, c)| on.then_some(c))
    .collect()
}

/// Value of the first header called `name` (case-insensitive), searched only
/// in the header block. Folded continuation lines are ignored: `Status:` and
/// `X-Status:` are single short tokens.
fn header_value(message: &[u8], name: &str) -> Option<String> {
    for line in message.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            break; // end of headers
        }
        let Some(colon) = line.iter().position(|&b| b == b':') else {
            continue;
        };
        if line[..colon].eq_ignore_ascii_case(name.as_bytes()) {
            return Some(
                String::from_utf8_lossy(&line[colon + 1..])
                    .trim()
                    .to_string(),
            );
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::builder;

    #[test]
    fn test_maildir_flags_from_status_headers() {
        let msg = b"Status: RO\r\nX-Status: AF\r\nSubject: x\r\n\r\nStatus: body is not a header\n";
        assert_eq!(maildir_flags(msg, &[]), "FRS");
        assert_eq!(maildir_flags(b"Subject: x\n\nbody\n", &[]), "");
        assert_eq!(maildir_flags(b"X-Status: DT\n\n", &[]), "DT");
    }

    #[test]
    fn test_maildir_flags_from_gmail_labels() {
        let labels = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let msg = b"Subject: x\n\nbody\n";
        assert_eq!(
            maildir_flags(msg, &labels(&["Inbox", "Opened", "Starred"])),
            "FS"
        );
        assert_eq!(maildir_flags(msg, &labels(&["Inbox", "Unread"])), "");
    }

    #[test]
    fn test_export_maildir_writes_one_clean_file_per_message() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.mbox");
        std::fs::write(
            &src,
            b"From a@b Thu Jan 04 10:00:00 2024\nStatus: RO\nSubject: A\n\n>From here\nbody\n\n\
              From c@d Fri Jan 05 10:00:00 2024\nSubject: B\n\nhi\n",
        )
        .unwrap();
        let entries = builder::build_index(&src, true, None).unwrap();
        let mut store = MboxStore::open(&src).unwrap();
        let selection: Vec<&MailEntry> = entries.iter().collect();

        let maildir = dir.path().join("Maildir");
        assert_eq!(
            export_maildir(&mut store, &selection, &maildir, &|_, _| {}).unwrap(),
            2
        );
        // Exporting again adds, never overwrites.
        export_maildir(&mut store, &selection, &maildir, &|_, _| {}).unwrap();

        assert_eq!(std::fs::read_dir(maildir.join("tmp")).unwrap().count(), 0);
        assert!(maildir.join("new").is_dir());
        let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(maildir.join("cur"))
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (
                    e.file_name().to_string_lossy().into_owned(),
                    std::fs::read(e.path()).unwrap(),
                )
            })
            .collect();
        files.sort();
        assert_eq!(files.len(), 4);

        let sep = INFO_SEPARATOR;
        let (name_a, body_a) = files
            .iter()
            .find(|(_, b)| b.starts_with(b"Status"))
            .unwrap();
        assert!(name_a.ends_with(&format!("{sep}2,S")), "{name_a}");
        assert_eq!(
            body_a.as_slice(),
            b"Status: RO\nSubject: A\n\nFrom here\nbody\n"
        );
        let (name_b, body_b) = files
            .iter()
            .find(|(_, b)| b.starts_with(b"Subject: B"))
            .unwrap();
        assert!(name_b.ends_with(&format!("{sep}2,")), "{name_b}");
        assert_eq!(body_b.as_slice(), b"Subject: B\n\nhi\n");
    }
}
