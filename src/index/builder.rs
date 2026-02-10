//! Index construction, validation, and persistence.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

use crate::error::MboxError;
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
    if !force_rebuild {
        if let Some(entries) = load_index(mbox_path)? {
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

/// Load and validate an index from a specific file.
fn load_index_from_file(
    idx_path: &Path,
    mbox_path: &Path,
) -> anyhow::Result<Option<Vec<MailEntry>>> {
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

    let mbox_mtime = mbox_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

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

    Ok(Some(entries))
}

/// Write the index to disk.
fn write_index(mbox_path: &Path, entries: &[MailEntry]) -> anyhow::Result<()> {
    let mbox_meta = std::fs::metadata(mbox_path).map_err(|e| MboxError::io(mbox_path, e))?;

    let mbox_mtime = mbox_meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

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
fn write_index_to_file(path: &Path, header: &[u8], entries: &[u8]) -> anyhow::Result<()> {
    let mut file = File::create(path).map_err(|e| MboxError::io(path, e))?;
    file.write_all(header).map_err(|e| MboxError::io(path, e))?;
    file.write_all(entries)
        .map_err(|e| MboxError::io(path, e))?;
    file.flush().map_err(|e| MboxError::io(path, e))?;
    Ok(())
}

/// Compute SHA-256 of the first `n` bytes of a file.
fn sha256_first_n(path: &Path, n: usize) -> anyhow::Result<[u8; 32]> {
    let mut file = File::open(path).map_err(|e| MboxError::io(path, e))?;
    let mut buf = vec![0u8; n];
    let bytes_read = file.read(&mut buf).map_err(|e| MboxError::io(path, e))?;
    let mut hasher = Sha256::new();
    hasher.update(&buf[..bytes_read]);
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
