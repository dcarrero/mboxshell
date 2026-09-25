//! Write MBOX mailboxes: merge several into one, or export a selection as a new one.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::fsutil;
use crate::i18n;
use crate::index::builder;
use crate::mailbox_naming;
use crate::model::mail::MailEntry;
use crate::store::reader::MboxStore;

/// Block size used to stream a mailbox through the fast (no-dedup) merge path.
const MERGE_COPY_BLOCK: usize = 1024 * 1024;

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
/// If `dedup` is true, a message is skipped when an earlier one had the same
/// Message-ID **and** the same content (SHA-256 of the raw message minus its
/// `From ` envelope line). Matching on the Message-ID alone would let anyone
/// who can get a message into one of the inputs make a legitimate message
/// disappear, just by reusing its Message-ID. Messages without a Message-ID
/// are never deduplicated.
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
/// The output must not be one of the inputs: that is refused with an error
/// before anything is written.
///
/// The progress callback receives `(current_file, total_files, mailbox_name)`.
pub fn merge_mbox_files(
    inputs: &[PathBuf],
    output: &Path,
    dedup: bool,
    add_source_header: bool,
    progress: &dyn Fn(usize, usize, &str),
) -> anyhow::Result<MergeStats> {
    // Write to a fresh sibling temp file and rename on success, so a mid-merge
    // error never leaves a half-written or corrupt output in place. Buffer the
    // writes to avoid one syscall per message on the dedup path.
    ensure_not_an_input(output, inputs.iter().map(PathBuf::as_path))?;
    let (tmp_output, tmp_file) = fsutil::create_temp_beside(output)?;
    let mut guard = TempFileGuard::new(&tmp_output);
    let mut out_file = std::io::BufWriter::new(tmp_file);
    let mut seen: HashSet<(String, [u8; 32])> = HashSet::new();
    let mut total_messages: u64 = 0;
    let mut duplicates_removed: u64 = 0;
    let mut source_header_added: u64 = 0;
    let total_files = inputs.len();
    // The blank line owed to the previous message, written only once another
    // message follows it. So the merge adds bytes solely at a junction that
    // would otherwise break (a `From ` right after a non-blank line), and a
    // merge of well-formed inputs stays byte-identical to their concatenation.
    let mut pending_gap: &'static [u8] = b"";

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
                let mut raw = store.get_raw_message(entry)?;

                if dedup && !entry.message_id.is_empty() {
                    let key = (entry.message_id.clone(), content_digest(&raw));
                    if seen.contains(&key) {
                        duplicates_removed += 1;
                        continue;
                    }
                    seen.insert(key);
                }

                if add_source_header {
                    raw = inject_source_header(&raw, &source_label);
                    source_header_added += 1;
                }
                out_file.write_all(pending_gap)?;
                out_file.write_all(&raw)?;
                // The last message of a file usually lacks the blank line the
                // next `From ` separator needs.
                pending_gap = separator_padding(&raw);

                total_messages += 1;
            }
        } else {
            // Byte-exact concatenation — no dedup. Never decode as UTF-8 (real
            // mail carries 8-bit bytes) and never rewrite line endings (CRLF
            // must survive for byte-exact archival). Streamed in blocks: a
            // multi-GB mailbox must not be loaded into memory whole.
            //
            // An input that does not end in a blank line would put the next
            // input's `From ` line right after a non-blank one: the last line
            // could swallow it, or a strict reader could merge two messages.
            // The missing blank line is added at that junction only.
            let input = std::fs::File::open(input_path)?;
            let copied =
                copy_counting_from_lines(input, &mut out_file, MERGE_COPY_BLOCK, pending_gap)?;
            if !copied.tail.is_empty() {
                pending_gap = separator_padding(&copied.tail);
            }
            total_messages += copied.messages;
        }
    }
    progress(total_files, total_files, "done");

    // Commit atomically: flush the buffer, then rename the temp file over the
    // destination. On any earlier error the guard removes the temp file and the
    // real output is never touched.
    out_file.flush()?;
    drop(out_file);
    std::fs::rename(&tmp_output, output)?;
    guard.disarm();

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
/// only those. The source file is never touched: an `output` that is the
/// source mailbox itself is refused with an error before anything is written.
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
    ensure_not_an_input(output, std::iter::once(store.path()))?;
    let (tmp_output, tmp_file) = fsutil::create_temp_beside(output)?;
    let mut guard = TempFileGuard::new(&tmp_output);
    let mut out_file = std::io::BufWriter::new(tmp_file);

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
    guard.disarm();

    Ok(total)
}

/// Removes a temp output file on drop unless disarmed, so an export or merge
/// that fails half-way does not leave a stray temp file next to the output.
///
/// Declare it *before* the writer on that file: locals drop in reverse order,
/// so the file handle is closed before the removal (required on Windows).
struct TempFileGuard {
    path: PathBuf,
    armed: bool,
}

impl TempFileGuard {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Refuse to write over a mailbox that is being read: the final rename
/// would replace it. The temp file needs no check — it is always a fresh,
/// uniquely named file (see [`fsutil::create_temp_beside`]).
fn ensure_not_an_input<'a>(
    output: &Path,
    mut inputs: impl Iterator<Item = &'a Path>,
) -> anyhow::Result<()> {
    if let Some(input) = inputs.find(|input| fsutil::same_file(output, input)) {
        anyhow::bail!("{}: {}", i18n::err_output_is_input(), input.display());
    }
    Ok(())
}

/// SHA-256 of a raw message as deduplication evidence.
///
/// The `From ` envelope line (and a leading BOM) is left out: it records when
/// and from where the message was delivered into *that* mailbox, so the same
/// email exported twice legitimately differs there. Trailing line breaks are
/// left out too, because the last message of a file may lack the blank line
/// that separates it from a following one.
fn content_digest(raw: &[u8]) -> [u8; 32] {
    let mut body = raw.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(raw);
    if body.starts_with(b"From ") {
        body = match body.iter().position(|&b| b == b'\n') {
            Some(nl) => &body[nl + 1..],
            None => &[],
        };
    }
    let end = body
        .iter()
        .rposition(|&b| b != b'\n' && b != b'\r')
        .map_or(0, |p| p + 1);
    Sha256::digest(&body[..end]).into()
}

/// What [`copy_counting_from_lines`] saw.
struct CopiedMailbox {
    /// `From ` lines found at the start of a line.
    messages: u64,
    /// The last (up to 4) bytes copied; empty for an empty input.
    tail: Vec<u8>,
}

/// Copy `input` to `out` in blocks of `block_size` bytes, counting the lines
/// that start with `From ` on the way. `gap_before` (the blank line the
/// previous input still owes) is written first, unless the input is empty.
///
/// The line-start state is carried across blocks, so a `From ` split by a
/// block boundary — or starting exactly at one — is counted exactly once.
fn copy_counting_from_lines(
    mut input: impl Read,
    out: &mut impl Write,
    block_size: usize,
    gap_before: &'static [u8],
) -> std::io::Result<CopiedMailbox> {
    let mut buf = vec![0u8; block_size.max(1)];
    let mut counter = FromLineCounter::default();
    let mut gap_before = gap_before;
    let mut tail: Vec<u8> = Vec::with_capacity(8);
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let block = &buf[..n];
        counter.feed(block);
        out.write_all(gap_before)?;
        gap_before = b"";
        out.write_all(block)?;
        tail.extend_from_slice(&block[n.saturating_sub(4)..]);
        let excess = tail.len().saturating_sub(4);
        tail.drain(..excess);
    }
    Ok(CopiedMailbox {
        messages: counter.count,
        tail,
    })
}

/// Streaming counter of lines that start with `From `.
struct FromLineCounter {
    /// Inside the first bytes of a line, still able to become `From `.
    candidate: bool,
    /// How many bytes of `From ` the current candidate has matched.
    matched: usize,
    count: u64,
}

impl Default for FromLineCounter {
    fn default() -> Self {
        // The first byte of the input is the start of a line.
        Self {
            candidate: true,
            matched: 0,
            count: 0,
        }
    }
}

impl FromLineCounter {
    const PATTERN: &'static [u8] = b"From ";

    fn feed(&mut self, block: &[u8]) {
        let mut i = 0;
        loop {
            if self.candidate {
                let need = &Self::PATTERN[self.matched..];
                let avail = &block[i..];
                let n = need.len().min(avail.len());
                if avail[..n] == need[..n] {
                    self.matched += n;
                    i += n;
                    if self.matched < Self::PATTERN.len() {
                        // Block exhausted mid-match: resume with the next one.
                        return;
                    }
                    self.count += 1;
                }
                // Matched or not, this line is settled; a mismatching byte is
                // not consumed, since it may itself be the newline.
                self.candidate = false;
            }
            match memchr::memchr(b'\n', &block[i..]) {
                Some(p) => {
                    i += p + 1;
                    self.candidate = true;
                    self.matched = 0;
                }
                None => return,
            }
        }
    }
}

/// One mbox record: separator line, message, trailing blank line.
///
/// A message read out of an MBOX already carries its own `From ` line and
/// whatever quoting the source used, so it is copied verbatim — rewriting the
/// envelope could only corrupt an archive that was already valid. A message
/// that came from an EML has neither, so both are synthesized. Either way the
/// record ends in a blank line, which the next record's `From ` needs.
pub fn mbox_record(raw: &[u8], entry: &MailEntry) -> Vec<u8> {
    let mut out = if raw.starts_with(b"From ") {
        raw.to_vec()
    } else {
        let mut v = from_line(entry);
        append_from_quoted(&mut v, raw);
        v
    };
    let pad = separator_padding(&out);
    out.extend_from_slice(pad);
    out
}

/// What to append so `data` ends in a blank line — the gap an mbox needs
/// before the next `From ` separator — using the data's own line ending.
/// Empty when it already ends in one (every message but a file's last does).
fn separator_padding(data: &[u8]) -> &'static [u8] {
    if data.is_empty() || data.ends_with(b"\n\n") || data.ends_with(b"\r\n\r\n") {
        b""
    } else if data.ends_with(b"\r\n") {
        b"\r\n"
    } else if data.ends_with(b"\n") {
        b"\n"
    } else {
        b"\n\n"
    }
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
        let raw = b"From user@x.com Thu Jan  4 10:00:00 2024\nSubject: Hi\n\nbody\n\n";
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
        assert_eq!(tmp_files_in(dir.path()), 0);
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
        // `a` lacks a trailing blank line, so exactly one is added at the
        // junction, in its own CRLF; nothing is appended after the last input.
        expected.extend_from_slice(b"\r\n");
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

    #[test]
    fn test_dedup_ignores_forged_message_id() {
        // An attacker's message reusing a legitimate Message-ID, seen first,
        // must not make the legitimate message disappear from the merge.
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mbox");
        let b = dir.path().join("b.mbox");
        std::fs::write(
            &a,
            b"From evil@x Thu Jan 01 00:00:00 2024\nMessage-ID: <victim@id>\nSubject: spoof\n\nnothing\n",
        )
        .unwrap();
        std::fs::write(
            &b,
            b"From boss@x Fri Jan 02 00:00:00 2024\nMessage-ID: <victim@id>\nSubject: contract\n\nthe real one\n",
        )
        .unwrap();

        let out = dir.path().join("out.mbox");
        let stats = merge_mbox_files(&[a, b], &out, true, false, &|_, _, _| {}).unwrap();

        assert_eq!(stats.duplicates_removed, 0);
        assert_eq!(stats.total_messages, 2);
        let merged = String::from_utf8(std::fs::read(&out).unwrap()).unwrap();
        assert!(merged.contains("the real one"));
    }

    #[test]
    fn test_dedup_ignores_envelope_line_differences() {
        // The same email exported twice differs only in its `From ` line (and
        // in the trailing blank line of a last-in-file message): still a copy.
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mbox");
        let b = dir.path().join("b.mbox");
        std::fs::write(
            &a,
            b"From x@y Thu Jan 01 00:00:00 2024\nMessage-ID: <same@id>\nSubject: A\n\nbody\n\n",
        )
        .unwrap();
        std::fs::write(
            &b,
            b"From other@z Sun Mar 03 12:00:00 2024\nMessage-ID: <same@id>\nSubject: A\n\nbody\n",
        )
        .unwrap();

        let out = dir.path().join("out.mbox");
        let stats = merge_mbox_files(&[a, b], &out, true, false, &|_, _, _| {}).unwrap();

        assert_eq!(stats.duplicates_removed, 1);
        assert_eq!(stats.total_messages, 1);
    }

    #[test]
    fn test_dedup_never_drops_messages_without_message_id() {
        let dir = tempfile::tempdir().unwrap();
        let msg: &[u8] = b"From x@y Thu Jan 01 00:00:00 2024\nSubject: A\n\nbody\n";
        let a = dir.path().join("a.mbox");
        let b = dir.path().join("b.mbox");
        std::fs::write(&a, msg).unwrap();
        std::fs::write(&b, msg).unwrap();

        let out = dir.path().join("out.mbox");
        let stats = merge_mbox_files(&[a, b], &out, true, false, &|_, _, _| {}).unwrap();

        assert_eq!(stats.duplicates_removed, 0);
        assert_eq!(stats.total_messages, 2);
    }

    #[test]
    fn test_non_dedup_merge_separates_input_without_trailing_newline() {
        // A mailbox that does not end in a newline used to glue its last line
        // to the next mailbox's `From ` line, losing that message on re-index.
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mbox");
        let b = dir.path().join("b.mbox");
        std::fs::write(
            &a,
            b"From x@y Thu Jan 01 00:00:00 2024\nSubject: A\n\nno newline",
        )
        .unwrap();
        std::fs::write(&b, b"From z@w Fri Jan 02 00:00:00 2024\nSubject: B\n\nhi\n").unwrap();

        let out = dir.path().join("out.mbox");
        let stats = merge_mbox_files(&[a, b], &out, false, false, &|_, _, _| {}).unwrap();

        assert_eq!(stats.total_messages, 2);
        let reindexed = builder::build_index(&out, true, None).unwrap();
        assert_eq!(reindexed.len(), 2, "the reported count must match the file");
        assert_eq!(reindexed[1].subject, "B");
    }

    #[test]
    fn test_dedup_merge_separates_input_without_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mbox");
        let b = dir.path().join("b.mbox");
        std::fs::write(
            &a,
            b"From x@y Thu Jan 01 00:00:00 2024\nMessage-ID: <1@x>\nSubject: A\n\nno newline",
        )
        .unwrap();
        std::fs::write(
            &b,
            b"From z@w Fri Jan 02 00:00:00 2024\nMessage-ID: <2@x>\nSubject: B\n\nhi\n",
        )
        .unwrap();

        let out = dir.path().join("out.mbox");
        let stats = merge_mbox_files(&[a, b], &out, true, false, &|_, _, _| {}).unwrap();

        assert_eq!(stats.total_messages, 2);
        assert_eq!(builder::build_index(&out, true, None).unwrap().len(), 2);
    }

    /// Reference count: lines starting with `From `, computed in one pass.
    fn naive_from_lines(data: &[u8]) -> u64 {
        let mut count = 0;
        let mut at_line_start = true;
        for line in data.split_inclusive(|&b| b == b'\n') {
            if at_line_start && line.starts_with(b"From ") {
                count += 1;
            }
            at_line_start = line.last() == Some(&b'\n');
        }
        count
    }

    #[test]
    fn test_copy_counting_from_line_at_block_start() {
        // With 7-byte blocks, the second `From ` starts exactly at offset 7,
        // the first byte of the second block, right after a newline that ended
        // the first block.
        let data = b"From a\nFrom b\nxFrom c\nFro\nFrom";
        assert_eq!(&data[7..12], b"From ");
        let mut out = Vec::new();
        let copied = copy_counting_from_lines(&data[..], &mut out, 7, b"").unwrap();
        assert_eq!(copied.messages, 2);
        assert_eq!(out, data.to_vec());
        assert_eq!(copied.tail, b"From");
    }

    #[test]
    fn test_copy_counting_matches_single_pass_for_every_block_size() {
        let data: &[u8] = b"From a@b Thu Jan 01 00:00:00 2024\r\nSubject: x\r\n\r\n\
            From here in the body? no, this is a new message by mbox rules\n\
            >From quoted\n\n\nFrom  \nFrom\nFro m\nFFrom x\n\xff\xfeFrom y\nFrom z";
        let expected = naive_from_lines(data);
        assert_eq!(expected, 4);
        for block in 1..=40 {
            let mut out = Vec::new();
            let copied = copy_counting_from_lines(data, &mut out, block, b"").unwrap();
            assert_eq!(copied.messages, expected, "block size {block}");
            assert_eq!(out, data.to_vec(), "block size {block}");
        }
        // An empty input neither writes the owed gap nor owes one itself.
        let mut out = Vec::new();
        let copied = copy_counting_from_lines(&b""[..], &mut out, 4, b"\n").unwrap();
        assert_eq!(
            (copied.messages, copied.tail.is_empty(), out.is_empty()),
            (0, true, true)
        );
    }

    #[test]
    fn test_export_mbox_refuses_to_overwrite_its_source() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source.mbox");
        let original: &[u8] = b"From a@x Thu Jan 01 00:00:00 2024\nSubject: A\n\nbody\n\
              From b@x Fri Jan 02 00:00:00 2024\nSubject: B\n\nhi\n";
        std::fs::write(&src, original).unwrap();
        let entries = builder::build_index(&src, false, None).unwrap();
        let mut store = MboxStore::open(&src).unwrap();
        let selection = vec![&entries[0]];

        // Same path, and the same file reached through a different spelling.
        let spelled = dir.path().join(".").join("source.mbox");
        for target in [src.clone(), spelled] {
            let err = export_mbox(&mut store, &selection, &target, &|_, _| {});
            assert!(err.is_err(), "writing over the source must be refused");
        }
        assert_eq!(std::fs::read(&src).unwrap(), original);
        assert!(!dir.path().join("source.mbox.tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_export_mbox_refuses_source_through_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source.mbox");
        let original: &[u8] = b"From a@x Thu Jan 01 00:00:00 2024\nSubject: A\n\nbody\n";
        std::fs::write(&src, original).unwrap();
        let entries = builder::build_index(&src, false, None).unwrap();
        let mut store = MboxStore::open(&src).unwrap();
        let selection = vec![&entries[0]];

        let sym = dir.path().join("sym.mbox");
        std::os::unix::fs::symlink(&src, &sym).unwrap();
        let hard = dir.path().join("hard.mbox");
        std::fs::hard_link(&src, &hard).unwrap();
        for target in [sym, hard] {
            assert!(export_mbox(&mut store, &selection, &target, &|_, _| {}).is_err());
        }
        assert_eq!(std::fs::read(&src).unwrap(), original);
    }

    #[test]
    fn test_merge_refuses_output_that_is_an_input() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mbox");
        let b = dir.path().join("b.mbox");
        let a_bytes: &[u8] = b"From x@y Thu Jan 01 00:00:00 2024\nSubject: A\n\nbody\n";
        std::fs::write(&a, a_bytes).unwrap();
        std::fs::write(&b, b"From z@w Fri Jan 02 00:00:00 2024\nSubject: B\n\nhi\n").unwrap();

        for dedup in [false, true] {
            let res = merge_mbox_files(&[a.clone(), b.clone()], &a, dedup, false, &|_, _, _| {});
            assert!(res.is_err(), "merging into an input must be refused");
            assert_eq!(std::fs::read(&a).unwrap(), a_bytes);
        }

        // An input named like the old fixed temp file (`c.mbox.tmp`) is safe
        // now: temp files are fresh and uniquely named, so it is only read.
        let tmp_named = dir.path().join("c.mbox.tmp");
        std::fs::write(&tmp_named, a_bytes).unwrap();
        let out = dir.path().join("c.mbox");
        merge_mbox_files(
            std::slice::from_ref(&tmp_named),
            &out,
            false,
            false,
            &|_, _, _| {},
        )
        .unwrap();
        assert_eq!(std::fs::read(&tmp_named).unwrap(), a_bytes);
        assert_eq!(std::fs::read(&out).unwrap(), a_bytes);
    }

    #[test]
    fn test_merge_failure_removes_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mbox");
        std::fs::write(
            &a,
            b"From x@y Thu Jan 01 00:00:00 2024\nSubject: A\n\nbody\n",
        )
        .unwrap();
        let missing = dir.path().join("missing.mbox");

        let out = dir.path().join("out.mbox");
        for dedup in [false, true] {
            let res = merge_mbox_files(
                &[a.clone(), missing.clone()],
                &out,
                dedup,
                false,
                &|_, _, _| {},
            );
            assert!(res.is_err());
            assert!(!out.exists());
            assert_eq!(tmp_files_in(dir.path()), 0, "temp file left behind");
        }
    }

    #[test]
    fn test_export_failure_removes_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source.mbox");
        std::fs::write(
            &src,
            b"From a@x Thu Jan 01 00:00:00 2024\nSubject: A\n\nbody\n",
        )
        .unwrap();
        let entries = builder::build_index(&src, false, None).unwrap();
        let mut store = MboxStore::open(&src).unwrap();

        // An entry pointing past the end of the file fails half-way through.
        let mut broken = entries[0].clone();
        broken.offset = 1 << 40;
        let selection = vec![&entries[0], &broken];
        let out = dir.path().join("sel.mbox");
        assert!(export_mbox(&mut store, &selection, &out, &|_, _| {}).is_err());
        assert!(!out.exists());
        assert_eq!(tmp_files_in(dir.path()), 0, "temp file left behind");
    }

    /// How many `*.tmp` files sit in `dir` (temp names are unique now).
    fn tmp_files_in(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .count()
    }

    #[test]
    fn test_merge_separates_inputs_with_a_blank_line() {
        // Neither input ends in a blank line; the merge must add one, or the
        // second input's first message sits right after a non-blank line.
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mbox");
        let b = dir.path().join("b.mbox");
        std::fs::write(
            &a,
            b"From x@y Thu Jan 01 00:00:00 2024\nMessage-ID: <a@x>\nSubject: A\n\nbody\n",
        )
        .unwrap();
        std::fs::write(
            &b,
            b"From z@w Fri Jan 02 00:00:00 2024\nMessage-ID: <b@x>\nSubject: B\n\nhi\n",
        )
        .unwrap();

        for dedup in [true, false] {
            let out = dir.path().join(format!("out-{dedup}.mbox"));
            merge_mbox_files(&[a.clone(), b.clone()], &out, dedup, false, &|_, _, _| {}).unwrap();
            let merged = String::from_utf8(std::fs::read(&out).unwrap()).unwrap();
            assert!(
                merged.contains("body\n\nFrom z@w"),
                "dedup={dedup}: {merged:?}"
            );
            assert!(
                merged.ends_with("hi\n"),
                "nothing added after the last input"
            );
        }
    }

    #[test]
    fn test_non_dedup_merge_of_well_formed_inputs_is_byte_identical() {
        // Inputs that already end in a blank line need no junction fix, so
        // the output must be their exact concatenation, byte for byte.
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.mbox");
        let b = dir.path().join("b.mbox");
        let empty = dir.path().join("empty.mbox");
        let a_bytes: &[u8] =
            b"From x@y Thu Jan 01 00:00:00 2024\r\nSubject: A\r\n\r\nbody\xff\r\n\r\n";
        let b_bytes: &[u8] = b"From z@w Fri Jan 02 00:00:00 2024\nSubject: B\n\nhi";
        std::fs::write(&a, a_bytes).unwrap();
        std::fs::write(&b, b_bytes).unwrap();
        std::fs::write(&empty, b"").unwrap();

        let out = dir.path().join("out.mbox");
        let inputs = [a, empty, b];
        merge_mbox_files(&inputs, &out, false, false, &|_, _, _| {}).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), [a_bytes, b_bytes].concat());

        // A single input comes out untouched, even without a final newline.
        let single = dir.path().join("single.mbox");
        merge_mbox_files(&inputs[2..], &single, false, false, &|_, _, _| {}).unwrap();
        assert_eq!(std::fs::read(&single).unwrap(), b_bytes);
    }

    #[test]
    fn test_mbox_record_pads_to_a_blank_line() {
        let out = mbox_record(b"Subject: Hi\n\nno trailing newline", &sample_entry());
        assert!(out.ends_with(b"no trailing newline\n\n"));
    }
}
