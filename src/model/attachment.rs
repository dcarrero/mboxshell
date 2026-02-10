//! Attachment metadata.
//!
//! The actual content is NOT loaded until export time.
//! Only offsets and metadata are stored.

/// Metadata about an email attachment.
///
/// Content is accessed lazily — the binary payload is only decoded when
/// explicitly requested (e.g. during export).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AttachmentMeta {
    /// Filename of the attachment. Generated if missing from the headers.
    pub filename: String,

    /// MIME content type (e.g. `"image/jpeg"`, `"application/pdf"`).
    pub content_type: String,

    /// Decoded size in bytes (estimated; exact value known only after decoding).
    pub size: u64,

    /// Content-Transfer-Encoding (`base64`, `quoted-printable`, `7bit`, `8bit`, `binary`).
    pub encoding: String,

    /// Content-ID for inline attachments referenced from HTML.
    pub content_id: Option<String>,

    /// `true` if the attachment is inline (embedded in HTML), `false` if a regular attachment.
    pub is_inline: bool,

    /// Byte offset of the encoded content within the message (relative to the MBOX message start).
    pub content_offset: u64,

    /// Length in bytes of the encoded content.
    pub content_length: u64,
}
