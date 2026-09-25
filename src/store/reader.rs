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

/// Most bytes of one message read in to display or search it (256 MB).
///
/// The length comes from the mailbox, so a single 20 GB "message" used to be
/// allocated whole the moment it was selected. Past this, the message is
/// decoded from its first bytes with a notice; exports still copy it whole.
pub const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;

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
    decode_limit: u64,
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
            decode_limit: MAX_DECODE_BYTES,
        })
    }

    /// Path of the mailbox this store reads from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read and decode a message, returning a shared handle to the cached body.
    ///
    /// The returned [`Rc`] is a cheap refcount bump, not a deep copy; it keeps
    /// the body alive even if a later `get_message` evicts it from the LRU.
    pub fn get_message(&mut self, entry: &MailEntry) -> Result<Rc<MailBody>> {
        if let Some(body) = self.cache.get(&entry.offset) {
            return Ok(Rc::clone(body));
        }
        let limit = entry.length.min(self.decode_limit);
        let raw = self.read_at(entry.offset, limit)?;
        let mut body = mime::parse_message_body(&raw)?;
        if limit < entry.length {
            let notice = format!(
                "[{} {} MB]\n\n",
                crate::i18n::msg_message_truncated(),
                self.decode_limit / (1024 * 1024)
            );
            body.text = Some(notice + body.text.as_deref().unwrap_or(""));
        }
        let body = Rc::new(body);
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

    /// Low-level: seek to the message and read all of it.
    fn read_raw(&mut self, entry: &MailEntry) -> Result<Vec<u8>> {
        self.read_at(entry.offset, entry.length)
    }

    /// Seek to `offset` and read `length` bytes.
    fn read_at(&mut self, offset: u64, length: u64) -> Result<Vec<u8>> {
        debug!(offset, length, "Reading message from MBOX");
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|e| MboxError::io(&self.path, e))?;
        // Explicit conversion: on 32-bit targets a length above usize::MAX
        // would silently truncate with `as`, under-allocating the buffer.
        let length = usize::try_from(length).map_err(|_| MboxError::ParseError {
            offset,
            reason: format!("message length {length} exceeds addressable memory"),
        })?;
        let mut buf = vec![0u8; length];
        self.file
            .read_exact(&mut buf)
            .map_err(|e| MboxError::io(&self.path, e))?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oversized_message_is_decoded_from_its_head_with_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.mbox");
        let mut data =
            b"From a@b Thu Jan 01 00:00:00 2024\nSubject: big\n\nstart of body\n".to_vec();
        data.extend(std::iter::repeat_n(b'x', 4096));
        std::fs::write(&path, &data).unwrap();

        let entries = crate::index::builder::build_index(&path, true, None).unwrap();
        let mut store = MboxStore::open(&path).unwrap();
        store.decode_limit = 256;

        let body = store.get_message(&entries[0]).unwrap();
        let text = body.text.as_deref().unwrap_or("");
        assert!(text.starts_with('['), "notice expected: {text:?}");
        assert!(text.contains("start of body"));
        assert!(text.len() < 1024, "only the head may be decoded");
        // Exports still get every byte.
        assert_eq!(store.get_raw_message(&entries[0]).unwrap(), data);
    }
}
