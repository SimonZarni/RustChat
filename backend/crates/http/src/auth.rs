//! Authentication and session endpoints.

use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Path, State},
    http::{StatusCode, request::Parts},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use rustchat_application::{
    auth::{AuthContext, LoginResult},
    error::AppError,
};
use rustchat_domain::identity::{Session, SessionId, User};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{ApiError, AppState};

/// JSON body extractor whose failures use the structured error format.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct ApiJson<T>(T);

/// The authenticated principal, resolved from the session cookie.
pub struct AuthUser(pub AuthContext);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get(state.config.cookie_name())
            .map(|cookie| cookie.value().to_owned())
            .ok_or(AppError::Unauthenticated)?;
        Ok(Self(state.auth.authenticate(&token).await?))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    username: String,
    password: String,
}

#[derive(Serialize)]
pub struct UserDto {
    id: Uuid,
    username: String,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
}

impl From<&User> for UserDto {
    fn from(user: &User) -> Self {
        Self {
            id: user.id.as_uuid(),
            username: user.username.to_string(),
            created_at: user.created_at,
        }
    }
}

#[derive(Serialize)]
pub struct UserEnvelope {
    user: UserDto,
}

#[derive(Serialize)]
pub struct SessionDto {
    id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    expires_at: OffsetDateTime,
    current: bool,
}

#[derive(Serialize)]
pub struct SessionsEnvelope {
    sessions: Vec<SessionDto>,
}

fn session_cookie(state: &AppState, value: String) -> Cookie<'static> {
    Cookie::build((state.config.cookie_name(), value))
        .path("/")
        .http_only(true)
        .secure(state.config.cookie_secure)
        .same_site(SameSite::Strict)
        .max_age(state.config.session_ttl)
        .build()
}

fn removal_cookie(state: &AppState) -> Cookie<'static> {
    Cookie::build((state.config.cookie_name(), ""))
        .path("/")
        .http_only(true)
        .secure(state.config.cookie_secure)
        .same_site(SameSite::Strict)
        .max_age(time::Duration::ZERO)
        .build()
}

pub async fn register(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<Credentials>,
) -> Result<(StatusCode, Json<UserEnvelope>), ApiError> {
    let user = state.auth.register(&body.username, body.password).await?;
    Ok((
        StatusCode::CREATED,
        Json(UserEnvelope {
            user: UserDto::from(&user),
        }),
    ))
}

pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    ApiJson(body): ApiJson<Credentials>,
) -> Result<(CookieJar, Json<UserEnvelope>), ApiError> {
    let LoginResult { user, token, .. } = state.auth.login(&body.username, body.password).await?;
    let jar = jar.add(session_cookie(&state, token));
    Ok((
        jar,
        Json(UserEnvelope {
            user: UserDto::from(&user),
        }),
    ))
}

pub async fn logout(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    jar: CookieJar,
) -> Result<(CookieJar, StatusCode), ApiError> {
    state.auth.logout(&auth).await?;
    Ok((jar.add(removal_cookie(&state)), StatusCode::NO_CONTENT))
}

pub async fn me(AuthUser(auth): AuthUser) -> Json<UserEnvelope> {
    Json(UserEnvelope {
        user: UserDto::from(&auth.user),
    })
}

pub async fn list_sessions(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
) -> Result<Json<SessionsEnvelope>, ApiError> {
    let sessions = state
        .auth
        .list_sessions(&auth)
        .await?
        .into_iter()
        .map(|info| {
            let Session {
                id,
                created_at,
                expires_at,
                ..
            } = info.session;
            SessionDto {
                id: id.as_uuid(),
                created_at,
                expires_at,
                current: info.is_current,
            }
        })
        .collect();
    Ok(Json(SessionsEnvelope { sessions }))
}

pub async fn revoke_session(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    // An unparseable id cannot name any session: same answer as "not found".
    let id = Uuid::parse_str(&session_id).map_err(|_| AppError::NotFound)?;
    state
        .auth
        .revoke_session(&auth, SessionId::from_uuid(id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
