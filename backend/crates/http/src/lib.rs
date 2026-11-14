//! HTTP adapter: translates HTTP requests into application operations.

mod auth;
mod client_ip;
mod error;

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderValue, Method, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use rustchat_application::{auth::AuthService, error::AppError};
use serde::Serialize;

pub use client_ip::ClientIpSource;
pub use error::ApiError;

/// Request bodies on the API are small JSON documents.
const MAX_BODY_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone)]
pub struct HttpConfig {
    /// Exact `Origin` values (for example `https://chat.example.com`) allowed
    /// to make state-changing requests.
    pub allowed_origins: Vec<String>,
    /// Mark the session cookie `Secure`. Must be true behind HTTPS.
    pub cookie_secure: bool,
    pub session_ttl: time::Duration,
    /// How the client address used for rate limiting is determined.
    pub client_ip: ClientIpSource,
}

impl HttpConfig {
    /// With `Secure` the `__Host-` prefix makes browsers pin the cookie to
    /// this exact host and path. Without TLS (local development) it is
    /// rejected by browsers, so a plain name is used.
    pub fn cookie_name(&self) -> &'static str {
        if self.cookie_secure {
            "__Host-rustchat_session"
        } else {
            "rustchat_session"
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub(crate) auth: Arc<AuthService>,
    pub(crate) config: Arc<HttpConfig>,
}

impl AppState {
    pub fn new(auth: Arc<AuthService>, config: HttpConfig) -> Self {
        Self {
            auth,
            config: Arc::new(config),
        }
    }
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/sessions", get(auth::list_sessions))
        .route("/auth/sessions/{session_id}", delete(auth::revoke_session))
        .fallback(|| async { ApiError::App(AppError::NotFound) })
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_allowed_origin,
        ))
        .layer(middleware::map_response(no_store))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES));

    Router::new()
        .route("/healthz", get(healthz))
        .nest("/api/v1", api)
        .with_state(state)
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

async fn healthz() -> Json<Health> {
    Json(Health { status: "ok" })
}

/// CSRF defence for cookie-authenticated, state-changing requests: browsers
/// always send `Origin` on cross-site POST/DELETE, and only configured
/// origins are accepted. Safe methods are not checked.
async fn require_allowed_origin(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return next.run(request).await;
    }
    let allowed = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| state.config.allowed_origins.iter().any(|a| a == origin));
    if allowed {
        next.run(request).await
    } else {
        ApiError::OriginNotAllowed.into_response()
    }
}

/// Authentication responses must never be cached.
async fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
