//! Structured, non-sensitive API errors.

use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::StatusCode,
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
                AppError::InvalidPassword(e) => body(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "VALIDATION_ERROR",
                    e.to_string(),
                ),
                AppError::UsernameTaken => body(
                    StatusCode::CONFLICT,
                    "USERNAME_TAKEN",
                    "That username is already taken.",
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
                AppError::NotFound => {
                    body(StatusCode::NOT_FOUND, "NOT_FOUND", "Resource not found.")
                }
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
