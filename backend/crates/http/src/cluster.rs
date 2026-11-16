//! Running several server instances against one database.
//!
//! Each instance serves its own WebSocket clients from process-local buses.
//! This module keeps those buses in step: events published locally are also
//! handed to an [`EventBus`] for the other instances, events other instances
//! publish are replayed into the local buses, and the outbox dispatcher and
//! presence registry coordinate through the database.

use std::{sync::Arc, time::Duration};

use rustchat_application::ports::{EventBus, OutboxRepository, PresenceStore};
use rustchat_domain::{identity::MessageId, message::Message};
use serde::Deserialize;
use tokio::{sync::broadcast, task::JoinHandle};
use uuid::Uuid;

use crate::{
    AppEvents, CommunityEvent, DeliveryCursorEvent, PresenceEvent, ReactionEvent, ReadCursorEvent,
    TypingEvent,
};

pub(crate) const TOPIC_MESSAGE: &str = "message";
pub(crate) const TOPIC_TYPING: &str = "typing";
pub(crate) const TOPIC_READ_CURSOR: &str = "read_cursor";
pub(crate) const TOPIC_DELIVERY_CURSOR: &str = "delivery_cursor";
pub(crate) const TOPIC_REACTION: &str = "reaction";
pub(crate) const TOPIC_PRESENCE: &str = "presence";
pub(crate) const TOPIC_COMMUNITY: &str = "community";

#[derive(Deserialize)]
struct MessageRef {
    id: Uuid,
}

#[derive(Deserialize)]
struct WireCommunityEvent {
    community_id: Uuid,
    kind: String,
    #[serde(with = "time::serde::rfc3339")]
    occurred_at: time::OffsetDateTime,
}

/// Replays events published by other instances; see [`remote_event_handler`].
pub type RemoteEventHandler = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// A handler for events other instances published: it replays them into this
/// instance's local buses so the WebSocket clients connected here receive them.
/// Message events carry only the message id (a message can exceed the size a
/// notification may carry), so the current message is read from the database.
pub fn remote_event_handler(
    events: AppEvents,
    outbox: Arc<dyn OutboxRepository>,
) -> RemoteEventHandler {
    Arc::new(move |topic, payload| match topic {
        TOPIC_MESSAGE => {
            let Ok(reference) = serde_json::from_value::<MessageRef>(payload) else {
                return;
            };
            let (outbox, sender) = (outbox.clone(), events.messages.clone());
            tokio::spawn(async move {
                match outbox
                    .message_by_id(MessageId::from_uuid(reference.id))
                    .await
                {
                    Ok(Some(message)) => {
                        let _ = sender.send(message);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::warn!(%error, "could not load a message announced by another instance")
                    }
                }
            });
        }
        TOPIC_TYPING => forward::<TypingEvent>(payload, &events.typing),
        TOPIC_READ_CURSOR => forward::<ReadCursorEvent>(payload, &events.read_cursors),
        TOPIC_DELIVERY_CURSOR => forward::<DeliveryCursorEvent>(payload, &events.delivery_cursors),
        TOPIC_REACTION => forward::<ReactionEvent>(payload, &events.reactions),
        TOPIC_PRESENCE => forward::<PresenceEvent>(payload, &events.presence),
        TOPIC_COMMUNITY => {
            if let Ok(wire) = serde_json::from_value::<WireCommunityEvent>(payload) {
                let kind = match wire.kind.as_str() {
                    "members.updated" => "members.updated",
                    "channels.updated" => "channels.updated",
                    _ => return,
                };
                let _ = events.community.send(CommunityEvent {
                    community_id: wire.community_id,
                    kind,
                    occurred_at: wire.occurred_at,
                });
            }
        }
        _ => {}
    })
}

fn forward<T: serde::de::DeserializeOwned>(
    payload: serde_json::Value,
    sender: &broadcast::Sender<T>,
) {
    if let Ok(event) = serde_json::from_value::<T>(payload) {
        let _ = sender.send(event);
    }
}

/// Called when notifications from other instances may have been lost (the
/// listening connection dropped). Connected clients are asked to reconnect and
/// resynchronize from durable history.
pub fn gap_handler(resync: broadcast::Sender<()>) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        let _ = resync.send(());
    })
}

/// Dispatch committed outbox events to realtime clients: claim them, hand each
/// message to this instance's clients and to the other instances, then mark it
/// published. Delivery is at least once; clients deduplicate and recover gaps
/// from durable history.
pub fn spawn_outbox_dispatcher(
    outbox: Arc<dyn OutboxRepository>,
    sender: broadcast::Sender<Message>,
    bus: Arc<dyn EventBus>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match outbox.pending_messages(128).await {
                Ok(pending) if pending.is_empty() => {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Ok(pending) => {
                    for message in pending {
                        let id = message.id;
                        // Broadcasts are bounded and process-local; a lagging connection is
                        // closed and recovers through the history API.
                        bus.publish(TOPIC_MESSAGE, serde_json::json!({ "id": id.as_uuid() }));
                        let _ = sender.send(message);
                        if let Err(error) = outbox.mark_message_published(id).await {
                            tracing::error!(%error, "failed to mark outbox message published");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            break;
                        }
                    }
                }
                Err(error) => {
                    tracing::error!(%error, "failed to poll message outbox");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    })
}

/// Keep this instance's presence rows fresh so its users stay online.
pub fn spawn_presence_heartbeat(store: Arc<dyn PresenceStore>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(20));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(error) = store.refresh().await {
                tracing::warn!(%error, "could not refresh presence");
            }
        }
    })
}
