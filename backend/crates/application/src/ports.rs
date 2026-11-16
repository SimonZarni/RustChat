//! Ports: capabilities the application needs from the outside world.
//! Infrastructure implements them; the application never names a concrete
//! database, hashing library or clock.

use async_trait::async_trait;
use rustchat_domain::{
    community::{Category, Channel, Community, Role},
    conversation::{Conversation, ConversationPage},
    identity::{
        CategoryId, ChannelId, CommunityId, ConversationId, EmailAddress, MessageId, PlainPassword,
        ProfilePrivacy, RoleId, Session, SessionId, User, UserId, UserProfile, Username,
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
    async fn set_user_suspended(
        &self,
        actor: UserId,
        target: UserId,
        suspended: bool,
        now: OffsetDateTime,
    ) -> Result<AdminUser, AppError>;
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
pub trait CommunityRepository: Send + Sync {
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
    async fn pending_messages(
        &self,
        limit: usize,
    ) -> Result<Vec<rustchat_domain::message::Message>, AppError>;
    async fn mark_message_published(&self, message_id: MessageId) -> Result<(), AppError>;
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
