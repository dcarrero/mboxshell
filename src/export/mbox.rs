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
}

/// Merge multiple MBOX files into a single output file.
///
/// If `dedup` is true, messages with duplicate Message-IDs are skipped
/// (the first occurrence is kept).
///
/// The progress callback receives `(current_file, total_files, filename)`.
pub fn merge_mbox_files(
    inputs: &[PathBuf],
    output: &Path,
    dedup: bool,
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
    let total_files = inputs.len();

    for (file_idx, input_path) in inputs.iter().enumerate() {
        let filename = input_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| input_path.to_string_lossy().to_string());
        progress(file_idx, total_files, &filename);

        if dedup {
            // Index to get Message-IDs, then copy non-duplicate messages
            let entries = builder::build_index(input_path, false, None)?;
            let mut store = crate::store::reader::MboxStore::open(input_path)?;

            for entry in &entries {
                let id = entry.message_id.clone();
                if dedup && !id.is_empty() && seen_ids.contains(&id) {
                    duplicates_removed += 1;
                    continue;
                }

                let raw = store.get_raw_message(entry)?;
                out_file.write_all(&raw)?;

                // Ensure there's a newline separator between messages
                if !raw.ends_with(b"\n") {
                    out_file.write_all(b"\n")?;
                }

                if !id.is_empty() {
                    seen_ids.insert(id);
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
    })
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
        let stats = merge_mbox_files(&[a, b], &out, false, &|_, _, _| {}).unwrap();

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
        let stats = merge_mbox_files(&[a, b], &out, true, &|_, _, _| {}).unwrap();

        assert_eq!(stats.duplicates_removed, 1);
        assert_eq!(stats.total_messages, 1);
    }
}
