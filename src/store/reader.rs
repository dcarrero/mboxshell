//! MBOX store: reads individual messages by offset with LRU caching.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::rc::Rc;

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
///
/// Bodies are stored behind an [`Rc`] so callers (the TUI's `current_body`,
/// exporters) obtain a cheap shared handle instead of deep-copying a
/// potentially multi-MB `MailBody` out of the cache on every access.
pub struct MboxStore {
    path: PathBuf,
    file: File,
    cache: LruCache<u64, Rc<MailBody>>,
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

    /// Read and decode a message, returning a shared handle to the cached body.
    ///
    /// The returned [`Rc`] is a cheap refcount bump, not a deep copy; it keeps
    /// the body alive even if a later `get_message` evicts it from the LRU.
    pub fn get_message(&mut self, entry: &MailEntry) -> Result<Rc<MailBody>> {
        if let Some(body) = self.cache.get(&entry.offset) {
            return Ok(Rc::clone(body));
        }
        let raw = self.read_raw(entry)?;
        let body = Rc::new(mime::parse_message_body(&raw)?);
        self.cache.put(entry.offset, Rc::clone(&body));
        Ok(body)
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
        // Explicit conversion: on 32-bit targets a length above usize::MAX
        // would silently truncate with `as`, under-allocating the buffer.
        let length = usize::try_from(entry.length).map_err(|_| MboxError::ParseError {
            offset: entry.offset,
            reason: format!("message length {} exceeds addressable memory", entry.length),
        })?;
        let mut buf = vec![0u8; length];
        self.file
            .read_exact(&mut buf)
            .map_err(|e| MboxError::io(&self.path, e))?;
        Ok(buf)
    }
}
