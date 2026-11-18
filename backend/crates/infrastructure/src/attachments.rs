//! S3-compatible attachment storage used by Garage and compatible services.

use std::time::Duration;

use async_trait::async_trait;
use aws_sdk_s3::{
    Client,
    config::{Credentials, Region},
    presigning::PresigningConfig,
};
use rustchat_application::{error::AppError, ports::AttachmentStorage};
use tokio::io::AsyncReadExt;

#[derive(Clone)]
pub struct S3AttachmentStorage {
    client: Client,
    bucket: String,
}

impl S3AttachmentStorage {
    pub fn new(
        endpoint: &str,
        region: &str,
        access_key: &str,
        secret_key: &str,
        bucket: &str,
    ) -> Self {
        let credentials = Credentials::new(access_key, secret_key, None, None, "rustchat-config");
        let config = aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .region(Region::new(region.to_owned()))
            .credentials_provider(credentials)
            .endpoint_url(endpoint)
            .force_path_style(true)
            .build();
        Self {
            client: Client::from_conf(config),
            bucket: bucket.to_owned(),
        }
    }

    fn presigning(expires_in: Duration) -> Result<PresigningConfig, AppError> {
        PresigningConfig::expires_in(expires_in).map_err(AppError::internal)
    }
}

#[async_trait]
impl AttachmentStorage for S3AttachmentStorage {
    async fn presign_upload(
        &self,
        storage_key: &str,
        content_type: &str,
        expires_in: Duration,
    ) -> Result<String, AppError> {
        let request = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(storage_key)
            .content_type(content_type)
            .presigned(Self::presigning(expires_in)?)
            .await
            .map_err(AppError::internal)?;
        Ok(request.uri().to_string())
    }

    async fn inspect_upload(&self, storage_key: &str) -> Result<(i64, Vec<u8>), AppError> {
        let head = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(storage_key)
            .send()
            .await
            .map_err(AppError::internal)?;
        let length = head
            .content_length()
            .ok_or_else(|| AppError::internal("S3 object has no content length"))?;
        let object = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(storage_key)
            .range("bytes=0-511")
            .send()
            .await
            .map_err(AppError::internal)?;
        let mut prefix = Vec::with_capacity(512);
        object
            .body
            .into_async_read()
            .take(512)
            .read_to_end(&mut prefix)
            .await
            .map_err(AppError::internal)?;
        Ok((length, prefix))
    }

    async fn promote_upload(&self, upload_key: &str, storage_key: &str) -> Result<(), AppError> {
        let source = format!("{}/{}", self.bucket, upload_key);
        self.client
            .copy_object()
            .bucket(&self.bucket)
            .key(storage_key)
            .copy_source(source)
            .send()
            .await
            .map_err(AppError::internal)?;
        Ok(())
    }

    async fn presign_download(
        &self,
        storage_key: &str,
        file_name: &str,
        expires_in: Duration,
    ) -> Result<String, AppError> {
        let safe_ascii: String = file_name
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ' ') {
                    ch
                } else {
                    '_'
                }
            })
            .collect();
        let disposition = format!("attachment; filename=\"{}\"", safe_ascii.replace('"', "_"));
        let request = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(storage_key)
            .response_content_type("application/octet-stream")
            .response_content_disposition(disposition)
            .presigned(Self::presigning(expires_in)?)
            .await
            .map_err(AppError::internal)?;
        Ok(request.uri().to_string())
    }

    async fn delete(&self, storage_key: &str) -> Result<(), AppError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(storage_key)
            .send()
            .await
            .map_err(AppError::internal)?;
        Ok(())
    }
}
