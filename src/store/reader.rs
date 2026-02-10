//! MBOX store: reads individual messages by offset with LRU caching.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use lru::LruCache;
use tracing::debug;

use crate::error::{MboxError, Result};
use crate::model::attachment::AttachmentMeta;
use crate::model::mail::{MailBody, MailEntry};
use crate::parser::mime;

/// Default number of decoded messages to keep in the LRU cache.
const DEFAULT_CACHE_SIZE: usize = 50;

/// Reads messages from an MBOX file using index offsets.
///
/// Maintains an LRU cache of decoded [`MailBody`] objects so that
/// scrolling back and forth through a message list does not require
/// repeated MIME decoding.
pub struct MboxStore {
    path: PathBuf,
    file: File,
    cache: LruCache<u64, MailBody>,
}

impl MboxStore {
    /// Open an MBOX file for random-access reading.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path).map_err(|e| MboxError::io(&path, e))?;
        let cache_size =
            NonZeroUsize::new(DEFAULT_CACHE_SIZE).expect("DEFAULT_CACHE_SIZE is non-zero");
        Ok(Self {
            path,
            file,
            cache: LruCache::new(cache_size),
        })
    }

    /// Read and decode a message. Cached results are returned immediately.
    pub fn get_message(&mut self, entry: &MailEntry) -> Result<&MailBody> {
        if !self.cache.contains(&entry.offset) {
            let raw = self.read_raw(entry)?;
            let body = mime::parse_message_body(&raw)?;
            self.cache.put(entry.offset, body);
        }
        // Safe: we just inserted if missing
        Ok(self.cache.get(&entry.offset).expect("just inserted"))
    }

    /// Read the raw bytes of a message (not cached).
    pub fn get_raw_message(&mut self, entry: &MailEntry) -> Result<Vec<u8>> {
        self.read_raw(entry)
    }

    /// Extract a decoded attachment from a message.
    pub fn get_attachment(
        &mut self,
        entry: &MailEntry,
        attachment: &AttachmentMeta,
    ) -> Result<Vec<u8>> {
        let raw = self.read_raw(entry)?;
        mime::extract_attachment(&raw, attachment)
    }

    /// Low-level: seek to offset and read `length` bytes.
    fn read_raw(&mut self, entry: &MailEntry) -> Result<Vec<u8>> {
        debug!(
            offset = entry.offset,
            length = entry.length,
            "Reading message from MBOX"
        );
        self.file
            .seek(SeekFrom::Start(entry.offset))
            .map_err(|e| MboxError::io(&self.path, e))?;
        let mut buf = vec![0u8; entry.length as usize];
        self.file
            .read_exact(&mut buf)
            .map_err(|e| MboxError::io(&self.path, e))?;
        Ok(buf)
    }
}
