//! Conversation attachment metadata. File bytes live in object storage.

use time::OffsetDateTime;

use crate::identity::{AttachmentId, ConversationId, MessageId, UserId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub id: AttachmentId,
    pub conversation_id: ConversationId,
    pub owner_id: UserId,
    pub message_id: Option<MessageId>,
    pub file_name: String,
    pub media_type: String,
    pub byte_size: i64,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AttachmentError {
    #[error("attachment name is invalid")]
    InvalidName,
    #[error("attachment size is outside the allowed range")]
    InvalidSize,
    #[error("attachment type is not supported")]
    UnsupportedType,
    #[error("uploaded file content does not match an allowed type")]
    InvalidContent,
}

pub fn safe_file_name(raw: &str) -> Result<String, AttachmentError> {
    let leaf = raw.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    if leaf.is_empty()
        || leaf.chars().count() > 120
        || leaf
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '/' | '\\'))
        || leaf == "."
        || leaf == ".."
    {
        return Err(AttachmentError::InvalidName);
    }
    Ok(leaf.to_owned())
}

/// Accept a narrow, preview-safe first set. All files are still downloaded as
/// attachments; these types are never served inline from the chat origin.
pub fn requested_media_type(raw: &str) -> Result<&'static str, AttachmentError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "image/png" => Ok("image/png"),
        "image/jpeg" => Ok("image/jpeg"),
        "image/gif" => Ok("image/gif"),
        "image/webp" => Ok("image/webp"),
        "application/pdf" => Ok("application/pdf"),
        "text/plain" => Ok("text/plain"),
        _ => Err(AttachmentError::UnsupportedType),
    }
}

pub fn sniff_media_type(prefix: &[u8]) -> Option<&'static str> {
    if prefix.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if prefix.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if prefix.starts_with(b"GIF87a") || prefix.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if prefix.len() >= 12 && &prefix[..4] == b"RIFF" && &prefix[8..12] == b"WEBP" {
        Some("image/webp")
    } else if prefix.starts_with(b"%PDF-") {
        Some("application/pdf")
    } else if std::str::from_utf8(prefix).is_ok()
        && !prefix
            .iter()
            .any(|byte| *byte == 0 || (*byte < 0x20 && !matches!(*byte, b'\n' | b'\r' | b'\t')))
    {
        Some("text/plain")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_keep_only_the_leaf_and_reject_control_characters() {
        assert_eq!(
            safe_file_name(r"C:\uploads\report.pdf").unwrap(),
            "report.pdf"
        );
        assert_eq!(safe_file_name("../report.pdf").unwrap(), "report.pdf");
        assert!(safe_file_name("  ").is_err());
        assert!(safe_file_name("report\n.pdf").is_err());
        assert!(safe_file_name(&"x".repeat(121)).is_err());
    }

    #[test]
    fn requested_types_are_allowlisted() {
        assert_eq!(requested_media_type(" IMAGE/PNG ").unwrap(), "image/png");
        assert_eq!(
            requested_media_type("application/octet-stream"),
            Err(AttachmentError::UnsupportedType)
        );
        assert_eq!(
            requested_media_type("text/html"),
            Err(AttachmentError::UnsupportedType)
        );
    }

    #[test]
    fn content_signatures_are_detected_without_trusting_extensions() {
        assert_eq!(
            sniff_media_type(b"\x89PNG\r\n\x1a\nrest"),
            Some("image/png")
        );
        assert_eq!(sniff_media_type(&[0xff, 0xd8, 0xff, 0]), Some("image/jpeg"));
        assert_eq!(sniff_media_type(b"GIF89a"), Some("image/gif"));
        assert_eq!(sniff_media_type(b"%PDF-1.7"), Some("application/pdf"));
        assert_eq!(sniff_media_type(b"<script>"), Some("text/plain"));
        assert_eq!(sniff_media_type(b"\0not text"), None);
    }
}
