//! Authentication of index files with a per-user key.
//!
//! An index sits next to its mailbox, so it travels with it: in an archive,
//! a shared folder, a repository. Checking it only against the mailbox's
//! size, mtime and first bytes let whoever supplied both files decide what
//! the list, `search`, `stats` and the CSV export showed — forged senders
//! and subjects, or messages left out — while the mailbox itself said
//! something else.
//!
//! Every index now carries an HMAC-SHA256 computed with a random key that
//! lives in the user's cache directory (`index.key`, owner-only). An index
//! made by anyone else does not verify and is rebuilt from the mailbox, as
//! a stale one already was.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use tracing::warn;

/// Length of the key and of the tag, in bytes.
pub const LEN: usize = 32;

/// Name of the key file inside the cache directory.
const KEY_FILE: &str = "index.key";

static KEY: OnceLock<[u8; LEN]> = OnceLock::new();

/// The key this user's indexes are authenticated with.
///
/// Read from (or created in) the cache directory on first use. When that is
/// not possible — a read-only home, a sandbox — a key that only lasts for
/// this process is used: indexes still work within the run and are rebuilt
/// on the next one.
fn key() -> &'static [u8; LEN] {
    KEY.get_or_init(|| {
        let path = key_path();
        load_or_create_key(&path).unwrap_or_else(|e| {
            warn!(path = %path.display(), error = %e, "No persistent index key; indexes will be rebuilt on every run");
            random_key().unwrap_or_else(|_| fallback_key())
        })
    })
}

/// Where the key file lives.
pub fn key_path() -> PathBuf {
    crate::config::cache_dir(crate::config::active()).join(KEY_FILE)
}

fn random_key() -> io::Result<[u8; LEN]> {
    let mut key = [0u8; LEN];
    getrandom::fill(&mut key).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(key)
}

/// Last resort when the system has no randomness to give: not secret enough
/// to resist forgery, but nothing persists under it either.
fn fallback_key() -> [u8; LEN] {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.finalize().into()
}

/// Read a well-formed key file: a regular file of exactly [`LEN`] bytes.
fn read_key(path: &Path) -> Option<[u8; LEN]> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() || meta.len() != LEN as u64 {
        return None;
    }
    std::fs::read(path).ok()?.try_into().ok()
}

/// Load the key at `path`, creating it (owner-only) if there is none.
fn load_or_create_key(path: &Path) -> io::Result<[u8; LEN]> {
    if let Some(key) = read_key(path) {
        return Ok(key);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let key = random_key()?;
    for attempt in 0..2 {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(path) {
            Ok(mut file) => {
                file.write_all(&key)?;
                file.sync_all()?;
                return Ok(key);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                // Another mboxshell is creating it right now: give it a
                // moment to finish and use its key, so both agree.
                for _ in 0..20 {
                    if let Some(existing) = read_key(path) {
                        return Ok(existing);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                // Not a key (truncated, a directory, a link): replace it.
                if attempt == 0 {
                    std::fs::remove_file(path)?;
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("index key file could not be created"))
}

/// Running HMAC over an index file.
pub struct Tagger(Hmac<Sha256>);

impl Tagger {
    /// Start a tag under this user's key.
    pub fn new() -> Self {
        Self::with_key(key())
    }

    fn with_key(key: &[u8; LEN]) -> Self {
        // HMAC accepts a key of any length, so this cannot fail; a failure
        // would only ever make every index look foreign and be rebuilt.
        Self(
            <Hmac<Sha256> as Mac>::new_from_slice(key).unwrap_or_else(|_| {
                <Hmac<Sha256> as hmac::digest::KeyInit>::new(&Default::default())
            }),
        )
    }

    /// Feed more of the file.
    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    /// The finished tag.
    pub fn finish(self) -> [u8; LEN] {
        self.0.finalize().into_bytes().into()
    }

    /// Whether the data fed so far carries `tag` (compared in constant time).
    pub fn verify(self, tag: &[u8; LEN]) -> bool {
        self.0.verify_slice(tag).is_ok()
    }
}

impl Default for Tagger {
    fn default() -> Self {
        Self::new()
    }
}

/// A writer that also feeds everything it writes to a [`Tagger`].
pub struct TaggingWriter<W: Write> {
    inner: W,
    tagger: Tagger,
}

impl<W: Write> TaggingWriter<W> {
    /// Wrap `inner`, tagging under this user's key.
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            tagger: Tagger::new(),
        }
    }

    /// The wrapped writer and the tag of everything written through it.
    pub fn finish(self) -> (W, [u8; LEN]) {
        (self.inner, self.tagger.finish())
    }
}

impl<W: Write> Write for TaggingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.tagger.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_is_created_once_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache").join(KEY_FILE);
        let first = load_or_create_key(&path).unwrap();
        let second = load_or_create_key(&path).unwrap();
        assert_eq!(first, second);
        assert_ne!(first, [0u8; LEN]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn test_malformed_key_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE);
        std::fs::write(&path, b"short").unwrap();
        let key = load_or_create_key(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), key);
    }

    #[test]
    fn test_tag_depends_on_key_and_data() {
        let tag = |key: &[u8; LEN], data: &[u8]| {
            let mut t = Tagger::with_key(key);
            t.update(data);
            t.finish()
        };
        let a = tag(&[1; LEN], b"index");
        assert_eq!(a, tag(&[1; LEN], b"index"));
        assert_ne!(a, tag(&[2; LEN], b"index"));
        assert_ne!(a, tag(&[1; LEN], b"indey"));

        let mut t = Tagger::with_key(&[1; LEN]);
        t.update(b"index");
        assert!(t.verify(&a));
        let mut t = Tagger::with_key(&[2; LEN]);
        t.update(b"index");
        assert!(!t.verify(&a));
    }
}
