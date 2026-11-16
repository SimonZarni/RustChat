//! Authentication and session endpoints.

use axum::{
    Json,
    extract::{FromRequest, FromRequestParts, Path, State},
    http::{StatusCode, request::Parts},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use rustchat_application::{
    auth::{AuthContext, LoginOutcome, LoginResult},
    error::AppError,
    ratelimit::ApiOperation,
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
        let authenticated = match jar.get(state.config.cookie_name()) {
            Some(cookie) => {
                let token = cookie.value().to_owned();
                state.auth.authenticate_request(&token).await
            }
            None => Err(AppError::Unauthenticated),
        };
        let request = match authenticated {
            Ok(request) => request,
            Err(AppError::Unauthenticated) => {
                // Requests that fail authentication spend a per-network budget, so a flood
                // of them is cut off instead of being answered indefinitely.
                if let Some(limits) = &state.api_limits {
                    let ClientIp(ip) = ClientIp::from_request_parts(parts, state).await?;
                    limits.enforce_unauthenticated(ip).await?;
                }
                return Err(AppError::Unauthenticated.into());
            }
            Err(other) => return Err(other.into()),
        };
        if let Some(limits) = &state.api_limits {
            let route = parts
                .extensions
                .get::<axum::extract::MatchedPath>()
                .map_or("", |path| path.as_str());
            limits
                .enforce_user(request.context.user.id, classify(&parts.method, route))
                .await?;
        }
        if let (Some(rotated), Some(slot)) = (
            request.rotated_token,
            parts.extensions.get::<RotationSlot>(),
        ) {
            slot.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .replace(rotated);
        }
        Ok(Self(request.context))
    }
}

/// Maps a request to its rate-limit class from the HTTP method and the matched
/// route pattern (never the raw path, which contains identifiers).
fn classify(method: &axum::http::Method, route: &str) -> ApiOperation {
    use axum::http::Method;
    if matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return ApiOperation::Read;
    }
    if *method == Method::POST {
        if route.ends_with("/messages") {
            return ApiOperation::SendMessage;
        }
        const CREATES: [&str; 6] = [
            "/conversations",
            "/communities",
            "/channels",
            "/categories",
            "/roles",
            "/invites",
        ];
        if CREATES.iter().any(|suffix| route.ends_with(suffix)) {
            return ApiOperation::Create;
        }
    }
    ApiOperation::Write
}

/// Carries a rotated session token from the extractor to the response layer.
#[derive(Clone, Default)]
pub struct RotationSlot(std::sync::Arc<std::sync::Mutex<Option<String>>>);

/// Sets the replacement session cookie when the request's session token was
/// rotated, unless the handler already set or cleared the session cookie
/// itself (login, logout, account closure).
pub async fn apply_session_rotation(
    State(state): State<AppState>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let slot = RotationSlot::default();
    request.extensions_mut().insert(slot.clone());
    let mut response = next.run(request).await;
    let rotated = slot.0.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(token) = rotated {
        let prefix = format!("{}=", state.config.cookie_name());
        let handler_set_cookie = response
            .headers()
            .get_all(axum::http::header::SET_COOKIE)
            .iter()
            .any(|value| value.to_str().is_ok_and(|v| v.starts_with(&prefix)));
        if !handler_set_cookie && let Ok(value) = session_cookie(&state, token).to_string().parse()
        {
            response
                .headers_mut()
                .append(axum::http::header::SET_COOKIE, value);
        }
    }
    response
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

#[derive(Serialize)]
pub struct EmailDto {
    /// The recovery address, or null when none is set.
    email: Option<String>,
    verified: bool,
}

impl From<rustchat_application::ports::EmailStatus> for EmailDto {
    fn from(status: rustchat_application::ports::EmailStatus) -> Self {
        Self {
            email: status.address.map(|address| address.as_str().to_owned()),
            verified: status.verified,
        }
    }
}

#[derive(Serialize)]
pub struct EmailEnvelope {
    email: EmailDto,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetEmail {
    /// The new address; null or an empty string removes it.
    email: Option<String>,
    current_password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyEmail {
    token: String,
}

pub async fn email_status(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
) -> Result<Json<EmailEnvelope>, ApiError> {
    let status = state.auth.email_status(auth.user.id).await?;
    Ok(Json(EmailEnvelope {
        email: status.into(),
    }))
}

pub async fn set_email(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<SetEmail>,
) -> Result<Json<EmailEnvelope>, ApiError> {
    let status = state
        .auth
        .set_email(&auth, ip, body.current_password, body.email.as_deref())
        .await?;
    Ok(Json(EmailEnvelope {
        email: status.into(),
    }))
}

pub async fn resend_email_verification(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
) -> Result<StatusCode, ApiError> {
    state.auth.resend_email_verification(&auth).await?;
    Ok(StatusCode::ACCEPTED)
}

pub async fn verify_email(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<VerifyEmail>,
) -> Result<StatusCode, ApiError> {
    state.auth.verify_email(ip, &body.token).await?;
    Ok(StatusCode::NO_CONTENT)
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
) -> Result<(StatusCode, CookieJar, Json<serde_json::Value>), ApiError> {
    match state
        .auth
        .login_step(ip, &body.username, body.password)
        .await?
    {
        LoginOutcome::Session(LoginResult { user, token, .. }) => Ok((
            StatusCode::OK,
            jar.add(session_cookie(&state, token)),
            Json(serde_json::json!({ "user": UserDto::from(&user) })),
        )),
        // No session yet: the client finishes with the code from the second factor.
        LoginOutcome::MfaRequired { challenge } => Ok((
            StatusCode::ACCEPTED,
            jar,
            Json(serde_json::json!({ "mfa_required": true, "mfa_token": challenge })),
        )),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MfaLogin {
    mfa_token: String,
    code: String,
}

pub async fn login_mfa(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    jar: CookieJar,
    ApiJson(body): ApiJson<MfaLogin>,
) -> Result<(CookieJar, Json<UserEnvelope>), ApiError> {
    let LoginResult { user, token, .. } = state
        .auth
        .complete_mfa_login(ip, &body.mfa_token, &body.code)
        .await?;
    Ok((
        jar.add(session_cookie(&state, token)),
        Json(UserEnvelope {
            user: UserDto::from(&user),
        }),
    ))
}

#[derive(Serialize)]
pub struct MfaStatusDto {
    /// The server can enrol authenticators (an encryption key is configured).
    available: bool,
    enabled: bool,
    recovery_codes_remaining: u32,
}

#[derive(Serialize)]
pub struct MfaStatusEnvelope {
    mfa: MfaStatusDto,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginTotp {
    current_password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmTotp {
    code: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MfaReauth {
    current_password: String,
    code: String,
}

#[derive(Serialize)]
pub struct RecoveryCodesEnvelope {
    recovery_codes: Vec<String>,
}

pub async fn mfa_status(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
) -> Result<Json<MfaStatusEnvelope>, ApiError> {
    let status = state.auth.mfa_status(auth.user.id).await?;
    Ok(Json(MfaStatusEnvelope {
        mfa: MfaStatusDto {
            available: status.available,
            enabled: status.enabled,
            recovery_codes_remaining: status.recovery_codes_remaining,
        },
    }))
}

pub async fn begin_totp(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<BeginTotp>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let enrollment = state
        .auth
        .begin_totp_enrollment(&auth, ip, body.current_password)
        .await?;
    Ok(Json(serde_json::json!({
        "secret": enrollment.secret_base32,
        "otpauth_uri": enrollment.provisioning_uri,
    })))
}

pub async fn confirm_totp(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ApiJson(body): ApiJson<ConfirmTotp>,
) -> Result<Json<RecoveryCodesEnvelope>, ApiError> {
    let recovery_codes = state
        .auth
        .confirm_totp_enrollment(&auth, &body.code)
        .await?;
    Ok(Json(RecoveryCodesEnvelope { recovery_codes }))
}

pub async fn disable_mfa(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<MfaReauth>,
) -> Result<StatusCode, ApiError> {
    state
        .auth
        .disable_mfa(&auth, ip, body.current_password, &body.code)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn regenerate_recovery_codes(
    State(state): State<AppState>,
    AuthUser(auth): AuthUser,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<MfaReauth>,
) -> Result<Json<RecoveryCodesEnvelope>, ApiError> {
    let recovery_codes = state
        .auth
        .regenerate_recovery_codes(&auth, ip, body.current_password, &body.code)
        .await?;
    Ok(Json(RecoveryCodesEnvelope { recovery_codes }))
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
