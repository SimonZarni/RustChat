//! Pure Rust VAPID signing, payload encryption and Rustls HTTPS Web Push delivery.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rustchat_application::{
    error::AppError,
    ports::{WebPushDelivery, WebPushSubscription},
};
use web_push_native::p256::elliptic_curve::sec1::ToEncodedPoint;
use web_push_native::{
    Auth, WebPushBuilder, jwt_simple::algorithms::ES256KeyPair, p256::PublicKey,
};

pub struct VapidWebPush {
    public_key: String,
    private_key: ES256KeyPair,
    subject: String,
    client: reqwest::Client,
}

impl VapidWebPush {
    pub fn new(
        public_key: String,
        private_key_pem: String,
        subject: String,
    ) -> Result<Self, AppError> {
        let private_key = ES256KeyPair::from_pem(&private_key_pem).map_err(AppError::internal)?;
        let point =
            web_push_native::p256::PublicKey::from_sec1_bytes(&private_key.public_key().to_bytes())
                .map_err(AppError::internal)?;
        let derived = URL_SAFE_NO_PAD.encode(point.to_encoded_point(false).as_bytes());
        if derived != public_key {
            return Err(AppError::internal(
                "Web Push VAPID public and private keys do not match",
            ));
        }
        Ok(Self {
            public_key,
            private_key,
            subject,
            client: reqwest::Client::builder()
                .https_only(true)
                .build()
                .map_err(AppError::internal)?,
        })
    }
}

#[async_trait::async_trait]
impl WebPushDelivery for VapidWebPush {
    fn public_key(&self) -> String {
        self.public_key.clone()
    }

    async fn send(
        &self,
        subscription: &WebPushSubscription,
        payload: &str,
    ) -> Result<bool, AppError> {
        let endpoint = subscription.endpoint.parse().map_err(AppError::internal)?;
        let p256dh = URL_SAFE_NO_PAD
            .decode(&subscription.p256dh)
            .map_err(AppError::internal)?;
        let auth_bytes = URL_SAFE_NO_PAD
            .decode(&subscription.auth_secret)
            .map_err(AppError::internal)?;
        if auth_bytes.len() != 16 {
            return Err(AppError::internal("invalid web push authentication key"));
        }
        let user_public = PublicKey::from_sec1_bytes(&p256dh).map_err(AppError::internal)?;
        let auth = Auth::clone_from_slice(&auth_bytes);
        let request = WebPushBuilder::new(endpoint, user_public, auth)
            .with_vapid(&self.private_key, &self.subject)
            .build(payload.as_bytes())
            .map_err(AppError::internal)?;
        let (parts, body) = request.into_parts();
        let mut request = self.client.request(parts.method, parts.uri.to_string());
        for (name, value) in parts.headers.iter() {
            request = request.header(name.as_str(), value.as_bytes());
        }
        let response = request
            .body(body)
            .send()
            .await
            .map_err(AppError::internal)?;
        if response.status().is_success() {
            return Ok(true);
        }
        if response.status() == reqwest::StatusCode::GONE
            || response.status() == reqwest::StatusCode::NOT_FOUND
        {
            return Ok(false);
        }
        Err(AppError::internal(format!(
            "push service returned status {}",
            response.status()
        )))
    }
}
