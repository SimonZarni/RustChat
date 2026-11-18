//! Session-authenticated WebSocket delivery and privacy-scoped presence.

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use axum::{
    extract::{
        State,
        ws::{CloseFrame, Message as WsMessage, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, header},
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use rustchat_domain::{
    identity::{ConversationId, UserId},
    message::Message as DomainMessage,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ApiError, AppState, auth::AuthUser};

const MAX_CONTROL_BYTES: usize = 16 * 1024;
const MAX_SUBSCRIPTIONS: usize = 100;
const SESSION_REVALIDATE_EVERY: Duration = Duration::from_secs(30);
const HEARTBEAT_EVERY: Duration = Duration::from_secs(25);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(75);

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ClientCommand {
    Subscribe {
        conversation_id: String,
    },
    Unsubscribe {
        conversation_id: String,
    },
    Typing {
        conversation_id: String,
        active: bool,
    },
    Ack {
        conversation_id: String,
        sequence: i64,
    },
}

#[derive(Serialize)]
struct OutboundEvent<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    version: u8,
    event_id: Uuid,
    conversation_id: Uuid,
    sequence: i64,
    #[serde(with = "time::serde::rfc3339")]
    occurred_at: time::OffsetDateTime,
    payload: EventPayload<'a>,
}

#[derive(Serialize)]
struct EventPayload<'a> {
    message_id: Uuid,
    sender_id: Uuid,
    client_message_id: Uuid,
    reply_to_message_id: Option<Uuid>,
    mentioned_user_ids: Vec<Uuid>,
    content: &'a str,
    edited_at: Option<time::OffsetDateTime>,
    deleted_at: Option<time::OffsetDateTime>,
}

#[derive(Serialize)]
struct ControlReply {
    #[serde(rename = "type")]
    kind: &'static str,
    conversation_id: Option<Uuid>,
    code: Option<&'static str>,
}

#[derive(Serialize)]
struct TypingEnvelope {
    #[serde(rename = "type")]
    kind: &'static str,
    version: u8,
    event_id: Uuid,
    conversation_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    occurred_at: time::OffsetDateTime,
    payload: TypingPayload,
}

#[derive(Serialize)]
struct TypingPayload {
    user_id: Uuid,
}

#[derive(Serialize)]
struct ReadCursorEnvelope {
    #[serde(rename = "type")]
    kind: &'static str,
    version: u8,
    event_id: Uuid,
    conversation_id: Uuid,
    sequence: i64,
    #[serde(with = "time::serde::rfc3339")]
    occurred_at: time::OffsetDateTime,
    payload: ReadCursorPayload,
}

#[derive(Serialize)]
struct ReadCursorPayload {
    user_id: Uuid,
    sequence: i64,
}

#[derive(Serialize)]
struct ReactionEnvelope {
    #[serde(rename = "type")]
    kind: &'static str,
    version: u8,
    event_id: Uuid,
    conversation_id: Uuid,
    sequence: i64,
    #[serde(with = "time::serde::rfc3339")]
    occurred_at: time::OffsetDateTime,
    payload: ReactionPayload,
}

#[derive(Serialize)]
struct ReactionPayload {
    message_id: Uuid,
    user_id: Uuid,
    emoji: String,
}

#[derive(Serialize)]
struct DeliveryAckReply {
    #[serde(rename = "type")]
    kind: &'static str,
    conversation_id: Uuid,
    sequence: i64,
}

#[derive(Serialize)]
struct DeliveryCursorEnvelope {
    #[serde(rename = "type")]
    kind: &'static str,
    version: u8,
    event_id: Uuid,
    conversation_id: Uuid,
    sequence: i64,
    #[serde(with = "time::serde::rfc3339")]
    occurred_at: time::OffsetDateTime,
    payload: ReadCursorPayload,
}

#[derive(Serialize)]
struct PresenceEnvelope {
    #[serde(rename = "type")]
    kind: &'static str,
    version: u8,
    event_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    occurred_at: time::OffsetDateTime,
    payload: PresencePayload,
}

#[derive(Serialize)]
struct PresencePayload {
    user_id: Uuid,
    online: Option<bool>,
}

#[derive(Serialize)]
struct CommunityUpdateEnvelope {
    #[serde(rename = "type")]
    kind: &'static str,
    version: u8,
    event_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    occurred_at: time::OffsetDateTime,
    payload: CommunityUpdatePayload,
}

#[derive(Serialize)]
struct CommunityUpdatePayload {
    community_id: Uuid,
}

#[derive(Serialize)]
struct NotificationEnvelope {
    #[serde(rename = "type")]
    kind: &'static str,
    version: u8,
    event_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    occurred_at: time::OffsetDateTime,
    payload: NotificationPayload,
}

#[derive(Serialize)]
struct NotificationPayload {
    kind: String,
}

fn notification_envelope(event: &crate::NotificationEvent) -> NotificationEnvelope {
    NotificationEnvelope {
        kind: "notification.created",
        version: 1,
        event_id: event.event_id,
        occurred_at: event.occurred_at,
        payload: NotificationPayload {
            kind: event.kind.clone(),
        },
    }
}

fn community_update_envelope(event: &crate::CommunityEvent) -> CommunityUpdateEnvelope {
    CommunityUpdateEnvelope {
        kind: event.kind,
        version: 1,
        event_id: Uuid::now_v7(),
        occurred_at: event.occurred_at,
        payload: CommunityUpdatePayload {
            community_id: event.community_id,
        },
    }
}

fn presence_envelope(event: &crate::PresenceEvent) -> PresenceEnvelope {
    PresenceEnvelope {
        kind: "presence.updated",
        version: 1,
        event_id: Uuid::now_v7(),
        occurred_at: event.occurred_at,
        payload: PresencePayload {
            user_id: event.user_id,
            online: event.online,
        },
    }
}

pub async fn upgrade(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: axum_extra::extract::cookie::CookieJar,
    AuthUser(auth): AuthUser,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    if !origin_allowed(&headers, &state.config.allowed_origins) {
        return Err(ApiError::OriginNotAllowed);
    }
    let token = jar
        .get(state.config.cookie_name())
        .map(|cookie| cookie.value().to_owned())
        .ok_or(rustchat_application::error::AppError::Unauthenticated)?;
    let user_id = auth.user.id;
    Ok(ws
        .max_message_size(MAX_CONTROL_BYTES)
        .max_frame_size(MAX_CONTROL_BYTES)
        .on_upgrade(move |socket| run_socket(socket, state, user_id, token)))
}

fn origin_allowed(headers: &HeaderMap, allowed: &[String]) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| allowed.iter().any(|entry| entry == origin))
}

async fn run_socket(socket: WebSocket, state: AppState, user_id: UserId, token: String) {
    let (mut outgoing, mut incoming) = socket.split();
    let mut events = state.events.subscribe();
    let mut subscriptions = HashSet::<ConversationId>::new();
    let mut typing_sent_at = HashMap::<ConversationId, Instant>::new();
    let mut typing_events = state.typing_events.subscribe();
    let mut read_cursor_events = state.read_cursor_events.subscribe();
    let mut reaction_events = state.reaction_events.subscribe();
    let mut delivery_cursor_events = state.delivery_cursor_events.subscribe();
    let mut presence_events = state.presence_events.subscribe();
    let mut community_events = state.community_events.subscribe();
    let mut notification_events = state.notification_events.subscribe();
    let mut resync_events = state.resync_events.subscribe();
    let became_online = state.presence.connect(user_id.as_uuid()).await;
    if became_online
        && state
            .auth
            .privacy(user_id)
            .await
            .is_ok_and(|privacy| privacy.presence_visible)
    {
        state.publish_presence(crate::PresenceEvent {
            user_id: user_id.as_uuid(),
            online: Some(true),
            occurred_at: time::OffsetDateTime::now_utc(),
        });
    }
    let mut heartbeat = tokio::time::interval(HEARTBEAT_EVERY);
    let mut last_pong_at = Instant::now();
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    let mut revalidate = tokio::time::interval(SESSION_REVALIDATE_EVERY);
    revalidate.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    revalidate.tick().await;

    loop {
        tokio::select! {
            frame = incoming.next() => {
                match frame {
                    Some(Ok(WsMessage::Text(text))) => {
                        let Ok(command) = serde_json::from_str::<ClientCommand>(text.as_str()) else {
                            close_socket(&mut outgoing, 1008, "invalid control message").await;
                            break;
                        };
                        match command {
                            ClientCommand::Subscribe { conversation_id } => {
                                let Ok(id) = Uuid::parse_str(&conversation_id).map(ConversationId::from_uuid) else {
                                    if !send_control(&mut outgoing, ControlReply { kind: "error", conversation_id: None, code: Some("BAD_REQUEST") }).await { break; }
                                    continue;
                                };
                                if subscriptions.contains(&id) { continue; }
                                if subscriptions.len() >= MAX_SUBSCRIPTIONS {
                                    if !send_control(&mut outgoing, ControlReply { kind: "error", conversation_id: Some(id.as_uuid()), code: Some("SUBSCRIPTION_LIMIT") }).await { break; }
                                    continue;
                                }
                                if state.conversations.get(user_id, id).await.is_err() {
                                    if !send_control(&mut outgoing, ControlReply { kind: "error", conversation_id: Some(id.as_uuid()), code: Some("NOT_FOUND") }).await { break; }
                                    continue;
                                }
                                subscriptions.insert(id);
                                if !send_control(&mut outgoing, ControlReply { kind: "subscribed", conversation_id: Some(id.as_uuid()), code: None }).await { break; }
                                if let Ok(members) = state.conversations.members_presence(user_id, id).await {
                                    let ids: Vec<Uuid> = members.iter().map(|(member, _)| member.id.as_uuid()).collect();
                                    let online = state.presence.online_among(&ids).await;
                                    for (member, visible) in members {
                                        if member.id == user_id { continue; }
                                        let snapshot = crate::PresenceEvent {
                                            user_id: member.id.as_uuid(),
                                            online: visible.then(|| online.contains(&member.id.as_uuid())),
                                            occurred_at: time::OffsetDateTime::now_utc(),
                                        };
                                        if !send_presence(&mut outgoing, &snapshot).await { break; }
                                    }
                                }
                            }
                            ClientCommand::Unsubscribe { conversation_id } => {
                                let Ok(id) = Uuid::parse_str(&conversation_id).map(ConversationId::from_uuid) else {
                                    if !send_control(&mut outgoing, ControlReply { kind: "error", conversation_id: None, code: Some("BAD_REQUEST") }).await { break; }
                                    continue;
                                };
                                subscriptions.remove(&id);
                                if !send_control(&mut outgoing, ControlReply { kind: "unsubscribed", conversation_id: Some(id.as_uuid()), code: None }).await { break; }
                            }
                            ClientCommand::Typing { conversation_id, active } => {
                                let Ok(id) = Uuid::parse_str(&conversation_id).map(ConversationId::from_uuid) else {
                                    if !send_control(&mut outgoing, ControlReply { kind: "error", conversation_id: None, code: Some("BAD_REQUEST") }).await { break; }
                                    continue;
                                };
                                if !subscriptions.contains(&id) || state.conversations.get(user_id, id).await.is_err() {
                                    if !send_control(&mut outgoing, ControlReply { kind: "error", conversation_id: Some(id.as_uuid()), code: Some("NOT_FOUND") }).await { break; }
                                    continue;
                                }
                                let now = Instant::now();
                                let allowed = !active || typing_sent_at.get(&id).is_none_or(|last| now.duration_since(*last) >= Duration::from_millis(700));
                                if allowed {
                                    typing_sent_at.insert(id, now);
                                    state.publish_typing(crate::TypingEvent {
                                        conversation_id: id.as_uuid(),
                                        user_id: user_id.as_uuid(),
                                        active,
                                        occurred_at: time::OffsetDateTime::now_utc(),
                                    });
                                }
                                if !send_control(&mut outgoing, ControlReply { kind: "typing_ack", conversation_id: Some(id.as_uuid()), code: None }).await { break; }
                            }
                            ClientCommand::Ack { conversation_id, sequence } => {
                                let Ok(id) = Uuid::parse_str(&conversation_id).map(ConversationId::from_uuid) else {
                                    if !send_control(&mut outgoing, ControlReply { kind: "error", conversation_id: None, code: Some("BAD_REQUEST") }).await { break; }
                                    continue;
                                };
                                if !subscriptions.contains(&id) {
                                    if !send_control(&mut outgoing, ControlReply { kind: "error", conversation_id: Some(id.as_uuid()), code: Some("NOT_FOUND") }).await { break; }
                                    continue;
                                }
                                match state.messages.advance_delivery_cursor(user_id, id, sequence).await {
                                    Ok(cursor) => {
                                        if cursor.advanced {
                                            state.publish_delivery_cursor(crate::DeliveryCursorEvent {
                                                conversation_id: id.as_uuid(),
                                                user_id: user_id.as_uuid(),
                                                sequence: cursor.sequence,
                                                occurred_at: cursor.updated_at,
                                            });
                                        }
                                        let reply = DeliveryAckReply { kind: "delivery_ack", conversation_id: id.as_uuid(), sequence: cursor.sequence };
                                        let Ok(text) = serde_json::to_string(&reply) else { break; };
                                        if !send_frame(&mut outgoing, WsMessage::Text(text.into())).await { break; }
                                    }
                                    Err(rustchat_application::error::AppError::NotFound) => {
                                        if !send_control(&mut outgoing, ControlReply { kind: "error", conversation_id: Some(id.as_uuid()), code: Some("NOT_FOUND") }).await { break; }
                                    }
                                    Err(rustchat_application::error::AppError::InvalidDeliveryCursor) => {
                                        if !send_control(&mut outgoing, ControlReply { kind: "error", conversation_id: Some(id.as_uuid()), code: Some("INVALID_DELIVERY_CURSOR") }).await { break; }
                                    }
                                    Err(_) => {
                                        close_socket(&mut outgoing, 1011, "delivery acknowledgement failed").await;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(WsMessage::Ping(payload))) => {
                        if !send_frame(&mut outgoing, WsMessage::Pong(payload)).await { break; }
                    }
                    Some(Ok(WsMessage::Pong(_))) => last_pong_at = Instant::now(),
                    Some(Ok(WsMessage::Binary(_))) => {
                        close_socket(&mut outgoing, 1003, "binary frames are not supported").await;
                        break;
                    }
                }
            }
            event = events.recv() => {
                match event {
                    Ok(message) if subscriptions.contains(&message.conversation_id) => {
                        let event = event_for(&message);
                        let Ok(text) = serde_json::to_string(&event) else {
                            close_socket(&mut outgoing, 1011, "event serialization failed").await;
                            break;
                        };
                        if !send_frame(&mut outgoing, WsMessage::Text(text.into())).await { break; }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        close_socket(&mut outgoing, 1013, "reconnect and sync missed messages").await;
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            event = typing_events.recv() => {
                match event {
                    Ok(event) if subscriptions.iter().any(|id| id.as_uuid() == event.conversation_id) && event.user_id != user_id.as_uuid() => {
                        let envelope = TypingEnvelope {
                            kind: if event.active { "typing.started" } else { "typing.stopped" },
                            version: 1,
                            event_id: Uuid::now_v7(),
                            conversation_id: event.conversation_id,
                            occurred_at: event.occurred_at,
                            payload: TypingPayload { user_id: event.user_id },
                        };
                        let Ok(text) = serde_json::to_string(&envelope) else {
                            close_socket(&mut outgoing, 1011, "event serialization failed").await;
                            break;
                        };
                        if !send_frame(&mut outgoing, WsMessage::Text(text.into())).await { break; }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            event = read_cursor_events.recv() => {
                match event {
                    Ok(event) if subscriptions.iter().any(|id| id.as_uuid() == event.conversation_id) && event.user_id != user_id.as_uuid() => {
                        let envelope = ReadCursorEnvelope {
                            kind: "read_cursor.updated",
                            version: 1,
                            event_id: Uuid::now_v7(),
                            conversation_id: event.conversation_id,
                            sequence: event.sequence,
                            occurred_at: event.occurred_at,
                            payload: ReadCursorPayload { user_id: event.user_id, sequence: event.sequence },
                        };
                        let Ok(text) = serde_json::to_string(&envelope) else {
                            close_socket(&mut outgoing, 1011, "event serialization failed").await;
                            break;
                        };
                        if !send_frame(&mut outgoing, WsMessage::Text(text.into())).await { break; }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        close_socket(&mut outgoing, 1013, "reconnect and sync missed read cursors").await;
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            event = reaction_events.recv() => {
                match event {
                    Ok(event) if subscriptions.iter().any(|id| id.as_uuid() == event.conversation_id) && event.user_id != user_id.as_uuid() => {
                        let envelope = ReactionEnvelope {
                            kind: if event.added { "reaction.added" } else { "reaction.removed" },
                            version: 1,
                            event_id: Uuid::now_v7(),
                            conversation_id: event.conversation_id,
                            sequence: event.sequence,
                            occurred_at: event.occurred_at,
                            payload: ReactionPayload { message_id: event.message_id, user_id: event.user_id, emoji: event.emoji },
                        };
                        let Ok(text) = serde_json::to_string(&envelope) else {
                            close_socket(&mut outgoing, 1011, "event serialization failed").await;
                            break;
                        };
                        if !send_frame(&mut outgoing, WsMessage::Text(text.into())).await { break; }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        close_socket(&mut outgoing, 1013, "reconnect and sync missed reactions").await;
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            event = delivery_cursor_events.recv() => {
                match event {
                    Ok(event) if subscriptions.iter().any(|id| id.as_uuid() == event.conversation_id) && event.user_id != user_id.as_uuid() => {
                        let envelope = DeliveryCursorEnvelope {
                            kind: "delivery_cursor.updated",
                            version: 1,
                            event_id: Uuid::now_v7(),
                            conversation_id: event.conversation_id,
                            sequence: event.sequence,
                            occurred_at: event.occurred_at,
                            payload: ReadCursorPayload { user_id: event.user_id, sequence: event.sequence },
                        };
                        let Ok(text) = serde_json::to_string(&envelope) else {
                            close_socket(&mut outgoing, 1011, "event serialization failed").await;
                            break;
                        };
                        if !send_frame(&mut outgoing, WsMessage::Text(text.into())).await { break; }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        close_socket(&mut outgoing, 1013, "reconnect and sync missed delivery cursors").await;
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            event = presence_events.recv() => {
                match event {
                    Ok(event) if event.user_id != user_id.as_uuid() => {
                        let target = UserId::from_uuid(event.user_id);
                        let visible = if event.online.is_some() {
                            state.auth.privacy(target).await.is_ok_and(|privacy| privacy.presence_visible)
                        } else { true };
                        if visible
                            && state.conversations.share_conversation(user_id, target).await.unwrap_or(false)
                            && !send_presence(&mut outgoing, &event).await
                        {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            event = community_events.recv() => {
                match event {
                    Ok(event) => {
                        let community_id = rustchat_domain::identity::CommunityId::from_uuid(event.community_id);
                        if state.communities.members(user_id, community_id).await.is_ok() {
                            let envelope = community_update_envelope(&event);
                            let Ok(text) = serde_json::to_string(&envelope) else {
                                close_socket(&mut outgoing, 1011, "event serialization failed").await;
                                break;
                            };
                            if !send_frame(&mut outgoing, WsMessage::Text(text.into())).await { break; }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        close_socket(&mut outgoing, 1013, "reconnect and reload community state").await;
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            event = notification_events.recv() => {
                match event {
                    Ok(event) if event.recipient_id == user_id.as_uuid() => {
                        let Ok(text) = serde_json::to_string(&notification_envelope(&event)) else {
                            close_socket(&mut outgoing, 1011, "event serialization failed").await;
                            break;
                        };
                        if !send_frame(&mut outgoing, WsMessage::Text(text.into())).await { break; }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        close_socket(&mut outgoing, 1013, "reconnect and refresh notifications").await;
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = heartbeat.tick() => {
                if Instant::now().duration_since(last_pong_at) > HEARTBEAT_TIMEOUT {
                    close_socket(&mut outgoing, 1001, "heartbeat timeout").await;
                    break;
                }
                if !send_frame(&mut outgoing, WsMessage::Ping(Vec::new().into())).await { break; }
            }
            _ = resync_events.recv() => {
                // Events from other instances may have been missed: reconnect and resync.
                close_socket(&mut outgoing, 1013, "realtime events were interrupted").await;
                break;
            }
            _ = revalidate.tick() => {
                if state.auth.authenticate(&token).await.is_err() {
                    close_socket(&mut outgoing, 1008, "session expired or revoked").await;
                    break;
                }
                // Membership is rechecked periodically so removed members stop receiving events.
                let mut revoked = Vec::new();
                for id in subscriptions.iter().copied() {
                    if state.conversations.get(user_id, id).await.is_err() { revoked.push(id); }
                }
                for id in revoked {
                    subscriptions.remove(&id);
                    if !send_control(&mut outgoing, ControlReply { kind: "error", conversation_id: Some(id.as_uuid()), code: Some("NOT_FOUND") }).await { break; }
                }
            }
        }
    }
    if state.presence.disconnect(user_id.as_uuid()).await
        && state
            .auth
            .privacy(user_id)
            .await
            .is_ok_and(|privacy| privacy.presence_visible)
    {
        state.publish_presence(crate::PresenceEvent {
            user_id: user_id.as_uuid(),
            online: Some(false),
            occurred_at: time::OffsetDateTime::now_utc(),
        });
    }
}

fn event_for(message: &DomainMessage) -> OutboundEvent<'_> {
    OutboundEvent {
        kind: if message.deleted_at.is_some() {
            "message.deleted"
        } else if message.edited_at.is_some() {
            "message.updated"
        } else {
            "message.created"
        },
        version: 1,
        event_id: message.id.as_uuid(),
        conversation_id: message.conversation_id.as_uuid(),
        sequence: message.sequence,
        occurred_at: message.created_at,
        payload: EventPayload {
            message_id: message.id.as_uuid(),
            sender_id: message.sender_id.as_uuid(),
            client_message_id: message.client_message_id,
            reply_to_message_id: message.reply_to_message_id.map(|id| id.as_uuid()),
            mentioned_user_ids: message
                .mentioned_user_ids
                .iter()
                .map(|id| id.as_uuid())
                .collect(),
            content: message.content.as_str(),
            edited_at: message.edited_at,
            deleted_at: message.deleted_at,
        },
    }
}

async fn send_control<S>(sink: &mut S, reply: ControlReply) -> bool
where
    S: futures_util::Sink<WsMessage> + Unpin,
{
    let Ok(text) = serde_json::to_string(&reply) else {
        return false;
    };
    send_frame(sink, WsMessage::Text(text.into())).await
}

async fn send_presence<S>(sink: &mut S, event: &crate::PresenceEvent) -> bool
where
    S: futures_util::Sink<WsMessage> + Unpin,
{
    let Ok(text) = serde_json::to_string(&presence_envelope(event)) else {
        return false;
    };
    send_frame(sink, WsMessage::Text(text.into())).await
}

async fn send_frame<S>(sink: &mut S, frame: WsMessage) -> bool
where
    S: futures_util::Sink<WsMessage> + Unpin,
{
    send_frame_with_timeout(sink, frame, Duration::from_secs(5)).await
}

async fn send_frame_with_timeout<S>(sink: &mut S, frame: WsMessage, timeout: Duration) -> bool
where
    S: futures_util::Sink<WsMessage> + Unpin,
{
    matches!(
        tokio::time::timeout(timeout, sink.send(frame)).await,
        Ok(Ok(()))
    )
}

async fn close_socket<S>(sink: &mut S, code: u16, reason: &'static str)
where
    S: futures_util::Sink<WsMessage> + Unpin,
{
    let _ = send_frame(
        sink,
        WsMessage::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        })),
    )
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustchat_domain::{identity::MessageId, message::MessageContent};
    use std::{
        convert::Infallible,
        pin::Pin,
        task::{Context, Poll},
    };
    use time::OffsetDateTime;

    struct StalledSink;

    impl futures_util::Sink<WsMessage> for StalledSink {
        type Error = Infallible;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }

        fn start_send(self: Pin<&mut Self>, _item: WsMessage) -> Result<(), Self::Error> {
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Pending
        }
    }

    #[tokio::test]
    async fn stalled_socket_send_times_out() {
        let sent = send_frame_with_timeout(
            &mut StalledSink,
            WsMessage::Ping(Vec::new().into()),
            Duration::from_millis(10),
        )
        .await;
        assert!(!sent);
    }

    #[test]
    fn message_event_matches_the_versioned_realtime_envelope() {
        let reply_to_message_id = MessageId::new();
        let message = DomainMessage {
            id: MessageId::new(),
            conversation_id: ConversationId::new(),
            sender_id: UserId::new(),
            sequence: 17,
            client_message_id: Uuid::now_v7(),
            reply_to_message_id: Some(reply_to_message_id),
            mentioned_user_ids: vec![UserId::new()],
            content: MessageContent::parse("hello".into()).unwrap(),
            created_at: OffsetDateTime::UNIX_EPOCH,
            edited_at: None,
            deleted_at: None,
        };
        let json = serde_json::to_value(event_for(&message)).unwrap();
        assert_eq!(json["type"], "message.created");
        assert_eq!(json["version"], 1);
        assert_eq!(json["event_id"], message.id.as_uuid().to_string());
        assert_eq!(
            json["conversation_id"],
            message.conversation_id.as_uuid().to_string()
        );
        assert_eq!(json["sequence"], 17);
        assert_eq!(json["payload"]["content"], "hello");
        assert_eq!(
            json["payload"]["reply_to_message_id"],
            reply_to_message_id.as_uuid().to_string()
        );
        assert_eq!(
            json["payload"]["mentioned_user_ids"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn socket_controls_reject_unknown_fields() {
        assert!(
            serde_json::from_str::<ClientCommand>(
                r#"{"type":"subscribe","conversation_id":"01900000-0000-7000-8000-000000000001"}"#
            )
            .is_ok()
        );
        assert!(
            serde_json::from_str::<ClientCommand>(
                r#"{"type":"subscribe","conversation_id":"x","admin":true}"#
            )
            .is_err()
        );
        assert!(serde_json::from_str::<ClientCommand>(
            r#"{"type":"typing","conversation_id":"01900000-0000-7000-8000-000000000001","active":true}"#
        ).is_ok());
        assert!(serde_json::from_str::<ClientCommand>(
            r#"{"type":"ack","conversation_id":"01900000-0000-7000-8000-000000000001","sequence":42}"#
        ).is_ok());
    }

    #[test]
    fn typing_event_is_ephemeral_and_identifies_only_its_sender() {
        let user_id = UserId::new().as_uuid();
        let event = TypingEnvelope {
            kind: "typing.started",
            version: 1,
            event_id: Uuid::now_v7(),
            conversation_id: ConversationId::new().as_uuid(),
            occurred_at: time::OffsetDateTime::UNIX_EPOCH,
            payload: TypingPayload { user_id },
        };
        let json = serde_json::to_value(event).unwrap();
        assert_eq!(json["type"], "typing.started");
        assert_eq!(json["payload"]["user_id"], user_id.to_string());
        assert!(json["payload"].get("content").is_none());
    }

    #[test]
    fn reaction_event_uses_the_versioned_message_sequence_envelope() {
        let conversation_id = ConversationId::new().as_uuid();
        let message_id = MessageId::new().as_uuid();
        let user_id = UserId::new().as_uuid();
        let json = serde_json::to_value(ReactionEnvelope {
            kind: "reaction.added",
            version: 1,
            event_id: Uuid::now_v7(),
            conversation_id,
            sequence: 23,
            occurred_at: time::OffsetDateTime::UNIX_EPOCH,
            payload: ReactionPayload {
                message_id,
                user_id,
                emoji: "👍".to_owned(),
            },
        })
        .unwrap();
        assert_eq!(json["type"], "reaction.added");
        assert_eq!(json["version"], 1);
        assert_eq!(json["conversation_id"], conversation_id.to_string());
        assert_eq!(json["sequence"], 23);
        assert_eq!(json["payload"]["message_id"], message_id.to_string());
        assert_eq!(json["payload"]["user_id"], user_id.to_string());
        assert_eq!(json["payload"]["emoji"], "👍");
    }

    #[test]
    fn delivery_cursor_event_contains_the_acknowledged_member_and_sequence() {
        let conversation_id = ConversationId::new().as_uuid();
        let user_id = UserId::new().as_uuid();
        let json = serde_json::to_value(DeliveryCursorEnvelope {
            kind: "delivery_cursor.updated",
            version: 1,
            event_id: Uuid::now_v7(),
            conversation_id,
            sequence: 31,
            occurred_at: time::OffsetDateTime::UNIX_EPOCH,
            payload: ReadCursorPayload {
                user_id,
                sequence: 31,
            },
        })
        .unwrap();
        assert_eq!(json["type"], "delivery_cursor.updated");
        assert_eq!(json["conversation_id"], conversation_id.to_string());
        assert_eq!(json["sequence"], 31);
        assert_eq!(json["payload"]["user_id"], user_id.to_string());
        assert_eq!(json["payload"]["sequence"], 31);
    }

    #[test]
    fn presence_event_carries_a_nullable_visibility_aware_status() {
        let user_id = UserId::new().as_uuid();
        let json = serde_json::to_value(presence_envelope(&crate::PresenceEvent {
            user_id,
            online: None,
            occurred_at: time::OffsetDateTime::UNIX_EPOCH,
        }))
        .unwrap();
        assert_eq!(json["type"], "presence.updated");
        assert_eq!(json["version"], 1);
        assert_eq!(json["payload"]["user_id"], user_id.to_string());
        assert_eq!(json["payload"]["online"], serde_json::Value::Null);
    }

    #[test]
    fn community_update_event_identifies_scope_and_refresh_kind() {
        let community_id = Uuid::now_v7();
        let json = serde_json::to_value(community_update_envelope(&crate::CommunityEvent {
            community_id,
            kind: "channels.updated",
            occurred_at: time::OffsetDateTime::UNIX_EPOCH,
        }))
        .unwrap();
        assert_eq!(json["type"], "channels.updated");
        assert_eq!(json["version"], 1);
        assert_eq!(json["payload"]["community_id"], community_id.to_string());
    }

    #[test]
    fn websocket_origins_must_match_the_allowlist_exactly() {
        let allowed = vec!["https://chat.example.test".to_owned()];
        let mut headers = HeaderMap::new();
        assert!(!origin_allowed(&headers, &allowed));
        headers.insert(header::ORIGIN, "https://chat.example.test".parse().unwrap());
        assert!(origin_allowed(&headers, &allowed));
        headers.insert(
            header::ORIGIN,
            "https://chat.example.test.evil".parse().unwrap(),
        );
        assert!(!origin_allowed(&headers, &allowed));
    }
}
