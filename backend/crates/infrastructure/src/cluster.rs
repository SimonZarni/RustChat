//! Coordination between server instances that share one PostgreSQL database:
//! an event bus on `LISTEN/NOTIFY` and a shared presence registry.

use std::{collections::HashSet, sync::Arc, time::Duration};

use async_trait::async_trait;
use rustchat_application::{
    error::AppError,
    ports::{EventBus, PresenceStore},
};
use rustchat_domain::identity::UserId;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, postgres::PgListener};
use tokio::sync::mpsc;
use uuid::Uuid;

/// The NOTIFY channel every instance listens on.
const CHANNEL: &str = "rustchat_events";
/// PostgreSQL rejects NOTIFY payloads of 8000 bytes or more; leave headroom.
const MAX_PAYLOAD_BYTES: usize = 7_500;
/// Events waiting to be published; beyond this new ones are dropped (they are hints).
const QUEUE_CAPACITY: usize = 1024;
/// A user whose instance has not refreshed for this long is no longer online.
const PRESENCE_TTL_SECONDS: i32 = 60;

#[derive(Serialize, Deserialize)]
struct Envelope {
    /// The publishing instance, so it can ignore its own events.
    o: Uuid,
    t: String,
    p: serde_json::Value,
}

/// Invoked on the listening task for each event another instance published.
pub type RemoteHandler = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;
/// Invoked when notifications may have been missed (the listening connection was lost).
pub type GapHandler = Arc<dyn Fn() + Send + Sync>;

/// An [`EventBus`] over PostgreSQL `NOTIFY`. Publishing queues the event for a
/// background task (bounded, never blocking the request); a second task
/// listens and hands other instances' events to `on_remote`.
pub struct PgEventBus {
    instance: Uuid,
    queue: mpsc::Sender<String>,
}

impl PgEventBus {
    /// Spawn the publisher and listener tasks. They run until the runtime stops.
    pub fn start(
        pool: PgPool,
        instance: Uuid,
        on_remote: RemoteHandler,
        on_gap: GapHandler,
    ) -> Arc<Self> {
        let (queue, mut receiver) = mpsc::channel::<String>(QUEUE_CAPACITY);

        let publisher_pool = pool.clone();
        tokio::spawn(async move {
            while let Some(payload) = receiver.recv().await {
                for attempt in 1..=3 {
                    match sqlx::query("SELECT pg_notify($1, $2)")
                        .bind(CHANNEL)
                        .bind(&payload)
                        .execute(&publisher_pool)
                        .await
                    {
                        Ok(_) => break,
                        Err(error) if attempt == 3 => {
                            tracing::warn!(%error, "dropping a realtime event the database would not accept");
                        }
                        Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
                    }
                }
            }
        });

        tokio::spawn(async move {
            loop {
                match PgListener::connect_with(&pool).await {
                    Ok(mut listener) => {
                        if let Err(error) = listener.listen(CHANNEL).await {
                            tracing::warn!(%error, "could not subscribe to realtime events");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                        loop {
                            match listener.try_recv().await {
                                Ok(Some(notification)) => {
                                    match serde_json::from_str::<Envelope>(notification.payload()) {
                                        Ok(envelope) if envelope.o != instance => {
                                            on_remote(&envelope.t, envelope.p);
                                        }
                                        Ok(_) => {}
                                        Err(error) => {
                                            tracing::warn!(%error, "ignoring a malformed realtime event")
                                        }
                                    }
                                }
                                // The connection was lost and re-established: anything sent in
                                // between is gone, so tell clients to resynchronize.
                                Ok(None) => on_gap(),
                                Err(error) => {
                                    tracing::warn!(%error, "realtime event listener failed; reconnecting");
                                    break;
                                }
                            }
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "could not connect the realtime event listener")
                    }
                }
                on_gap();
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        Arc::new(Self { instance, queue })
    }
}

impl EventBus for PgEventBus {
    fn publish(&self, topic: &str, payload: serde_json::Value) {
        let envelope = Envelope {
            o: self.instance,
            t: topic.to_owned(),
            p: payload,
        };
        let Ok(text) = serde_json::to_string(&envelope) else {
            return;
        };
        if text.len() > MAX_PAYLOAD_BYTES {
            tracing::warn!(
                topic,
                bytes = text.len(),
                "realtime event too large to share between instances"
            );
            return;
        }
        if self.queue.try_send(text).is_err() {
            tracing::warn!(
                topic,
                "realtime event queue full; event not shared between instances"
            );
        }
    }
}

/// Presence shared through the `presence_connections` table.
pub struct PgPresenceStore {
    pool: PgPool,
    instance: Uuid,
}

impl PgPresenceStore {
    pub fn new(pool: PgPool, instance: Uuid) -> Self {
        Self { pool, instance }
    }
}

fn db(error: sqlx::Error) -> AppError {
    AppError::internal(error)
}

#[async_trait]
impl PresenceStore for PgPresenceStore {
    async fn connect(&self, user: UserId) -> Result<bool, AppError> {
        // Register this instance first, then ask whether someone connected earlier is still
        // live. Of two instances connecting at once exactly one is "earlier", so the user is
        // announced online once and never zero times.
        sqlx::query(
            "INSERT INTO presence_connections (user_id, instance_id, connected_at, refreshed_at) \
             VALUES ($1, $2, now(), now()) \
             ON CONFLICT (user_id, instance_id) DO UPDATE SET refreshed_at = now()",
        )
        .bind(user.as_uuid())
        .bind(self.instance)
        .execute(&self.pool)
        .await
        .map_err(db)?;
        let first: bool = sqlx::query_scalar(
            "SELECT NOT EXISTS ( \
                SELECT 1 FROM presence_connections other, presence_connections mine \
                WHERE other.user_id = $1 AND mine.user_id = $1 AND mine.instance_id = $2 \
                  AND other.instance_id <> $2 \
                  AND other.refreshed_at > now() - make_interval(secs => $3) \
                  AND (other.connected_at, other.instance_id) < (mine.connected_at, mine.instance_id))",
        )
        .bind(user.as_uuid())
        .bind(self.instance)
        .bind(f64::from(PRESENCE_TTL_SECONDS))
        .fetch_one(&self.pool)
        .await
        .map_err(db)?;
        Ok(first)
    }

    async fn disconnect(&self, user: UserId) -> Result<bool, AppError> {
        // Remove this instance first, then see whether anyone is left; of two instances
        // closing at once both may announce offline, which is harmless.
        sqlx::query("DELETE FROM presence_connections WHERE user_id = $1 AND instance_id = $2")
            .bind(user.as_uuid())
            .bind(self.instance)
            .execute(&self.pool)
            .await
            .map_err(db)?;
        let nobody_left: bool = sqlx::query_scalar(
            "SELECT NOT EXISTS (SELECT 1 FROM presence_connections \
             WHERE user_id = $1 AND refreshed_at > now() - make_interval(secs => $2))",
        )
        .bind(user.as_uuid())
        .bind(f64::from(PRESENCE_TTL_SECONDS))
        .fetch_one(&self.pool)
        .await
        .map_err(db)?;
        Ok(nobody_left)
    }

    async fn online(&self, users: &[UserId]) -> Result<HashSet<UserId>, AppError> {
        if users.is_empty() {
            return Ok(HashSet::new());
        }
        let ids: Vec<Uuid> = users.iter().map(|user| user.as_uuid()).collect();
        let rows: Vec<Uuid> = sqlx::query_scalar(
            "SELECT DISTINCT user_id FROM presence_connections \
             WHERE user_id = ANY($1) AND refreshed_at > now() - make_interval(secs => $2)",
        )
        .bind(&ids)
        .bind(f64::from(PRESENCE_TTL_SECONDS))
        .fetch_all(&self.pool)
        .await
        .map_err(db)?;
        Ok(rows.into_iter().map(UserId::from_uuid).collect())
    }

    async fn refresh(&self) -> Result<(), AppError> {
        sqlx::query("UPDATE presence_connections SET refreshed_at = now() WHERE instance_id = $1")
            .bind(self.instance)
            .execute(&self.pool)
            .await
            .map_err(db)?;
        // Rows of instances that stopped refreshing long ago are garbage.
        sqlx::query(
            "DELETE FROM presence_connections WHERE refreshed_at < now() - make_interval(secs => $1)",
        )
        .bind(f64::from(PRESENCE_TTL_SECONDS) * 10.0)
        .execute(&self.pool)
        .await
        .map_err(db)?;
        Ok(())
    }
}
