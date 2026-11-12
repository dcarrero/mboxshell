//! Binary index file format.
//!
//! ```text
//! ┌──────────────────────────────────────┐
//! │ HEADER (128 bytes, fixed)            │
//! │  magic: [u8; 8] = b"MBOXTUI\0"      │
//! │  version: u32                        │
//! │  flags: u32                          │
//! │  message_count: u64                  │
//! │  mbox_file_size: u64                 │
//! │  mbox_modified_time: i64 (nanos)     │
//! │  sha256_first_4kb: [u8; 32]         │
//! │  mac: [u8; 32] (HMAC-SHA256)        │
//! │  (padding to 128 bytes)              │
//! ├──────────────────────────────────────┤
//! │ ENTRIES (variable)                   │
//! │  postcard-serialized Vec<MailEntry>  │
//! └──────────────────────────────────────┘
//! ```
//!
//! The header is written by hand, little-endian at fixed offsets, so it does
//! not depend on any serializer: every index version so far (including the
//! bincode ones, v1-v5) has this same header, and an old file is recognised
//! by its `version` field and rebuilt.

/// Magic bytes identifying an mboxShell index file.
pub const MAGIC: &[u8; 8] = b"MBOXTUI\0";

/// Current index format version.
///
/// v2: same binary layout as v1, bumped so that indexes built before the
/// issue #16 separator fix (quoted `git format-patch` mails splitting
/// messages) are rebuilt automatically.
/// v3: bumped again (layout still unchanged) because the unreleased v2
/// parser rejected bare `From ` separators (GYB-style mboxes), so v2
/// indexes built from `main` may carry mis-split boundaries.
/// v4: `MailEntry` gained `thread_id` (`X-GM-THRID`), and
/// `mbox_modified_time` switched from seconds to nanoseconds — both change
/// what an index means, so v3 files are rebuilt.
/// v5: `has_attachments` now comes from the MIME parts in the body, not from
/// a top-level `multipart/mixed` alone (issue #51), so v4 files are rebuilt.
/// v6: entries encoded with `postcard` instead of `bincode` (unmaintained,
/// RUSTSEC-2025-0141). Same header bytes; v5 files are rebuilt.
/// v7: the header carries an HMAC of the whole file under a per-user key
/// (a sidecar that arrived with the mailbox is no longer trusted), and
/// `has_attachments` also covers single-part non-text messages and named or
/// non-plain `text/*` parts. v6 files are rebuilt.
pub const VERSION: u32 = 7;

/// Fixed header size in bytes.
pub const HEADER_SIZE: usize = 128;

/// Size of the SHA-256 hash prefix used for integrity checking.
pub const HASH_PREFIX_LEN: usize = 4096;

/// Index header, stored in the first [`HEADER_SIZE`] bytes of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexHeader {
    /// Magic bytes (must equal [`MAGIC`]).
    pub magic: [u8; 8],
    /// Format version (must equal [`VERSION`]).
    pub version: u32,
    /// Reserved flags (currently unused).
    pub flags: u32,
    /// Number of messages in the index.
    pub message_count: u64,
    /// Size of the original MBOX file when the index was built.
    pub mbox_file_size: u64,
    /// Modification time of the MBOX file (Unix timestamp in nanoseconds).
    ///
    /// Nanoseconds, not seconds: at 1-second resolution a mailbox rewritten
    /// within the same second as its index looked unchanged, and the stale
    /// index was served for a file that had moved underneath it.
    pub mbox_modified_time: i64,
    /// SHA-256 of the first 4 KB of the MBOX file.
    pub sha256_first_4kb: [u8; 32],
    /// HMAC-SHA256, under the user's index key, of this header (with this
    /// field zeroed) followed by the entries. See [`crate::index::auth`].
    pub mac: [u8; 32],
}

// Byte offsets of each header field.
const OFF_VERSION: usize = 8;
const OFF_FLAGS: usize = 12;
const OFF_COUNT: usize = 16;
const OFF_SIZE: usize = 24;
const OFF_MTIME: usize = 32;
const OFF_SHA: usize = 40;
/// Offset of the [`IndexHeader::mac`] field.
pub const OFF_MAC: usize = OFF_SHA + 32;
const HEADER_USED: usize = OFF_MAC + 32;

impl IndexHeader {
    /// Encode the header as its fixed [`HEADER_SIZE`]-byte form (zero padded).
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut b = [0u8; HEADER_SIZE];
        b[..OFF_VERSION].copy_from_slice(&self.magic);
        b[OFF_VERSION..OFF_FLAGS].copy_from_slice(&self.version.to_le_bytes());
        b[OFF_FLAGS..OFF_COUNT].copy_from_slice(&self.flags.to_le_bytes());
        b[OFF_COUNT..OFF_SIZE].copy_from_slice(&self.message_count.to_le_bytes());
        b[OFF_SIZE..OFF_MTIME].copy_from_slice(&self.mbox_file_size.to_le_bytes());
        b[OFF_MTIME..OFF_SHA].copy_from_slice(&self.mbox_modified_time.to_le_bytes());
        b[OFF_SHA..OFF_MAC].copy_from_slice(&self.sha256_first_4kb);
        b[OFF_MAC..HEADER_USED].copy_from_slice(&self.mac);
        b
    }

    /// Decode a header from the start of `bytes`; `None` if it is too short.
    ///
    /// Only reads the fields: [`IndexHeader::validate`] decides whether the
    /// magic and version are acceptable.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let b = bytes.get(..HEADER_USED)?;
        let mut magic = [0u8; 8];
        magic.copy_from_slice(&b[..OFF_VERSION]);
        let mut sha256_first_4kb = [0u8; 32];
        sha256_first_4kb.copy_from_slice(&b[OFF_SHA..OFF_MAC]);
        let mut mac = [0u8; 32];
        mac.copy_from_slice(&b[OFF_MAC..HEADER_USED]);
        Some(Self {
            magic,
            version: u32::from_le_bytes(b[OFF_VERSION..OFF_FLAGS].try_into().ok()?),
            flags: u32::from_le_bytes(b[OFF_FLAGS..OFF_COUNT].try_into().ok()?),
            message_count: u64::from_le_bytes(b[OFF_COUNT..OFF_SIZE].try_into().ok()?),
            mbox_file_size: u64::from_le_bytes(b[OFF_SIZE..OFF_MTIME].try_into().ok()?),
            mbox_modified_time: i64::from_le_bytes(b[OFF_MTIME..OFF_SHA].try_into().ok()?),
            sha256_first_4kb,
            mac,
        })
    }

    /// Validate that the header is well-formed and matches the current format.
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.magic != *MAGIC {
            return Err("Invalid magic bytes".into());
        }
        if self.version != VERSION {
            return Err(format!(
                "Incompatible version: expected {VERSION}, found {}",
                self.version
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> IndexHeader {
        IndexHeader {
            magic: *MAGIC,
            version: VERSION,
            flags: 0,
            message_count: 123_456,
            mbox_file_size: 50_000_000_000,
            mbox_modified_time: -1_700_000_000_123_456_789,
            sha256_first_4kb: [7u8; 32],
            mac: [9u8; 32],
        }
    }

    #[test]
    fn test_header_round_trip() {
        let h = sample();
        assert_eq!(IndexHeader::from_bytes(&h.to_bytes()), Some(h));
        assert_eq!(IndexHeader::from_bytes(&[0u8; 10]), None);
    }

    #[test]
    fn test_header_layout_matches_the_bincode_v1_to_v5_files() {
        // bincode 1 wrote the same struct as fixed-width little-endian fields
        // in declaration order; keeping that layout is what lets an old index
        // be read far enough to see its version and be rebuilt.
        let b = sample().to_bytes();
        assert_eq!(&b[..8], MAGIC);
        assert_eq!(&b[8..12], &VERSION.to_le_bytes());
        assert_eq!(&b[16..24], &123_456u64.to_le_bytes());
        assert_eq!(&b[24..32], &50_000_000_000u64.to_le_bytes());
        assert_eq!(&b[32..40], &(-1_700_000_000_123_456_789i64).to_le_bytes());
        assert_eq!(&b[40..72], &[7u8; 32]);
        // v7 appended the MAC after the fields the old files had.
        assert_eq!(&b[72..104], &[9u8; 32]);
        assert!(b[104..].iter().all(|&x| x == 0));
    }
}
