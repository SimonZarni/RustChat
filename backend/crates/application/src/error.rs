use rustchat_domain::{
    attachment::AttachmentError,
    conversation::ConversationNameError,
    identity::{EmailAddressError, PasswordError, UsernameError},
    message::{MessageContentError, ReactionEmojiError},
};

/// Errors returned by application use cases. `Internal` carries a
/// diagnostic for server-side logs only and must never reach clients.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("invalid username: {0}")]
    InvalidUsername(#[from] UsernameError),
    #[error("invalid email: {0}")]
    InvalidEmail(#[from] EmailAddressError),
    #[error("invalid password: {0}")]
    InvalidPassword(#[from] PasswordError),
    #[error("username is already taken")]
    UsernameTaken,
    #[error("email is already associated with an account")]
    EmailTaken,
    #[error("registration is closed")]
    RegistrationClosed,
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("not authenticated")]
    Unauthenticated,
    #[error("too many attempts; retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },
    #[error("not found")]
    NotFound,
    #[error("community ownership must be transferred before leaving")]
    CommunityOwnerCannotLeave,
    #[error("transfer community ownership before deleting this account")]
    AccountOwnsCommunities,
    #[error("explicit account deletion confirmation is required")]
    InvalidAccountConfirmation,
    #[error("email delivery is not configured")]
    EmailUnavailable,
    #[error("instance administrator privileges are required")]
    InstanceAdminRequired,
    #[error("administrators cannot suspend their own account")]
    CannotSuspendSelf,
    #[error("administrator accounts cannot be modified here")]
    CannotModifyInstanceAdmin,
    #[error("account token is invalid or expired")]
    InvalidAccountToken,
    #[error("email address is already verified by another account")]
    EmailAddressInUse,
    #[error("a second factor is required")]
    MfaRequired,
    #[error("multi-factor authentication is not configured on this server")]
    MfaUnavailable,
    #[error("multi-factor authentication is already enabled")]
    MfaAlreadyEnabled,
    #[error("the verification code is invalid or has expired")]
    InvalidMfaCode,
    #[error("conversation request is invalid")]
    InvalidConversationRequest,
    #[error("conversation member limit reached")]
    ConversationMemberLimit,
    #[error("invalid group name: {0}")]
    InvalidConversationName(#[from] ConversationNameError),
    #[error("invalid message: {0}")]
    InvalidMessage(#[from] MessageContentError),
    #[error("invalid reaction: {0}")]
    InvalidReaction(#[from] ReactionEmojiError),
    #[error("idempotency key was already used with different content")]
    IdempotencyConflict,
    #[error("profile field is outside its allowed length")]
    InvalidProfile,
    #[error("read cursor cannot exceed the latest conversation sequence")]
    InvalidReadCursor,
    #[error("delivery cursor cannot exceed the latest conversation sequence")]
    InvalidDeliveryCursor,
    #[error("reaction request is invalid")]
    InvalidReactionRequest,
    #[error("attachment request is invalid")]
    InvalidAttachmentRequest,
    #[error("attachment quota or size limit was exceeded")]
    AttachmentLimitExceeded,
    #[error("uploaded attachment content is not allowed")]
    InvalidAttachmentContent(#[from] AttachmentError),
    #[error("attachment storage is not configured")]
    AttachmentStorageUnavailable,
    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    pub fn internal(error: impl std::fmt::Display) -> Self {
        Self::Internal(error.to_string())
    }
}
