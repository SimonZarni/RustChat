//! PostgreSQL implementations of the repository ports.
//!
//! All statements are parameterized; no SQL is built from user input.

use std::{collections::HashSet, time::Duration};

use async_trait::async_trait;
use rustchat_application::{
    error::AppError,
    ports::{
        ActiveSession, AdminUser, AuditEvent, AuditPage, AuditUser, CommunityRepository,
        ConversationRepository, CreateUserOutcome, EmailStatus, EncryptedSecret, MessageRepository,
        MfaRepository, NewSession, NewUser, SessionRepository, TokenDigest, TotpRecord,
        UserCredentials, UserRepository, VerifyEmailOutcome,
    },
};
use rustchat_domain::{
    community::{Category, Channel, Community, Role},
    conversation::{Conversation, ConversationKind, ConversationName, ConversationPage},
    identity::{
        CategoryId, ChannelId, CommunityId, ConversationId, MessageId, ProfilePrivacy,
        ProfileVisibility, RoleId, Session, SessionId, User, UserId, UserProfile, Username,
    },
    message::{
        DeliveryCursor, Message, MessageContent, MessagePage, MessageReaction, ReactionEmoji,
        ReactionMutation, ReadCursor,
    },
};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, postgres::PgPoolOptions, postgres::PgRow};
use time::OffsetDateTime;
use uuid::Uuid;

/// Open a bounded connection pool.
pub async fn connect(database_url: &str, max_connections: u32) -> Result<PgPool, AppError> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
        .map_err(db_error)
}

/// Check that the pool can acquire a connection and execute a lightweight query.
pub async fn ready(pool: &PgPool) -> bool {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
        .is_ok()
}

/// Apply all pending migrations.
pub async fn migrate(pool: &PgPool) -> Result<(), AppError> {
    crate::MIGRATOR.run(pool).await.map_err(AppError::internal)
}

fn db_error(error: sqlx::Error) -> AppError {
    AppError::internal(error)
}

fn username_from(row: &PgRow, column: &str) -> Result<Username, AppError> {
    let raw: String = row.try_get(column).map_err(db_error)?;
    Username::parse(&raw).map_err(AppError::internal)
}

fn user_from(row: &PgRow, id: &str, username: &str, created_at: &str) -> Result<User, AppError> {
    Ok(User {
        id: UserId::from_uuid(row.try_get::<Uuid, _>(id).map_err(db_error)?),
        username: username_from(row, username)?,
        created_at: row
            .try_get::<OffsetDateTime, _>(created_at)
            .map_err(db_error)?,
    })
}

fn admin_user_from(row: &PgRow) -> Result<AdminUser, AppError> {
    Ok(AdminUser {
        user: user_from(row, "id", "username", "created_at")?,
        is_instance_admin: false,
        deactivated_at: row.try_get("deactivated_at").map_err(db_error)?,
        suspended_at: row.try_get("suspended_at").map_err(db_error)?,
        deleted_at: row.try_get("deleted_at").map_err(db_error)?,
    })
}

fn session_from(row: &PgRow) -> Result<Session, AppError> {
    Ok(Session {
        id: SessionId::from_uuid(row.try_get::<Uuid, _>("id").map_err(db_error)?),
        user_id: UserId::from_uuid(row.try_get::<Uuid, _>("user_id").map_err(db_error)?),
        created_at: row
            .try_get::<OffsetDateTime, _>("created_at")
            .map_err(db_error)?,
        expires_at: row
            .try_get::<OffsetDateTime, _>("expires_at")
            .map_err(db_error)?,
    })
}

/// TOTP secrets, recovery codes and login challenges for multi-factor authentication.
#[derive(Clone)]
pub struct PgMfaRepository {
    pool: PgPool,
}

impl PgMfaRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MfaRepository for PgMfaRepository {
    async fn totp(&self, user_id: UserId) -> Result<Option<TotpRecord>, AppError> {
        let row = sqlx::query(
            "SELECT secret_ciphertext, secret_nonce, confirmed_at, last_used_step \
             FROM mfa_totp WHERE user_id = $1",
        )
        .bind(user_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        row.map(|row| {
            let nonce: Vec<u8> = row.try_get("secret_nonce").map_err(db_error)?;
            let confirmed_at: Option<OffsetDateTime> =
                row.try_get("confirmed_at").map_err(db_error)?;
            Ok(TotpRecord {
                secret: EncryptedSecret {
                    ciphertext: row.try_get("secret_ciphertext").map_err(db_error)?,
                    nonce: nonce.try_into().map_err(|_| {
                        AppError::internal("stored TOTP nonce has the wrong length")
                    })?,
                },
                confirmed: confirmed_at.is_some(),
                last_used_step: row.try_get("last_used_step").map_err(db_error)?,
            })
        })
        .transpose()
    }

    async fn begin_totp(
        &self,
        user_id: UserId,
        secret: &EncryptedSecret,
        now: OffsetDateTime,
    ) -> Result<bool, AppError> {
        let result = sqlx::query(
            "INSERT INTO mfa_totp (user_id, secret_ciphertext, secret_nonce, created_at) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (user_id) DO UPDATE \
                SET secret_ciphertext = EXCLUDED.secret_ciphertext, \
                    secret_nonce = EXCLUDED.secret_nonce, \
                    created_at = EXCLUDED.created_at \
                WHERE mfa_totp.confirmed_at IS NULL",
        )
        .bind(user_id.as_uuid())
        .bind(&secret.ciphertext)
        .bind(secret.nonce.as_slice())
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn confirm_totp(
        &self,
        user_id: UserId,
        step: i64,
        recovery_digests: &[TokenDigest],
        now: OffsetDateTime,
    ) -> Result<bool, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let confirmed = sqlx::query(
            "UPDATE mfa_totp SET confirmed_at = $3, last_used_step = $2 \
             WHERE user_id = $1 AND confirmed_at IS NULL",
        )
        .bind(user_id.as_uuid())
        .bind(step)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        if confirmed.rows_affected() == 0 {
            return Ok(false);
        }
        replace_recovery_codes_in(&mut tx, user_id, recovery_digests, now).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn use_totp_step(&self, user_id: UserId, step: i64) -> Result<bool, AppError> {
        // One atomic compare-and-set: of two concurrent requests presenting the same
        // code, only one moves the step forward.
        let result = sqlx::query(
            "UPDATE mfa_totp SET last_used_step = $2 \
             WHERE user_id = $1 AND confirmed_at IS NOT NULL \
               AND (last_used_step IS NULL OR last_used_step < $2)",
        )
        .bind(user_id.as_uuid())
        .bind(step)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn use_recovery_code(
        &self,
        user_id: UserId,
        digest: &TokenDigest,
        now: OffsetDateTime,
    ) -> Result<bool, AppError> {
        let result = sqlx::query(
            "UPDATE mfa_recovery_codes SET used_at = $3 \
             WHERE user_id = $1 AND code_digest = $2 AND used_at IS NULL",
        )
        .bind(user_id.as_uuid())
        .bind(digest.0.as_slice())
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn recovery_codes_remaining(&self, user_id: UserId) -> Result<u32, AppError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM mfa_recovery_codes WHERE user_id = $1 AND used_at IS NULL",
        )
        .bind(user_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    async fn replace_recovery_codes(
        &self,
        user_id: UserId,
        digests: &[TokenDigest],
        now: OffsetDateTime,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        replace_recovery_codes_in(&mut tx, user_id, digests, now).await?;
        tx.commit().await.map_err(db_error)
    }

    async fn disable_totp(&self, user_id: UserId) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        for statement in [
            "DELETE FROM mfa_challenges WHERE user_id = $1",
            "DELETE FROM mfa_recovery_codes WHERE user_id = $1",
            "DELETE FROM mfa_totp WHERE user_id = $1",
        ] {
            sqlx::query(statement)
                .bind(user_id.as_uuid())
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
        }
        tx.commit().await.map_err(db_error)
    }

    async fn create_challenge(
        &self,
        user_id: UserId,
        digest: TokenDigest,
        now: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        // Expired challenges, and all but the newest few of this user's, are discarded.
        sqlx::query("DELETE FROM mfa_challenges WHERE expires_at <= $1")
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query(
            "DELETE FROM mfa_challenges WHERE user_id = $1 AND id NOT IN \
             (SELECT id FROM mfa_challenges WHERE user_id = $1 ORDER BY created_at DESC, id DESC LIMIT 4)",
        )
        .bind(user_id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        sqlx::query(
            "INSERT INTO mfa_challenges (id, user_id, token_digest, created_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(user_id.as_uuid())
        .bind(digest.0.as_slice())
        .bind(now)
        .bind(expires_at)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)
    }

    async fn attempt_challenge(
        &self,
        digest: &TokenDigest,
        now: OffsetDateTime,
        max_attempts: i32,
    ) -> Result<Option<User>, AppError> {
        let row = sqlx::query(
            "UPDATE mfa_challenges c SET attempts = c.attempts + 1 \
             FROM users u \
             WHERE c.token_digest = $1 AND c.expires_at > $2 AND c.attempts < $3 \
               AND u.id = c.user_id \
               AND u.deleted_at IS NULL AND u.deactivated_at IS NULL AND u.suspended_at IS NULL \
             RETURNING u.id, u.username, u.created_at",
        )
        .bind(digest.0.as_slice())
        .bind(now)
        .bind(max_attempts)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        row.map(|row| user_from(&row, "id", "username", "created_at"))
            .transpose()
    }

    async fn delete_challenge(&self, digest: &TokenDigest) -> Result<(), AppError> {
        sqlx::query("DELETE FROM mfa_challenges WHERE token_digest = $1")
            .bind(digest.0.as_slice())
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        Ok(())
    }
}

async fn replace_recovery_codes_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: UserId,
    digests: &[TokenDigest],
    now: OffsetDateTime,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM mfa_recovery_codes WHERE user_id = $1")
        .bind(user_id.as_uuid())
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    for digest in digests {
        sqlx::query(
            "INSERT INTO mfa_recovery_codes (id, user_id, code_digest, created_at) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(Uuid::now_v7())
        .bind(user_id.as_uuid())
        .bind(digest.0.as_slice())
        .bind(now)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?;
    }
    Ok(())
}

/// Builds a page from rows fetched with one extra row to detect a following page.
fn audit_page(rows: &[PgRow], limit: usize) -> Result<AuditPage, AppError> {
    let mut events = Vec::with_capacity(rows.len().min(limit));
    for row in rows.iter().take(limit) {
        let named = |id_column: &str, name_column: &str| -> Result<Option<AuditUser>, AppError> {
            let id: Option<Uuid> = row.try_get(id_column).map_err(db_error)?;
            let name: Option<String> = row.try_get(name_column).map_err(db_error)?;
            Ok(id.zip(name).map(|(id, username)| AuditUser {
                id: UserId::from_uuid(id),
                username,
            }))
        };
        events.push(AuditEvent {
            id: rustchat_domain::identity::AuditEventId::from_uuid(
                row.try_get("id").map_err(db_error)?,
            ),
            event_type: row.try_get("event_type").map_err(db_error)?,
            actor: named("actor_user_id", "actor_username")?,
            target: named("target_user_id", "target_username")?,
            details: row.try_get("details").map_err(db_error)?,
            created_at: row.try_get("created_at").map_err(db_error)?,
        });
    }
    let next_cursor = (rows.len() > limit)
        .then(|| events.last().map(|event| event.id))
        .flatten();
    Ok(AuditPage {
        events,
        next_cursor,
    })
}

#[derive(Clone)]
pub struct PgUserRepository {
    pool: PgPool,
}

impl PgUserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for PgUserRepository {
    async fn create(&self, user: NewUser) -> Result<CreateUserOutcome, AppError> {
        // The unique constraint on `username` is the source of truth; a
        // concurrent duplicate registration loses here rather than in a
        // check-then-insert race.
        let result = sqlx::query(
            "WITH inserted_user AS (\
                 INSERT INTO users (id, username, email, password_hash, created_at) \
                 VALUES ($1, $2, $3, $4, $5) \
                 ON CONFLICT DO NOTHING \
                 RETURNING id, username\
             ) \
             INSERT INTO user_profiles (user_id, display_name) \
             SELECT id, username FROM inserted_user",
        )
        .bind(user.id.as_uuid())
        .bind(user.username.as_str())
        .bind(user.email.as_ref().map(|email| email.as_str()))
        .bind(&user.password_hash)
        .bind(user.created_at)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        if result.rows_affected() == 1 {
            return Ok(CreateUserOutcome::Created);
        }
        let username_taken: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE username = $1)")
                .bind(user.username.as_str())
                .fetch_one(&self.pool)
                .await
                .map_err(db_error)?;
        Ok(if username_taken {
            CreateUserOutcome::UsernameTaken
        } else {
            CreateUserOutcome::EmailTaken
        })
    }

    async fn find_credentials(
        &self,
        username: &Username,
    ) -> Result<Option<UserCredentials>, AppError> {
        let row = sqlx::query(
            "SELECT id, username, password_hash, created_at FROM users WHERE username = $1 AND deleted_at IS NULL AND suspended_at IS NULL",
        )
        .bind(username.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;

        row.map(|row| {
            Ok(UserCredentials {
                user: user_from(&row, "id", "username", "created_at")?,
                password_hash: row.try_get("password_hash").map_err(db_error)?,
            })
        })
        .transpose()
    }

    async fn find_discoverable(
        &self,
        caller: UserId,
        username: &Username,
    ) -> Result<Option<User>, AppError> {
        let row = sqlx::query("SELECT u.id, u.username, u.created_at FROM users u JOIN user_profiles p ON p.user_id = u.id WHERE u.deleted_at IS NULL AND u.deactivated_at IS NULL AND u.suspended_at IS NULL AND u.username = $2 AND (u.id = $1 OR p.discoverable OR EXISTS (SELECT 1 FROM conversation_members mine JOIN conversation_members theirs ON theirs.conversation_id = mine.conversation_id WHERE mine.user_id = $1 AND theirs.user_id = u.id))")
            .bind(caller.as_uuid()).bind(username.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.map(|row| user_from(&row, "id", "username", "created_at"))
            .transpose()
    }

    async fn find_profile_for_caller(
        &self,
        caller: UserId,
        username: &Username,
    ) -> Result<Option<UserProfile>, AppError> {
        let row = sqlx::query("SELECT u.id, u.username, p.display_name, p.bio, p.updated_at FROM users u JOIN user_profiles p ON p.user_id = u.id WHERE u.deleted_at IS NULL AND u.deactivated_at IS NULL AND u.suspended_at IS NULL AND u.username = $2 AND (u.id = $1 OR (p.discoverable AND p.profile_visibility = 'everyone') OR (p.profile_visibility = 'conversation_members' AND EXISTS (SELECT 1 FROM conversation_members mine JOIN conversation_members theirs ON theirs.conversation_id = mine.conversation_id WHERE mine.user_id = $1 AND theirs.user_id = u.id)))")
            .bind(caller.as_uuid()).bind(username.as_str()).fetch_optional(&self.pool).await.map_err(db_error)?;
        row.map(|row| {
            Ok(UserProfile {
                user_id: UserId::from_uuid(row.try_get("id").map_err(db_error)?),
                username: username_from(&row, "username")?,
                display_name: row.try_get("display_name").map_err(db_error)?,
                bio: row.try_get("bio").map_err(db_error)?,
                updated_at: row.try_get("updated_at").map_err(db_error)?,
            })
        })
        .transpose()
    }

    async fn get_privacy(&self, user_id: UserId) -> Result<ProfilePrivacy, AppError> {
        let row = sqlx::query(
            "SELECT discoverable, profile_visibility, presence_visible FROM user_profiles WHERE user_id = $1",
        )
        .bind(user_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?
        .ok_or(AppError::NotFound)?;
        let visibility: String = row.try_get("profile_visibility").map_err(db_error)?;
        Ok(ProfilePrivacy {
            discoverable: row.try_get("discoverable").map_err(db_error)?,
            visibility: match visibility.as_str() {
                "everyone" => ProfileVisibility::Everyone,
                "conversation_members" => ProfileVisibility::ConversationMembers,
                _ => return Err(AppError::internal("invalid stored profile visibility")),
            },
            presence_visible: row.try_get("presence_visible").map_err(db_error)?,
        })
    }

    async fn update_privacy(
        &self,
        user_id: UserId,
        privacy: ProfilePrivacy,
    ) -> Result<ProfilePrivacy, AppError> {
        let visibility = match privacy.visibility {
            ProfileVisibility::Everyone => "everyone",
            ProfileVisibility::ConversationMembers => "conversation_members",
        };
        let result = sqlx::query("UPDATE user_profiles SET discoverable = $2, profile_visibility = $3, presence_visible = $4 WHERE user_id = $1")
            .bind(user_id.as_uuid()).bind(privacy.discoverable).bind(visibility).bind(privacy.presence_visible).execute(&self.pool).await.map_err(db_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        Ok(privacy)
    }

    async fn update_username(
        &self,
        user_id: UserId,
        username: &Username,
    ) -> Result<User, AppError> {
        let row = sqlx::query(
            "UPDATE users SET username = $2 WHERE id = $1 RETURNING id, username, created_at",
        )
        .bind(user_id.as_uuid())
        .bind(username.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| match &error {
            sqlx::Error::Database(db) if db.constraint() == Some("users_username_key") => {
                AppError::UsernameTaken
            }
            _ => db_error(error),
        })?
        .ok_or(AppError::NotFound)?;
        user_from(&row, "id", "username", "created_at")
    }

    async fn get_profile(&self, user_id: UserId) -> Result<UserProfile, AppError> {
        let row = sqlx::query("SELECT u.id, u.username, p.display_name, p.bio, p.updated_at FROM users u JOIN user_profiles p ON p.user_id = u.id WHERE u.id = $1 AND u.deleted_at IS NULL")
            .bind(user_id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?
            .ok_or(AppError::NotFound)?;
        Ok(UserProfile {
            user_id,
            username: username_from(&row, "username")?,
            display_name: row.try_get("display_name").map_err(db_error)?,
            bio: row.try_get("bio").map_err(db_error)?,
            updated_at: row.try_get("updated_at").map_err(db_error)?,
        })
    }

    async fn update_profile(
        &self,
        user_id: UserId,
        display_name: &str,
        bio: &str,
        now: OffsetDateTime,
    ) -> Result<UserProfile, AppError> {
        let result = sqlx::query("UPDATE user_profiles SET display_name = $2, bio = $3, updated_at = $4 WHERE user_id = $1")
            .bind(user_id.as_uuid())
            .bind(display_name)
            .bind(bio)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        self.get_profile(user_id).await
    }

    async fn deactivate_account(
        &self,
        user_id: UserId,
        now: OffsetDateTime,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let owner: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM communities WHERE owner_id = $1)")
                .bind(user_id.as_uuid())
                .fetch_one(&mut *tx)
                .await
                .map_err(db_error)?;
        if owner {
            return Err(AppError::AccountOwnsCommunities);
        }
        let changed = sqlx::query(
            "UPDATE users SET deactivated_at = $2 WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(user_id.as_uuid())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        if changed.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        sqlx::query(
            "UPDATE sessions SET revoked_at = $2 WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user_id.as_uuid())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)
    }

    async fn delete_account(
        &self,
        user_id: UserId,
        tombstone_username: &Username,
        unusable_password_hash: &str,
        now: OffsetDateTime,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let owner: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM communities WHERE owner_id = $1)")
                .bind(user_id.as_uuid())
                .fetch_one(&mut *tx)
                .await
                .map_err(db_error)?;
        if owner {
            return Err(AppError::AccountOwnsCommunities);
        }
        let changed = sqlx::query("UPDATE users SET username = $2, email = NULL, email_verified_at = NULL, password_hash = $3, deactivated_at = $4, deleted_at = $4 WHERE id = $1 AND deleted_at IS NULL")
            .bind(user_id.as_uuid()).bind(tombstone_username.as_str()).bind(unusable_password_hash).bind(now)
            .execute(&mut *tx).await.map_err(db_error)?;
        if changed.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        sqlx::query("UPDATE user_profiles SET display_name = 'Deleted account', bio = '', discoverable = false, profile_visibility = 'conversation_members', updated_at = $2 WHERE user_id = $1")
            .bind(user_id.as_uuid()).bind(now).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query(
            "UPDATE sessions SET revoked_at = $2 WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user_id.as_uuid())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        sqlx::query("DELETE FROM conversation_members WHERE user_id = $1")
            .bind(user_id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query("DELETE FROM community_members WHERE user_id = $1")
            .bind(user_id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query("DELETE FROM account_tokens WHERE user_id = $1")
            .bind(user_id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)
    }

    async fn issue_password_reset_token(
        &self,
        email: &rustchat_domain::identity::EmailAddress,
        digest: TokenDigest,
        now: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> Result<bool, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let user_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM users WHERE email = $1 AND email_verified_at IS NOT NULL AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(email.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
        let Some(user_id) = user_id else {
            tx.commit().await.map_err(db_error)?;
            return Ok(false);
        };
        sqlx::query(
            "UPDATE account_tokens SET used_at = $2 WHERE user_id = $1 AND purpose = 'password_reset' AND used_at IS NULL",
        )
        .bind(user_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        sqlx::query("INSERT INTO account_tokens (id, user_id, token_digest, created_at, expires_at) VALUES ($1, $2, $3, $4, $5)")
            .bind(Uuid::now_v7())
            .bind(user_id)
            .bind(digest.0.to_vec())
            .bind(now)
            .bind(expires_at)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn reset_password_with_token(
        &self,
        digest: TokenDigest,
        password_hash: String,
        now: OffsetDateTime,
    ) -> Result<bool, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let row = sqlx::query("UPDATE account_tokens SET used_at = $2 WHERE token_digest = $1 AND purpose = 'password_reset' AND used_at IS NULL AND expires_at > $2 RETURNING user_id")
            .bind(digest.0.to_vec())
            .bind(now)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
        let Some(row) = row else {
            tx.commit().await.map_err(db_error)?;
            return Ok(false);
        };
        let user_id: Uuid = row.try_get("user_id").map_err(db_error)?;
        let user_changed = sqlx::query("UPDATE users SET password_hash = $2, deactivated_at = NULL WHERE id = $1 AND deleted_at IS NULL")
            .bind(user_id)
            .bind(password_hash)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        if user_changed.rows_affected() == 0 {
            tx.rollback().await.map_err(db_error)?;
            return Ok(false);
        }
        sqlx::query(
            "UPDATE sessions SET revoked_at = $2 WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        sqlx::query(
            "UPDATE account_tokens SET used_at = $2 WHERE user_id = $1 AND purpose = 'password_reset' AND used_at IS NULL",
        )
        .bind(user_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn email_status(&self, user_id: UserId) -> Result<EmailStatus, AppError> {
        let row = sqlx::query(
            "SELECT email, email_verified_at FROM users WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(user_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?
        .ok_or(AppError::NotFound)?;
        let address: Option<String> = row.try_get("email").map_err(db_error)?;
        let verified_at: Option<OffsetDateTime> =
            row.try_get("email_verified_at").map_err(db_error)?;
        let address = address
            .map(|value| {
                rustchat_domain::identity::EmailAddress::parse(&value)
                    .map_err(|_| AppError::internal("stored email address is invalid"))
            })
            .transpose()?;
        Ok(EmailStatus {
            address,
            verified: verified_at.is_some(),
        })
    }

    async fn set_email(
        &self,
        user_id: UserId,
        email: Option<&rustchat_domain::identity::EmailAddress>,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let changed = sqlx::query(
            "UPDATE users SET email = $2, email_verified_at = NULL WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(user_id.as_uuid())
        .bind(email.map(|email| email.as_str()))
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        if changed.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        // Links sent for the previous address must not work for the new one.
        sqlx::query("DELETE FROM account_tokens WHERE user_id = $1 AND used_at IS NULL")
            .bind(user_id.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)
    }

    async fn issue_email_verification_token(
        &self,
        user_id: UserId,
        email: &rustchat_domain::identity::EmailAddress,
        digest: TokenDigest,
        now: OffsetDateTime,
        expires_at: OffsetDateTime,
    ) -> Result<bool, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let current: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM users WHERE id = $1 AND email = $2 AND email_verified_at IS NULL AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(user_id.as_uuid())
        .bind(email.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
        if current.is_none() {
            tx.commit().await.map_err(db_error)?;
            return Ok(false);
        }
        sqlx::query(
            "DELETE FROM account_tokens WHERE user_id = $1 AND purpose = 'email_verification' AND used_at IS NULL",
        )
        .bind(user_id.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        sqlx::query(
            "INSERT INTO account_tokens (id, user_id, token_digest, created_at, expires_at, purpose, email) \
             VALUES ($1, $2, $3, $4, $5, 'email_verification', $6)",
        )
        .bind(Uuid::now_v7())
        .bind(user_id.as_uuid())
        .bind(digest.0.to_vec())
        .bind(now)
        .bind(expires_at)
        .bind(email.as_str())
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(true)
    }

    async fn verify_email_with_token(
        &self,
        digest: TokenDigest,
        now: OffsetDateTime,
    ) -> Result<VerifyEmailOutcome, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let token = sqlx::query(
            "UPDATE account_tokens SET used_at = $2 \
             WHERE token_digest = $1 AND purpose = 'email_verification' AND used_at IS NULL AND expires_at > $2 \
             RETURNING user_id, email",
        )
        .bind(digest.0.to_vec())
        .bind(now)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
        let Some(token) = token else {
            tx.commit().await.map_err(db_error)?;
            return Ok(VerifyEmailOutcome::Invalid);
        };
        let user_id: Uuid = token.try_get("user_id").map_err(db_error)?;
        let email: String = token.try_get("email").map_err(db_error)?;
        let updated = sqlx::query(
            "UPDATE users SET email_verified_at = $3 \
             WHERE id = $1 AND email = $2 AND email_verified_at IS NULL AND deleted_at IS NULL",
        )
        .bind(user_id)
        .bind(&email)
        .bind(now)
        .execute(&mut *tx)
        .await;
        match updated {
            Ok(result) if result.rows_affected() == 1 => {
                tx.commit().await.map_err(db_error)?;
                Ok(VerifyEmailOutcome::Verified)
            }
            // The account no longer holds this unverified address.
            Ok(_) => {
                tx.commit().await.map_err(db_error)?;
                Ok(VerifyEmailOutcome::Invalid)
            }
            // Another account verified the address first; the token stays unused.
            Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
                tx.rollback().await.map_err(db_error)?;
                Ok(VerifyEmailOutcome::AddressInUse)
            }
            Err(error) => Err(db_error(error)),
        }
    }

    async fn list_admin_audit(
        &self,
        before: Option<rustchat_domain::identity::AuditEventId>,
        limit: usize,
    ) -> Result<AuditPage, AppError> {
        let rows = sqlx::query(
            "SELECT a.id, a.event_type, a.created_at, a.details, \
                    a.actor_user_id, actor.username AS actor_username, \
                    a.target_user_id, target.username AS target_username \
             FROM instance_admin_audit_log a \
             LEFT JOIN users actor ON actor.id = a.actor_user_id \
             LEFT JOIN users target ON target.id = a.target_user_id \
             WHERE $1::uuid IS NULL \
                OR (a.created_at, a.id) < (SELECT created_at, id FROM instance_admin_audit_log WHERE id = $1) \
             ORDER BY a.created_at DESC, a.id DESC \
             LIMIT $2",
        )
        .bind(before.map(|id| id.as_uuid()))
        .bind(i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        audit_page(&rows, limit)
    }

    async fn list_admin_users(&self, limit: i64) -> Result<Vec<AdminUser>, AppError> {
        let rows = sqlx::query(
            "SELECT id, username, created_at, deactivated_at, suspended_at, deleted_at \
             FROM users ORDER BY created_at DESC, id DESC LIMIT $1",
        )
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        rows.iter().map(admin_user_from).collect()
    }

    async fn set_user_suspended(
        &self,
        actor: UserId,
        target: UserId,
        suspended: bool,
        now: OffsetDateTime,
    ) -> Result<AdminUser, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let row = sqlx::query(
            "SELECT id, username, created_at, deactivated_at, suspended_at, deleted_at \
             FROM users WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
        )
        .bind(target.as_uuid())
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?
        .ok_or(AppError::NotFound)?;
        let current: Option<OffsetDateTime> = row.try_get("suspended_at").map_err(db_error)?;
        if suspended != current.is_some() {
            sqlx::query("UPDATE users SET suspended_at = $2 WHERE id = $1")
                .bind(target.as_uuid())
                .bind(suspended.then_some(now))
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
            if suspended {
                sqlx::query("DELETE FROM sessions WHERE user_id = $1")
                    .bind(target.as_uuid())
                    .execute(&mut *tx)
                    .await
                    .map_err(db_error)?;
            }
            sqlx::query(
                "INSERT INTO instance_admin_audit_log (id, actor_user_id, target_user_id, event_type, created_at, details) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(Uuid::now_v7())
            .bind(actor.as_uuid())
            .bind(target.as_uuid())
            .bind(if suspended { "user.suspended" } else { "user.unsuspended" })
            .bind(now)
            .bind(serde_json::json!({"suspended": suspended}))
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        }
        let updated = sqlx::query(
            "SELECT id, username, created_at, deactivated_at, suspended_at, deleted_at \
             FROM users WHERE id = $1",
        )
        .bind(target.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        admin_user_from(&updated)
    }
}

#[derive(Clone)]
pub struct PgSessionRepository {
    pool: PgPool,
}

impl PgSessionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SessionRepository for PgSessionRepository {
    async fn insert(&self, new: NewSession) -> Result<(), AppError> {
        let result = sqlx::query(
            "WITH active_user AS (UPDATE users SET deactivated_at = NULL WHERE id = $2 AND deleted_at IS NULL AND suspended_at IS NULL RETURNING id) \
             INSERT INTO sessions (id, user_id, token_digest, created_at, expires_at, last_seen_at, token_rotated_at) \
             SELECT $1, id, $3, $4, $5, $4, $4 FROM active_user",
        )
        .bind(new.session.id.as_uuid())
        .bind(new.session.user_id.as_uuid())
        .bind(new.token_digest.0.as_slice())
        .bind(new.session.created_at)
        .bind(new.session.expires_at)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::Unauthenticated);
        }
        Ok(())
    }

    async fn find_active(
        &self,
        digest: &TokenDigest,
        now: OffsetDateTime,
    ) -> Result<Option<ActiveSession>, AppError> {
        // The previous token also matches while its rotation grace period lasts.
        let row = sqlx::query(
            "SELECT s.id, s.user_id, s.created_at, s.expires_at, s.last_seen_at, s.token_rotated_at, \
                    (s.token_digest <> $1) AS via_previous_token, \
                    u.username, u.created_at AS user_created_at \
             FROM sessions s JOIN users u ON u.id = s.user_id \
             WHERE (s.token_digest = $1 \
                    OR (s.previous_token_digest = $1 AND s.previous_token_valid_until > $2)) \
               AND s.revoked_at IS NULL AND s.expires_at > $2 \
               AND u.deleted_at IS NULL AND u.deactivated_at IS NULL AND u.suspended_at IS NULL",
        )
        .bind(digest.0.as_slice())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;

        row.map(|row| {
            let session = session_from(&row)?;
            let user = User {
                id: session.user_id,
                username: username_from(&row, "username")?,
                created_at: row
                    .try_get::<OffsetDateTime, _>("user_created_at")
                    .map_err(db_error)?,
            };
            Ok(ActiveSession {
                session,
                user,
                last_seen_at: row
                    .try_get::<OffsetDateTime, _>("last_seen_at")
                    .map_err(db_error)?,
                token_rotated_at: row
                    .try_get::<OffsetDateTime, _>("token_rotated_at")
                    .map_err(db_error)?,
                via_previous_token: row
                    .try_get::<bool, _>("via_previous_token")
                    .map_err(db_error)?,
            })
        })
        .transpose()
    }

    async fn touch(&self, session_id: SessionId, now: OffsetDateTime) -> Result<(), AppError> {
        sqlx::query(
            "UPDATE sessions SET last_seen_at = $2 WHERE id = $1 AND revoked_at IS NULL AND last_seen_at < $2",
        )
        .bind(session_id.as_uuid())
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(())
    }

    async fn rotate_token(
        &self,
        session_id: SessionId,
        old_digest: &TokenDigest,
        new_digest: &TokenDigest,
        now: OffsetDateTime,
        grace_until: OffsetDateTime,
    ) -> Result<bool, AppError> {
        // The compare-and-set on the current digest lets exactly one of several
        // concurrent requests rotate the session.
        let result = sqlx::query(
            "UPDATE sessions SET token_digest = $3, previous_token_digest = $2, \
                    previous_token_valid_until = $5, token_rotated_at = $4, last_seen_at = $4 \
             WHERE id = $1 AND token_digest = $2 AND revoked_at IS NULL",
        )
        .bind(session_id.as_uuid())
        .bind(old_digest.0.as_slice())
        .bind(new_digest.0.as_slice())
        .bind(now)
        .bind(grace_until)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn list_active(
        &self,
        user_id: UserId,
        now: OffsetDateTime,
    ) -> Result<Vec<Session>, AppError> {
        let rows = sqlx::query(
            "SELECT id, user_id, created_at, expires_at FROM sessions \
             WHERE user_id = $1 AND revoked_at IS NULL AND expires_at > $2 \
             ORDER BY created_at DESC, id DESC \
             LIMIT 100",
        )
        .bind(user_id.as_uuid())
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        rows.iter().map(session_from).collect()
    }

    async fn revoke(
        &self,
        user_id: UserId,
        session_id: SessionId,
        now: OffsetDateTime,
    ) -> Result<bool, AppError> {
        // Scoped to the owner in the statement itself so a foreign session id
        // can never be revoked, regardless of caller checks.
        let result = sqlx::query(
            "UPDATE sessions SET revoked_at = $3 \
             WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
        )
        .bind(session_id.as_uuid())
        .bind(user_id.as_uuid())
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn update_password_and_revoke_other_sessions(
        &self,
        user_id: UserId,
        current_session_id: SessionId,
        password_hash: String,
        now: OffsetDateTime,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let updated = sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
            .bind(user_id.as_uuid())
            .bind(password_hash)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        if updated.rows_affected() != 1 {
            return Err(AppError::NotFound);
        }
        sqlx::query("UPDATE sessions SET revoked_at = $3 WHERE user_id = $1 AND id <> $2 AND revoked_at IS NULL")
            .bind(user_id.as_uuid())
            .bind(current_session_id.as_uuid())
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct PgConversationRepository {
    pool: PgPool,
}

impl PgConversationRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

async fn conversation_members(pool: &PgPool, id: Uuid) -> Result<Vec<UserId>, AppError> {
    let rows = sqlx::query(
        "SELECT user_id FROM conversation_members WHERE conversation_id = $1 ORDER BY user_id",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(db_error)?;
    rows.into_iter()
        .map(|row| {
            row.try_get::<Uuid, _>("user_id")
                .map(UserId::from_uuid)
                .map_err(db_error)
        })
        .collect()
}

/// The stored name, if any. A CHECK constraint keeps stored names valid, so one that
/// no longer parses is treated as unset instead of failing the whole read.
fn conversation_name(row: &PgRow) -> Result<Option<ConversationName>, AppError> {
    let stored: Option<String> = row.try_get("name").map_err(db_error)?;
    Ok(stored.and_then(|value| ConversationName::parse(&value).ok()))
}

async fn conversation_by_id(pool: &PgPool, id: Uuid) -> Result<Conversation, AppError> {
    let row = sqlx::query(
        "SELECT id, kind, name, created_by, created_at FROM conversations WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_error)?
    .ok_or(AppError::NotFound)?;
    let kind: String = row.try_get("kind").map_err(db_error)?;
    let conversation = Conversation {
        id: ConversationId::from_uuid(row.try_get("id").map_err(db_error)?),
        kind: if kind == "direct" {
            ConversationKind::Direct
        } else {
            ConversationKind::Group
        },
        name: conversation_name(&row)?,
        created_by: UserId::from_uuid(row.try_get("created_by").map_err(db_error)?),
        created_at: row.try_get("created_at").map_err(db_error)?,
        members: conversation_members(pool, id).await?,
    };
    Ok(conversation)
}

#[async_trait]
impl ConversationRepository for PgConversationRepository {
    async fn create_or_get_direct(
        &self,
        caller: UserId,
        target: UserId,
    ) -> Result<Conversation, AppError> {
        let a = caller.as_uuid();
        let b = target.as_uuid();
        let (low, high) = if a < b { (a, b) } else { (b, a) };
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        sqlx::query("INSERT INTO conversations (id, kind, created_by, created_at, direct_user_low, direct_user_high) VALUES ($1, 'direct', $2, $3, $4, $5) ON CONFLICT (direct_user_low, direct_user_high) DO NOTHING")
            .bind(Uuid::now_v7()).bind(a).bind(OffsetDateTime::now_utc()).bind(low).bind(high)
            .execute(&mut *tx).await.map_err(db_error)?;
        let id: Uuid = sqlx::query_scalar(
            "SELECT id FROM conversations WHERE direct_user_low = $1 AND direct_user_high = $2",
        )
        .bind(low)
        .bind(high)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        let raw_now = OffsetDateTime::now_utc();
        let now = raw_now
            .replace_nanosecond(raw_now.nanosecond() / 1000 * 1000)
            .unwrap_or(raw_now);
        sqlx::query("INSERT INTO conversation_members (conversation_id, user_id, joined_at) VALUES ($1, $2, $3), ($1, $4, $3) ON CONFLICT DO NOTHING")
            .bind(id).bind(low).bind(now).bind(high).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        conversation_by_id(&self.pool, id).await
    }

    async fn create_group(
        &self,
        creator: UserId,
        name: Option<ConversationName>,
        members: Vec<UserId>,
        max_members: usize,
    ) -> Result<Conversation, AppError> {
        if members.len() < 2 || members.len() > max_members {
            return Err(AppError::ConversationMemberLimit);
        }
        let id = Uuid::now_v7();
        let now = OffsetDateTime::now_utc();
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        sqlx::query("INSERT INTO conversations (id, kind, name, created_by, created_at) VALUES ($1, 'group', $2, $3, $4)")
            .bind(id).bind(name.as_ref().map(ConversationName::as_str)).bind(creator.as_uuid()).bind(now).execute(&mut *tx).await.map_err(db_error)?;
        for member in members {
            sqlx::query("INSERT INTO conversation_members (conversation_id, user_id, joined_at) VALUES ($1, $2, $3)")
                .bind(id).bind(member.as_uuid()).bind(now).execute(&mut *tx).await.map_err(db_error)?;
        }
        tx.commit().await.map_err(db_error)?;
        conversation_by_id(&self.pool, id).await
    }

    async fn add_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
        member: UserId,
        max_members: usize,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let row = sqlx::query("SELECT kind FROM conversations WHERE id = $1 FOR UPDATE")
            .bind(conversation.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?
            .ok_or(AppError::NotFound)?;
        let kind: String = row.try_get("kind").map_err(db_error)?;
        let caller_is_member: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM conversation_members WHERE conversation_id = $1 AND user_id = $2)")
            .bind(conversation.as_uuid()).bind(caller.as_uuid()).fetch_one(&mut *tx).await.map_err(db_error)?;
        if !caller_is_member {
            return Err(AppError::NotFound);
        }
        if kind != "group" {
            return Err(AppError::InvalidConversationRequest);
        }
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM conversation_members WHERE conversation_id = $1",
        )
        .bind(conversation.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM conversation_members WHERE conversation_id = $1 AND user_id = $2)")
            .bind(conversation.as_uuid()).bind(member.as_uuid()).fetch_one(&mut *tx).await.map_err(db_error)?;
        if !exists && usize::try_from(count).unwrap_or(usize::MAX) >= max_members {
            return Err(AppError::ConversationMemberLimit);
        }
        sqlx::query("INSERT INTO conversation_members (conversation_id, user_id, joined_at) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
            .bind(conversation.as_uuid()).bind(member.as_uuid()).bind(OffsetDateTime::now_utc()).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    async fn remove_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
        member: UserId,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let row = sqlx::query("SELECT kind FROM conversations WHERE id = $1 FOR UPDATE")
            .bind(conversation.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?
            .ok_or(AppError::NotFound)?;
        let kind: String = row.try_get("kind").map_err(db_error)?;
        let caller_is_member: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM conversation_members WHERE conversation_id = $1 AND user_id = $2)")
            .bind(conversation.as_uuid()).bind(caller.as_uuid()).fetch_one(&mut *tx).await.map_err(db_error)?;
        if !caller_is_member {
            return Err(AppError::NotFound);
        }
        if kind != "group" {
            return Err(AppError::InvalidConversationRequest);
        }
        let result = sqlx::query(
            "DELETE FROM conversation_members WHERE conversation_id = $1 AND user_id = $2",
        )
        .bind(conversation.as_uuid())
        .bind(member.as_uuid())
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    async fn list_for_member(
        &self,
        caller: UserId,
        before: Option<ConversationId>,
        limit: usize,
    ) -> Result<ConversationPage, AppError> {
        let take = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
        let rows = sqlx::query("WITH page AS (SELECT c.id, c.kind, c.name, c.created_by, c.created_at FROM conversations c JOIN conversation_members cm ON cm.conversation_id = c.id AND cm.user_id = $1 WHERE NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = c.id) AND ($2::uuid IS NULL OR (c.created_at, c.id) < (SELECT created_at, id FROM conversations WHERE id = $2)) ORDER BY c.created_at DESC, c.id DESC LIMIT $3) SELECT p.id, p.kind, p.name, p.created_by, p.created_at, cm.user_id FROM page p JOIN conversation_members cm ON cm.conversation_id = p.id ORDER BY p.created_at DESC, p.id DESC, cm.user_id")
            .bind(caller.as_uuid()).bind(before.map(|id| id.as_uuid())).bind(take).fetch_all(&self.pool).await.map_err(db_error)?;
        let mut conversations: Vec<Conversation> = Vec::new();
        for row in rows {
            let id: Uuid = row.try_get("id").map_err(db_error)?;
            if conversations
                .last()
                .is_none_or(|conversation| conversation.id.as_uuid() != id)
            {
                let kind: String = row.try_get("kind").map_err(db_error)?;
                conversations.push(Conversation {
                    id: ConversationId::from_uuid(id),
                    kind: if kind == "direct" {
                        ConversationKind::Direct
                    } else {
                        ConversationKind::Group
                    },
                    name: conversation_name(&row)?,
                    created_by: UserId::from_uuid(row.try_get("created_by").map_err(db_error)?),
                    created_at: row.try_get("created_at").map_err(db_error)?,
                    members: Vec::new(),
                });
            }
            conversations
                .last_mut()
                .expect("row created its conversation")
                .members
                .push(UserId::from_uuid(row.try_get("user_id").map_err(db_error)?));
        }
        let has_more = conversations.len() > limit;
        if has_more {
            conversations.truncate(limit);
        }
        let next_cursor = if has_more {
            conversations.last().map(|c| c.id)
        } else {
            None
        };
        Ok(ConversationPage {
            conversations,
            next_cursor,
        })
    }

    async fn find_for_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
    ) -> Result<Conversation, AppError> {
        let row = sqlx::query("SELECT c.id, c.kind, c.name, c.created_by, c.created_at, array_agg(all_members.user_id ORDER BY all_members.user_id) AS members FROM conversations c JOIN conversation_members caller_members ON caller_members.conversation_id = c.id AND caller_members.user_id = $2 JOIN conversation_members all_members ON all_members.conversation_id = c.id WHERE c.id = $1 AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = c.id AND NOT channel_has_permission($2, ch.id, 129)) GROUP BY c.id")
            .bind(conversation.as_uuid()).bind(caller.as_uuid()).fetch_optional(&self.pool).await.map_err(db_error)?.ok_or(AppError::NotFound)?;
        let kind: String = row.try_get("kind").map_err(db_error)?;
        let members: Vec<Uuid> = row.try_get("members").map_err(db_error)?;
        Ok(Conversation {
            id: ConversationId::from_uuid(row.try_get("id").map_err(db_error)?),
            kind: if kind == "direct" {
                ConversationKind::Direct
            } else {
                ConversationKind::Group
            },
            name: conversation_name(&row)?,
            created_by: UserId::from_uuid(row.try_get("created_by").map_err(db_error)?),
            created_at: row.try_get("created_at").map_err(db_error)?,
            members: members.into_iter().map(UserId::from_uuid).collect(),
        })
    }

    async fn members_for_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
    ) -> Result<Vec<User>, AppError> {
        let rows = sqlx::query("SELECT u.id, u.username, u.created_at FROM users u JOIN conversation_members target_members ON target_members.user_id = u.id JOIN conversation_members caller_members ON caller_members.conversation_id = target_members.conversation_id AND caller_members.user_id = $2 WHERE target_members.conversation_id = $1 AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = $1 AND NOT channel_has_permission($2, ch.id, 129)) ORDER BY u.username")
            .bind(conversation.as_uuid())
            .bind(caller.as_uuid())
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        if rows.is_empty() {
            return Err(AppError::NotFound);
        }
        rows.iter()
            .map(|row| user_from(row, "id", "username", "created_at"))
            .collect()
    }

    async fn members_presence_for_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
    ) -> Result<Vec<(User, bool)>, AppError> {
        let rows = sqlx::query("SELECT u.id, u.username, u.created_at, p.presence_visible FROM users u JOIN user_profiles p ON p.user_id = u.id JOIN conversation_members target_members ON target_members.user_id = u.id JOIN conversation_members caller_members ON caller_members.conversation_id = target_members.conversation_id AND caller_members.user_id = $2 WHERE target_members.conversation_id = $1 AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = $1 AND NOT channel_has_permission($2, ch.id, 129)) ORDER BY u.username")
            .bind(conversation.as_uuid()).bind(caller.as_uuid()).fetch_all(&self.pool).await.map_err(db_error)?;
        if rows.is_empty() {
            return Err(AppError::NotFound);
        }
        rows.iter()
            .map(|row| {
                Ok((
                    user_from(row, "id", "username", "created_at")?,
                    row.try_get("presence_visible").map_err(db_error)?,
                ))
            })
            .collect()
    }

    async fn share_conversation(&self, first: UserId, second: UserId) -> Result<bool, AppError> {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM conversation_members a JOIN conversation_members b ON b.conversation_id = a.conversation_id WHERE a.user_id = $1 AND b.user_id = $2)")
            .bind(first.as_uuid()).bind(second.as_uuid()).fetch_one(&self.pool).await.map_err(db_error)
    }
}

#[derive(Clone)]
pub struct PgCommunityRepository {
    pool: PgPool,
}

impl PgCommunityRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn community_from(row: &PgRow) -> Result<Community, AppError> {
    let count: i64 = row.try_get("member_count").map_err(db_error)?;
    Ok(Community {
        id: CommunityId::from_uuid(row.try_get("id").map_err(db_error)?),
        owner_id: UserId::from_uuid(row.try_get("owner_id").map_err(db_error)?),
        name: row.try_get("name").map_err(db_error)?,
        description: row.try_get("description").map_err(db_error)?,
        created_at: row.try_get("created_at").map_err(db_error)?,
        member_count: usize::try_from(count).unwrap_or(usize::MAX),
    })
}

fn channel_from(row: &PgRow) -> Result<Channel, AppError> {
    let category_id: Option<Uuid> = row.try_get("category_id").map_err(db_error)?;
    Ok(Channel {
        id: ChannelId::from_uuid(row.try_get("id").map_err(db_error)?),
        community_id: CommunityId::from_uuid(row.try_get("community_id").map_err(db_error)?),
        conversation_id: ConversationId::from_uuid(
            row.try_get("conversation_id").map_err(db_error)?,
        ),
        category_id: category_id.map(CategoryId::from_uuid),
        name: row.try_get("name").map_err(db_error)?,
        description: row.try_get("description").map_err(db_error)?,
        position: row.try_get("position").map_err(db_error)?,
        read_only: row.try_get("read_only").map_err(db_error)?,
        created_at: row.try_get("created_at").map_err(db_error)?,
    })
}

fn category_from(row: &PgRow) -> Result<Category, AppError> {
    Ok(Category {
        id: CategoryId::from_uuid(row.try_get("id").map_err(db_error)?),
        community_id: CommunityId::from_uuid(row.try_get("community_id").map_err(db_error)?),
        name: row.try_get("name").map_err(db_error)?,
        position: row.try_get("position").map_err(db_error)?,
    })
}

fn role_from(row: &PgRow) -> Result<Role, AppError> {
    let permissions: i64 = row.try_get("permissions").map_err(db_error)?;
    Ok(Role {
        id: RoleId::from_uuid(row.try_get("id").map_err(db_error)?),
        community_id: CommunityId::from_uuid(row.try_get("community_id").map_err(db_error)?),
        name: row.try_get("name").map_err(db_error)?,
        position: row.try_get("position").map_err(db_error)?,
        permissions: u64::try_from(permissions).unwrap_or(0),
        is_default: row.try_get("is_default").map_err(db_error)?,
    })
}

#[async_trait]
impl CommunityRepository for PgCommunityRepository {
    async fn list_audit(
        &self,
        caller: UserId,
        community: CommunityId,
        before: Option<rustchat_domain::identity::AuditEventId>,
        limit: usize,
    ) -> Result<AuditPage, AppError> {
        // 2 = MANAGE_COMMUNITY. Hidden from anyone without it, like other management reads.
        let permitted: bool = sqlx::query_scalar("SELECT community_has_permission($1, $2, 2)")
            .bind(caller.as_uuid())
            .bind(community.as_uuid())
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        if !permitted {
            return Err(AppError::NotFound);
        }
        let rows = sqlx::query(
            "SELECT a.id, a.event_type, a.created_at, a.details, \
                    a.actor_id AS actor_user_id, actor.username AS actor_username, \
                    NULL::uuid AS target_user_id, NULL::text AS target_username \
             FROM community_audit_log a \
             LEFT JOIN users actor ON actor.id = a.actor_id \
             WHERE a.community_id = $2 \
               AND ($1::uuid IS NULL \
                    OR (a.created_at, a.id) < (SELECT created_at, id FROM community_audit_log WHERE id = $1)) \
             ORDER BY a.created_at DESC, a.id DESC \
             LIMIT $3",
        )
        .bind(before.map(|id| id.as_uuid()))
        .bind(community.as_uuid())
        .bind(i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        audit_page(&rows, limit)
    }

    async fn create(
        &self,
        owner: UserId,
        name: &str,
        description: &str,
        now: OffsetDateTime,
    ) -> Result<Community, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO communities (id, owner_id, name, description, created_at) VALUES ($1, $2, $3, $4, $5)")
            .bind(id).bind(owner.as_uuid()).bind(name).bind(description).bind(now)
            .execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query(
            "INSERT INTO community_members (community_id, user_id, joined_at) VALUES ($1, $2, $3)",
        )
        .bind(id)
        .bind(owner.as_uuid())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        let default_role = Uuid::now_v7();
        sqlx::query("INSERT INTO community_roles (id, community_id, name, position, permissions, is_default) VALUES ($1, $2, 'everyone', 0, 193, true)")
            .bind(default_role).bind(id).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("INSERT INTO community_member_roles (community_id, user_id, role_id) VALUES ($1, $2, $3)")
            .bind(id).bind(owner.as_uuid()).bind(default_role).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(Community {
            id: CommunityId::from_uuid(id),
            owner_id: owner,
            name: name.to_owned(),
            description: description.to_owned(),
            created_at: now,
            member_count: 1,
        })
    }

    async fn list_for_member(&self, caller: UserId) -> Result<Vec<Community>, AppError> {
        let rows = sqlx::query("SELECT c.id, c.owner_id, c.name, c.description, c.created_at, count(m.user_id)::bigint AS member_count FROM communities c JOIN community_members access ON access.community_id = c.id AND access.user_id = $1 JOIN community_members m ON m.community_id = c.id GROUP BY c.id ORDER BY c.created_at, c.id")
            .bind(caller.as_uuid()).fetch_all(&self.pool).await.map_err(db_error)?;
        rows.iter().map(community_from).collect()
    }

    async fn update_settings(
        &self,
        caller: UserId,
        community: CommunityId,
        name: &str,
        description: &str,
    ) -> Result<Community, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let row = sqlx::query("UPDATE communities c SET name=$3, description=$4 WHERE c.id=$1 AND community_has_permission($2,c.id,2) RETURNING c.id,c.owner_id,c.name,c.description,c.created_at,(SELECT count(*)::bigint FROM community_members m WHERE m.community_id=c.id) AS member_count")
            .bind(community.as_uuid()).bind(caller.as_uuid()).bind(name).bind(description).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(AppError::NotFound)?;
        sqlx::query("INSERT INTO community_audit_log(id,community_id,actor_id,event_type,details,created_at) VALUES($1,$2,$3,'settings.updated',$4,now())")
            .bind(Uuid::now_v7()).bind(community.as_uuid()).bind(caller.as_uuid()).bind(serde_json::json!({"name":name,"description":description})).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        community_from(&row)
    }

    async fn transfer_ownership(
        &self,
        caller: UserId,
        community: CommunityId,
        new_owner: UserId,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let owner: Option<Uuid> =
            sqlx::query_scalar("SELECT owner_id FROM communities WHERE id=$1 FOR UPDATE")
                .bind(community.as_uuid())
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_error)?;
        if owner != Some(caller.as_uuid()) {
            return Err(AppError::NotFound);
        }
        let member: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM community_members WHERE community_id=$1 AND user_id=$2)",
        )
        .bind(community.as_uuid())
        .bind(new_owner.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        if !member {
            return Err(AppError::NotFound);
        }
        sqlx::query("INSERT INTO community_audit_log(id,community_id,actor_id,event_type,details,created_at) VALUES($1,$2,$3,'ownership.transferred',$4,now())")
            .bind(Uuid::now_v7()).bind(community.as_uuid()).bind(caller.as_uuid()).bind(serde_json::json!({"previous_owner":caller.as_uuid(),"new_owner":new_owner.as_uuid()})).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("UPDATE communities SET owner_id=$2 WHERE id=$1")
            .bind(community.as_uuid())
            .bind(new_owner.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    async fn delete_community(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let owner: Option<Uuid> =
            sqlx::query_scalar("SELECT owner_id FROM communities WHERE id=$1 FOR UPDATE")
                .bind(community.as_uuid())
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_error)?;
        if owner != Some(caller.as_uuid()) {
            return Err(AppError::NotFound);
        }
        sqlx::query("INSERT INTO community_audit_log(id,community_id,actor_id,event_type,created_at) VALUES($1,$2,$3,'community.deleted',now())")
            .bind(Uuid::now_v7()).bind(community.as_uuid()).bind(caller.as_uuid()).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("DELETE FROM conversations WHERE id IN (SELECT conversation_id FROM community_channels WHERE community_id=$1)")
            .bind(community.as_uuid()).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("DELETE FROM communities WHERE id=$1")
            .bind(community.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    async fn members(&self, caller: UserId, community: CommunityId) -> Result<Vec<User>, AppError> {
        let rows = sqlx::query("SELECT u.id, u.username, u.created_at FROM users u JOIN community_members target ON target.user_id = u.id JOIN community_members access ON access.community_id = target.community_id AND access.user_id = $2 WHERE target.community_id = $1 ORDER BY u.username")
            .bind(community.as_uuid()).bind(caller.as_uuid()).fetch_all(&self.pool).await.map_err(db_error)?;
        if rows.is_empty() {
            return Err(AppError::NotFound);
        }
        rows.iter()
            .map(|row| user_from(row, "id", "username", "created_at"))
            .collect()
    }

    async fn leave(&self, caller: UserId, community: CommunityId) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let owner: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM communities WHERE id = $1 AND owner_id = $2)",
        )
        .bind(community.as_uuid())
        .bind(caller.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        if owner {
            return Err(AppError::CommunityOwnerCannotLeave);
        }
        let deleted =
            sqlx::query("DELETE FROM community_members WHERE community_id = $1 AND user_id = $2")
                .bind(community.as_uuid())
                .bind(caller.as_uuid())
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
        if deleted.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        sqlx::query("DELETE FROM conversation_members cm USING community_channels ch WHERE ch.conversation_id = cm.conversation_id AND ch.community_id = $1 AND cm.user_id = $2")
            .bind(community.as_uuid()).bind(caller.as_uuid()).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    async fn remove_member(
        &self,
        caller: UserId,
        community: CommunityId,
        member: UserId,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let community_row = sqlx::query("SELECT owner_id FROM communities WHERE id=$1 FOR UPDATE")
            .bind(community.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?
            .ok_or(AppError::NotFound)?;
        let owner_id: Uuid = community_row.try_get("owner_id").map_err(db_error)?;
        let permitted: bool = sqlx::query_scalar("SELECT community_has_permission($1,$2,16) AND EXISTS (SELECT 1 FROM community_members WHERE community_id=$2 AND user_id=$3) AND $1 <> $3 AND $3 <> $4")
            .bind(caller.as_uuid())
            .bind(community.as_uuid())
            .bind(member.as_uuid())
            .bind(owner_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        if !permitted {
            return Err(AppError::NotFound);
        }
        sqlx::query("DELETE FROM conversation_members cm USING community_channels ch WHERE ch.conversation_id=cm.conversation_id AND ch.community_id=$1 AND cm.user_id=$2")
            .bind(community.as_uuid()).bind(member.as_uuid()).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("DELETE FROM community_members WHERE community_id=$1 AND user_id=$2")
            .bind(community.as_uuid())
            .bind(member.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        let details = serde_json::json!({"removed_user_id": member.as_uuid()});
        sqlx::query("INSERT INTO community_audit_log(id,community_id,actor_id,event_type,details,created_at) VALUES($1,$2,$3,'member.removed',$4,now())")
            .bind(Uuid::now_v7()).bind(community.as_uuid()).bind(caller.as_uuid()).bind(details).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    async fn create_invite(
        &self,
        caller: UserId,
        community: CommunityId,
        token_digest: TokenDigest,
        now: OffsetDateTime,
        expires_at: Option<OffsetDateTime>,
        max_uses: Option<i32>,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let id = Uuid::now_v7();
        let result = sqlx::query("INSERT INTO community_invites (id, community_id, token_hash, created_by, created_at, expires_at, max_uses) SELECT $1, c.id, $2, $3, $4, $5, $6 FROM communities c WHERE c.id = $7 AND community_has_permission($3, c.id, 32)")
            .bind(id).bind(token_digest.0.to_vec()).bind(caller.as_uuid()).bind(now).bind(expires_at).bind(max_uses).bind(community.as_uuid())
            .execute(&mut *tx).await.map_err(db_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        sqlx::query("INSERT INTO community_audit_log(id,community_id,actor_id,event_type,details,created_at) VALUES($1,$2,$3,'invite.created',$4,$5)")
            .bind(Uuid::now_v7()).bind(community.as_uuid()).bind(caller.as_uuid()).bind(serde_json::json!({"invite_id":id,"expires_at":expires_at.map(|value| value.to_string()),"max_uses":max_uses})).bind(now)
            .execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    async fn revoke_invite(
        &self,
        caller: UserId,
        community: CommunityId,
        token_digest: TokenDigest,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let permitted: bool = sqlx::query_scalar("SELECT community_has_permission($1, $2, 32)")
            .bind(caller.as_uuid())
            .bind(community.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        if !permitted {
            return Err(AppError::NotFound);
        }
        let result = sqlx::query("UPDATE community_invites SET revoked_at = now() WHERE community_id = $1 AND token_hash = $2 AND revoked_at IS NULL")
            .bind(community.as_uuid()).bind(token_digest.0.to_vec()).execute(&mut *tx).await.map_err(db_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        sqlx::query("INSERT INTO community_audit_log(id,community_id,actor_id,event_type,created_at) VALUES($1,$2,$3,'invite.revoked',now())")
            .bind(Uuid::now_v7()).bind(community.as_uuid()).bind(caller.as_uuid()).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    async fn revoke_invite_by_id(
        &self,
        caller: UserId,
        community: CommunityId,
        invite: rustchat_domain::identity::InviteId,
    ) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let result = sqlx::query("UPDATE community_invites SET revoked_at = now() WHERE community_id = $1 AND id = $2 AND revoked_at IS NULL AND community_has_permission($3, community_id, 32)")
            .bind(community.as_uuid()).bind(invite.as_uuid()).bind(caller.as_uuid()).execute(&mut *tx).await.map_err(db_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        sqlx::query("INSERT INTO community_audit_log(id,community_id,actor_id,event_type,created_at) VALUES($1,$2,$3,'invite.revoked',now())")
            .bind(Uuid::now_v7()).bind(community.as_uuid()).bind(caller.as_uuid()).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    async fn list_invites(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<Vec<rustchat_domain::community::Invite>, AppError> {
        let permitted: bool = sqlx::query_scalar("SELECT community_has_permission($1, $2, 32)")
            .bind(caller.as_uuid())
            .bind(community.as_uuid())
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        if !permitted {
            return Err(AppError::NotFound);
        }
        let rows = sqlx::query("SELECT i.id, i.community_id, i.created_by, i.created_at, i.expires_at, i.max_uses, i.use_count, i.revoked_at FROM community_invites i WHERE i.community_id = $1 AND community_has_permission($2, i.community_id, 32) ORDER BY i.created_at DESC, i.id DESC")
            .bind(community.as_uuid()).bind(caller.as_uuid()).fetch_all(&self.pool).await.map_err(db_error)?;
        rows.into_iter()
            .map(|row| {
                Ok(rustchat_domain::community::Invite {
                    id: rustchat_domain::identity::InviteId::from_uuid(
                        row.try_get("id").map_err(db_error)?,
                    ),
                    community_id: CommunityId::from_uuid(
                        row.try_get("community_id").map_err(db_error)?,
                    ),
                    created_by: UserId::from_uuid(row.try_get("created_by").map_err(db_error)?),
                    created_at: row.try_get("created_at").map_err(db_error)?,
                    expires_at: row.try_get("expires_at").map_err(db_error)?,
                    max_uses: row.try_get("max_uses").map_err(db_error)?,
                    use_count: row.try_get("use_count").map_err(db_error)?,
                    revoked_at: row.try_get("revoked_at").map_err(db_error)?,
                })
            })
            .collect()
    }

    async fn join(
        &self,
        caller: UserId,
        token_digest: TokenDigest,
        now: OffsetDateTime,
    ) -> Result<Community, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let row = sqlx::query("SELECT i.id, i.community_id, i.expires_at, i.max_uses, i.use_count, i.revoked_at, c.owner_id, c.name, c.description, c.created_at FROM community_invites i JOIN communities c ON c.id = i.community_id WHERE i.token_hash = $1 FOR UPDATE OF i")
            .bind(token_digest.0.to_vec()).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(AppError::NotFound)?;
        let community_id: Uuid = row.try_get("community_id").map_err(db_error)?;
        let already_member: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM community_members WHERE community_id = $1 AND user_id = $2)")
            .bind(community_id).bind(caller.as_uuid()).fetch_one(&mut *tx).await.map_err(db_error)?;
        if !already_member {
            let expires_at: Option<OffsetDateTime> = row.try_get("expires_at").map_err(db_error)?;
            let revoked_at: Option<OffsetDateTime> = row.try_get("revoked_at").map_err(db_error)?;
            let max_uses: Option<i32> = row.try_get("max_uses").map_err(db_error)?;
            let uses: i32 = row.try_get("use_count").map_err(db_error)?;
            if revoked_at.is_some()
                || expires_at.is_some_and(|expires| expires <= now)
                || max_uses.is_some_and(|limit| uses >= limit)
            {
                return Err(AppError::NotFound);
            }
            sqlx::query("INSERT INTO community_members (community_id, user_id, joined_at) VALUES ($1, $2, $3)")
                .bind(community_id).bind(caller.as_uuid()).bind(now).execute(&mut *tx).await.map_err(db_error)?;
            let default_role: Uuid = sqlx::query_scalar(
                "SELECT id FROM community_roles WHERE community_id = $1 AND is_default",
            )
            .bind(community_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
            sqlx::query("INSERT INTO community_member_roles (community_id, user_id, role_id) VALUES ($1, $2, $3)")
                .bind(community_id).bind(caller.as_uuid()).bind(default_role).execute(&mut *tx).await.map_err(db_error)?;
            sqlx::query("INSERT INTO conversation_members (conversation_id, user_id, joined_at) SELECT conversation_id, $2, $3 FROM community_channels WHERE community_id = $1 ON CONFLICT DO NOTHING")
                .bind(community_id).bind(caller.as_uuid()).bind(now).execute(&mut *tx).await.map_err(db_error)?;
            sqlx::query("UPDATE community_invites SET use_count = use_count + 1 WHERE id = $1")
                .bind(row.try_get::<Uuid, _>("id").map_err(db_error)?)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
        }
        let row = sqlx::query("SELECT c.id, c.owner_id, c.name, c.description, c.created_at, count(m.user_id)::bigint AS member_count FROM communities c JOIN community_members access ON access.community_id = c.id AND access.user_id = $2 JOIN community_members m ON m.community_id = c.id WHERE c.id = $1 GROUP BY c.id")
            .bind(community_id).bind(caller.as_uuid()).fetch_one(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        community_from(&row)
    }

    async fn create_category(
        &self,
        caller: UserId,
        community: CommunityId,
        name: &str,
    ) -> Result<Category, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let permitted: bool = sqlx::query_scalar("SELECT community_has_permission($1, $2, 4)")
            .bind(caller.as_uuid())
            .bind(community.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        if !permitted {
            return Err(AppError::NotFound);
        }
        let position: i32 = sqlx::query_scalar("SELECT COALESCE(max(position) + 1, 0) FROM community_categories WHERE community_id = $1")
            .bind(community.as_uuid()).fetch_one(&mut *tx).await.map_err(db_error)?;
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO community_categories (id, community_id, name, position) VALUES ($1, $2, $3, $4)")
            .bind(id).bind(community.as_uuid()).bind(name).bind(position).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(Category {
            id: CategoryId::from_uuid(id),
            community_id: community,
            name: name.to_owned(),
            position,
        })
    }

    async fn list_categories(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<Vec<Category>, AppError> {
        let permitted: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM community_members WHERE community_id = $1 AND user_id = $2)")
            .bind(community.as_uuid()).bind(caller.as_uuid()).fetch_one(&self.pool).await.map_err(db_error)?;
        if !permitted {
            return Err(AppError::NotFound);
        }
        let rows = sqlx::query("SELECT id, community_id, name, position FROM community_categories WHERE community_id = $1 ORDER BY position, id")
            .bind(community.as_uuid()).fetch_all(&self.pool).await.map_err(db_error)?;
        rows.iter().map(category_from).collect()
    }

    async fn update_category(
        &self,
        caller: UserId,
        category: CategoryId,
        name: &str,
        position: i32,
    ) -> Result<Category, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let row = sqlx::query("SELECT community_id FROM community_categories WHERE id = $1 AND community_has_permission($2, community_id, 4) FOR UPDATE")
            .bind(category.as_uuid()).bind(caller.as_uuid()).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(AppError::NotFound)?;
        let community: Uuid = row.try_get("community_id").map_err(db_error)?;
        let rows = sqlx::query("SELECT id FROM community_categories WHERE community_id = $1 ORDER BY position, id FOR UPDATE")
            .bind(community).fetch_all(&mut *tx).await.map_err(db_error)?;
        let mut ids: Vec<Uuid> = rows
            .iter()
            .map(|row| row.try_get("id").map_err(db_error))
            .collect::<Result<_, _>>()?;
        let old = ids
            .iter()
            .position(|id| *id == category.as_uuid())
            .ok_or(AppError::NotFound)?;
        let moving = ids.remove(old);
        let index = usize::try_from(position)
            .unwrap_or(usize::MAX)
            .min(ids.len());
        ids.insert(index, moving);
        let offset = i32::try_from(ids.len() + 1).map_err(AppError::internal)?;
        sqlx::query(
            "UPDATE community_categories SET position = position + $2 WHERE community_id = $1",
        )
        .bind(community)
        .bind(offset)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        for (position, id) in ids.iter().enumerate() {
            sqlx::query("UPDATE community_categories SET position = $2 WHERE id = $1")
                .bind(id)
                .bind(i32::try_from(position).map_err(AppError::internal)?)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
        }
        sqlx::query("UPDATE community_categories SET name = $2 WHERE id = $1")
            .bind(category.as_uuid())
            .bind(name)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        let row = sqlx::query(
            "SELECT id, community_id, name, position FROM community_categories WHERE id = $1",
        )
        .bind(category.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        category_from(&row)
    }

    async fn delete_category(&self, caller: UserId, category: CategoryId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM community_categories WHERE id = $1 AND community_has_permission($2, community_id, 4)")
            .bind(category.as_uuid()).bind(caller.as_uuid()).execute(&self.pool).await.map_err(db_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

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
    ) -> Result<Channel, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let permitted: bool = sqlx::query_scalar("SELECT community_has_permission($1, $2, 4)")
            .bind(caller.as_uuid())
            .bind(community.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        if !permitted {
            return Err(AppError::NotFound);
        }
        if let Some(category) = category {
            let belongs: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM community_categories WHERE id = $1 AND community_id = $2)")
                .bind(category.as_uuid()).bind(community.as_uuid()).fetch_one(&mut *tx).await.map_err(db_error)?;
            if !belongs {
                return Err(AppError::NotFound);
            }
        }
        let position: i32 = sqlx::query_scalar(
            "SELECT COALESCE(max(position) + 1, 0) FROM community_channels WHERE community_id = $1",
        )
        .bind(community.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        let conversation = Uuid::now_v7();
        sqlx::query("INSERT INTO conversations (id, kind, created_by, created_at) VALUES ($1, 'group', $2, $3)")
            .bind(conversation).bind(caller.as_uuid()).bind(now).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("INSERT INTO conversation_members (conversation_id, user_id, joined_at) SELECT $1, user_id, $2 FROM community_members WHERE community_id = $3")
            .bind(conversation).bind(now).bind(community.as_uuid()).execute(&mut *tx).await.map_err(db_error)?;
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO community_channels (id, community_id, conversation_id, name, description, position, read_only, created_at, category_id) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)")
            .bind(id).bind(community.as_uuid()).bind(conversation).bind(name).bind(description).bind(position).bind(read_only).bind(now).bind(category.map(|id| id.as_uuid()))
            .execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(Channel {
            id: ChannelId::from_uuid(id),
            community_id: community,
            conversation_id: ConversationId::from_uuid(conversation),
            category_id: category,
            name: name.to_owned(),
            description: description.to_owned(),
            position,
            read_only,
            created_at: now,
        })
    }

    async fn list_channels(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<Vec<Channel>, AppError> {
        let rows = sqlx::query("SELECT ch.id, ch.community_id, ch.conversation_id, ch.name, ch.description, ch.position, ch.read_only, ch.created_at, ch.category_id FROM community_channels ch WHERE ch.community_id = $1 AND channel_has_permission($2, ch.id, 129) ORDER BY COALESCE(ch.category_id, '00000000-0000-0000-0000-000000000000'::uuid), ch.position, ch.id")
            .bind(community.as_uuid()).bind(caller.as_uuid()).fetch_all(&self.pool).await.map_err(db_error)?;
        if rows.is_empty() {
            let is_member: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM community_members WHERE community_id = $1 AND user_id = $2)")
                .bind(community.as_uuid()).bind(caller.as_uuid()).fetch_one(&self.pool).await.map_err(db_error)?;
            if !is_member {
                return Err(AppError::NotFound);
            }
        }
        rows.iter().map(channel_from).collect()
    }

    #[allow(clippy::too_many_arguments)]
    async fn update_channel(
        &self,
        caller: UserId,
        channel: ChannelId,
        name: &str,
        description: &str,
        category: Option<Option<CategoryId>>,
        read_only: bool,
        requested_position: i32,
    ) -> Result<Channel, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let row = sqlx::query("SELECT community_id FROM community_channels WHERE id = $1 AND community_has_permission($2, community_id, 4) FOR UPDATE")
            .bind(channel.as_uuid()).bind(caller.as_uuid()).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(AppError::NotFound)?;
        let community: Uuid = row.try_get("community_id").map_err(db_error)?;
        if let Some(Some(category)) = category {
            let belongs: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM community_categories WHERE id = $1 AND community_id = $2)")
                .bind(category.as_uuid()).bind(community).fetch_one(&mut *tx).await.map_err(db_error)?;
            if !belongs {
                return Err(AppError::NotFound);
            }
        }
        let rows = sqlx::query("SELECT id FROM community_channels WHERE community_id = $1 ORDER BY position, id FOR UPDATE")
            .bind(community).fetch_all(&mut *tx).await.map_err(db_error)?;
        let mut ids: Vec<Uuid> = rows
            .iter()
            .map(|r| r.try_get("id").map_err(db_error))
            .collect::<Result<_, _>>()?;
        let old_index = ids
            .iter()
            .position(|id| *id == channel.as_uuid())
            .ok_or(AppError::NotFound)?;
        let id = ids.remove(old_index);
        let new_index = usize::try_from(requested_position)
            .unwrap_or(usize::MAX)
            .min(ids.len());
        ids.insert(new_index, id);
        sqlx::query(
            "UPDATE community_channels SET position = position + $2 WHERE community_id = $1",
        )
        .bind(community)
        .bind(i32::try_from(ids.len() + 1).map_err(AppError::internal)?)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        for (position, channel_id) in ids.iter().enumerate() {
            sqlx::query("UPDATE community_channels SET position = $2 WHERE id = $1")
                .bind(channel_id)
                .bind(i32::try_from(position).map_err(AppError::internal)?)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
        }
        let updated = sqlx::query("UPDATE community_channels SET name = $2, description = $3, read_only = $4, category_id = CASE WHEN $6 THEN $5 ELSE category_id END WHERE id = $1 RETURNING id, community_id, conversation_id, name, description, position, read_only, created_at, category_id")
            .bind(channel.as_uuid()).bind(name).bind(description).bind(read_only).bind(category.flatten().map(|id| id.as_uuid())).bind(category.is_some()).fetch_one(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        channel_from(&updated)
    }

    async fn delete_channel(&self, caller: UserId, channel: ChannelId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM conversations WHERE id = (SELECT ch.conversation_id FROM community_channels ch WHERE ch.id = $1 AND community_has_permission($2, ch.community_id, 4))")
            .bind(channel.as_uuid()).bind(caller.as_uuid()).execute(&self.pool).await.map_err(db_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    async fn create_role(
        &self,
        caller: UserId,
        community: CommunityId,
        name: &str,
        permissions: u64,
    ) -> Result<Role, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let permitted: bool = sqlx::query_scalar("SELECT community_has_permission($1, $2, 8) AND (($3::bigint & ~community_permissions($1, $2)) = 0)")
            .bind(caller.as_uuid()).bind(community.as_uuid()).bind(permissions as i64).fetch_one(&mut *tx).await.map_err(db_error)?;
        if !permitted {
            return Err(AppError::NotFound);
        }
        let position: i32 = sqlx::query_scalar(
            "SELECT COALESCE(max(position) + 1, 1) FROM community_roles WHERE community_id = $1",
        )
        .bind(community.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO community_roles (id, community_id, name, position, permissions) VALUES ($1, $2, $3, $4, $5)")
            .bind(id).bind(community.as_uuid()).bind(name).bind(position).bind(permissions as i64).execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(Role {
            id: RoleId::from_uuid(id),
            community_id: community,
            name: name.to_owned(),
            position,
            permissions,
            is_default: false,
        })
    }

    async fn list_roles(
        &self,
        caller: UserId,
        community: CommunityId,
    ) -> Result<Vec<Role>, AppError> {
        let is_member: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM community_members WHERE community_id = $1 AND user_id = $2)")
            .bind(community.as_uuid()).bind(caller.as_uuid()).fetch_one(&self.pool).await.map_err(db_error)?;
        if !is_member {
            return Err(AppError::NotFound);
        }
        let rows = sqlx::query("SELECT id, community_id, name, position, permissions, is_default FROM community_roles WHERE community_id = $1 ORDER BY position, id")
            .bind(community.as_uuid()).fetch_all(&self.pool).await.map_err(db_error)?;
        rows.iter().map(role_from).collect()
    }

    async fn update_role(
        &self,
        caller: UserId,
        community: CommunityId,
        role: RoleId,
        name: &str,
        permissions: u64,
        requested_position: i32,
    ) -> Result<Role, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let allowed: bool = sqlx::query_scalar("SELECT community_has_permission($1,$2,8) AND EXISTS (SELECT 1 FROM community_roles target WHERE target.id=$3 AND target.community_id=$2 AND NOT target.is_default AND (EXISTS (SELECT 1 FROM communities c WHERE c.id=$2 AND c.owner_id=$1) OR target.position > COALESCE((SELECT min(r.position) FROM community_member_roles mr JOIN community_roles r ON r.id=mr.role_id WHERE mr.community_id=$2 AND mr.user_id=$1 AND NOT r.is_default), 0)) AND (($4::bigint & ~community_permissions($1,$2)) = 0) AND (EXISTS (SELECT 1 FROM communities c WHERE c.id=$2 AND c.owner_id=$1) OR $5 > COALESCE((SELECT min(r.position) FROM community_member_roles mr JOIN community_roles r ON r.id=mr.role_id WHERE mr.community_id=$2 AND mr.user_id=$1 AND NOT r.is_default), 0)))")
            .bind(caller.as_uuid()).bind(community.as_uuid()).bind(role.as_uuid()).bind(permissions as i64).bind(requested_position).fetch_one(&mut *tx).await.map_err(db_error)?;
        if !allowed {
            return Err(AppError::NotFound);
        }
        let row = sqlx::query("SELECT id FROM community_roles WHERE community_id=$1 AND NOT is_default ORDER BY position,id FOR UPDATE")
            .bind(community.as_uuid()).fetch_all(&mut *tx).await.map_err(db_error)?;
        let mut ids: Vec<Uuid> = row
            .iter()
            .map(|r| r.try_get("id").map_err(db_error))
            .collect::<Result<_, _>>()?;
        let old = ids
            .iter()
            .position(|id| *id == role.as_uuid())
            .ok_or(AppError::NotFound)?;
        ids.remove(old);
        let position = usize::try_from(requested_position - 1)
            .unwrap_or(usize::MAX)
            .min(ids.len());
        ids.insert(position, role.as_uuid());
        sqlx::query("UPDATE community_roles SET position = position + $2 WHERE community_id=$1 AND NOT is_default")
            .bind(community.as_uuid()).bind(i32::try_from(ids.len() + 1).map_err(AppError::internal)?)
            .execute(&mut *tx).await.map_err(db_error)?;
        for (index, role_id) in ids.iter().enumerate() {
            sqlx::query("UPDATE community_roles SET position=$2 WHERE id=$1")
                .bind(role_id)
                .bind(i32::try_from(index + 1).map_err(AppError::internal)?)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?;
        }
        let updated = sqlx::query("UPDATE community_roles SET name=$3,permissions=$4 WHERE community_id=$1 AND id=$2 RETURNING id,community_id,name,position,permissions,is_default")
            .bind(community.as_uuid()).bind(role.as_uuid()).bind(name).bind(permissions as i64).fetch_one(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        role_from(&updated)
    }

    async fn delete_role(
        &self,
        caller: UserId,
        community: CommunityId,
        role: RoleId,
    ) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM community_roles r WHERE r.id=$3 AND r.community_id=$2 AND NOT r.is_default AND community_has_permission($1,$2,8) AND (EXISTS (SELECT 1 FROM communities c WHERE c.id=$2 AND c.owner_id=$1) OR r.position > COALESCE((SELECT min(own.position) FROM community_member_roles mr JOIN community_roles own ON own.id=mr.role_id WHERE mr.community_id=$2 AND mr.user_id=$1 AND NOT own.is_default),0))")
            .bind(caller.as_uuid()).bind(community.as_uuid()).bind(role.as_uuid()).execute(&self.pool).await.map_err(db_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    async fn assign_role(
        &self,
        caller: UserId,
        community: CommunityId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AppError> {
        let permitted: bool = sqlx::query_scalar("SELECT community_has_permission($1, $2, 16) AND EXISTS (SELECT 1 FROM community_members m JOIN community_roles r ON r.community_id = m.community_id WHERE m.community_id = $2 AND m.user_id = $3 AND r.id = $4) AND (EXISTS (SELECT 1 FROM communities c WHERE c.id = $2 AND c.owner_id = $1) OR (SELECT r.position FROM community_roles r WHERE r.id = $4 AND r.community_id = $2) > COALESCE((SELECT min(r.position) FROM community_member_roles mr JOIN community_roles r ON r.id = mr.role_id WHERE mr.community_id = $2 AND mr.user_id = $1 AND NOT r.is_default), 0))")
            .bind(caller.as_uuid()).bind(community.as_uuid()).bind(member.as_uuid()).bind(role.as_uuid()).fetch_one(&self.pool).await.map_err(db_error)?;
        if !permitted {
            return Err(AppError::NotFound);
        }
        sqlx::query("INSERT INTO community_member_roles (community_id, user_id, role_id) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
            .bind(community.as_uuid()).bind(member.as_uuid()).bind(role.as_uuid()).execute(&self.pool).await.map_err(db_error)?;
        Ok(())
    }

    async fn remove_role(
        &self,
        caller: UserId,
        community: CommunityId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM community_member_roles mr USING community_roles target WHERE mr.community_id=$2 AND mr.user_id=$3 AND mr.role_id=$4 AND target.id=mr.role_id AND NOT target.is_default AND community_has_permission($1,$2,16) AND (EXISTS (SELECT 1 FROM communities c WHERE c.id=$2 AND c.owner_id=$1) OR target.position > COALESCE((SELECT min(own.position) FROM community_member_roles own_mr JOIN community_roles own ON own.id=own_mr.role_id WHERE own_mr.community_id=$2 AND own_mr.user_id=$1 AND NOT own.is_default),0))")
            .bind(caller.as_uuid()).bind(community.as_uuid()).bind(member.as_uuid()).bind(role.as_uuid()).execute(&self.pool).await.map_err(db_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    async fn set_channel_override(
        &self,
        caller: UserId,
        channel: ChannelId,
        role: RoleId,
        allow: u64,
        deny: u64,
    ) -> Result<CommunityId, AppError> {
        let community_id: Uuid = sqlx::query_scalar("SELECT c.community_id FROM community_channels c JOIN community_roles r ON r.id = $3 AND r.community_id = c.community_id WHERE c.id = $2 AND community_has_permission($1, c.community_id, 4) AND ((($4::bigint | $5::bigint) & ~community_permissions($1, c.community_id)) = 0)")
            .bind(caller.as_uuid()).bind(channel.as_uuid()).bind(role.as_uuid()).bind(allow as i64).bind(deny as i64).fetch_optional(&self.pool).await.map_err(db_error)?.ok_or(AppError::NotFound)?;
        sqlx::query("INSERT INTO channel_role_overrides (channel_id, role_id, allow_permissions, deny_permissions) VALUES ($1, $2, $3, $4) ON CONFLICT (channel_id, role_id) DO UPDATE SET allow_permissions = EXCLUDED.allow_permissions, deny_permissions = EXCLUDED.deny_permissions")
            .bind(channel.as_uuid()).bind(role.as_uuid()).bind(allow as i64).bind(deny as i64).execute(&self.pool).await.map_err(db_error)?;
        Ok(CommunityId::from_uuid(community_id))
    }
}

fn message_from_row(row: &PgRow) -> Result<Message, AppError> {
    let raw_content: String = row.try_get("content").map_err(db_error)?;
    let mentioned_user_ids: Vec<Uuid> = row.try_get("mentioned_user_ids").map_err(db_error)?;
    Ok(Message {
        id: MessageId::from_uuid(row.try_get("id").map_err(db_error)?),
        conversation_id: ConversationId::from_uuid(
            row.try_get("conversation_id").map_err(db_error)?,
        ),
        sender_id: UserId::from_uuid(row.try_get("sender_id").map_err(db_error)?),
        sequence: row.try_get("sequence").map_err(db_error)?,
        client_message_id: row.try_get("client_message_id").map_err(db_error)?,
        reply_to_message_id: row
            .try_get::<Option<Uuid>, _>("reply_to_message_id")
            .map_err(db_error)?
            .map(MessageId::from_uuid),
        mentioned_user_ids: mentioned_user_ids
            .into_iter()
            .map(UserId::from_uuid)
            .collect(),
        content: MessageContent::parse(raw_content).map_err(AppError::internal)?,
        created_at: row.try_get("created_at").map_err(db_error)?,
        edited_at: row.try_get("edited_at").map_err(db_error)?,
        deleted_at: row.try_get("deleted_at").map_err(db_error)?,
    })
}

fn mention_usernames(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut found = HashSet::new();
    for (at, byte) in bytes.iter().enumerate() {
        if *byte != b'@' || (at > 0 && is_username_byte(bytes[at - 1])) {
            continue;
        }
        let mut end = at + 1;
        while end < bytes.len() && is_username_byte(bytes[end]) {
            end += 1;
        }
        if !(3..=32).contains(&(end - at - 1)) {
            continue;
        }
        if let Ok(username) = Username::parse(&content[at + 1..end]) {
            found.insert(username.to_string());
        }
        if found.len() >= 100 {
            break;
        }
    }
    let mut names: Vec<_> = found.into_iter().collect();
    names.sort_unstable();
    names
}

fn is_username_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

async fn resolve_mentions(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    conversation: ConversationId,
    content: &str,
) -> Result<Vec<UserId>, AppError> {
    let usernames = mention_usernames(content);
    if usernames.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<Uuid> = sqlx::query_scalar(
        "SELECT u.id FROM users u JOIN conversation_members cm ON cm.user_id = u.id \
         WHERE cm.conversation_id = $1 AND u.username = ANY($2) ORDER BY u.id",
    )
    .bind(conversation.as_uuid())
    .bind(usernames)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_error)?;
    Ok(rows.into_iter().map(UserId::from_uuid).collect())
}

async fn insert_mentions(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    message: MessageId,
    conversation: ConversationId,
    mentioned_user_ids: &[UserId],
) -> Result<(), AppError> {
    for user in mentioned_user_ids {
        sqlx::query("INSERT INTO message_mentions (message_id, conversation_id, user_id) VALUES ($1, $2, $3)")
            .bind(message.as_uuid())
            .bind(conversation.as_uuid())
            .bind(user.as_uuid())
            .execute(&mut **tx)
            .await
            .map_err(db_error)?;
    }
    Ok(())
}

async fn append_message_update_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    row: &PgRow,
    event_type: &str,
    now: OffsetDateTime,
) -> Result<(), AppError> {
    let id: Uuid = row.try_get("id").map_err(db_error)?;
    let conversation: Uuid = row.try_get("conversation_id").map_err(db_error)?;
    let sequence: i64 = row.try_get("sequence").map_err(db_error)?;
    let event_id = MessageId::new().as_uuid();
    let payload = serde_json::json!({"message_id": id, "event_type": event_type});
    sqlx::query("INSERT INTO outbox_events (id, conversation_id, sequence, event_type, payload, created_at) VALUES ($1, $2, $3, $4, $5, $6)")
        .bind(event_id).bind(conversation).bind(sequence).bind(event_type).bind(payload).bind(now)
        .execute(&mut **tx).await.map_err(db_error)?;
    Ok(())
}

#[derive(Clone)]
pub struct PgMessageRepository {
    pool: PgPool,
}

impl PgMessageRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MessageRepository for PgMessageRepository {
    async fn send(
        &self,
        caller: UserId,
        conversation: ConversationId,
        client_message_id: MessageId,
        reply_to_message_id: Option<MessageId>,
        content: MessageContent,
    ) -> Result<Message, AppError> {
        let fingerprint: [u8; 32] = if let Some(reply_id) = reply_to_message_id {
            let mut fingerprint_hash = Sha256::new();
            fingerprint_hash.update(content.as_str().as_bytes());
            fingerprint_hash.update([0]);
            fingerprint_hash.update(reply_id.as_uuid().as_bytes());
            fingerprint_hash.finalize().into()
        } else {
            Sha256::digest(content.as_str().as_bytes()).into()
        };
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let allowed = sqlx::query("SELECT c.id FROM conversations c JOIN conversation_members cm ON cm.conversation_id = c.id AND cm.user_id = $2 WHERE c.id = $1 AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = c.id AND (ch.read_only OR NOT channel_has_permission($2, ch.id, 65))) FOR UPDATE OF c")
            .bind(conversation.as_uuid()).bind(caller.as_uuid()).fetch_optional(&mut *tx).await.map_err(db_error)?;
        if allowed.is_none() {
            return Err(AppError::NotFound);
        }

        let existing = sqlx::query("SELECT id, conversation_id, sender_id, sequence, client_message_id, reply_to_message_id, content, request_fingerprint, created_at, edited_at, deleted_at, ARRAY(SELECT mm.user_id FROM message_mentions mm WHERE mm.message_id = messages.id ORDER BY mm.user_id) AS mentioned_user_ids FROM messages WHERE conversation_id = $1 AND sender_id = $2 AND client_message_id = $3")
            .bind(conversation.as_uuid()).bind(caller.as_uuid()).bind(client_message_id.as_uuid()).fetch_optional(&mut *tx).await.map_err(db_error)?;
        if let Some(row) = existing {
            let stored_fingerprint: Vec<u8> =
                row.try_get("request_fingerprint").map_err(db_error)?;
            if stored_fingerprint.as_slice() != fingerprint {
                return Err(AppError::IdempotencyConflict);
            }
            let message = message_from_row(&row)?;
            tx.commit().await.map_err(db_error)?;
            return Ok(message);
        }

        if let Some(reply_id) = reply_to_message_id {
            let target_exists: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM messages WHERE id = $1 AND conversation_id = $2)",
            )
            .bind(reply_id.as_uuid())
            .bind(conversation.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
            if !target_exists {
                return Err(AppError::NotFound);
            }
        }

        let mentioned_user_ids = resolve_mentions(&mut tx, conversation, content.as_str()).await?;

        let sequence: i64 = sqlx::query_scalar("UPDATE conversations SET last_sequence = last_sequence + 1 WHERE id = $1 RETURNING last_sequence")
            .bind(conversation.as_uuid()).fetch_one(&mut *tx).await.map_err(db_error)?;
        let id = MessageId::new();
        let raw_now = OffsetDateTime::now_utc();
        let now = raw_now
            .replace_nanosecond(raw_now.nanosecond() / 1000 * 1000)
            .unwrap_or(raw_now);
        sqlx::query("INSERT INTO messages (id, conversation_id, sequence, sender_id, client_message_id, reply_to_message_id, content, request_fingerprint, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)")
            .bind(id.as_uuid()).bind(conversation.as_uuid()).bind(sequence).bind(caller.as_uuid()).bind(client_message_id.as_uuid()).bind(reply_to_message_id.map(|reply| reply.as_uuid())).bind(content.as_str()).bind(fingerprint.as_slice()).bind(now)
            .execute(&mut *tx).await.map_err(db_error)?;
        insert_mentions(&mut tx, id, conversation, &mentioned_user_ids).await?;
        let payload = serde_json::json!({
            "type": "message.created",
            "version": 1,
            "event_id": id.as_uuid(),
            "conversation_id": conversation.as_uuid(),
            "sequence": sequence,
            "occurred_at": now.format(&time::format_description::well_known::Rfc3339).map_err(AppError::internal)?,
            "payload": { "message_id": id.as_uuid(), "sender_id": caller.as_uuid(), "client_message_id": client_message_id.as_uuid(), "reply_to_message_id": reply_to_message_id.map(|reply| reply.as_uuid()), "mentioned_user_ids": mentioned_user_ids.iter().map(|user| user.as_uuid()).collect::<Vec<_>>(), "content": content.as_str() }
        });
        sqlx::query("INSERT INTO outbox_events (id, conversation_id, sequence, event_type, payload, created_at) VALUES ($1, $2, $3, 'message.created', $4, $5)")
            .bind(id.as_uuid()).bind(conversation.as_uuid()).bind(sequence).bind(payload).bind(now)
            .execute(&mut *tx).await.map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(Message {
            id,
            conversation_id: conversation,
            sender_id: caller,
            sequence,
            client_message_id: client_message_id.as_uuid(),
            reply_to_message_id,
            mentioned_user_ids,
            content,
            created_at: now,
            edited_at: None,
            deleted_at: None,
        })
    }

    async fn list_for_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
        before_sequence: Option<i64>,
        after_sequence: Option<i64>,
        limit: usize,
    ) -> Result<MessagePage, AppError> {
        let take = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
        let (rows, is_after) = if let Some(after) = after_sequence {
            (sqlx::query("SELECT m.id, m.conversation_id, m.sender_id, m.sequence, m.client_message_id, m.reply_to_message_id, m.content, m.created_at, m.edited_at, m.deleted_at, ARRAY(SELECT mm.user_id FROM message_mentions mm WHERE mm.message_id = m.id ORDER BY mm.user_id) AS mentioned_user_ids FROM messages m WHERE m.conversation_id = $1 AND EXISTS (SELECT 1 FROM conversation_members cm WHERE cm.conversation_id = m.conversation_id AND cm.user_id = $2) AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = m.conversation_id AND NOT channel_has_permission($2, ch.id, 129)) AND m.sequence > $3 ORDER BY m.sequence ASC LIMIT $4")
                .bind(conversation.as_uuid()).bind(caller.as_uuid()).bind(after).bind(take).fetch_all(&self.pool).await.map_err(db_error)?, true)
        } else {
            (sqlx::query("SELECT m.id, m.conversation_id, m.sender_id, m.sequence, m.client_message_id, m.reply_to_message_id, m.content, m.created_at, m.edited_at, m.deleted_at, ARRAY(SELECT mm.user_id FROM message_mentions mm WHERE mm.message_id = m.id ORDER BY mm.user_id) AS mentioned_user_ids FROM messages m WHERE m.conversation_id = $1 AND EXISTS (SELECT 1 FROM conversation_members cm WHERE cm.conversation_id = m.conversation_id AND cm.user_id = $2) AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = m.conversation_id AND NOT channel_has_permission($2, ch.id, 129)) AND ($3::bigint IS NULL OR m.sequence < $3) ORDER BY m.sequence DESC LIMIT $4")
                .bind(conversation.as_uuid()).bind(caller.as_uuid()).bind(before_sequence).bind(take).fetch_all(&self.pool).await.map_err(db_error)?, false)
        };
        // An empty history and a non-member must have distinct outcomes only for members; check
        // membership separately so unknown conversations and non-members both remain 404.
        if rows.is_empty() {
            let allowed: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM conversation_members cm WHERE cm.conversation_id = $1 AND cm.user_id = $2 AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = cm.conversation_id AND NOT channel_has_permission($2, ch.id, 129)))")
                .bind(conversation.as_uuid()).bind(caller.as_uuid()).fetch_one(&self.pool).await.map_err(db_error)?;
            if !allowed {
                return Err(AppError::NotFound);
            }
        }
        let mut messages = rows
            .iter()
            .map(message_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = messages.len() > limit;
        if has_more {
            messages.truncate(limit);
        }
        let cursor = if has_more {
            messages.last().map(|message| message.sequence)
        } else {
            None
        };
        Ok(MessagePage {
            messages,
            next_before: if is_after { None } else { cursor },
            next_after: if is_after { cursor } else { None },
        })
    }

    async fn edit(
        &self,
        caller: UserId,
        conversation: ConversationId,
        message: MessageId,
        content: MessageContent,
    ) -> Result<Message, AppError> {
        let raw_now = OffsetDateTime::now_utc();
        let now = raw_now
            .replace_nanosecond(raw_now.nanosecond() / 1000 * 1000)
            .unwrap_or(raw_now);
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let updated = sqlx::query("UPDATE messages m SET content = $4, edited_at = $5 FROM conversation_members cm WHERE m.id = $3 AND m.conversation_id = $2 AND cm.conversation_id = m.conversation_id AND cm.user_id = $1 AND m.deleted_at IS NULL AND (m.sender_id = $1 OR EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = m.conversation_id AND channel_has_permission($1, ch.id, 256))) RETURNING m.id")
            .bind(caller.as_uuid()).bind(conversation.as_uuid()).bind(message.as_uuid()).bind(content.as_str()).bind(now)
            .fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(AppError::NotFound)?;
        let mentioned_user_ids = resolve_mentions(&mut tx, conversation, content.as_str()).await?;
        sqlx::query("DELETE FROM message_mentions WHERE message_id = $1")
            .bind(message.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        insert_mentions(&mut tx, message, conversation, &mentioned_user_ids).await?;
        let row = sqlx::query("SELECT m.id, m.conversation_id, m.sender_id, m.sequence, m.client_message_id, m.reply_to_message_id, m.content, m.created_at, m.edited_at, m.deleted_at, ARRAY(SELECT mm.user_id FROM message_mentions mm WHERE mm.message_id = m.id ORDER BY mm.user_id) AS mentioned_user_ids FROM messages m WHERE m.id = $1")
            .bind(updated.try_get::<Uuid, _>("id").map_err(db_error)?)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        append_message_update_event(&mut tx, &row, "message.updated", now).await?;
        tx.commit().await.map_err(db_error)?;
        message_from_row(&row)
    }

    async fn delete(
        &self,
        caller: UserId,
        conversation: ConversationId,
        message: MessageId,
    ) -> Result<Message, AppError> {
        let raw_now = OffsetDateTime::now_utc();
        let now = raw_now
            .replace_nanosecond(raw_now.nanosecond() / 1000 * 1000)
            .unwrap_or(raw_now);
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let updated = sqlx::query("UPDATE messages m SET content = '[deleted]', deleted_at = $4 FROM conversation_members cm WHERE m.id = $3 AND m.conversation_id = $2 AND cm.conversation_id = m.conversation_id AND cm.user_id = $1 AND m.deleted_at IS NULL AND (m.sender_id = $1 OR EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = m.conversation_id AND channel_has_permission($1, ch.id, 256))) RETURNING m.id")
            .bind(caller.as_uuid()).bind(conversation.as_uuid()).bind(message.as_uuid()).bind(now)
            .fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(AppError::NotFound)?;
        sqlx::query("DELETE FROM message_mentions WHERE message_id = $1")
            .bind(message.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        let row = sqlx::query("SELECT m.id, m.conversation_id, m.sender_id, m.sequence, m.client_message_id, m.reply_to_message_id, m.content, m.created_at, m.edited_at, m.deleted_at, ARRAY(SELECT mm.user_id FROM message_mentions mm WHERE mm.message_id = m.id ORDER BY mm.user_id) AS mentioned_user_ids FROM messages m WHERE m.id = $1")
            .bind(updated.try_get::<Uuid, _>("id").map_err(db_error)?)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        append_message_update_event(&mut tx, &row, "message.deleted", now).await?;
        tx.commit().await.map_err(db_error)?;
        message_from_row(&row)
    }

    async fn advance_read_cursor(
        &self,
        caller: UserId,
        conversation: ConversationId,
        sequence: i64,
        now: OffsetDateTime,
    ) -> Result<ReadCursor, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let row = sqlx::query("SELECT c.last_sequence FROM conversations c JOIN conversation_members cm ON cm.conversation_id = c.id AND cm.user_id = $2 WHERE c.id = $1 AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = c.id AND NOT channel_has_permission($2, ch.id, 129)) FOR UPDATE OF c")
            .bind(conversation.as_uuid())
            .bind(caller.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?
            .ok_or(AppError::NotFound)?;
        let last_sequence: i64 = row.try_get("last_sequence").map_err(db_error)?;
        if sequence < 0 || sequence > last_sequence {
            return Err(AppError::InvalidReadCursor);
        }
        let row = sqlx::query("INSERT INTO read_cursors (conversation_id, user_id, sequence, updated_at) VALUES ($1, $2, $3, $4) ON CONFLICT (conversation_id, user_id) DO UPDATE SET sequence = GREATEST(read_cursors.sequence, EXCLUDED.sequence), updated_at = CASE WHEN EXCLUDED.sequence > read_cursors.sequence THEN EXCLUDED.updated_at ELSE read_cursors.updated_at END RETURNING user_id, sequence, updated_at")
            .bind(conversation.as_uuid())
            .bind(caller.as_uuid())
            .bind(sequence)
            .bind(now)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(ReadCursor {
            user_id: UserId::from_uuid(row.try_get("user_id").map_err(db_error)?),
            sequence: row.try_get("sequence").map_err(db_error)?,
            updated_at: row.try_get("updated_at").map_err(db_error)?,
        })
    }

    async fn read_cursors_for_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
    ) -> Result<Vec<ReadCursor>, AppError> {
        let rows = sqlx::query("SELECT cm.user_id, COALESCE(rc.sequence, 0)::bigint AS sequence, COALESCE(rc.updated_at, c.created_at) AS updated_at FROM conversations c JOIN conversation_members cm ON cm.conversation_id = c.id LEFT JOIN read_cursors rc ON rc.conversation_id = cm.conversation_id AND rc.user_id = cm.user_id WHERE c.id = $1 AND EXISTS (SELECT 1 FROM conversation_members own WHERE own.conversation_id = c.id AND own.user_id = $2) AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = c.id AND NOT channel_has_permission($2, ch.id, 129)) AND (NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = c.id) OR channel_has_permission(cm.user_id, (SELECT ch.id FROM community_channels ch WHERE ch.conversation_id = c.id), 129)) ORDER BY cm.user_id")
            .bind(conversation.as_uuid())
            .bind(caller.as_uuid())
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        if rows.is_empty() {
            return Err(AppError::NotFound);
        }
        rows.iter()
            .map(|row| {
                Ok(ReadCursor {
                    user_id: UserId::from_uuid(row.try_get("user_id").map_err(db_error)?),
                    sequence: row.try_get("sequence").map_err(db_error)?,
                    updated_at: row.try_get("updated_at").map_err(db_error)?,
                })
            })
            .collect()
    }

    async fn list_reactions(
        &self,
        caller: UserId,
        conversation: ConversationId,
        messages: &[MessageId],
    ) -> Result<Vec<MessageReaction>, AppError> {
        let allowed: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM conversation_members cm WHERE cm.conversation_id = $1 AND cm.user_id = $2 AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = cm.conversation_id AND NOT channel_has_permission($2, ch.id, 129)))")
            .bind(conversation.as_uuid())
            .bind(caller.as_uuid())
            .fetch_one(&self.pool)
            .await
            .map_err(db_error)?;
        if !allowed {
            return Err(AppError::NotFound);
        }
        if messages.is_empty() {
            return Ok(Vec::new());
        }
        let message_ids = messages
            .iter()
            .map(|message| message.as_uuid())
            .collect::<Vec<_>>();
        let rows = sqlx::query("SELECT r.message_id, r.emoji, COUNT(*)::bigint AS count, BOOL_OR(r.user_id = $1) AS reacted_by_me FROM message_reactions r JOIN messages m ON m.id = r.message_id AND m.conversation_id = r.conversation_id WHERE r.conversation_id = $2 AND r.message_id = ANY($3) AND m.deleted_at IS NULL AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = m.conversation_id AND NOT channel_has_permission($1, ch.id, 129)) GROUP BY r.message_id, r.emoji ORDER BY r.message_id, r.emoji")
            .bind(caller.as_uuid())
            .bind(conversation.as_uuid())
            .bind(message_ids)
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        rows.iter()
            .map(|row| {
                Ok(MessageReaction {
                    message_id: MessageId::from_uuid(row.try_get("message_id").map_err(db_error)?),
                    emoji: row.try_get("emoji").map_err(db_error)?,
                    count: row.try_get("count").map_err(db_error)?,
                    reacted_by_me: row.try_get("reacted_by_me").map_err(db_error)?,
                })
            })
            .collect()
    }

    async fn set_reaction(
        &self,
        caller: UserId,
        conversation: ConversationId,
        message: MessageId,
        emoji: ReactionEmoji,
        added: bool,
        now: OffsetDateTime,
    ) -> Result<ReactionMutation, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let row = sqlx::query("SELECT m.sequence FROM messages m JOIN conversation_members cm ON cm.conversation_id = m.conversation_id AND cm.user_id = $1 WHERE m.id = $2 AND m.conversation_id = $3 AND m.deleted_at IS NULL AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = m.conversation_id AND NOT channel_has_permission($1, ch.id, 129)) FOR UPDATE OF m")
            .bind(caller.as_uuid())
            .bind(message.as_uuid())
            .bind(conversation.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?
            .ok_or(AppError::NotFound)?;
        let sequence: i64 = row.try_get("sequence").map_err(db_error)?;
        let changed = if added {
            sqlx::query("INSERT INTO message_reactions (conversation_id, message_id, user_id, emoji, created_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (message_id, user_id, emoji) DO NOTHING")
                .bind(conversation.as_uuid())
                .bind(message.as_uuid())
                .bind(caller.as_uuid())
                .bind(emoji.as_str())
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(db_error)?
                .rows_affected() > 0
        } else {
            sqlx::query("DELETE FROM message_reactions WHERE conversation_id = $1 AND message_id = $2 AND user_id = $3 AND emoji = $4")
                .bind(conversation.as_uuid())
                .bind(message.as_uuid())
                .bind(caller.as_uuid())
                .bind(emoji.as_str())
                .execute(&mut *tx)
                .await
                .map_err(db_error)?
                .rows_affected() > 0
        };
        let rows = sqlx::query("SELECT message_id, emoji, COUNT(*)::bigint AS count, BOOL_OR(user_id = $1) AS reacted_by_me FROM message_reactions WHERE conversation_id = $2 AND message_id = $3 GROUP BY message_id, emoji ORDER BY emoji")
            .bind(caller.as_uuid())
            .bind(conversation.as_uuid())
            .bind(message.as_uuid())
            .fetch_all(&mut *tx)
            .await
            .map_err(db_error)?;
        let reactions = rows
            .iter()
            .map(|row| {
                Ok(MessageReaction {
                    message_id: MessageId::from_uuid(row.try_get("message_id").map_err(db_error)?),
                    emoji: row.try_get("emoji").map_err(db_error)?,
                    count: row.try_get("count").map_err(db_error)?,
                    reacted_by_me: row.try_get("reacted_by_me").map_err(db_error)?,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        tx.commit().await.map_err(db_error)?;
        Ok(ReactionMutation {
            sequence,
            changed,
            reactions,
        })
    }

    async fn advance_delivery_cursor(
        &self,
        caller: UserId,
        conversation: ConversationId,
        sequence: i64,
        now: OffsetDateTime,
    ) -> Result<DeliveryCursor, AppError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let row = sqlx::query("SELECT c.last_sequence FROM conversations c JOIN conversation_members cm ON cm.conversation_id = c.id AND cm.user_id = $2 WHERE c.id = $1 AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = c.id AND NOT channel_has_permission($2, ch.id, 129)) FOR UPDATE OF c")
            .bind(conversation.as_uuid())
            .bind(caller.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?
            .ok_or(AppError::NotFound)?;
        let last_sequence: i64 = row.try_get("last_sequence").map_err(db_error)?;
        if sequence > last_sequence {
            return Err(AppError::InvalidDeliveryCursor);
        }
        let inserted_or_advanced = sqlx::query("INSERT INTO delivery_cursors (conversation_id, user_id, sequence, updated_at) VALUES ($1, $2, $3, $4) ON CONFLICT (conversation_id, user_id) DO UPDATE SET sequence = EXCLUDED.sequence, updated_at = EXCLUDED.updated_at WHERE delivery_cursors.sequence < EXCLUDED.sequence RETURNING user_id, sequence, updated_at")
            .bind(conversation.as_uuid())
            .bind(caller.as_uuid())
            .bind(sequence)
            .bind(now)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_error)?;
        let advanced = inserted_or_advanced.is_some() && sequence > 0;
        let row = match inserted_or_advanced {
            Some(row) => row,
            None => sqlx::query("SELECT user_id, sequence, updated_at FROM delivery_cursors WHERE conversation_id = $1 AND user_id = $2")
                .bind(conversation.as_uuid())
                .bind(caller.as_uuid())
                .fetch_one(&mut *tx)
                .await
                .map_err(db_error)?,
        };
        tx.commit().await.map_err(db_error)?;
        Ok(DeliveryCursor {
            user_id: UserId::from_uuid(row.try_get("user_id").map_err(db_error)?),
            sequence: row.try_get("sequence").map_err(db_error)?,
            updated_at: row.try_get("updated_at").map_err(db_error)?,
            advanced,
        })
    }

    async fn delivery_cursors_for_member(
        &self,
        caller: UserId,
        conversation: ConversationId,
    ) -> Result<Vec<DeliveryCursor>, AppError> {
        let rows = sqlx::query("SELECT cm.user_id, COALESCE(dc.sequence, 0)::bigint AS sequence, COALESCE(dc.updated_at, c.created_at) AS updated_at FROM conversations c JOIN conversation_members cm ON cm.conversation_id = c.id LEFT JOIN delivery_cursors dc ON dc.conversation_id = cm.conversation_id AND dc.user_id = cm.user_id WHERE c.id = $1 AND EXISTS (SELECT 1 FROM conversation_members own WHERE own.conversation_id = c.id AND own.user_id = $2) AND NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = c.id AND NOT channel_has_permission($2, ch.id, 129)) AND (NOT EXISTS (SELECT 1 FROM community_channels ch WHERE ch.conversation_id = c.id) OR channel_has_permission(cm.user_id, (SELECT ch.id FROM community_channels ch WHERE ch.conversation_id = c.id), 129)) ORDER BY cm.user_id")
            .bind(conversation.as_uuid())
            .bind(caller.as_uuid())
            .fetch_all(&self.pool)
            .await
            .map_err(db_error)?;
        if rows.is_empty() {
            return Err(AppError::NotFound);
        }
        rows.iter()
            .map(|row| {
                Ok(DeliveryCursor {
                    user_id: UserId::from_uuid(row.try_get("user_id").map_err(db_error)?),
                    sequence: row.try_get("sequence").map_err(db_error)?,
                    updated_at: row.try_get("updated_at").map_err(db_error)?,
                    advanced: false,
                })
            })
            .collect()
    }
}

#[derive(Clone)]
pub struct PgOutboxRepository {
    pool: PgPool,
}

impl PgOutboxRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl rustchat_application::ports::OutboxRepository for PgOutboxRepository {
    async fn message_by_id(&self, message_id: MessageId) -> Result<Option<Message>, AppError> {
        let row = sqlx::query("SELECT m.id, m.conversation_id, m.sender_id, m.sequence, m.client_message_id, m.reply_to_message_id, m.content, m.created_at, m.edited_at, m.deleted_at, ARRAY(SELECT mm.user_id FROM message_mentions mm WHERE mm.message_id = m.id ORDER BY mm.user_id) AS mentioned_user_ids FROM messages m WHERE m.id = $1")
            .bind(message_id.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(db_error)?;
        row.as_ref().map(message_from_row).transpose()
    }

    async fn pending_messages(&self, limit: usize) -> Result<Vec<Message>, AppError> {
        let limit = i64::try_from(limit.clamp(1, 500)).unwrap_or(500);
        // Claim first, then read: SKIP LOCKED gives concurrent instances disjoint events,
        // and the lease lets another instance take over if this one dies mid-dispatch.
        let claimed: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE outbox_events SET claimed_until = now() + interval '30 seconds' \
             WHERE id IN (SELECT id FROM outbox_events \
                          WHERE published_at IS NULL AND (claimed_until IS NULL OR claimed_until <= now()) \
                          ORDER BY created_at, id LIMIT $1 FOR UPDATE SKIP LOCKED) \
             RETURNING id",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        if claimed.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query("SELECT m.id, m.conversation_id, m.sender_id, m.sequence, m.client_message_id, m.reply_to_message_id, m.content, m.created_at, m.edited_at, m.deleted_at, ARRAY(SELECT mm.user_id FROM message_mentions mm WHERE mm.message_id = m.id ORDER BY mm.user_id) AS mentioned_user_ids FROM outbox_events o JOIN messages m ON m.id = COALESCE(NULLIF(o.payload->>'message_id', '')::uuid, NULLIF(o.payload->'payload'->>'message_id', '')::uuid, o.id) WHERE o.id = ANY($1) ORDER BY o.created_at, o.id")
            .bind(&claimed).fetch_all(&self.pool).await.map_err(db_error)?;
        rows.iter().map(message_from_row).collect()
    }

    async fn mark_message_published(&self, message_id: MessageId) -> Result<(), AppError> {
        sqlx::query("UPDATE outbox_events SET published_at = $2 WHERE published_at IS NULL AND (id = $1 OR payload->>'message_id' = $1::text OR payload->'payload'->>'message_id' = $1::text)")
        .bind(message_id.as_uuid())
        .bind(OffsetDateTime::now_utc())
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration as TimeDuration;

    fn now() -> OffsetDateTime {
        // Truncate to microseconds, the precision PostgreSQL stores.
        let t = OffsetDateTime::now_utc();
        t.replace_nanosecond((t.nanosecond() / 1000) * 1000)
            .unwrap()
    }

    async fn add_user(users: &PgUserRepository, name: &str) -> NewUser {
        let user = NewUser {
            id: UserId::new(),
            username: Username::parse(name).unwrap(),
            email: None,
            password_hash: "$argon2id$placeholder".into(),
            created_at: now(),
        };
        let outcome = users
            .create(NewUser {
                id: user.id,
                username: user.username.clone(),
                email: user.email.clone(),
                password_hash: user.password_hash.clone(),
                created_at: user.created_at,
            })
            .await
            .unwrap();
        assert_eq!(outcome, CreateUserOutcome::Created);
        user
    }

    fn new_session(user_id: UserId, byte: u8, ttl: TimeDuration) -> NewSession {
        let created_at = now();
        NewSession {
            session: Session {
                id: SessionId::new(),
                user_id,
                created_at,
                expires_at: created_at + ttl,
            },
            token_digest: TokenDigest([byte; 32]),
        }
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn create_and_find_user(pool: PgPool) {
        let users = PgUserRepository::new(pool);
        let created = add_user(&users, "alice").await;
        let found = users
            .find_credentials(&Username::parse("ALICE").unwrap())
            .await
            .unwrap()
            .expect("user should exist");
        assert_eq!(found.user.id, created.id);
        assert_eq!(found.password_hash, created.password_hash);
        assert!(
            users
                .find_credentials(&Username::parse("nobody").unwrap())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn duplicate_username_is_rejected_by_the_database(pool: PgPool) {
        let users = PgUserRepository::new(pool);
        add_user(&users, "alice").await;
        let outcome = users
            .create(NewUser {
                id: UserId::new(),
                username: Username::parse("Alice").unwrap(),
                email: None,
                password_hash: "x".into(),
                created_at: now(),
            })
            .await
            .unwrap();
        assert_eq!(outcome, CreateUserOutcome::UsernameTaken);
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn schema_rejects_unnormalized_usernames(pool: PgPool) {
        let result = sqlx::query(
            "INSERT INTO users (id, username, password_hash, created_at) \
             VALUES ($1, 'Mixed_Case', 'x', now())",
        )
        .bind(Uuid::now_v7())
        .execute(&pool)
        .await;
        assert!(result.is_err(), "CHECK constraint must reject upper case");
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn session_lifecycle(pool: PgPool) {
        let users = PgUserRepository::new(pool.clone());
        let sessions = PgSessionRepository::new(pool);
        let alice = add_user(&users, "alice").await;
        let new = new_session(alice.id, 7, TimeDuration::hours(1));
        let (id, digest) = (new.session.id, new.token_digest);
        sessions.insert(new).await.unwrap();

        let found = sessions
            .find_active(&digest, now())
            .await
            .unwrap()
            .expect("active session");
        assert_eq!(found.session.id, id);
        assert_eq!(found.user.id, alice.id);
        assert_eq!(found.user.username.as_str(), "alice");

        assert!(sessions.revoke(alice.id, id, now()).await.unwrap());
        assert!(
            sessions
                .find_active(&digest, now())
                .await
                .unwrap()
                .is_none()
        );
        assert!(!sessions.revoke(alice.id, id, now()).await.unwrap());
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn expired_sessions_are_not_active(pool: PgPool) {
        let users = PgUserRepository::new(pool.clone());
        let sessions = PgSessionRepository::new(pool);
        let alice = add_user(&users, "alice").await;
        let new = new_session(alice.id, 9, TimeDuration::hours(1));
        let (digest, expires_at) = (new.token_digest, new.session.expires_at);
        sessions.insert(new).await.unwrap();

        assert!(
            sessions
                .find_active(&digest, expires_at - TimeDuration::seconds(1))
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            sessions
                .find_active(&digest, expires_at)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            sessions
                .list_active(alice.id, expires_at)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn revoke_is_scoped_to_the_owner(pool: PgPool) {
        let users = PgUserRepository::new(pool.clone());
        let sessions = PgSessionRepository::new(pool);
        let alice = add_user(&users, "alice").await;
        let bobby = add_user(&users, "bobby").await;
        let new = new_session(alice.id, 3, TimeDuration::hours(1));
        let (id, digest) = (new.session.id, new.token_digest);
        sessions.insert(new).await.unwrap();

        assert!(!sessions.revoke(bobby.id, id, now()).await.unwrap());
        assert!(
            sessions
                .find_active(&digest, now())
                .await
                .unwrap()
                .is_some()
        );
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn list_active_only_returns_own_live_sessions(pool: PgPool) {
        let users = PgUserRepository::new(pool.clone());
        let sessions = PgSessionRepository::new(pool);
        let alice = add_user(&users, "alice").await;
        let bobby = add_user(&users, "bobby").await;
        let revoked = new_session(alice.id, 1, TimeDuration::hours(1));
        let revoked_id = revoked.session.id;
        sessions.insert(revoked).await.unwrap();
        sessions
            .insert(new_session(alice.id, 2, TimeDuration::hours(1)))
            .await
            .unwrap();
        sessions
            .insert(new_session(bobby.id, 4, TimeDuration::hours(1)))
            .await
            .unwrap();
        sessions.revoke(alice.id, revoked_id, now()).await.unwrap();

        let listed = sessions.list_active(alice.id, now()).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].user_id, alice.id);
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn duplicate_token_digest_is_rejected(pool: PgPool) {
        let users = PgUserRepository::new(pool.clone());
        let sessions = PgSessionRepository::new(pool);
        let alice = add_user(&users, "alice").await;
        sessions
            .insert(new_session(alice.id, 5, TimeDuration::hours(1)))
            .await
            .unwrap();
        let again = sessions
            .insert(new_session(alice.id, 5, TimeDuration::hours(1)))
            .await;
        assert!(again.is_err());
    }

    fn sealed(byte: u8) -> EncryptedSecret {
        EncryptedSecret {
            ciphertext: vec![byte; 36],
            nonce: [byte; 12],
        }
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn totp_enrolment_confirms_once_and_never_replaces_a_confirmed_secret(pool: PgPool) {
        let users = PgUserRepository::new(pool.clone());
        let mfa = PgMfaRepository::new(pool);
        let alice = add_user(&users, "alice").await;

        assert!(mfa.totp(alice.id).await.unwrap().is_none());
        assert!(mfa.begin_totp(alice.id, &sealed(1), now()).await.unwrap());
        // A pending secret can be restarted.
        assert!(mfa.begin_totp(alice.id, &sealed(2), now()).await.unwrap());
        let pending = mfa.totp(alice.id).await.unwrap().unwrap();
        assert_eq!(pending.secret, sealed(2));
        assert!(!pending.confirmed);
        // An unconfirmed secret cannot be used for a step.
        assert!(!mfa.use_totp_step(alice.id, 10).await.unwrap());

        let digests = [TokenDigest([1; 32]), TokenDigest([2; 32])];
        assert!(
            mfa.confirm_totp(alice.id, 10, &digests, now())
                .await
                .unwrap()
        );
        assert!(
            !mfa.confirm_totp(alice.id, 11, &digests, now())
                .await
                .unwrap()
        );
        assert!(!mfa.begin_totp(alice.id, &sealed(3), now()).await.unwrap());
        let confirmed = mfa.totp(alice.id).await.unwrap().unwrap();
        assert!(confirmed.confirmed);
        assert_eq!(
            confirmed.secret,
            sealed(2),
            "the confirmed secret is untouched"
        );
        assert_eq!(confirmed.last_used_step, Some(10));
        assert_eq!(mfa.recovery_codes_remaining(alice.id).await.unwrap(), 2);
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn a_totp_step_and_a_recovery_code_are_accepted_once(pool: PgPool) {
        let users = PgUserRepository::new(pool.clone());
        let mfa = PgMfaRepository::new(pool);
        let alice = add_user(&users, "alice").await;
        let bobby = add_user(&users, "bobby").await;
        mfa.begin_totp(alice.id, &sealed(1), now()).await.unwrap();
        mfa.confirm_totp(alice.id, 10, &[TokenDigest([9; 32])], now())
            .await
            .unwrap();

        for (step, accepted) in [(10, false), (9, false), (11, true), (11, false), (12, true)] {
            assert_eq!(
                mfa.use_totp_step(alice.id, step).await.unwrap(),
                accepted,
                "step {step}"
            );
        }
        assert!(
            !mfa.use_totp_step(bobby.id, 50).await.unwrap(),
            "no secret, no step"
        );

        assert!(
            !mfa.use_recovery_code(bobby.id, &TokenDigest([9; 32]), now())
                .await
                .unwrap()
        );
        assert!(
            mfa.use_recovery_code(alice.id, &TokenDigest([9; 32]), now())
                .await
                .unwrap()
        );
        assert!(
            !mfa.use_recovery_code(alice.id, &TokenDigest([9; 32]), now())
                .await
                .unwrap()
        );
        assert_eq!(mfa.recovery_codes_remaining(alice.id).await.unwrap(), 0);

        mfa.replace_recovery_codes(alice.id, &[TokenDigest([5; 32])], now())
            .await
            .unwrap();
        assert_eq!(mfa.recovery_codes_remaining(alice.id).await.unwrap(), 1);
        mfa.disable_totp(alice.id).await.unwrap();
        assert!(mfa.totp(alice.id).await.unwrap().is_none());
        assert_eq!(mfa.recovery_codes_remaining(alice.id).await.unwrap(), 0);
    }

    #[sqlx::test(migrator = "crate::MIGRATOR")]
    async fn challenges_allow_limited_attempts_expire_and_need_an_active_user(pool: PgPool) {
        let users = PgUserRepository::new(pool.clone());
        let mfa = PgMfaRepository::new(pool);
        let alice = add_user(&users, "alice").await;
        let digest = TokenDigest([3; 32]);
        let start = now();
        mfa.create_challenge(alice.id, digest, start, start + TimeDuration::minutes(5))
            .await
            .unwrap();

        for attempt in 1..=3 {
            let user = mfa.attempt_challenge(&digest, start, 3).await.unwrap();
            assert_eq!(
                user.map(|u| u.username.as_str().to_owned()).as_deref(),
                Some("alice"),
                "attempt {attempt}"
            );
        }
        assert!(
            mfa.attempt_challenge(&digest, start, 3)
                .await
                .unwrap()
                .is_none(),
            "attempts spent"
        );

        let fresh = TokenDigest([4; 32]);
        mfa.create_challenge(alice.id, fresh, start, start + TimeDuration::minutes(5))
            .await
            .unwrap();
        assert!(
            mfa.attempt_challenge(&fresh, start + TimeDuration::minutes(5), 3)
                .await
                .unwrap()
                .is_none(),
            "expired"
        );
        assert!(
            mfa.attempt_challenge(&TokenDigest([8; 32]), start, 3)
                .await
                .unwrap()
                .is_none()
        );

        // A suspended user's challenge no longer works.
        let user_repo = PgUserRepository::new(mfa.pool.clone());
        sqlx::query("UPDATE users SET suspended_at = now() WHERE id = $1")
            .bind(alice.id.as_uuid())
            .execute(&user_repo.pool)
            .await
            .unwrap();
        mfa.create_challenge(
            alice.id,
            TokenDigest([5; 32]),
            start,
            start + TimeDuration::minutes(5),
        )
        .await
        .unwrap();
        assert!(
            mfa.attempt_challenge(&TokenDigest([5; 32]), start, 3)
                .await
                .unwrap()
                .is_none()
        );

        mfa.delete_challenge(&TokenDigest([5; 32])).await.unwrap();
    }
}
