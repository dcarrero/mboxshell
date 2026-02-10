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
//! │  mbox_modified_time: i64             │
//! │  sha256_first_4kb: [u8; 32]         │
//! │  (padding to 128 bytes)              │
//! ├──────────────────────────────────────┤
//! │ ENTRIES (variable)                   │
//! │  bincode-serialized Vec<MailEntry>   │
//! └──────────────────────────────────────┘
//! ```

/// Magic bytes identifying an mboxShell index file.
pub const MAGIC: &[u8; 8] = b"MBOXTUI\0";

/// Current index format version.
pub const VERSION: u32 = 1;

/// Fixed header size in bytes.
pub const HEADER_SIZE: usize = 128;

/// Size of the SHA-256 hash prefix used for integrity checking.
pub const HASH_PREFIX_LEN: usize = 4096;

/// Serializable index header.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
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
    /// Modification time of the MBOX file (Unix timestamp in seconds).
    pub mbox_modified_time: i64,
    /// SHA-256 of the first 4 KB of the MBOX file.
    pub sha256_first_4kb: [u8; 32],
}

impl IndexHeader {
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
