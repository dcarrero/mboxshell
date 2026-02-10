//! Core mail entry and body types.

use chrono::{DateTime, Utc};

use super::address::EmailAddress;
use super::attachment::AttachmentMeta;

/// Compact metadata for a single email message, stored in the binary index.
///
/// All indexed messages are kept in memory as `Vec<MailEntry>`.
/// At ~500 bytes per entry, 1 million messages ≈ 500 MB of RAM.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MailEntry {
    /// Byte offset of the message start inside the MBOX file
    /// (points to the `From ` separator line).
    pub offset: u64,

    /// Total byte length of the message (from `From ` to next separator or EOF).
    pub length: u64,

    /// Parsed date from the `Date:` header.
    /// Falls back to the `From ` separator date, then to Unix epoch.
    pub date: DateTime<Utc>,

    /// Sender (first `From:` header).
    pub from: EmailAddress,

    /// Primary recipients (`To:`), truncated to the first 5.
    pub to: Vec<EmailAddress>,

    /// Carbon-copy recipients (`CC:`), truncated to the first 5.
    pub cc: Vec<EmailAddress>,

    /// Decoded subject line (RFC 2047 encoded-words resolved).
    pub subject: String,

    /// The `Message-ID` header value.
    pub message_id: String,

    /// The `In-Reply-To` header value, if present.
    pub in_reply_to: Option<String>,

    /// Message-IDs from the `References` header.
    pub references: Vec<String>,

    /// Whether the message contains attachments
    /// (detected via `Content-Type: multipart/mixed` or similar).
    pub has_attachments: bool,

    /// Top-level `Content-Type` of the message.
    pub content_type: String,

    /// Estimated plain-text body size in bytes.
    pub text_size: u64,

    /// Gmail labels from the `X-Gmail-Labels` header.
    pub labels: Vec<String>,

    /// Sequential index within the MBOX (0, 1, 2, …).
    pub sequence: u64,
}

/// Full body of a message, loaded on demand.
///
/// This is **not** stored in the index — it is decoded fresh each time
/// (with an LRU cache in `MboxStore` to avoid redundant work).
#[derive(Debug, Clone)]
pub struct MailBody {
    /// Plain-text body (from `text/plain` part, or stripped from HTML).
    pub text: Option<String>,

    /// HTML body (from `text/html` part, if present).
    pub html: Option<String>,

    /// Raw headers as a single string.
    pub raw_headers: String,

    /// Attachment metadata list.
    pub attachments: Vec<AttachmentMeta>,
}
