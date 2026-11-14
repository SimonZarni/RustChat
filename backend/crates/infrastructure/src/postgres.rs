//! PostgreSQL implementations of the repository ports.
//!
//! All statements are parameterized; no SQL is built from user input.

use std::time::Duration;

use async_trait::async_trait;
use rustchat_application::{
    error::AppError,
    ports::{
        ActiveSession, CreateUserOutcome, NewSession, NewUser, SessionRepository, TokenDigest,
        UserCredentials, UserRepository,
    },
};
use rustchat_domain::identity::{Session, SessionId, User, UserId, Username};
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
            "INSERT INTO users (id, username, password_hash, created_at) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT ON CONSTRAINT users_username_key DO NOTHING",
        )
        .bind(user.id.as_uuid())
        .bind(user.username.as_str())
        .bind(&user.password_hash)
        .bind(user.created_at)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;

        Ok(if result.rows_affected() == 1 {
            CreateUserOutcome::Created
        } else {
            CreateUserOutcome::UsernameTaken
        })
    }

    async fn find_credentials(
        &self,
        username: &Username,
    ) -> Result<Option<UserCredentials>, AppError> {
        let row = sqlx::query(
            "SELECT id, username, password_hash, created_at FROM users WHERE username = $1",
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
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_digest, created_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(new.session.id.as_uuid())
        .bind(new.session.user_id.as_uuid())
        .bind(new.token_digest.0.as_slice())
        .bind(new.session.created_at)
        .bind(new.session.expires_at)
        .execute(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(())
    }

    async fn find_active(
        &self,
        digest: &TokenDigest,
        now: OffsetDateTime,
    ) -> Result<Option<ActiveSession>, AppError> {
        let row = sqlx::query(
            "SELECT s.id, s.user_id, s.created_at, s.expires_at, \
                    u.username, u.created_at AS user_created_at \
             FROM sessions s JOIN users u ON u.id = s.user_id \
             WHERE s.token_digest = $1 AND s.revoked_at IS NULL AND s.expires_at > $2",
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
            Ok(ActiveSession { session, user })
        })
        .transpose()
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
            password_hash: "$argon2id$placeholder".into(),
            created_at: now(),
        };
        let outcome = users
            .create(NewUser {
                id: user.id,
                username: user.username.clone(),
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
}
