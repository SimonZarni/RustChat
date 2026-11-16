//! HTTP adapter: translates HTTP requests into application operations.

mod admin;
mod auth;
mod client_ip;
mod communities;
mod conversations;
mod error;
mod messages;
mod presence;
mod realtime;

use std::future::Future;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use uuid::Uuid;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
};
use futures_util::future::BoxFuture;
use rustchat_application::{auth::AuthService, error::AppError, messages::MessageService};
use rustchat_domain::message::Message;
use serde::Serialize;
use tokio::sync::broadcast;

pub use client_ip::ClientIpSource;
pub use error::ApiError;

/// Request bodies on the API are small JSON documents.
const MAX_BODY_BYTES: usize = 16 * 1024;

#[derive(Default)]
struct HttpMetrics {
    responses_by_class: [AtomicU64; 5],
}

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
    pub(crate) api_limits: Option<Arc<rustchat_application::ratelimit::ApiRateLimiter>>,
    pub(crate) conversations: Arc<rustchat_application::conversations::ConversationService>,
    pub(crate) messages: Arc<MessageService>,
    pub(crate) communities: Arc<rustchat_application::communities::CommunityService>,
    pub(crate) events: broadcast::Sender<Message>,
    pub(crate) typing_events: broadcast::Sender<TypingEvent>,
    pub(crate) read_cursor_events: broadcast::Sender<ReadCursorEvent>,
    pub(crate) reaction_events: broadcast::Sender<ReactionEvent>,
    pub(crate) delivery_cursor_events: broadcast::Sender<DeliveryCursorEvent>,
    pub(crate) presence_events: broadcast::Sender<PresenceEvent>,
    pub(crate) community_events: broadcast::Sender<CommunityEvent>,
    pub(crate) presence: Arc<presence::PresenceTracker>,
    pub(crate) config: Arc<HttpConfig>,
    readiness_probe: Arc<dyn Fn() -> BoxFuture<'static, bool> + Send + Sync>,
    metrics: Arc<HttpMetrics>,
}

#[derive(Clone)]
pub struct AppEvents {
    pub messages: broadcast::Sender<Message>,
    pub typing: broadcast::Sender<TypingEvent>,
    pub read_cursors: broadcast::Sender<ReadCursorEvent>,
    pub reactions: broadcast::Sender<ReactionEvent>,
    pub delivery_cursors: broadcast::Sender<DeliveryCursorEvent>,
    pub presence: broadcast::Sender<PresenceEvent>,
    pub community: broadcast::Sender<CommunityEvent>,
}

impl AppState {
    pub fn new(
        auth: Arc<AuthService>,
        conversations: Arc<rustchat_application::conversations::ConversationService>,
        messages: Arc<MessageService>,
        communities: Arc<rustchat_application::communities::CommunityService>,
        events: AppEvents,
        config: HttpConfig,
    ) -> Self {
        Self {
            auth,
            api_limits: None,
            conversations,
            messages,
            communities,
            events: events.messages,
            typing_events: events.typing,
            read_cursor_events: events.read_cursors,
            reaction_events: events.reactions,
            delivery_cursor_events: events.delivery_cursors,
            presence_events: events.presence,
            community_events: events.community,
            presence: Arc::new(presence::PresenceTracker::default()),
            config: Arc::new(config),
            readiness_probe: Arc::new(|| Box::pin(async { true })),
            metrics: Arc::new(HttpMetrics::default()),
        }
    }

    /// Enforce per-user and unauthenticated request budgets on authenticated routes.
    pub fn with_api_rate_limiter(
        mut self,
        limiter: Arc<rustchat_application::ratelimit::ApiRateLimiter>,
    ) -> Self {
        self.api_limits = Some(limiter);
        self
    }

    pub fn with_readiness_probe<F, Fut>(mut self, probe: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.readiness_probe = Arc::new(move || Box::pin(probe()));
        self
    }
}

#[derive(Debug, Clone)]
pub struct TypingEvent {
    pub conversation_id: Uuid,
    pub user_id: Uuid,
    pub active: bool,
    pub occurred_at: time::OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct ReadCursorEvent {
    pub conversation_id: Uuid,
    pub user_id: Uuid,
    pub sequence: i64,
    pub occurred_at: time::OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct ReactionEvent {
    pub conversation_id: Uuid,
    pub message_id: Uuid,
    pub user_id: Uuid,
    pub sequence: i64,
    pub emoji: String,
    pub added: bool,
    pub occurred_at: time::OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct DeliveryCursorEvent {
    pub conversation_id: Uuid,
    pub user_id: Uuid,
    pub sequence: i64,
    pub occurred_at: time::OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct PresenceEvent {
    pub user_id: Uuid,
    /// `None` clears a previously visible status when the user hides presence.
    pub online: Option<bool>,
    pub occurred_at: time::OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct CommunityEvent {
    pub community_id: Uuid,
    /// `members.updated` or `channels.updated`.
    pub kind: &'static str,
    pub occurred_at: time::OffsetDateTime,
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/admin/users", get(admin::list_users))
        .route("/admin/audit", get(admin::audit_log))
        .route(
            "/admin/users/{user_id}/suspension",
            axum::routing::put(admin::set_suspension),
        )
        .route(
            "/conversations",
            post(conversations::create).get(conversations::list),
        )
        .route("/conversations/{conversation_id}", get(conversations::get))
        .route(
            "/conversations/{conversation_id}/members",
            get(conversations::members).post(conversations::add_member),
        )
        .route(
            "/conversations/{conversation_id}/messages",
            post(messages::send).get(messages::list),
        )
        .route(
            "/conversations/{conversation_id}/messages/{message_id}",
            axum::routing::patch(messages::edit).delete(messages::delete),
        )
        .route(
            "/conversations/{conversation_id}/messages/{message_id}/reactions",
            put(messages::add_reaction).delete(messages::remove_reaction),
        )
        .route(
            "/conversations/{conversation_id}/reactions",
            get(messages::list_reactions),
        )
        .route(
            "/conversations/{conversation_id}/read-cursor",
            put(messages::advance_read_cursor),
        )
        .route(
            "/conversations/{conversation_id}/read-cursors",
            get(messages::read_cursors),
        )
        .route(
            "/conversations/{conversation_id}/delivery-cursor",
            put(messages::advance_delivery_cursor),
        )
        .route(
            "/conversations/{conversation_id}/delivery-cursors",
            get(messages::delivery_cursors),
        )
        .route("/realtime", get(realtime::upgrade))
        .route(
            "/communities",
            post(communities::create).get(communities::list),
        )
        .route(
            "/communities/{community_id}",
            patch(communities::update_settings).delete(communities::delete_community),
        )
        .route(
            "/communities/{community_id}/audit",
            get(communities::audit_log),
        )
        .route(
            "/communities/{community_id}/ownership",
            post(communities::transfer_ownership),
        )
        .route("/communities/join", post(communities::join))
        .route(
            "/communities/{community_id}/members",
            get(communities::members).delete(communities::leave),
        )
        .route(
            "/communities/{community_id}/members/{user_id}",
            delete(communities::remove_member),
        )
        .route(
            "/communities/{community_id}/invites",
            get(communities::list_invites)
                .post(communities::create_invite)
                .delete(communities::revoke_invite),
        )
        .route(
            "/communities/{community_id}/invites/{invite_id}",
            delete(communities::revoke_invite_by_id),
        )
        .route(
            "/communities/{community_id}/channels",
            get(communities::channels).post(communities::create_channel),
        )
        .route(
            "/communities/{community_id}/categories",
            get(communities::categories).post(communities::create_category),
        )
        .route(
            "/communities/{community_id}/categories/{category_id}",
            patch(communities::update_category).delete(communities::delete_category),
        )
        .route(
            "/communities/{community_id}/channels/{channel_id}",
            patch(communities::update_channel).delete(communities::delete_channel),
        )
        .route(
            "/communities/{community_id}/roles",
            get(communities::roles).post(communities::create_role),
        )
        .route(
            "/communities/{community_id}/roles/{role_id}",
            patch(communities::update_role).delete(communities::delete_role),
        )
        .route(
            "/communities/{community_id}/members/{user_id}/roles/{role_id}",
            post(communities::assign_role).delete(communities::remove_role),
        )
        .route(
            "/channels/{channel_id}/permissions/{role_id}",
            put(communities::set_channel_override),
        )
        .route(
            "/conversations/{conversation_id}/members/{user_id}",
            delete(conversations::remove_member),
        )
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/password", post(auth::change_password))
        .route("/auth/email", get(auth::email_status).put(auth::set_email))
        .route("/auth/email/resend", post(auth::resend_email_verification))
        .route("/auth/email/verify", post(auth::verify_email))
        .route(
            "/auth/password-reset/request",
            post(auth::request_password_reset),
        )
        .route(
            "/auth/password-reset/complete",
            post(auth::complete_password_reset),
        )
        .route("/auth/deactivate", post(auth::deactivate_account))
        .route("/auth/delete", post(auth::delete_account))
        .route(
            "/auth/profile",
            get(auth::profile).patch(auth::update_profile),
        )
        .route(
            "/auth/privacy",
            get(auth::privacy).patch(auth::update_privacy),
        )
        .route("/auth/username", patch(auth::update_username))
        .route("/users/{username}/profile", get(auth::user_profile))
        .route("/auth/sessions", get(auth::list_sessions))
        .route("/auth/sessions/{session_id}", delete(auth::revoke_session))
        .fallback(|| async { ApiError::App(AppError::NotFound) })
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::apply_session_rotation,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_allowed_origin,
        ))
        .layer(middleware::map_response(no_store))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES));

    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .nest("/api/v1", api)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            collect_http_metrics,
        ))
        .with_state(state)
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

async fn healthz() -> Json<Health> {
    Json(Health { status: "ok" })
}

async fn readyz(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    let ready = (state.readiness_probe)().await;
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(Health {
            status: if ready { "ok" } else { "unavailable" },
        }),
    )
}

async fn collect_http_metrics(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let response = next.run(request).await;
    let status_class = (response.status().as_u16() as usize / 100).clamp(1, 5);
    state.metrics.responses_by_class[status_class - 1].fetch_add(1, Ordering::Relaxed);
    response
}

async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let mut body = String::from(
        "# HELP rustchat_http_responses_total HTTP responses grouped by status class.\n# TYPE rustchat_http_responses_total counter\n",
    );
    for (index, class) in ["1xx", "2xx", "3xx", "4xx", "5xx"].into_iter().enumerate() {
        let value = state.metrics.responses_by_class[index].load(Ordering::Relaxed);
        body.push_str(&format!(
            "rustchat_http_responses_total{{status_class=\"{class}\"}} {value}\n"
        ));
    }
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
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
