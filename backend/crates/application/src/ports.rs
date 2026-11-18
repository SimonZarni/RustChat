//! Ports: capabilities the application needs from the outside world.
//! Infrastructure implements them; the application never names a concrete
//! database, hashing library or clock.

use async_trait::async_trait;
use rustchat_domain::{
    attachment::Attachment,
    community::{Category, Channel, Community, Role},
    conversation::{Conversation, ConversationName, ConversationPage},
    identity::{
        AttachmentId, CategoryId, ChannelId, CommunityId, ConversationId, EmailAddress, MessageId,
        PlainPassword, ProfilePrivacy, RoleId, Session, SessionId, User, UserId, UserProfile,
        Username,
    },
    message::{
        DeliveryCursor, Message, MessageContent, MessagePage, MessageReaction, ReactionEmoji,
        ReactionMutation, ReadCursor,
    },
};
use time::OffsetDateTime;

use crate::error::AppError;

/// SHA-256 digest of a session token. Only digests are persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenDigest(pub [u8; 32]);

/// A freshly generated secret session token and its digest.
pub struct NewToken {
    pub raw: String,
    pub digest: TokenDigest,
}

pub struct NewUser {
    pub id: UserId,
    pub username: Username,
    pub email: Option<EmailAddress>,
    pub password_hash: String,
    pub created_at: OffsetDateTime,
}

pub struct UserCredentials {
    pub user: User,
    pub password_hash: String,
}

pub struct NewSession {
    pub session: Session,
    pub token_digest: TokenDigest,
}

/// A session joined with its owner, as found by token digest.
pub struct ActiveSession {
    pub session: Session,
    pub user: User,
    /// Last recorded activity; used for the idle timeout.
    pub last_seen_at: OffsetDateTime,
    /// When the cookie token was last replaced; used to schedule rotation.
    pub token_rotated_at: OffsetDateTime,
    /// The request presented the previous token during its rotation grace period.
    pub via_previous_token: bool,
}

/// A TOTP secret sealed with the server key. Only the ciphertext and nonce are stored.
#[derive(Clone, PartialEq, Eq)]
pub struct EncryptedSecret {
    pub ciphertext: Vec<u8>,
    pub nonce: [u8; 12],
}

impl std::fmt::Debug for EncryptedSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EncryptedSecret(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TotpRecord {
    pub secret: EncryptedSecret,
    /// False while enrolment is pending a first valid code.
    pub confirmed: bool,
    pub last_used_step: Option<i64>,
}

/// TOTP (RFC 6238) and the sealing of its secrets. Implemented with
/// maintained, reviewed libraries; no cryptography lives in this crate.
pub trait TotpEngine: Send + Sync {
    /// A new random secret of at least 160 bits.
    fn generate_secret(&self) -> Result<Vec<u8>, AppError>;
    /// Encrypt a secret so that it can only be opened for `user_id`.
    fn seal(&self, user_id: UserId, secret: &[u8]) -> Result<EncryptedSecret, AppError>;
    fn open(&self, user_id: UserId, sealed: &EncryptedSecret) -> Result<Vec<u8>, AppError>;
    fn base32(&self, secret: &[u8]) -> String;
    /// An `otpauth://` URI for authenticator apps.
    fn provisioning_uri(&self, secret: &[u8], account: &str) -> Result<String, AppError>;
    /// The time step a six-digit `code` matches at `now` (allowing one step of
    /// clock skew either way), in constant time with respect to the code.
    fn matching_step(&self, secret: &[u8], code: &str, now: OffsetDateTime) -> Option<i64>;
}

#[async_trait]
pub trait MfaRepository: Send + Sync {
    async fn totp(&self, user_id: UserId) -> Result<Option<TotpRecord>, AppError>;
    /// Store a new pending secret, replacing an earlier pending one. Returns
    /// false when the user already has a confirmed secret.
    async fn begin_totp(
        &self,
        user_id: UserId,
        secret: &EncryptedSecret,
        now: OffsetDateTime,
    ) -> Result<bool, AppError>;
    /// Confirm a pending secret, recording the accepted step and replacing the
    /// recovery codes, atomically. Returns false when nothing was pending.
    async fn confirm_totp(
        &self,
        user_id: UserId,
        step: i64,
        recovery_digests: &[TokenDigest],
        now: OffsetDateTime,
    ) -> Result<bool, AppError>;
    /// Accept `step` only if it is newer than every step accepted before.
    async fn use_totp_step(&self, user_id: UserId, step: i64) -> Result<bool, AppError>;
    async fn use_recovery_code(
        &self,
        user_id: UserId,
        digest: &TokenDigest,
        now: OffsetDateTime,
    ) -> Result<bool, AppError>;
    async fn recovery_codes_remaining(&self, user_id: UserId) -> Result<u32, AppError>;
    async fn replace_recovery_codes(
        &self,
        user_id: UserId,
        digests: &[TokenDigest],
        now: OffsetDateTime,
    ) -> Result<(), AppError>;
    /// Remove the secret, recovery codes and pending challenges.
    async fn disable_totp(&self, user_id: UserId) -> Result<(), AppError>;
    async fn create_challenge(
        &self,
        user_id: UserId,
        digest: TokenDigest,
        now: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> Result<(), AppError>;
    /// Spend one attempt on a live challenge and return its (still active) user.
    /// None when unknown, expired, out of attempts or the user is not active.
    async fn attempt_challenge(
        &self,
        digest: &TokenDigest,
        now: OffsetDateTime,
        max_attempts: i32,
    ) -> Result<Option<User>, AppError>;
    async fn delete_challenge(&self, digest: &TokenDigest) -> Result<(), AppError>;
}

/// A user as named in an audit entry. The username is as currently stored, so a
/// deleted account appears under its tombstone name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditUser {
    pub id: UserId,
    pub username: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuditEvent {
    pub id: rustchat_domain::identity::AuditEventId,
    pub event_type: String,
    pub actor: Option<AuditUser>,
    /// The account acted upon (instance audit only).
    pub target: Option<AuditUser>,
    pub details: serde_json::Value,
    pub created_at: OffsetDateTime,
}

/// One newest-first page of audit events; `next_cursor` is the id to pass as
/// `before` to continue, present only when more events exist.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditPage {
    pub events: Vec<AuditEvent>,
    pub next_cursor: Option<rustchat_domain::identity::AuditEventId>,
}

/// The recovery email address of an account and whether its owner confirmed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailStatus {
    pub address: Option<EmailAddress>,
    pub verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyEmailOutcome {
    Verified,
    /// Unknown, used, expired, or issued for an address the account no longer has.
    Invalid,
    /// Another account has already verified this address.
    AddressInUse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminUser {
    pub user: User,
    pub is_instance_admin: bool,
    pub deactivated_at: Option<OffsetDateTime>,
    pub suspended_at: Option<OffsetDateTime>,
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CreateUserOutcome {
    Created,
    UsernameTaken,
    EmailTaken,
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn create(&self, user: NewUser) -> Result<CreateUserOutcome, AppError>;
    async fn find_credentials(
        &self,
        username: &Username,
    ) -> Result<Option<UserCredentials>, AppError>;
    async fn find_discoverable(
        &self,
        caller: UserId,
        username: &Username,
    ) -> Result<Option<User>, AppError>;
    async fn find_profile_for_caller(
        &self,
        caller: UserId,
        username: &Username,
    ) -> Result<Option<UserProfile>, AppError>;
    async fn update_username(&self, user_id: UserId, username: &Username)
    -> Result<User, AppError>;
    async fn get_profile(&self, user_id: UserId) -> Result<UserProfile, AppError>;
    async fn update_profile(
        &self,
        user_id: UserId,
        display_name: &str,
        bio: &str,
        now: OffsetDateTime,
    ) -> Result<UserProfile, AppError>;
    async fn get_privacy(&self, user_id: UserId) -> Result<ProfilePrivacy, AppError>;
    async fn update_privacy(
        &self,
        user_id: UserId,
        privacy: ProfilePrivacy,
    ) -> Result<ProfilePrivacy, AppError>;
    async fn deactivate_account(
        &self,
        user_id: UserId,
        now: OffsetDateTime,
    ) -> Result<(), AppError>;
    async fn delete_account(
        &self,
        user_id: UserId,
        tombstone_username: &Username,
        unusable_password_hash: &str,
        now: OffsetDateTime,
    ) -> Result<(), AppError>;
    async fn email_status(&self, user_id: UserId) -> Result<EmailStatus, AppError>;
    /// Set or clear the address. A new address is unverified, and verification
    /// tokens issued for the previous one are discarded.
    async fn set_email(
        &self,
        user_id: UserId,
        email: Option<&EmailAddress>,
    ) -> Result<(), AppError>;
    /// Store a verification token for the account's current address, replacing
    /// earlier ones. Returns false when the account no longer has `email`.
    async fn issue_email_verification_token(
        &self,
        user_id: UserId,
        email: &EmailAddress,
        digest: TokenDigest,
        now: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> Result<bool, AppError>;
    /// Consume a verification token and mark its address verified.
    async fn verify_email_with_token(
        &self,
        digest: TokenDigest,
        now: OffsetDateTime,
    ) -> Result<VerifyEmailOutcome, AppError>;
    async fn issue_password_reset_token(
        &self,
        email: &EmailAddress,
        digest: TokenDigest,
        now: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> Result<bool, AppError>;
    async fn reset_password_with_token(
        &self,
        digest: TokenDigest,
        password_hash: String,
        now: OffsetDateTime,
    ) -> Result<bool, AppError>;
    async fn list_admin_users(&self, limit: i64) -> Result<Vec<AdminUser>, AppError>;
    /// Instance administration audit events, newest first.
    async fn list_admin_audit(
        &self,
        before: Option<rustchat_domain::identity::AuditEventId>,
        limit: usize,
    ) -> Result<AuditPage, AppError>;
    async fn set_user_suspended(
        &self,
        actor: UserId,
        target: UserId,
        suspended: bool,
        now: OffsetDateTime,
    ) -> Result<AdminUser, AppError>;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SocialUser {
    pub id: uuid::Uuid,
    pub username: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FriendRequest {
    pub id: uuid::Uuid,
    pub user: SocialUser,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[async_trait]
pub trait SocialRepository: Send + Sync {
    async fn requests(&self, user: UserId, incoming: bool) -> Result<Vec<FriendRequest>, AppError>;
    async fn friends(&self, user: UserId) -> Result<Vec<SocialUser>, AppError>;
    async fn blocks(&self, user: UserId) -> Result<Vec<SocialUser>, AppError>;
    async fn send_request(
        &self,
        sender: UserId,
        username: &Username,
    ) -> Result<uuid::Uuid, AppError>;
    async fn accept_request(&self, user: UserId, request: uuid::Uuid) -> Result<(), AppError>;
    async fn reject_request(&self, user: UserId, request: uuid::Uuid) -> Result<(), AppError>;
    async fn cancel_request(&self, user: UserId, request: uuid::Uuid) -> Result<(), AppError>;
    async fn remove_friend(&self, user: UserId, friend: UserId) -> Result<(), AppError>;
    async fn block(&self, user: UserId, username: &Username) -> Result<(), AppError>;
    async fn unblock(&self, user: UserId, blocked: UserId) -> Result<(), AppError>;
}

#[async_trait]
pub trait ConversationRepository: Send + Sync {
    async fn create_or_get_direct(
        &self,
        caller: UserId,
        target: UserId,
    ) -> Result<Conversation, AppError>;
    async fn create_group(
        &self,
        creator: UserId,
        name: Option<ConversationName>,
        members: Vec<UserId>,
        max_members: usize,
    ) -> Result<Conversation, AppError>;
    async fn add_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
        member: UserId,
        max_members: usize,
    ) -> Result<(), AppError>;
    async fn remove_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
        member: UserId,
    ) -> Result<(), AppError>;
    async fn list_for_member(
        &self,
        caller: UserId,
        before: Option<ConversationId>,
        limit: usize,
    ) -> Result<ConversationPage, AppError>;
    async fn find_for_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
    ) -> Result<Conversation, AppError>;
    async fn members_for_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
    ) -> Result<Vec<User>, AppError>;
    async fn members_presence_for_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
    ) -> Result<Vec<(User, bool)>, AppError>;
    async fn share_conversation(&self, first: UserId, second: UserId) -> Result<bool, AppError>;
}

#[async_trait]
pub trait MessageRepository: Send + Sync {
    async fn send(
        &self,
        caller: UserId,
        conversation: ConversationId,
        client_message_id: MessageId,
        reply_to_message_id: Option<MessageId>,
        content: MessageContent,
    ) -> Result<Message, AppError>;
    async fn send_with_attachments(
        &self,
        caller: UserId,
        conversation: ConversationId,
        client_message_id: MessageId,
        reply_to_message_id: Option<MessageId>,
        content: MessageContent,
        attachments: Vec<AttachmentId>,
    ) -> Result<Message, AppError> {
        if !attachments.is_empty() {
            return Err(AppError::InvalidAttachmentRequest);
        }
        self.send(
            caller,
            conversation,
            client_message_id,
            reply_to_message_id,
            content,
        )
        .await
    }
    async fn edit(
        &self,
        caller: UserId,
        conversation: ConversationId,
        message: MessageId,
        content: MessageContent,
    ) -> Result<Message, AppError>;
    async fn delete(
        &self,
        caller: UserId,
        conversation: ConversationId,
        message: MessageId,
    ) -> Result<Message, AppError>;
    async fn list_for_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
        before_sequence: Option<i64>,
        after_sequence: Option<i64>,
        limit: usize,
    ) -> Result<MessagePage, AppError>;
    async fn advance_read_cursor(
        &self,
        caller: UserId,
        conversation: ConversationId,
        sequence: i64,
        now: OffsetDateTime,
    ) -> Result<ReadCursor, AppError>;
    async fn read_cursors_for_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
    ) -> Result<Vec<ReadCursor>, AppError>;
    async fn list_reactions(
        &self,
        caller: UserId,
        conversation: ConversationId,
        messages: &[MessageId],
    ) -> Result<Vec<MessageReaction>, AppError>;
    async fn set_reaction(
        &self,
        caller: UserId,
        conversation: ConversationId,
        message: MessageId,
        emoji: ReactionEmoji,
        added: bool,
        now: OffsetDateTime,
    ) -> Result<ReactionMutation, AppError>;
    async fn advance_delivery_cursor(
        &self,
        caller: UserId,
        conversation: ConversationId,
        sequence: i64,
        now: OffsetDateTime,
    ) -> Result<DeliveryCursor, AppError>;
    async fn delivery_cursors_for_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
    ) -> Result<Vec<DeliveryCursor>, AppError>;
}

#[async_trait]
pub trait AttachmentRepository: Send + Sync {
    async fn reserve(
        &self,
        caller: UserId,
        conversation: ConversationId,
        id: AttachmentId,
        storage_key: &str,
        file_name: &str,
        declared_type: &str,
        byte_size: i64,
        now: OffsetDateTime,
        expires_at: OffsetDateTime,
        quota_bytes: i64,
    ) -> Result<(String, String), AppError>;
    async fn complete(
        &self,
        caller: UserId,
        attachment: AttachmentId,
        media_type: &str,
        actual_size: i64,
        now: OffsetDateTime,
    ) -> Result<Attachment, AppError>;
    async fn pending_upload(
        &self,
        caller: UserId,
        attachment: AttachmentId,
    ) -> Result<(String, String, i64), AppError>;
    async fn for_download(
        &self,
        caller: UserId,
        attachment: AttachmentId,
    ) -> Result<(Attachment, String), AppError>;
    async fn attach_to_message(
        &self,
        caller: UserId,
        conversation: ConversationId,
        message: MessageId,
        attachments: &[AttachmentId],
    ) -> Result<(), AppError>;
    async fn list_for_messages(
        &self,
        caller: UserId,
        conversation: ConversationId,
        messages: &[MessageId],
    ) -> Result<Vec<Attachment>, AppError>;
    async fn delete_unattached(
        &self,
        caller: UserId,
        attachment: AttachmentId,
    ) -> Result<(String, String), AppError>;
    async fn claim_expired_pending_uploads(
        &self,
        now: OffsetDateTime,
        limit: usize,
    ) -> Result<Vec<ExpiredAttachmentUpload>, AppError>;
    async fn release_expired_pending_uploads(&self, ids: &[AttachmentId]) -> Result<(), AppError>;
    async fn purge_expired_pending_uploads(
        &self,
        ids: &[AttachmentId],
        now: OffsetDateTime,
    ) -> Result<u64, AppError>;
}

#[derive(Debug, Clone)]
pub struct ExpiredAttachmentUpload {
    pub id: AttachmentId,
    pub upload_key: String,
    pub storage_key: String,
}

#[async_trait]
pub trait AttachmentStorage: Send + Sync {
    async fn presign_upload(
        &self,
        storage_key: &str,
        content_type: &str,
        expires_in: std::time::Duration,
    ) -> Result<String, AppError>;
    async fn inspect_upload(&self, storage_key: &str) -> Result<(i64, Vec<u8>), AppError>;
    async fn promote_upload(&self, upload_key: &str, storage_key: &str) -> Result<(), AppError>;
    async fn presign_download(
        &self,
        storage_key: &str,
        file_name: &str,
        expires_in: std::time::Duration,
    ) -> Result<String, AppError>;
    async fn delete(&self, storage_key: &str) -> Result<(), AppError>;
}

#[async_trait]
pub trait CommunityRepository: Send + Sync {
    /// Community audit events, newest first. Requires the community-management
    /// permission; callers without it get `NotFound`.
    async fn list_audit(
        &self,
        caller: UserId,
        community: CommunityId,
        before: Option<rustchat_domain::identity::AuditEventId>,
        limit: usize,
    ) -> Result<AuditPage, AppError>;
    async fn create(
        &self,
        owner: UserId,
        name: &str,
        description: &str,
        now: OffsetDateTime,
    ) -> Result<Community, AppError>;
    async fn list_for_member(&self, caller: UserId) -> Result<Vec<Community>, AppError>;
    async fn update_settings(
        &self,
        caller: UserId,
        community: CommunityId,
        name: &str,
        description: &str,
    ) -> Result<Community, AppError>;
    async fn transfer_ownership(
        &self,
        caller: UserId,
        community: CommunityId,
        new_owner: UserId,
    ) -> Result<(), AppError>;
    async fn delete_community(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<(), AppError>;
    async fn members(&self, caller: UserId, community: CommunityId) -> Result<Vec<User>, AppError>;
    async fn leave(&self, caller: UserId, community: CommunityId) -> Result<(), AppError>;
    async fn remove_member(
        &self,
        caller: UserId,
        community: CommunityId,
        member: UserId,
    ) -> Result<(), AppError>;
    async fn create_invite(
        &self,
        caller: UserId,
        community: CommunityId,
        token_digest: TokenDigest,
        now: OffsetDateTime,
        expires_at: Option<OffsetDateTime>,
        max_uses: Option<i32>,
    ) -> Result<(), AppError>;
    async fn list_invites(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<Vec<rustchat_domain::community::Invite>, AppError>;
    async fn revoke_invite(
        &self,
        caller: UserId,
        community: CommunityId,
        token_digest: TokenDigest,
    ) -> Result<(), AppError>;
    async fn revoke_invite_by_id(
        &self,
        caller: UserId,
        community: CommunityId,
        invite: rustchat_domain::identity::InviteId,
    ) -> Result<(), AppError>;
    async fn join(
        &self,
        caller: UserId,
        token_digest: TokenDigest,
        now: OffsetDateTime,
    ) -> Result<Community, AppError>;
    #[allow(clippy::too_many_arguments)]
    async fn create_channel(
        &self,
        caller: UserId,
        community: CommunityId,
        name: &str,
        description: &str,
        category: Option<CategoryId>,
        read_only: bool,
        now: OffsetDateTime,
    ) -> Result<Channel, AppError>;
    async fn create_category(
        &self,
        caller: UserId,
        community: CommunityId,
        name: &str,
    ) -> Result<Category, AppError>;
    async fn list_categories(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<Vec<Category>, AppError>;
    async fn update_category(
        &self,
        caller: UserId,
        category: CategoryId,
        name: &str,
        position: i32,
    ) -> Result<Category, AppError>;
    async fn delete_category(&self, caller: UserId, category: CategoryId) -> Result<(), AppError>;
    async fn list_channels(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<Vec<Channel>, AppError>;
    #[allow(clippy::too_many_arguments)]
    async fn update_channel(
        &self,
        caller: UserId,
        channel: ChannelId,
        name: &str,
        description: &str,
        category: Option<Option<CategoryId>>,
        read_only: bool,
        position: i32,
    ) -> Result<Channel, AppError>;
    async fn delete_channel(&self, caller: UserId, channel: ChannelId) -> Result<(), AppError>;
    async fn create_role(
        &self,
        caller: UserId,
        community: CommunityId,
        name: &str,
        permissions: u64,
    ) -> Result<Role, AppError>;
    async fn list_roles(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<Vec<Role>, AppError>;
    async fn update_role(
        &self,
        caller: UserId,
        community: CommunityId,
        role: RoleId,
        name: &str,
        permissions: u64,
        position: i32,
    ) -> Result<Role, AppError>;
    async fn delete_role(
        &self,
        caller: UserId,
        community: CommunityId,
        role: RoleId,
    ) -> Result<(), AppError>;
    async fn assign_role(
        &self,
        caller: UserId,
        community: CommunityId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AppError>;
    async fn remove_role(
        &self,
        caller: UserId,
        community: CommunityId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AppError>;
    async fn set_channel_override(
        &self,
        caller: UserId,
        channel: ChannelId,
        role: RoleId,
        allow: u64,
        deny: u64,
    ) -> Result<CommunityId, AppError>;
}

#[async_trait]
pub trait OutboxRepository: Send + Sync {
    /// Claim up to `limit` undispatched events for this instance with a lease and
    /// return the messages they describe, oldest first. Instances sharing the
    /// database receive disjoint events; an unfinished claim becomes available
    /// again when its lease expires.
    async fn pending_messages(
        &self,
        limit: usize,
    ) -> Result<Vec<rustchat_domain::message::Message>, AppError>;
    async fn mark_message_published(&self, message_id: MessageId) -> Result<(), AppError>;
    /// The current state of one message, as dispatched to realtime clients.
    async fn message_by_id(
        &self,
        message_id: MessageId,
    ) -> Result<Option<rustchat_domain::message::Message>, AppError>;
}

/// Tells the other server instances about a realtime event. Events are hints:
/// clients recover anything missed from durable history, so implementations
/// never block the caller and may drop events under load.
pub trait EventBus: Send + Sync {
    fn publish(&self, topic: &str, payload: serde_json::Value);
}

/// Used when a single instance runs: there is nobody else to tell.
pub struct NoEventBus;

impl EventBus for NoEventBus {
    fn publish(&self, _topic: &str, _payload: serde_json::Value) {}
}

/// Which users have a live WebSocket on any instance. The store decides the
/// transitions: `connect` is true only when the user was not already online
/// elsewhere, and `disconnect` only when no instance still has them.
#[async_trait]
pub trait PresenceStore: Send + Sync {
    async fn connect(&self, user: UserId) -> Result<bool, AppError>;
    async fn disconnect(&self, user: UserId) -> Result<bool, AppError>;
    async fn online(&self, users: &[UserId])
    -> Result<std::collections::HashSet<UserId>, AppError>;
    /// Keep this instance's connections alive; call periodically.
    async fn refresh(&self) -> Result<(), AppError>;
}

#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn insert(&self, session: NewSession) -> Result<(), AppError>;
    /// Find a session by token digest that is neither revoked nor expired at `now`.
    async fn find_active(
        &self,
        digest: &TokenDigest,
        now: OffsetDateTime,
    ) -> Result<Option<ActiveSession>, AppError>;
    async fn list_active(
        &self,
        user_id: UserId,
        now: OffsetDateTime,
    ) -> Result<Vec<Session>, AppError>;
    /// Record activity for the session.
    async fn touch(&self, session_id: SessionId, now: OffsetDateTime) -> Result<(), AppError>;
    /// Replace the session token, keeping `old_digest` valid until `grace_until`.
    /// Succeeds only while `old_digest` is still the current token, so concurrent
    /// rotations of one session cannot both win. Returns whether it rotated.
    async fn rotate_token(
        &self,
        session_id: SessionId,
        old_digest: &TokenDigest,
        new_digest: &TokenDigest,
        now: OffsetDateTime,
        grace_until: OffsetDateTime,
    ) -> Result<bool, AppError>;
    /// Revoke a session only if it belongs to `user_id`. Returns whether a
    /// session was revoked.
    async fn revoke(
        &self,
        user_id: UserId,
        session_id: SessionId,
        now: OffsetDateTime,
    ) -> Result<bool, AppError>;
    /// Replace the user's password hash and revoke every other live session
    /// atomically, retaining the session used to authorize the change.
    async fn update_password_and_revoke_other_sessions(
        &self,
        user_id: UserId,
        current_session_id: SessionId,
        password_hash: String,
        now: OffsetDateTime,
    ) -> Result<(), AppError>;
}

#[async_trait]
pub trait PasswordHasher: Send + Sync {
    async fn hash(&self, password: PlainPassword) -> Result<String, AppError>;
    async fn verify(&self, password: PlainPassword, hash: String) -> Result<bool, AppError>;
    /// Spend the same effort as `verify` against a fixed dummy hash, so that
    /// login timing does not reveal whether an account exists.
    async fn verify_dummy(&self, password: PlainPassword) -> Result<(), AppError>;
}

pub trait TokenService: Send + Sync {
    fn generate(&self) -> NewToken;
    /// A human-typable single-use recovery code (about 80 bits), for example
    /// `k3m9-x2pq-7vhd-4tnw`.
    fn recovery_code(&self) -> String;
    fn digest(&self, raw: &str) -> TokenDigest;
}

#[async_trait]
pub trait EmailDelivery: Send + Sync {
    async fn send(
        &self,
        recipient: &EmailAddress,
        subject: &str,
        text: &str,
    ) -> Result<(), AppError>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> OffsetDateTime;
}

/// At most `max` events per `window`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitRule {
    pub max: u32,
    pub window: time::Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitDecision {
    Allowed,
    Limited { retry_after: time::Duration },
}

/// Counts events per key within a window. Implementations must keep their
/// memory bounded regardless of how many distinct keys are presented.
///
/// `check` does not consume budget; callers `record` events explicitly so a
/// policy can decide which outcomes count (every attempt, or failures only).
#[async_trait]
pub trait RateLimiter: Send + Sync {
    async fn check(
        &self,
        key: &str,
        rule: RateLimitRule,
        now: OffsetDateTime,
    ) -> Result<RateLimitDecision, AppError>;
    async fn record(
        &self,
        key: &str,
        rule: RateLimitRule,
        now: OffsetDateTime,
    ) -> Result<(), AppError>;
    async fn reset(&self, key: &str) -> Result<(), AppError>;
}
