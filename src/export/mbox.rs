//! Merge multiple MBOX files into one.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
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
    let mut out_file = std::fs::File::create(output)?;
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
            // Simple concatenation — no dedup, just copy bytes
            let in_file = std::fs::File::open(input_path)?;
            let reader = BufReader::new(in_file);
            let mut message_count: u64 = 0;

            for line in reader.lines() {
                let line = line?;
                if line.starts_with("From ") {
                    message_count += 1;
                }
                writeln!(out_file, "{line}")?;
            }

            total_messages += message_count;
        }
    }
    progress(total_files, total_files, "done");

    let output_size = std::fs::metadata(output)?.len();

    Ok(MergeStats {
        total_messages,
        duplicates_removed,
        output_size,
        input_files: total_files,
    })
}
