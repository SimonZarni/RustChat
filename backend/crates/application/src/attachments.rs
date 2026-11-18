//! Authorization and policy for conversation attachments.

use std::sync::Arc;

use rustchat_domain::{
    attachment::{Attachment, requested_media_type, safe_file_name, sniff_media_type},
    identity::{AttachmentId, ConversationId, UserId},
};

use crate::{
    error::AppError,
    ports::{AttachmentRepository, AttachmentStorage},
};

const URL_TTL: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const UPLOAD_TTL: time::Duration = time::Duration::minutes(30);

#[derive(Debug, Clone, Copy)]
pub struct AttachmentConfig {
    pub max_file_bytes: i64,
    pub quota_bytes: i64,
}

pub struct UploadReservation {
    pub attachment_id: AttachmentId,
    pub upload_url: String,
    pub expires_at: time::OffsetDateTime,
    pub required_content_type: &'static str,
}

pub struct AttachmentDownload {
    pub attachment: Attachment,
    pub download_url: String,
    pub expires_at: time::OffsetDateTime,
}

pub struct AttachmentService {
    repository: Arc<dyn AttachmentRepository>,
    storage: Arc<dyn AttachmentStorage>,
    config: AttachmentConfig,
}

impl AttachmentService {
    pub fn new(
        repository: Arc<dyn AttachmentRepository>,
        storage: Arc<dyn AttachmentStorage>,
        config: AttachmentConfig,
    ) -> Self {
        Self {
            repository,
            storage,
            config,
        }
    }

    pub async fn reserve(
        &self,
        caller: UserId,
        conversation: ConversationId,
        file_name: &str,
        declared_type: &str,
        byte_size: i64,
    ) -> Result<UploadReservation, AppError> {
        if !(1..=self.config.max_file_bytes).contains(&byte_size) {
            return Err(AppError::AttachmentLimitExceeded);
        }
        let file_name = safe_file_name(file_name)?;
        let media_type = requested_media_type(declared_type)?;
        let id = AttachmentId::new();
        let now = time::OffsetDateTime::now_utc();
        let expires_at = now + UPLOAD_TTL;
        let storage_key = format!("attachments/{}/{}", conversation.as_uuid(), id.as_uuid());
        let (upload_key, _storage_key) = self
            .repository
            .reserve(
                caller,
                conversation,
                id,
                &storage_key,
                &file_name,
                media_type,
                byte_size,
                now,
                expires_at,
                self.config.quota_bytes,
            )
            .await?;
        let upload_url = self
            .storage
            .presign_upload(&upload_key, media_type, URL_TTL)
            .await?;
        Ok(UploadReservation {
            attachment_id: id,
            upload_url,
            expires_at: now + time::Duration::seconds(URL_TTL.as_secs() as i64),
            required_content_type: media_type,
        })
    }

    pub async fn complete(
        &self,
        caller: UserId,
        attachment: AttachmentId,
    ) -> Result<Attachment, AppError> {
        let (upload_key, storage_key, expected_size) =
            self.repository.pending_upload(caller, attachment).await?;
        let (actual_size, prefix) = self.storage.inspect_upload(&upload_key).await?;
        if !(1..=self.config.max_file_bytes).contains(&actual_size) {
            return Err(AppError::AttachmentLimitExceeded);
        }
        if actual_size != expected_size {
            return Err(AppError::InvalidAttachmentRequest);
        }
        let actual_type = sniff_media_type(&prefix).ok_or(AppError::InvalidAttachmentContent(
            rustchat_domain::attachment::AttachmentError::InvalidContent,
        ))?;
        self.storage
            .promote_upload(&upload_key, &storage_key)
            .await?;
        let completed = match self
            .repository
            .complete(
                caller,
                attachment,
                actual_type,
                actual_size,
                time::OffsetDateTime::now_utc(),
            )
            .await
        {
            Ok(record) => record,
            Err(error) => {
                let _ = self.storage.delete(&storage_key).await;
                return Err(error);
            }
        };
        let _ = self.storage.delete(&upload_key).await;
        Ok(completed)
    }

    pub async fn attach_to_message(
        &self,
        caller: UserId,
        conversation: ConversationId,
        message: rustchat_domain::identity::MessageId,
        attachments: &[AttachmentId],
    ) -> Result<(), AppError> {
        if attachments.is_empty() || attachments.len() > 10 {
            return Err(AppError::InvalidAttachmentRequest);
        }
        self.repository
            .attach_to_message(caller, conversation, message, attachments)
            .await
    }

    pub async fn list_for_messages(
        &self,
        caller: UserId,
        conversation: ConversationId,
        messages: &[rustchat_domain::identity::MessageId],
    ) -> Result<Vec<Attachment>, AppError> {
        self.repository
            .list_for_messages(caller, conversation, messages)
            .await
    }

    pub async fn delete_unattached(
        &self,
        caller: UserId,
        attachment: AttachmentId,
    ) -> Result<(), AppError> {
        let (upload_key, storage_key) = self
            .repository
            .delete_unattached(caller, attachment)
            .await?;
        self.storage.delete(&upload_key).await?;
        self.storage.delete(&storage_key).await?;
        Ok(())
    }

    pub async fn download(
        &self,
        caller: UserId,
        attachment: AttachmentId,
    ) -> Result<AttachmentDownload, AppError> {
        let (record, storage_key) = self.repository.for_download(caller, attachment).await?;
        let download_url = self
            .storage
            .presign_download(&storage_key, &record.file_name, URL_TTL)
            .await?;
        Ok(AttachmentDownload {
            attachment: record,
            download_url,
            expires_at: time::OffsetDateTime::now_utc()
                + time::Duration::seconds(URL_TTL.as_secs() as i64),
        })
    }
}
