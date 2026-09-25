//! Index construction, validation, and persistence.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

use crate::error::MboxError;
use crate::fsutil;
use crate::index::format::{IndexHeader, HASH_PREFIX_LEN, HEADER_SIZE, MAGIC, VERSION};
use crate::model::mail::MailEntry;
use crate::parser::header;
use crate::parser::mbox::MboxParser;

/// Build (or load) the index for an MBOX file.
///
/// 1. If a valid index already exists and `force_rebuild` is false, load it.
/// 2. Otherwise, parse headers of all messages and write a new index file.
///
/// Returns the list of [`MailEntry`] for every message in the MBOX.
pub fn build_index(
    mbox_path: &Path,
    force_rebuild: bool,
    progress: Option<&dyn Fn(u64, u64)>,
) -> anyhow::Result<Vec<MailEntry>> {
    build_index_cancelable(mbox_path, force_rebuild, progress, &|| false)
}

/// Como [`build_index`] pero cancelable: `should_cancel` se consulta por cada mensaje; si
/// devuelve `true`, el parseo se detiene, NO se escribe índice parcial y se devuelve un error.
pub fn build_index_cancelable(
    mbox_path: &Path,
    force_rebuild: bool,
    progress: Option<&dyn Fn(u64, u64)>,
    should_cancel: &dyn Fn() -> bool,
) -> anyhow::Result<Vec<MailEntry>> {
    if !force_rebuild {
        // An empty index for a non-empty file predates the non-mailbox check
        // below; rebuild it so that check gets to run.
        let stale_empty = |entries: &Vec<MailEntry>| {
            entries.is_empty() && std::fs::metadata(mbox_path).is_ok_and(|m| m.len() > 0)
        };
        if let Some(entries) = load_index(mbox_path)?.filter(|e| !stale_empty(e)) {
            debug!(
                path = %mbox_path.display(),
                count = entries.len(),
                "Loaded existing index"
            );
            return Ok(entries);
        }
    }

    info!(path = %mbox_path.display(), "Building index");

    let parser = MboxParser::new(mbox_path)?;
    let mut entries: Vec<MailEntry> = Vec::new();
    let mut sequence: u64 = 0;

    parser.parse_headers_only(
        &mut |offset, length, header_bytes| {
            if should_cancel() {
                return false; // detiene el parseo
            }
            match header::parse_headers_to_entry(header_bytes, offset, length, sequence) {
                Ok(entry) => {
                    entries.push(entry);
                    sequence += 1;
                }
                Err(e) => {
                    warn!(offset = offset, error = %e, "Skipping unparseable message");
                }
            }
            true
        },
        progress,
    )?;

    if should_cancel() {
        anyhow::bail!("indexing cancelled");
    }

    // A non-empty file without a single `From ` separator is not a mailbox
    // (an .eml, a text file, a zip). Reporting "0 messages" with success and
    // leaving a hidden index behind made that look like an empty mailbox.
    if entries.is_empty() && parser.file_size() > 0 {
        return Err(MboxError::InvalidMbox(mbox_path.to_path_buf()).into());
    }

    // Write the index file
    if let Err(e) = write_index(mbox_path, &entries) {
        warn!(error = %e, "Could not write index file; continuing without persistence");
    }

    Ok(entries)
}

/// Attempt to load an existing index. Returns `None` if the index is missing or invalid.
pub fn load_index(mbox_path: &Path) -> anyhow::Result<Option<Vec<MailEntry>>> {
    let idx_path = index_path_for(mbox_path);
    if !idx_path.exists() {
        // Try cache location
        let cache_path = cache_index_path_for(mbox_path);
        if cache_path.exists() {
            return load_index_from_file(&cache_path, mbox_path);
        }
        return Ok(None);
    }
    load_index_from_file(&idx_path, mbox_path)
}

/// Whether an index file of `idx_len` bytes is plausibly valid for an MBOX of
/// `mbox_len` bytes. The binary index is always far smaller than the mailbox;
/// a wildly larger sidecar is treated as corrupt/hostile and rejected before it
/// is read into memory.
fn index_size_acceptable(idx_len: u64, mbox_len: u64) -> bool {
    idx_len <= mbox_len.saturating_add(64 * 1024 * 1024)
}

/// Load and validate an index from a specific file.
fn load_index_from_file(
    idx_path: &Path,
    mbox_path: &Path,
) -> anyhow::Result<Option<Vec<MailEntry>>> {
    // A crafted/corrupt `.idx` sidecar could be arbitrarily large; reading it
    // whole before validation would OOM. Reject an implausibly large one first.
    let idx_len = std::fs::metadata(idx_path)
        .map_err(|e| MboxError::io(idx_path, e))?
        .len();
    let mbox_len = std::fs::metadata(mbox_path).map(|m| m.len()).unwrap_or(0);
    if !index_size_acceptable(idx_len, mbox_len) {
        debug!("Index file implausibly large; ignoring");
        return Ok(None);
    }

    let data = std::fs::read(idx_path).map_err(|e| MboxError::io(idx_path, e))?;

    if data.len() < HEADER_SIZE {
        debug!("Index file too small");
        return Ok(None);
    }

    let header: IndexHeader =
        bincode::deserialize(&data[..HEADER_SIZE]).map_err(|e| MboxError::InvalidIndex {
            path: idx_path.to_path_buf(),
            reason: format!("Header deserialization failed: {e}"),
        })?;

    if let Err(reason) = header.validate() {
        debug!(reason = %reason, "Index header invalid");
        return Ok(None);
    }

    // Validate against current MBOX file
    let mbox_meta = std::fs::metadata(mbox_path).map_err(|e| MboxError::io(mbox_path, e))?;

    if header.mbox_file_size != mbox_meta.len() {
        debug!("MBOX file size changed");
        return Ok(None);
    }

    let mbox_mtime = mtime_nanos(&mbox_meta);

    if header.mbox_modified_time != mbox_mtime {
        debug!("MBOX modification time changed");
        return Ok(None);
    }

    // Verify SHA-256 of first 4 KB
    let current_hash = sha256_first_n(mbox_path, HASH_PREFIX_LEN)?;
    if header.sha256_first_4kb != current_hash {
        debug!("MBOX content hash changed");
        return Ok(None);
    }

    let entries: Vec<MailEntry> =
        bincode::deserialize(&data[HEADER_SIZE..]).map_err(|e| MboxError::InvalidIndex {
            path: idx_path.to_path_buf(),
            reason: format!("Entry deserialization failed: {e}"),
        })?;

    if entries.len() as u64 != header.message_count {
        debug!("Message count mismatch");
        return Ok(None);
    }

    // A corrupt or crafted index could carry offsets/lengths pointing outside
    // the MBOX; reading such an entry would attempt an arbitrarily large
    // allocation before the read fails. Treat it as invalid and rebuild.
    let mbox_len = mbox_meta.len();
    let in_bounds = entries.iter().all(|e| {
        e.offset
            .checked_add(e.length)
            .is_some_and(|end| end <= mbox_len)
    });
    if !in_bounds {
        debug!("Index contains entries beyond the MBOX bounds");
        return Ok(None);
    }

    Ok(Some(entries))
}

/// Write the index to disk.
fn write_index(mbox_path: &Path, entries: &[MailEntry]) -> anyhow::Result<()> {
    let mbox_meta = std::fs::metadata(mbox_path).map_err(|e| MboxError::io(mbox_path, e))?;

    let mbox_mtime = mtime_nanos(&mbox_meta);

    let hash = sha256_first_n(mbox_path, HASH_PREFIX_LEN)?;

    let header = IndexHeader {
        magic: *MAGIC,
        version: VERSION,
        flags: 0,
        message_count: entries.len() as u64,
        mbox_file_size: mbox_meta.len(),
        mbox_modified_time: mbox_mtime,
        sha256_first_4kb: hash,
    };

    let header_bytes = bincode::serialize(&header)?;
    let entries_bytes = bincode::serialize(entries)?;

    // Pad header to HEADER_SIZE
    let mut padded_header = vec![0u8; HEADER_SIZE];
    let copy_len = header_bytes.len().min(HEADER_SIZE);
    padded_header[..copy_len].copy_from_slice(&header_bytes[..copy_len]);

    // Try writing next to the MBOX file first
    let idx_path = index_path_for(mbox_path);
    match write_index_to_file(&idx_path, &padded_header, &entries_bytes) {
        Ok(()) => {
            info!(path = %idx_path.display(), "Index written");
            return Ok(());
        }
        Err(e) => {
            debug!(error = %e, "Cannot write index next to MBOX, trying cache dir");
        }
    }

    // Fallback: write to cache directory
    let cache_path = cache_index_path_for(mbox_path);
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_index_to_file(&cache_path, &padded_header, &entries_bytes)?;
    info!(path = %cache_path.display(), "Index written to cache");
    Ok(())
}

/// Write header + entries to a file.
///
/// Written to a fresh temp file and renamed into place. Opening `path`
/// directly followed whatever sat there: a mailbox shipped with a planted
/// `.name.mboxshell.idx -> ~/.zshrc` symlink got that file truncated and
/// overwritten. `rename` replaces the link itself, never its target, and a
/// crash mid-write no longer leaves a truncated index behind.
fn write_index_to_file(path: &Path, header: &[u8], entries: &[u8]) -> anyhow::Result<()> {
    let (tmp, file) = fsutil::create_temp_beside(path).map_err(|e| MboxError::io(path, e))?;
    if let Err(e) = write_and_commit(file, &tmp, path, header, entries) {
        let _ = std::fs::remove_file(&tmp);
        return Err(MboxError::io(path, e).into());
    }
    Ok(())
}

/// Fill the temp file and rename it over `path`.
fn write_and_commit(
    mut file: File,
    tmp: &Path,
    path: &Path,
    header: &[u8],
    entries: &[u8],
) -> std::io::Result<()> {
    file.write_all(header)?;
    file.write_all(entries)?;
    file.flush()?;
    drop(file);
    std::fs::rename(tmp, path)
}

/// Compute SHA-256 of the first `n` bytes of a file.
/// Modification time of a file as nanoseconds since the Unix epoch.
///
/// Nanoseconds rather than seconds: a mailbox rewritten in the same second the
/// index was written looked unchanged at 1-second resolution, and the stale
/// index was served for a file that had moved underneath it. Times before the
/// epoch, or beyond what an `i64` of nanoseconds can hold (year 2262), fall
/// back to `0` — the index is then simply rebuilt.
fn mtime_nanos(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_nanos()).ok())
        .unwrap_or(0)
}

fn sha256_first_n(path: &Path, n: usize) -> anyhow::Result<[u8; 32]> {
    let file = File::open(path).map_err(|e| MboxError::io(path, e))?;
    // Read exactly the first `n` bytes (or the whole file if shorter) with a
    // bounded `read_to_end`. A single `read` can return fewer bytes than
    // available, which would hash a different prefix length between build and
    // load and trigger a spurious rebuild.
    let mut buf = Vec::with_capacity(n);
    file.take(n as u64)
        .read_to_end(&mut buf)
        .map_err(|e| MboxError::io(path, e))?;
    let mut hasher = Sha256::new();
    hasher.update(&buf);
    Ok(hasher.finalize().into())
}

/// Primary index path: hidden file next to the MBOX.
///
/// Example: `/data/mail.mbox` → `/data/.mail.mbox.mboxshell.idx`
pub fn index_path_for(mbox_path: &Path) -> PathBuf {
    let filename = mbox_path.file_name().unwrap_or_default().to_string_lossy();
    let idx_name = format!(".{filename}.mboxshell.idx");
    mbox_path.with_file_name(idx_name)
}

/// Fallback index path inside the user cache directory.
///
/// Example: `~/.cache/mboxshell/<sha256_of_path>.idx`
pub fn cache_index_path_for(mbox_path: &Path) -> PathBuf {
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(".cache"))
        .join("mboxshell");

    let mut hasher = Sha256::new();
    hasher.update(mbox_path.to_string_lossy().as_bytes());
    let hash = format!("{:x}", hasher.finalize());

    cache_dir.join(format!("{hash}.idx"))
}

/// Return the size in bytes of the index file for the given MBOX (0 if missing).
pub fn index_file_size(mbox_path: &Path) -> u64 {
    let idx_path = index_path_for(mbox_path);
    std::fs::metadata(&idx_path)
        .or_else(|_| std::fs::metadata(cache_index_path_for(mbox_path)))
        .map(|m| m.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_mbox_file_is_rejected_without_writing_an_index() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("notes.txt");
        std::fs::write(&path, b"Subject: not a mailbox\n\njust text\n").expect("write");

        let err = build_index(&path, true, None).expect_err("must be rejected");
        assert!(matches!(
            err.downcast_ref::<MboxError>(),
            Some(MboxError::InvalidMbox(_))
        ));
        assert!(
            !index_path_for(&path).exists(),
            "no index for a non-mailbox"
        );

        // An empty file is still just an empty mailbox.
        let empty = dir.path().join("empty.mbox");
        std::fs::write(&empty, b"").expect("write");
        assert!(build_index(&empty, true, None)
            .expect("empty is fine")
            .is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_index_write_replaces_a_planted_symlink_not_its_target() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mbox = dir.path().join("inbox.mbox");
        std::fs::write(
            &mbox,
            b"From a@b Thu Jan 01 00:00:00 2024\nSubject: x\n\nbody\n",
        )
        .expect("write mbox");
        let victim = dir.path().join("victim");
        std::fs::write(&victim, b"keep me").expect("write victim");
        let idx = index_path_for(&mbox);
        std::os::unix::fs::symlink(&victim, &idx).expect("plant symlink");

        build_index(&mbox, true, None).expect("index");

        assert_eq!(std::fs::read(&victim).expect("read victim"), b"keep me");
        assert!(!std::fs::symlink_metadata(&idx)
            .expect("idx")
            .file_type()
            .is_symlink());
    }

    #[test]
    fn test_index_size_acceptable() {
        // A normal small index for a large mailbox is accepted.
        assert!(index_size_acceptable(10_000, 50_000_000_000));
        // Within the fixed slack even for a tiny/zero-size mailbox.
        assert!(index_size_acceptable(64 * 1024 * 1024, 0));
        // A wildly oversized sidecar is rejected before being read into memory.
        assert!(!index_size_acceptable(200 * 1024 * 1024, 0));
        assert!(!index_size_acceptable(
            50_000_000_000u64 + 64 * 1024 * 1024 + 1,
            50_000_000_000
        ));
    }

    #[test]
    fn test_load_index_rejects_out_of_bounds_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mbox_path = dir.path().join("test.mbox");
        std::fs::copy("tests/fixtures/simple.mbox", &mbox_path).expect("copy fixture");

        let entries = build_index(&mbox_path, true, None).expect("build index");
        assert!(!entries.is_empty());
        assert!(load_index(&mbox_path).expect("load").is_some());

        // Tamper: point the first entry past the end of the MBOX. A length of
        // u64::MAX would also overflow offset + length without checked_add.
        let mut tampered = entries.clone();
        tampered[0].length = u64::MAX;
        write_index(&mbox_path, &tampered).expect("write tampered index");
        assert!(load_index(&mbox_path).expect("load").is_none());

        // Tamper: offset + length just one byte beyond the file.
        let mbox_len = std::fs::metadata(&mbox_path).expect("metadata").len();
        let mut tampered = entries.clone();
        tampered[0].offset = mbox_len - tampered[0].length + 1;
        write_index(&mbox_path, &tampered).expect("write tampered index");
        assert!(load_index(&mbox_path).expect("load").is_none());

        // A valid index still loads after rebuilding.
        write_index(&mbox_path, &entries).expect("write valid index");
        assert!(load_index(&mbox_path).expect("load").is_some());
    }
}
