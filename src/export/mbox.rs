//! Write MBOX mailboxes: merge several into one, or export a selection as a new one.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::index::builder;
use crate::mailbox_naming;
use crate::model::mail::MailEntry;
use crate::store::reader::MboxStore;

/// Statistics returned by a merge operation.
#[derive(Debug)]
pub struct MergeStats {
    pub total_messages: u64,
    pub duplicates_removed: u64,
    pub output_size: u64,
    pub input_files: usize,
    /// Number of messages that got an `X-Mbox-Source` header injected.
    pub source_header_added: u64,
}

/// Merge multiple MBOX files into a single output file.
///
/// If `dedup` is true, messages with duplicate Message-IDs are skipped
/// (the first occurrence is kept).
///
/// If `add_source_header` is true, every message gets an
/// `X-Mbox-Source: <mailbox name>` header injected as its first header, so
/// the merged archive stays traceable back to which mailbox each email came
/// from. This forces the per-message path (it needs message boundaries), so it
/// is slower than the raw byte-exact block copy used by a plain no-dedup merge.
///
/// The mailbox name is the one the user sees (`Inbox.mbox`, not Apple Mail's
/// inner `mbox` file), disambiguated across the inputs when two of them would
/// otherwise share a name — see [`crate::mailbox_naming`].
///
/// The progress callback receives `(current_file, total_files, mailbox_name)`.
pub fn merge_mbox_files(
    inputs: &[PathBuf],
    output: &Path,
    dedup: bool,
    add_source_header: bool,
    progress: &dyn Fn(usize, usize, &str),
) -> anyhow::Result<MergeStats> {
    // Write to a sibling temp file and rename on success, so a mid-merge error
    // never leaves a half-written or corrupt output in place. Buffer the writes
    // to avoid one syscall per message on the dedup path.
    let tmp_output = output.with_extension("mbox.tmp");
    let mut out_file = std::io::BufWriter::new(std::fs::File::create(&tmp_output)?);
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut total_messages: u64 = 0;
    let mut duplicates_removed: u64 = 0;
    let mut source_header_added: u64 = 0;
    let total_files = inputs.len();

    // Name every input the way the user sees it, disambiguated as a set: taking
    // `file_name()` here would label every Apple Mail package "mbox".
    let mailbox_names = mailbox_naming::unique_display_names(inputs);

    for (file_idx, input_path) in inputs.iter().enumerate() {
        let filename = mailbox_names[file_idx].as_str();
        progress(file_idx, total_files, filename);

        // Both dedup and source-header injection need per-message boundaries, so
        // they share the parsing path. A plain no-dedup / no-header merge stays
        // on the fast raw block copy below.
        if dedup || add_source_header {
            // The source label is the mailbox name (e.g. "Inbox.mbox"),
            // sanitized so a crafted name can't inject extra headers. A
            // disambiguated name may carry a `/`, harmless in a header value.
            let source_label = if add_source_header {
                sanitize_header_value(filename)
            } else {
                String::new()
            };

            // Index to get Message-IDs, then copy (and optionally tag) messages.
            let entries = builder::build_index(input_path, false, None)?;
            let mut store = crate::store::reader::MboxStore::open(input_path)?;

            for entry in &entries {
                if dedup {
                    let id = &entry.message_id;
                    if !id.is_empty() && seen_ids.contains(id) {
                        duplicates_removed += 1;
                        continue;
                    }
                    if !id.is_empty() {
                        seen_ids.insert(id.clone());
                    }
                }

                let mut raw = store.get_raw_message(entry)?;
                if add_source_header {
                    raw = inject_source_header(&raw, &source_label);
                    source_header_added += 1;
                }
                out_file.write_all(&raw)?;

                // Ensure there's a newline separator between messages
                if !raw.ends_with(b"\n") {
                    out_file.write_all(b"\n")?;
                }

                total_messages += 1;
            }
        } else {
            // Byte-exact concatenation — no dedup. Never decode as UTF-8 (real
            // mail carries 8-bit bytes that would abort `lines()`) and never
            // rewrite line endings (CRLF must survive for byte-exact archival).
            let bytes = std::fs::read(input_path)?;
            let mut message_count: u64 = 0;
            let mut at_line_start = true;
            for window in bytes.split_inclusive(|&b| b == b'\n') {
                if at_line_start && window.starts_with(b"From ") {
                    message_count += 1;
                }
                at_line_start = window.last() == Some(&b'\n');
            }
            out_file.write_all(&bytes)?;

            total_messages += message_count;
        }
    }
    progress(total_files, total_files, "done");

    // Commit atomically: flush the buffer, then rename the temp file over the
    // destination. On any earlier error the temp file is left behind (harmless)
    // and the real output is never touched.
    out_file.flush()?;
    drop(out_file);
    std::fs::rename(&tmp_output, output)?;

    let output_size = std::fs::metadata(output)?.len();

    Ok(MergeStats {
        total_messages,
        duplicates_removed,
        output_size,
        input_files: total_files,
        source_header_added,
    })
}

/// Write `entries` out as a single new MBOX mailbox at `output`.
///
/// This is the delivery half of the tool: filter a large archive down to the
/// messages that actually belong in a handover — a legal request, a records
/// request, a mailbox someone else has to read — and produce a mailbox holding
/// only those. The source file is never touched.
///
/// The progress callback receives `(current, total)` and returns the number of
/// messages written.
pub fn export_mbox(
    store: &mut MboxStore,
    entries: &[&MailEntry],
    output: &Path,
    progress: &dyn Fn(usize, usize),
) -> anyhow::Result<usize> {
    // Same commit discipline as the merge: write to a sibling temp file and
    // rename on success, so a mid-export error never leaves a half-written
    // mailbox behind under the name the user asked for.
    let tmp_output = output.with_extension("mbox.tmp");
    let mut out_file = std::io::BufWriter::new(std::fs::File::create(&tmp_output)?);

    let total = entries.len();
    for (i, entry) in entries.iter().enumerate() {
        progress(i, total);
        let raw = store.get_raw_message(entry)?;
        out_file.write_all(&mbox_record(&raw, entry))?;
    }
    progress(total, total);

    out_file.flush()?;
    drop(out_file);
    std::fs::rename(&tmp_output, output)?;

    Ok(total)
}

/// One mbox record: separator line, message, trailing newline.
///
/// A message read out of an MBOX already carries its own `From ` line and
/// whatever quoting the source used, so it is copied verbatim — rewriting the
/// envelope could only corrupt an archive that was already valid. A message
/// that came from an EML has neither, so both are synthesized.
pub fn mbox_record(raw: &[u8], entry: &MailEntry) -> Vec<u8> {
    let mut out = if raw.starts_with(b"From ") {
        raw.to_vec()
    } else {
        let mut v = from_line(entry);
        append_from_quoted(&mut v, raw);
        v
    };
    if out.last() != Some(&b'\n') {
        out.push(b'\n');
    }
    out
}

/// `From sender Thu Jan  4 09:00:00 2024` — the mbox separator line.
///
/// The timestamp is C `asctime` in UTC with the day-of-month space-padded to
/// two columns (`%e`). It must not follow the machine's locale; chrono's
/// weekday and month names are fixed English, so the format stays stable.
fn from_line(entry: &MailEntry) -> Vec<u8> {
    let stamp = entry.date.format("%a %b %e %H:%M:%S %Y");

    // Whitespace inside the address would split the line into extra fields.
    let address: String = entry
        .from
        .address
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let sender = if address.is_empty() {
        "MAILER-DAEMON"
    } else {
        address.as_str()
    };

    format!("From {sender} {stamp}\n").into_bytes()
}

/// Append `raw`, prefixing body lines that start with `From ` with `>` so they
/// are not read back as the start of the next message.
fn append_from_quoted(out: &mut Vec<u8>, raw: &[u8]) {
    out.reserve(raw.len());
    for line in raw.split_inclusive(|&b| b == b'\n') {
        if line.starts_with(b"From ") {
            out.push(b'>');
        }
        out.extend_from_slice(line);
    }
}

/// Insert an `X-Mbox-Source: <source>` header into a raw MBOX message.
///
/// The header is placed right after the `From ` envelope line (so it becomes
/// the first real RFC 5322 header) and matches the message's own line
/// terminator (CRLF vs LF). A message without an envelope line gets the header
/// prepended. Any leading UTF-8 BOM is preserved. Header injection is safe
/// because `source` is sanitized by the caller.
fn inject_source_header(raw: &[u8], source: &str) -> Vec<u8> {
    // Skip a UTF-8 BOM if the very first message of a file carries one.
    let start = if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else {
        0
    };

    let body = &raw[start..];
    if body.starts_with(b"From ") {
        if let Some(rel_nl) = body.iter().position(|&b| b == b'\n') {
            // Index of the newline ending the envelope line, relative to `raw`.
            let nl = start + rel_nl;
            // Match the envelope line's terminator so we don't mix CRLF and LF.
            let terminator: &[u8] = if nl > 0 && raw[nl - 1] == 0x0D {
                b"\r\n"
            } else {
                b"\n"
            };
            let insert_pos = nl + 1;

            let mut out = Vec::with_capacity(raw.len() + source.len() + 18);
            out.extend_from_slice(&raw[..insert_pos]);
            out.extend_from_slice(b"X-Mbox-Source: ");
            out.extend_from_slice(source.as_bytes());
            out.extend_from_slice(terminator);
            out.extend_from_slice(&raw[insert_pos..]);
            return out;
        }
    }

    // No envelope line: prepend the header (after any BOM).
    let mut out = Vec::with_capacity(raw.len() + source.len() + 18);
    out.extend_from_slice(&raw[..start]);
    out.extend_from_slice(b"X-Mbox-Source: ");
    out.extend_from_slice(source.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(&raw[start..]);
    out
}

/// Strip control characters (CR/LF/NUL/DEL…) so an origin file name can never
/// break out of its header value and inject additional headers.
fn sanitize_header_value(value: &str) -> String {
    value
        .chars()
        .filter(|c| {
            let u = *c as u32;
            u >= 0x20 && u != 0x7F
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A message stamped 2024-01-04 09:00:00 UTC — a single-digit day, so the
    /// two-column padding of the `From ` line is actually exercised.
    fn sample_entry() -> MailEntry {
        use crate::model::address::EmailAddress;
        use chrono::TimeZone;
        MailEntry {
            offset: 0,
            length: 100,
            date: chrono::Utc.with_ymd_and_hms(2024, 1, 4, 9, 0, 0).unwrap(),
            from: EmailAddress {
                display_name: "Test User".to_string(),
                address: "test@example.com".to_string(),
            },
            to: vec![],
            cc: vec![],
            subject: "Hello".to_string(),
            message_id: "<msg@test>".to_string(),
            in_reply_to: None,
            references: vec![],
            has_attachments: false,
            content_type: "text/plain".to_string(),
            text_size: 50,
            labels: vec![],
            sequence: 0,
            thread_id: None,
        }
    }

    #[test]
    fn test_mbox_record_keeps_mbox_message_verbatim() {
        let raw = b"From user@x.com Thu Jan  4 10:00:00 2024\nSubject: Hi\n\nbody\n";
        let out = mbox_record(raw, &sample_entry());
        // A message that already came out of a mailbox is copied as-is:
        // rewriting its envelope line could only corrupt a valid archive.
        assert_eq!(out, raw.to_vec());
    }

    #[test]
    fn test_mbox_record_adds_envelope_line_for_eml() {
        let raw = b"Subject: Hi\n\nbody\n";
        let out = String::from_utf8(mbox_record(raw, &sample_entry())).unwrap();
        // asctime, UTC, day space-padded to two columns and locale-independent.
        assert!(
            out.starts_with("From test@example.com Thu Jan  4 09:00:00 2024\n"),
            "unexpected envelope line: {out}"
        );
        assert!(out.contains("Subject: Hi"));
    }

    #[test]
    fn test_mbox_record_quotes_from_lines_in_eml_body() {
        let raw = b"Subject: Hi\n\nFrom here it broke\nok\n";
        let out = String::from_utf8(mbox_record(raw, &sample_entry())).unwrap();
        // Otherwise that body line reads back as the start of the next message.
        assert!(out.contains("\n>From here it broke\n"), "not quoted: {out}");
        assert!(out.contains("\nok\n"));
    }

    #[test]
    fn test_mbox_record_ends_with_newline() {
        let raw = b"Subject: Hi\n\nno trailing newline";
        let out = mbox_record(raw, &sample_entry());
        assert_eq!(out.last(), Some(&b'\n'));
    }

    #[test]
    fn test_from_line_falls_back_to_mailer_daemon() {
        let mut entry = sample_entry();
        entry.from.address = String::new();
        let line = String::from_utf8(from_line(&entry)).unwrap();
        // An empty sender would leave "From  Thu…", which parses as a message
        // whose sender is the weekday.
        assert!(line.starts_with("From MAILER-DAEMON "), "got: {line}");
    }

    #[test]
    fn test_from_line_strips_whitespace_from_address() {
        let mut entry = sample_entry();
        entry.from.address = "a b@x.com".to_string();
        let line = String::from_utf8(from_line(&entry)).unwrap();
        // Whitespace would split the line into extra fields.
        assert!(line.starts_with("From ab@x.com Thu Jan  4 "), "got: {line}");
    }

    #[test]
    fn test_export_mbox_writes_a_readable_mailbox() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source.mbox");
        std::fs::write(
            &src,
            b"From a@x Thu Jan 01 00:00:00 2024\nMessage-ID: <1@x>\nSubject: A\n\nbody\n\
              From b@x Fri Jan 02 00:00:00 2024\nMessage-ID: <2@x>\nSubject: B\n\nhi\n\
              From c@x Sat Jan 03 00:00:00 2024\nMessage-ID: <3@x>\nSubject: C\n\nbye\n",
        )
        .unwrap();

        let entries = builder::build_index(&src, false, None).unwrap();
        assert_eq!(entries.len(), 3);
        let mut store = MboxStore::open(&src).unwrap();

        // Export a selection — the whole point: only these go in the handover.
        let selection = vec![&entries[0], &entries[2]];
        let out = dir.path().join("selection.mbox");
        let n = export_mbox(&mut store, &selection, &out, &|_, _| {}).unwrap();
        assert_eq!(n, 2);

        // The result must be a mailbox the tool can read back.
        let reexported = builder::build_index(&out, true, None).unwrap();
        assert_eq!(
            reexported.len(),
            2,
            "the export must re-index as 2 messages"
        );
        assert_eq!(reexported[0].subject, "A");
        assert_eq!(reexported[1].subject, "C");
        // The source is never touched.
        assert_eq!(builder::build_index(&src, true, None).unwrap().len(), 3);
        // And no temp file is left behind on success.
        assert!(!dir.path().join("selection.mbox.tmp").exists());
    }

    #[test]
    fn test_non_dedup_merge_preserves_bytes() {
        let dir = tempfile::tempdir().unwrap();
        // Inputs carry a non-UTF-8 byte (0xff) and CRLF line endings — the old
        // `lines()` path would abort on 0xff and rewrite CRLF to LF.
        let a = dir.path().join("a.mbox");
        let b = dir.path().join("b.mbox");
        let a_bytes: &[u8] = b"From x@y Thu Jan 01 00:00:00 2024\r\nSubject: A\r\n\r\nbody\xff\r\n";
        let b_bytes: &[u8] = b"From z@w Fri Jan 02 00:00:00 2024\r\nSubject: B\r\n\r\nhi\r\n";
        std::fs::write(&a, a_bytes).unwrap();
        std::fs::write(&b, b_bytes).unwrap();

        let out = dir.path().join("out.mbox");
        let stats = merge_mbox_files(&[a, b], &out, false, false, &|_, _, _| {}).unwrap();

        let merged = std::fs::read(&out).unwrap();
        let mut expected = Vec::new();
        expected.extend_from_slice(a_bytes);
        expected.extend_from_slice(b_bytes);
        assert_eq!(merged, expected, "bytes must be concatenated verbatim");
        assert_eq!(stats.total_messages, 2);
    }

    #[test]
    fn test_dedup_merge_removes_duplicate_message_id() {
        let dir = tempfile::tempdir().unwrap();
        let msg: &[u8] =
            b"From x@y Thu Jan 01 00:00:00 2024\nMessage-ID: <same@id>\nSubject: A\n\nbody\n";
        let a = dir.path().join("a.mbox");
        let b = dir.path().join("b.mbox");
        std::fs::write(&a, msg).unwrap();
        std::fs::write(&b, msg).unwrap();

        let out = dir.path().join("out.mbox");
        let stats = merge_mbox_files(&[a, b], &out, true, false, &|_, _, _| {}).unwrap();

        assert_eq!(stats.duplicates_removed, 1);
        assert_eq!(stats.total_messages, 1);
        assert_eq!(stats.source_header_added, 0);
    }

    #[test]
    fn test_inject_source_header_after_envelope_lf() {
        let raw = b"From x@y Thu Jan 01 00:00:00 2024\nSubject: A\n\nbody\n";
        let out = inject_source_header(raw, "Inbox.mbox");
        assert_eq!(
            out,
            b"From x@y Thu Jan 01 00:00:00 2024\nX-Mbox-Source: Inbox.mbox\nSubject: A\n\nbody\n"
                .to_vec(),
            "header must be the first real header, after the From_ line, with LF"
        );
    }

    #[test]
    fn test_inject_source_header_preserves_crlf() {
        let raw = b"From x@y Thu Jan 01 00:00:00 2024\r\nSubject: A\r\n\r\nbody\r\n";
        let out = inject_source_header(raw, "Sent");
        assert_eq!(
            out,
            b"From x@y Thu Jan 01 00:00:00 2024\r\nX-Mbox-Source: Sent\r\nSubject: A\r\n\r\nbody\r\n"
                .to_vec(),
            "the injected header must reuse the message's CRLF terminator"
        );
    }

    #[test]
    fn test_inject_source_header_preserves_bom() {
        let mut raw = vec![0xEF, 0xBB, 0xBF];
        raw.extend_from_slice(b"From x@y Thu Jan 01 00:00:00 2024\nSubject: A\n\nbody\n");
        let out = inject_source_header(&raw, "Inbox");
        let mut expected = vec![0xEF, 0xBB, 0xBF];
        expected.extend_from_slice(
            b"From x@y Thu Jan 01 00:00:00 2024\nX-Mbox-Source: Inbox\nSubject: A\n\nbody\n",
        );
        assert_eq!(out, expected, "a leading UTF-8 BOM must be preserved");
    }

    #[test]
    fn test_inject_source_header_no_envelope_prepends() {
        let raw = b"Subject: A\n\nbody\n";
        let out = inject_source_header(raw, "orphan");
        assert_eq!(
            out,
            b"X-Mbox-Source: orphan\nSubject: A\n\nbody\n".to_vec(),
            "a message with no From_ line gets the header prepended"
        );
    }

    #[test]
    fn test_sanitize_header_value_strips_control_chars() {
        // A crafted file name trying to inject a second header.
        let dirty = "evil\r\nBcc: attacker@example.com";
        assert_eq!(
            sanitize_header_value(dirty),
            "evilBcc: attacker@example.com",
            "CR/LF must be stripped so no extra header can be injected"
        );
        assert_eq!(sanitize_header_value("  Inbox.mbox  "), "Inbox.mbox");
    }

    #[test]
    fn test_source_header_merge_tags_every_message() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("Inbox.mbox");
        let b = dir.path().join("Sent.mbox");
        std::fs::write(
            &a,
            b"From x@y Thu Jan 01 00:00:00 2024\nSubject: A\n\nbody\n",
        )
        .unwrap();
        std::fs::write(&b, b"From z@w Fri Jan 02 00:00:00 2024\nSubject: B\n\nhi\n").unwrap();

        let out = dir.path().join("out.mbox");
        // dedup off, source header on: proves the two options are independent.
        let stats = merge_mbox_files(&[a, b], &out, false, true, &|_, _, _| {}).unwrap();

        assert_eq!(stats.total_messages, 2);
        assert_eq!(stats.source_header_added, 2);
        let merged = String::from_utf8(std::fs::read(&out).unwrap()).unwrap();
        assert!(merged.contains("X-Mbox-Source: Inbox.mbox"));
        assert!(merged.contains("X-Mbox-Source: Sent.mbox"));
    }

    #[test]
    fn test_source_header_names_apple_mail_packages() {
        // Apple Mail stores each mailbox as a DIRECTORY "Inbox.mbox" holding a
        // file literally called "mbox" — the path the app actually reads. The
        // header must carry the package name, not "mbox".
        let dir = tempfile::tempdir().unwrap();
        let inbox_pkg = dir.path().join("Inbox.mbox");
        let sent_pkg = dir.path().join("Sent.mbox");
        std::fs::create_dir(&inbox_pkg).unwrap();
        std::fs::create_dir(&sent_pkg).unwrap();

        let inbox = inbox_pkg.join("mbox");
        let sent = sent_pkg.join("mbox");
        std::fs::write(
            &inbox,
            b"From x@y Thu Jan 01 00:00:00 2024\nSubject: A\n\nbody\n",
        )
        .unwrap();
        std::fs::write(
            &sent,
            b"From z@w Fri Jan 02 00:00:00 2024\nSubject: B\n\nhi\n",
        )
        .unwrap();

        let out = dir.path().join("out.mbox");
        let stats = merge_mbox_files(&[inbox, sent], &out, true, true, &|_, _, _| {}).unwrap();

        assert_eq!(stats.source_header_added, 2);
        let merged = String::from_utf8(std::fs::read(&out).unwrap()).unwrap();
        assert!(merged.contains("X-Mbox-Source: Inbox.mbox"));
        assert!(merged.contains("X-Mbox-Source: Sent.mbox"));
        assert!(
            !merged.contains("X-Mbox-Source: mbox"),
            "the inner file name must never be used as the source label"
        );
    }

    #[test]
    fn test_source_header_disambiguates_same_named_packages() {
        // Two accounts, both with an "Inbox.mbox": the labels must stay
        // distinguishable, otherwise the header can't say where a mail is from.
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("Work").join("Inbox.mbox");
        let personal = dir.path().join("Personal").join("Inbox.mbox");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(&personal).unwrap();

        let a = work.join("mbox");
        let b = personal.join("mbox");
        std::fs::write(
            &a,
            b"From x@y Thu Jan 01 00:00:00 2024\nSubject: A\n\nbody\n",
        )
        .unwrap();
        std::fs::write(&b, b"From z@w Fri Jan 02 00:00:00 2024\nSubject: B\n\nhi\n").unwrap();

        let out = dir.path().join("out.mbox");
        merge_mbox_files(&[a, b], &out, true, true, &|_, _, _| {}).unwrap();

        let merged = String::from_utf8(std::fs::read(&out).unwrap()).unwrap();
        assert!(merged.contains("X-Mbox-Source: Work/Inbox.mbox"));
        assert!(merged.contains("X-Mbox-Source: Personal/Inbox.mbox"));
    }
}
