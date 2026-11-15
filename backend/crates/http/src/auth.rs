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
use rustchat_domain::identity::{
    ProfilePrivacy, ProfileVisibility, Session, SessionId, User, UserProfile,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{ApiError, AppState, client_ip::ClientIp};

/// JSON body extractor whose failures use the structured error format.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct ApiJson<T>(pub T);

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordResetRequest {
    email: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordResetCompletion {
    token: String,
    new_password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterRequest {
    username: String,
    password: String,
    #[serde(default)]
    email: Option<String>,
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

#[derive(Serialize)]
pub struct ProfileDto {
    id: Uuid,
    username: String,
    display_name: String,
    bio: String,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: OffsetDateTime,
}

impl From<UserProfile> for ProfileDto {
    fn from(profile: UserProfile) -> Self {
        Self {
            id: profile.user_id.as_uuid(),
            username: profile.username.to_string(),
            display_name: profile.display_name,
            bio: profile.bio,
            updated_at: profile.updated_at,
        }
    }
}

#[derive(Serialize)]
pub struct ProfileEnvelope {
    profile: ProfileDto,
}

#[derive(Serialize)]
pub struct PrivacyDto {
    discoverable: bool,
    profile_visibility: &'static str,
    presence_visible: bool,
}

impl From<ProfilePrivacy> for PrivacyDto {
    fn from(value: ProfilePrivacy) -> Self {
        Self {
            discoverable: value.discoverable,
            profile_visibility: match value.visibility {
                ProfileVisibility::Everyone => "everyone",
                ProfileVisibility::ConversationMembers => "conversation_members",
            },
            presence_visible: value.presence_visible,
        }
    }
}

#[derive(Serialize)]
pub struct PrivacyEnvelope {
    privacy: PrivacyDto,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePrivacy {
    discoverable: bool,
    profile_visibility: PrivacyChoice,
    #[serde(default = "default_presence_visible")]
    presence_visible: bool,
}

fn default_presence_visible() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyChoice {
    Everyone,
    ConversationMembers,
}

impl From<PrivacyChoice> for ProfileVisibility {
    fn from(value: PrivacyChoice) -> Self {
        match value {
            PrivacyChoice::Everyone => Self::Everyone,
            PrivacyChoice::ConversationMembers => Self::ConversationMembers,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateProfile {
    display_name: String,
    bio: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateUsername {
    username: String,
}

pub async fn update_username(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ApiJson(body): ApiJson<UpdateUsername>,
) -> Result<Json<UserEnvelope>, ApiError> {
    let user = state
        .auth
        .update_username(auth.user.id, &body.username)
        .await?;
    Ok(Json(UserEnvelope {
        user: UserDto::from(&user),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangePassword {
    current_password: String,
    new_password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountAction {
    current_password: String,
    #[serde(default)]
    confirm: bool,
}

pub async fn deactivate_account(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ClientIp(ip): ClientIp,
    jar: CookieJar,
    ApiJson(body): ApiJson<AccountAction>,
) -> Result<(CookieJar, StatusCode), ApiError> {
    state
        .auth
        .deactivate_account(&auth, ip, body.current_password)
        .await?;
    Ok((jar.add(removal_cookie(&state)), StatusCode::NO_CONTENT))
}

pub async fn delete_account(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ClientIp(ip): ClientIp,
    jar: CookieJar,
    ApiJson(body): ApiJson<AccountAction>,
) -> Result<(CookieJar, StatusCode), ApiError> {
    if !body.confirm {
        return Err(AppError::InvalidAccountConfirmation.into());
    }
    state
        .auth
        .delete_account(&auth, ip, body.current_password)
        .await?;
    Ok((jar.add(removal_cookie(&state)), StatusCode::NO_CONTENT))
}

pub async fn change_password(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<ChangePassword>,
) -> Result<StatusCode, ApiError> {
    state
        .auth
        .change_password(&auth, ip, body.current_password, body.new_password)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn profile(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
) -> Result<Json<ProfileEnvelope>, ApiError> {
    let profile = state.auth.profile(auth.user.id).await?;
    Ok(Json(ProfileEnvelope {
        profile: profile.into(),
    }))
}

pub async fn update_profile(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ApiJson(body): ApiJson<UpdateProfile>,
) -> Result<Json<ProfileEnvelope>, ApiError> {
    let profile = state
        .auth
        .update_profile(auth.user.id, &body.display_name, &body.bio)
        .await?;
    Ok(Json(ProfileEnvelope {
        profile: profile.into(),
    }))
}

pub async fn user_profile(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    Path(username): Path<String>,
) -> Result<Json<ProfileEnvelope>, ApiError> {
    let profile = state.auth.user_profile(auth.user.id, &username).await?;
    Ok(Json(ProfileEnvelope {
        profile: profile.into(),
    }))
}

pub async fn privacy(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
) -> Result<Json<PrivacyEnvelope>, ApiError> {
    let privacy = state.auth.privacy(auth.user.id).await?;
    Ok(Json(PrivacyEnvelope {
        privacy: privacy.into(),
    }))
}

pub async fn update_privacy(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ApiJson(body): ApiJson<UpdatePrivacy>,
) -> Result<Json<PrivacyEnvelope>, ApiError> {
    let privacy = state
        .auth
        .update_privacy(
            auth.user.id,
            ProfilePrivacy {
                discoverable: body.discoverable,
                visibility: body.profile_visibility.into(),
                presence_visible: body.presence_visible,
            },
        )
        .await?;
    let online = privacy
        .presence_visible
        .then(|| state.presence.is_online(auth.user.id.as_uuid()));
    let _ = state.presence_events.send(crate::PresenceEvent {
        user_id: auth.user.id.as_uuid(),
        online,
        occurred_at: OffsetDateTime::now_utc(),
    });
    Ok(Json(PrivacyEnvelope {
        privacy: privacy.into(),
    }))
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
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<RegisterRequest>,
) -> Result<(StatusCode, Json<UserEnvelope>), ApiError> {
    let user = state
        .auth
        .register_with_email(ip, &body.username, body.email.as_deref(), body.password)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(UserEnvelope {
            user: UserDto::from(&user),
        }),
    ))
}

pub async fn request_password_reset(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<PasswordResetRequest>,
) -> Result<StatusCode, ApiError> {
    state.auth.request_password_reset(ip, &body.email).await?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn complete_password_reset(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<PasswordResetCompletion>,
) -> Result<StatusCode, ApiError> {
    state
        .auth
        .complete_password_reset(&body.token, body.new_password)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    jar: CookieJar,
    ApiJson(body): ApiJson<Credentials>,
) -> Result<(CookieJar, Json<UserEnvelope>), ApiError> {
    let LoginResult { user, token, .. } =
        state.auth.login(ip, &body.username, body.password).await?;
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
