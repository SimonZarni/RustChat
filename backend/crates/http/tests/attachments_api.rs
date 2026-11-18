//! End-to-end attachment routes against PostgreSQL and a local S3-compatible store.
//! Run explicitly with the S3 environment variables and Garage configured.

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use common::{Options, app, call, sign_in};
use rustchat_application::attachments::{AttachmentConfig, AttachmentService};
use rustchat_infrastructure::{
    MIGRATOR, attachments::S3AttachmentStorage, postgres::PgAttachmentRepository,
};
use serde_json::json;
use sqlx::PgPool;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use uuid::Uuid;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

#[sqlx::test(migrator = "MIGRATOR")]
#[ignore = "requires DATABASE_URL and a local S3-compatible service configured with RUSTCHAT_S3_*"]
async fn uploads_attaches_and_authorizes_downloads(pool: PgPool) {
    let endpoint = std::env::var("RUSTCHAT_S3_ENDPOINT").expect("RUSTCHAT_S3_ENDPOINT");
    let region = std::env::var("RUSTCHAT_S3_REGION").expect("RUSTCHAT_S3_REGION");
    let bucket = std::env::var("RUSTCHAT_S3_BUCKET").expect("RUSTCHAT_S3_BUCKET");
    let access_key = std::env::var("RUSTCHAT_S3_ACCESS_KEY_ID").expect("RUSTCHAT_S3_ACCESS_KEY_ID");
    let secret_key =
        std::env::var("RUSTCHAT_S3_SECRET_ACCESS_KEY").expect("RUSTCHAT_S3_SECRET_ACCESS_KEY");
    let storage = Arc::new(S3AttachmentStorage::new(
        &endpoint,
        &region,
        &access_key,
        &secret_key,
        &bucket,
    ));
    let service = Arc::new(AttachmentService::new(
        Arc::new(PgAttachmentRepository::new(pool.clone())),
        storage,
        AttachmentConfig {
            max_file_bytes: 1024 * 1024,
            quota_bytes: 4 * 1024 * 1024,
        },
    ));
    let api = app(
        pool,
        Options {
            attachments: Some(service),
            ..Options::default()
        },
    );
    let alice_cookie = sign_in(&api, "attachalice").await;
    let bob_cookie = sign_in(&api, "attachbob").await;
    let outsider_cookie = sign_in(&api, "attachoutsider").await;

    let conversation = call(
        &api,
        Method::POST,
        "/api/v1/conversations",
        Some(json!({"type":"direct", "username":"attachbob"})),
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(
        conversation.status,
        StatusCode::CREATED,
        "{:?}",
        conversation.json
    );
    let conversation_id = conversation.json["conversation"]["id"].as_str().unwrap();

    let reservation = call(
        &api,
        Method::POST,
        &format!("/api/v1/conversations/{conversation_id}/attachments"),
        Some(json!({"file_name":"tiny.png", "content_type":"image/png", "byte_size":PNG_SIGNATURE.len()})),
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(
        reservation.status,
        StatusCode::CREATED,
        "{:?}",
        reservation.json
    );
    let attachment_id = reservation.json["attachment_id"].as_str().unwrap();
    let upload_url = reservation.json["upload_url"].as_str().unwrap();
    let upload_response = raw_http(upload_url, "PUT", Some(("image/png", PNG_SIGNATURE))).await;
    assert!(
        upload_response.starts_with(b"HTTP/1.1 200"),
        "{}",
        String::from_utf8_lossy(&upload_response)
    );

    let completed = call(
        &api,
        Method::POST,
        &format!("/api/v1/attachments/{attachment_id}/complete"),
        None,
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(completed.status, StatusCode::OK, "{:?}", completed.json);

    let client_message_id = Uuid::now_v7();
    let sent = call(
        &api,
        Method::POST,
        &format!("/api/v1/conversations/{conversation_id}/messages"),
        Some(json!({"client_message_id":client_message_id, "content":"", "attachment_ids":[attachment_id]})),
        Some(&alice_cookie),
    )
    .await;
    assert_eq!(sent.status, StatusCode::CREATED, "{:?}", sent.json);
    assert_eq!(
        sent.json["message"]["attachments"][0]["file_name"],
        "tiny.png"
    );

    let hidden = call(
        &api,
        Method::GET,
        &format!("/api/v1/attachments/{attachment_id}/download"),
        None,
        Some(&outsider_cookie),
    )
    .await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);

    let download = call(
        &api,
        Method::GET,
        &format!("/api/v1/attachments/{attachment_id}/download"),
        None,
        Some(&bob_cookie),
    )
    .await;
    assert_eq!(download.status, StatusCode::OK, "{:?}", download.json);
    let downloaded = raw_http(download.json["download_url"].as_str().unwrap(), "GET", None).await;
    assert!(
        downloaded.starts_with(b"HTTP/1.1 200"),
        "{}",
        String::from_utf8_lossy(&downloaded)
    );
    assert!(
        downloaded
            .windows(PNG_SIGNATURE.len())
            .any(|window| window == PNG_SIGNATURE)
    );
}

async fn raw_http(url: &str, method: &str, upload: Option<(&str, &[u8])>) -> Vec<u8> {
    let rest = url.strip_prefix("http://").expect("local HTTP Garage URL");
    let (authority, suffix) = rest.split_once('/').expect("URL path");
    let path = format!("/{suffix}");
    let mut stream = TcpStream::connect(authority)
        .await
        .expect("S3 service connection");
    let content_type = upload
        .map(|(media_type, _)| format!("Content-Type: {media_type}\r\n"))
        .unwrap_or_default();
    let body = upload.map(|(_, bytes)| bytes).unwrap_or_default();
    let headers = format!(
        "{method} {path} HTTP/1.1\r\nHost: {authority}\r\n{content_type}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(headers.as_bytes())
        .await
        .expect("request headers");
    stream.write_all(body).await.expect("request body");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .expect("S3 response");
    assert!(!response.is_empty(), "empty S3 response");
    response
}
