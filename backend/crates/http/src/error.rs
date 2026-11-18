//! Structured, non-sensitive API errors.

use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use rustchat_application::error::AppError;
use serde::Serialize;

pub enum ApiError {
    App(AppError),
    OriginNotAllowed,
    /// Malformed, oversized or wrongly typed request body.
    BadRequest(StatusCode),
}

impl From<AppError> for ApiError {
    fn from(error: AppError) -> Self {
        Self::App(error)
    }
}

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        Self::BadRequest(rejection.status())
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: &'static str,
    message: String,
}

fn body(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    let body = ErrorBody {
        error: ErrorDetail {
            code,
            message: message.into(),
        },
    };
    (status, Json(body)).into_response()
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::OriginNotAllowed => body(
                StatusCode::FORBIDDEN,
                "ORIGIN_NOT_ALLOWED",
                "This request origin is not allowed.",
            ),
            Self::BadRequest(status) if status == StatusCode::PAYLOAD_TOO_LARGE => body(
                status,
                "PAYLOAD_TOO_LARGE",
                "The request body is too large.",
            ),
            Self::BadRequest(status) => body(
                status,
                "BAD_REQUEST",
                "The request body is missing or malformed.",
            ),
            Self::App(error) => match error {
                // Validation messages come from the domain and are safe to show.
                AppError::InvalidUsername(e) => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "VALIDATION_ERROR",
                    e.to_string(),
                ),
                AppError::InvalidEmail(e) => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "VALIDATION_ERROR",
                    e.to_string(),
                ),
                AppError::InvalidPassword(e) => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "VALIDATION_ERROR",
                    e.to_string(),
                ),
                AppError::UsernameTaken => body(
                    StatusCode::CONFLICT,
                    "USERNAME_TAKEN",
                    "That username or email is already in use.",
                ),
                AppError::EmailTaken => body(
                    StatusCode::CONFLICT,
                    "USERNAME_TAKEN",
                    "That username or email is already in use.",
                ),
                AppError::RegistrationClosed => body(
                    StatusCode::FORBIDDEN,
                    "REGISTRATION_CLOSED",
                    "Registration is not open on this server.",
                ),
                AppError::InvalidCredentials => body(
                    StatusCode::UNAUTHORIZED,
                    "INVALID_CREDENTIALS",
                    "Invalid username or password.",
                ),
                AppError::Unauthenticated => body(
                    StatusCode::UNAUTHORIZED,
                    "UNAUTHENTICATED",
                    "Authentication is required.",
                ),
                AppError::RateLimited { retry_after_secs } => {
                    let mut response = body(
                        StatusCode::TOO_MANY_REQUESTS,
                        "RATE_LIMITED",
                        "Too many attempts. Please wait before trying again.",
                    );
                    response
                        .headers_mut()
                        .insert(header::RETRY_AFTER, HeaderValue::from(retry_after_secs));
                    response
                }
                AppError::NotFound => {
                    body(StatusCode::NOT_FOUND, "NOT_FOUND", "Resource not found.")
                }
                AppError::InvalidNotificationPreferences => body(
                    StatusCode::BAD_REQUEST,
                    "INVALID_NOTIFICATION_PREFERENCES",
                    "Notification preferences are invalid.",
                ),
                AppError::WebPushUnavailable => body(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "WEB_PUSH_UNAVAILABLE",
                    "Background notifications are not configured on this server.",
                ),
                AppError::WebPushSubscriptionLimit => body(
                    StatusCode::TOO_MANY_REQUESTS,
                    "WEB_PUSH_SUBSCRIPTION_LIMIT",
                    "This account has reached its device notification limit.",
                ),
                AppError::VoiceUnavailable => body(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "VOICE_UNAVAILABLE",
                    "Voice service is not configured on this server.",
                ),
                AppError::CommunityOwnerCannotLeave => body(
                    StatusCode::CONFLICT,
                    "COMMUNITY_OWNER_CANNOT_LEAVE",
                    "Transfer community ownership before leaving.",
                ),
                AppError::AccountOwnsCommunities => body(
                    StatusCode::CONFLICT,
                    "ACCOUNT_OWNS_COMMUNITIES",
                    "Transfer community ownership before deactivating or deleting this account.",
                ),
                AppError::InvalidAccountConfirmation => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "CONFIRMATION_REQUIRED",
                    "Set confirm to true to delete this account.",
                ),
                AppError::EmailUnavailable => body(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "EMAIL_UNAVAILABLE",
                    "Account email delivery is not configured on this server.",
                ),
                AppError::InstanceAdminRequired => body(
                    StatusCode::FORBIDDEN,
                    "INSTANCE_ADMIN_REQUIRED",
                    "Instance administrator privileges are required.",
                ),
                AppError::CannotSuspendSelf => body(
                    StatusCode::CONFLICT,
                    "CANNOT_SUSPEND_SELF",
                    "Administrators cannot suspend their own account.",
                ),
                AppError::CannotModifyInstanceAdmin => body(
                    StatusCode::CONFLICT,
                    "CANNOT_MODIFY_INSTANCE_ADMIN",
                    "Configured instance administrator accounts cannot be modified here.",
                ),
                AppError::EmailAddressInUse => body(
                    StatusCode::CONFLICT,
                    "EMAIL_ADDRESS_IN_USE",
                    "Another account has already verified this email address.",
                ),
                AppError::MfaRequired => body(
                    StatusCode::UNAUTHORIZED,
                    "MFA_REQUIRED",
                    "A verification code is required to sign in.",
                ),
                AppError::MfaUnavailable => body(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "MFA_UNAVAILABLE",
                    "Multi-factor authentication is not configured on this server.",
                ),
                AppError::MfaAlreadyEnabled => body(
                    StatusCode::CONFLICT,
                    "MFA_ALREADY_ENABLED",
                    "Multi-factor authentication is already enabled.",
                ),
                AppError::InvalidMfaCode => body(
                    StatusCode::UNAUTHORIZED,
                    "INVALID_MFA_CODE",
                    "The verification code is invalid or has expired.",
                ),
                AppError::InvalidAccountToken => body(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ACCOUNT_TOKEN",
                    "The account link is invalid or expired. Request a new one.",
                ),
                AppError::InvalidConversationRequest => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "INVALID_CONVERSATION",
                    "The conversation request is invalid.",
                ),
                AppError::SocialConflict => body(
                    StatusCode::CONFLICT,
                    "SOCIAL_CONFLICT",
                    "That relationship action conflicts with the current state.",
                ),
                AppError::ConversationMemberLimit => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "MEMBER_LIMIT",
                    "The conversation member limit was reached.",
                ),
                AppError::InvalidConversationName(e) => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "VALIDATION_ERROR",
                    e.to_string(),
                ),
                AppError::InvalidMessage(e) => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "VALIDATION_ERROR",
                    e.to_string(),
                ),
                AppError::IdempotencyConflict => body(
                    StatusCode::CONFLICT,
                    "IDEMPOTENCY_CONFLICT",
                    "The client message ID was already used with different content.",
                ),
                AppError::InvalidProfile => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "VALIDATION_ERROR",
                    "Display name must be at most 64 characters and bio at most 280 characters.",
                ),
                AppError::InvalidReadCursor => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "INVALID_READ_CURSOR",
                    "Read position must be between zero and the latest message sequence.",
                ),
                AppError::InvalidDeliveryCursor => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "INVALID_DELIVERY_CURSOR",
                    "Delivered position must be between zero and the latest message sequence.",
                ),
                AppError::InvalidReaction(e) => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "INVALID_REACTION",
                    e.to_string(),
                ),
                AppError::InvalidReactionRequest => body(
                    StatusCode::BAD_REQUEST,
                    "INVALID_REACTION_REQUEST",
                    "Provide at most 100 valid message IDs.",
                ),
                AppError::InvalidAttachmentRequest => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "INVALID_ATTACHMENT_REQUEST",
                    "The attachment request is invalid.",
                ),
                AppError::AttachmentLimitExceeded => body(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "ATTACHMENT_LIMIT_EXCEEDED",
                    "The attachment exceeds the file-size or account-storage limit.",
                ),
                AppError::InvalidAttachmentContent(error) => body(
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "UNSUPPORTED_ATTACHMENT_TYPE",
                    error.to_string(),
                ),
                AppError::AttachmentStorageUnavailable => body(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "ATTACHMENT_STORAGE_UNAVAILABLE",
                    "Attachment storage is not configured.",
                ),
                AppError::Internal(detail) => {
                    // Log the cause server-side; never return it to the client.
                    tracing::error!(error = %detail, "internal error");
                    body(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "INTERNAL",
                        "An internal error occurred.",
                    )
                }
            },
        }
    }
}
