//! Merge multiple MBOX files into one.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::index::builder;

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
/// `X-Mbox-Source: <origin file name>` header injected as its first header, so
/// the merged archive stays traceable back to which mailbox each email came
/// from. This forces the per-message path (it needs message boundaries), so it
/// is slower than the raw byte-exact block copy used by a plain no-dedup merge.
///
/// The progress callback receives `(current_file, total_files, filename)`.
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

    for (file_idx, input_path) in inputs.iter().enumerate() {
        let filename = input_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| input_path.to_string_lossy().to_string());
        progress(file_idx, total_files, &filename);

        // Both dedup and source-header injection need per-message boundaries, so
        // they share the parsing path. A plain no-dedup / no-header merge stays
        // on the fast raw block copy below.
        if dedup || add_source_header {
            // The source label is the origin file name (e.g. "Inbox.mbox"),
            // sanitized so a crafted name can't inject extra headers.
            let source_label = if add_source_header {
                sanitize_header_value(&filename)
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
            let nl = start + rel_nl; // index of the '\n' in `raw`
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
        std::fs::write(
            &b,
            b"From z@w Fri Jan 02 00:00:00 2024\nSubject: B\n\nhi\n",
        )
        .unwrap();

        let out = dir.path().join("out.mbox");
        // dedup off, source header on: proves the two options are independent.
        let stats = merge_mbox_files(&[a, b], &out, false, true, &|_, _, _| {}).unwrap();

        assert_eq!(stats.total_messages, 2);
        assert_eq!(stats.source_header_added, 2);
        let merged = String::from_utf8(std::fs::read(&out).unwrap()).unwrap();
        assert!(merged.contains("X-Mbox-Source: Inbox.mbox"));
        assert!(merged.contains("X-Mbox-Source: Sent.mbox"));
    }
}
