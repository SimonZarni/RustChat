//! Several server instances sharing one PostgreSQL database: events, outbox
//! dispatch and presence must behave as if there were a single server.
//! Requires `DATABASE_URL` (see README "Local services").

mod common;

use std::{
    collections::HashSet,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{Router, http::Method};
use common::{Options, app, call, node, register, sign_in_existing};
use futures_util::{SinkExt, StreamExt};
use rustchat_application::ports::{EventBus, OutboxRepository};
use rustchat_infrastructure::{MIGRATOR, cluster::PgEventBus, postgres::PgOutboxRepository};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message as Frame, client::IntoClientRequest},
};
use uuid::Uuid;

/// Each test opens listener connections beside its pool, so running them all at once can
/// exhaust a default PostgreSQL (100 connections). They run one at a time instead.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Serve a router on a free port and return its realtime WebSocket URL.
async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    format!("ws://{address}/api/v1/realtime")
}

async fn connect(url: &str, cookie: &str) -> Socket {
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("cookie", cookie.parse().unwrap());
    request
        .headers_mut()
        .insert("origin", common::ORIGIN.parse().unwrap());
    connect_async(request).await.unwrap().0
}

/// The next JSON frame matching `wanted`, ignoring others, or a panic after five seconds.
async fn next_matching(socket: &mut Socket, what: &str, wanted: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                Some(Ok(Frame::Text(text))) => {
                    let value: Value = serde_json::from_str(text.as_str()).unwrap();
                    if wanted(&value) {
                        return value;
                    }
                }
                Some(Ok(_)) => {}
                other => panic!("socket ended while waiting for {what}: {other:?}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

/// True when no JSON frame matching `unwanted` arrives within `window`.
async fn stays_quiet(
    socket: &mut Socket,
    window: Duration,
    unwanted: impl Fn(&Value) -> bool,
) -> bool {
    let deadline = tokio::time::Instant::now() + window;
    loop {
        match tokio::time::timeout_at(deadline, socket.next()).await {
            Err(_) => return true,
            Ok(Some(Ok(Frame::Text(text)))) => {
                if unwanted(&serde_json::from_str(text.as_str()).unwrap()) {
                    return false;
                }
            }
            Ok(Some(Ok(_))) => {}
            Ok(_) => return true,
        }
    }
}

async fn subscribe(socket: &mut Socket, conversation: &str) {
    socket
        .send(Frame::Text(
            json!({"type": "subscribe", "conversation_id": conversation})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    next_matching(socket, "subscribed", |v| v["type"] == "subscribed").await;
}

struct Fixture {
    node_a: common::Node,
    node_b: common::Node,
    url_a: String,
    url_b: String,
    alice: String,
    bobby: String,
    conversation: String,
}

/// Two instances, alice and bobby in a direct conversation, each signed in.
async fn fixture(pool: PgPool) -> Fixture {
    let cluster = |id| Options {
        cluster: Some(id),
        ..Options::default()
    };
    let node_a = node(pool.clone(), cluster(Uuid::now_v7()));
    let node_b = node(pool, cluster(Uuid::now_v7()));
    register(&node_a.router, "alice").await;
    register(&node_a.router, "bobby").await;
    let alice = sign_in_existing(&node_a.router, "alice").await;
    let bobby = sign_in_existing(&node_a.router, "bobby").await;
    let created = call(
        &node_a.router,
        Method::POST,
        "/api/v1/conversations",
        Some(json!({"type": "direct", "username": "bobby"})),
        Some(&alice),
    )
    .await;
    let conversation = created.json["conversation"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let url_a = serve(node_a.router.clone()).await;
    let url_b = serve(node_b.router.clone()).await;
    Fixture {
        node_a,
        node_b,
        url_a,
        url_b,
        alice,
        bobby,
        conversation,
    }
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_message_sent_on_one_instance_reaches_a_client_on_another_exactly_once(pool: PgPool) {
    let _serial = ONE_AT_A_TIME.lock().await;
    let f = fixture(pool).await;
    let mut bobby = connect(&f.url_b, &f.bobby).await;
    subscribe(&mut bobby, &f.conversation).await;

    let sent = call(
        &f.node_a.router,
        Method::POST,
        &format!("/api/v1/conversations/{}/messages", f.conversation),
        Some(json!({"client_message_id": Uuid::now_v7(), "content": "across instances"})),
        Some(&f.alice),
    )
    .await;
    assert_eq!(
        sent.status,
        axum::http::StatusCode::CREATED,
        "{:?}",
        sent.json
    );

    let event = next_matching(&mut bobby, "message.created", |v| {
        v["type"] == "message.created"
    })
    .await;
    assert_eq!(event["payload"]["content"], "across instances");
    // Two instances run dispatchers, but only the one that claimed the row delivers it.
    assert!(
        stays_quiet(&mut bobby, Duration::from_millis(1500), |v| v["type"]
            == "message.created")
        .await,
        "the message must not be delivered twice"
    );

    // Edits made on one instance reach a client on the other too.
    let message_id = sent.json["message"]["id"].as_str().unwrap();
    let edited = call(
        &f.node_a.router,
        Method::PATCH,
        &format!(
            "/api/v1/conversations/{}/messages/{message_id}",
            f.conversation
        ),
        Some(json!({"content": "edited"})),
        Some(&f.alice),
    )
    .await;
    assert_eq!(
        edited.status,
        axum::http::StatusCode::OK,
        "{:?}",
        edited.json
    );
    let update = next_matching(&mut bobby, "message.updated", |v| {
        v["type"] == "message.updated"
    })
    .await;
    assert_eq!(update["payload"]["content"], "edited");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn typing_reactions_and_cursors_cross_instances(pool: PgPool) {
    let _serial = ONE_AT_A_TIME.lock().await;
    let f = fixture(pool).await;
    let mut alice = connect(&f.url_a, &f.alice).await;
    let mut bobby = connect(&f.url_b, &f.bobby).await;
    subscribe(&mut alice, &f.conversation).await;
    subscribe(&mut bobby, &f.conversation).await;

    alice
        .send(Frame::Text(
            json!({"type": "typing", "conversation_id": f.conversation, "active": true})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let typing = next_matching(&mut bobby, "typing.started", |v| {
        v["type"] == "typing.started"
    })
    .await;
    assert_eq!(typing["conversation_id"], f.conversation.as_str());

    let sent = call(
        &f.node_a.router,
        Method::POST,
        &format!("/api/v1/conversations/{}/messages", f.conversation),
        Some(json!({"client_message_id": Uuid::now_v7(), "content": "hello"})),
        Some(&f.alice),
    )
    .await;
    let message_id = sent.json["message"]["id"].as_str().unwrap().to_owned();
    let sequence = sent.json["message"]["sequence"].as_i64().unwrap();

    // A reaction made through instance A, a read cursor made through instance B.
    let reacted = call(
        &f.node_a.router,
        Method::PUT,
        &format!(
            "/api/v1/conversations/{}/messages/{message_id}/reactions",
            f.conversation
        ),
        Some(json!({"emoji": "👍"})),
        Some(&f.alice),
    )
    .await;
    assert_eq!(
        reacted.status,
        axum::http::StatusCode::OK,
        "{:?}",
        reacted.json
    );
    next_matching(&mut bobby, "reaction.added", |v| {
        v["type"] == "reaction.added"
    })
    .await;

    let read = call(
        &f.node_b.router,
        Method::PUT,
        &format!("/api/v1/conversations/{}/read-cursor", f.conversation),
        Some(json!({ "sequence": sequence })),
        Some(&f.bobby),
    )
    .await;
    assert_eq!(read.status, axum::http::StatusCode::OK, "{:?}", read.json);
    let cursor = next_matching(&mut alice, "read_cursor.updated", |v| {
        v["type"] == "read_cursor.updated"
    })
    .await;
    assert_eq!(cursor["payload"]["sequence"], sequence);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn presence_is_shared_so_a_user_stays_online_until_their_last_socket_anywhere_closes(
    pool: PgPool,
) {
    let _serial = ONE_AT_A_TIME.lock().await;
    let f = fixture(pool).await;
    let bobby_id = call(
        &f.node_a.router,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&f.bobby),
    )
    .await
    .json["user"]["id"]
        .clone();
    let about_bobby =
        move |v: &Value| v["type"] == "presence.updated" && v["payload"]["user_id"] == bobby_id;

    // Alice, on instance A, watches the conversation while bobby is offline.
    let mut alice = connect(&f.url_a, &f.alice).await;
    subscribe(&mut alice, &f.conversation).await;
    let snapshot = next_matching(&mut alice, "offline snapshot", &about_bobby).await;
    assert_eq!(snapshot["payload"]["online"], false);

    // Bobby connects on A: online once. His second socket, on B, is not a new "online".
    let mut bobby_a = connect(&f.url_a, &f.bobby).await;
    subscribe(&mut bobby_a, &f.conversation).await;
    let online = next_matching(&mut alice, "online", |v| {
        about_bobby(v) && v["payload"]["online"] == true
    })
    .await;
    assert_eq!(online["payload"]["online"], true);
    let mut bobby_b = connect(&f.url_b, &f.bobby).await;
    subscribe(&mut bobby_b, &f.conversation).await;
    assert!(
        stays_quiet(&mut alice, Duration::from_millis(1200), &about_bobby).await,
        "a second socket on another instance is not a new online transition"
    );

    // Closing the socket on A leaves him online through B; closing B's goes offline everywhere.
    bobby_a.close(None).await.unwrap();
    assert!(
        stays_quiet(&mut alice, Duration::from_millis(1500), |v| about_bobby(v)
            && v["payload"]["online"] == false)
        .await,
        "still connected through instance B"
    );
    bobby_b.close(None).await.unwrap();
    let offline = next_matching(&mut alice, "offline", |v| {
        about_bobby(v) && v["payload"]["online"] == false
    })
    .await;
    assert_eq!(offline["payload"]["online"], false);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_user_connected_on_another_instance_shows_as_online_in_the_snapshot(pool: PgPool) {
    let _serial = ONE_AT_A_TIME.lock().await;
    let f = fixture(pool).await;
    let bobby_id = call(
        &f.node_a.router,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&f.bobby),
    )
    .await
    .json["user"]["id"]
        .clone();
    let mut bobby = connect(&f.url_b, &f.bobby).await;
    subscribe(&mut bobby, &f.conversation).await;

    let mut alice = connect(&f.url_a, &f.alice).await;
    subscribe(&mut alice, &f.conversation).await;
    let snapshot = next_matching(&mut alice, "bobby's status", |v| {
        v["type"] == "presence.updated" && v["payload"]["user_id"] == bobby_id
    })
    .await;
    assert_eq!(snapshot["payload"]["online"], true);
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn a_lost_notification_connection_makes_sockets_resynchronize(pool: PgPool) {
    let _serial = ONE_AT_A_TIME.lock().await;
    let f = fixture(pool).await;
    let mut alice = connect(&f.url_a, &f.alice).await;
    subscribe(&mut alice, &f.conversation).await;

    f.node_a.resync.send(()).unwrap();
    let closed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match alice.next().await {
                Some(Ok(Frame::Close(Some(frame)))) => return u16::from(frame.code),
                Some(Ok(_)) => {}
                other => panic!("expected a close frame, got {other:?}"),
            }
        }
    })
    .await
    .expect("close frame");
    assert_eq!(closed, 1013, "try again later: reconnect and resync");
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn instances_claim_disjoint_outbox_events_with_a_lease(pool: PgPool) {
    let _serial = ONE_AT_A_TIME.lock().await;
    // No dispatchers here, so the events stay pending until the test claims them.
    let plain = app(pool.clone(), Options::default());
    register(&plain, "alice").await;
    register(&plain, "bobby").await;
    let alice = sign_in_existing(&plain, "alice").await;
    let created = call(
        &plain,
        Method::POST,
        "/api/v1/conversations",
        Some(json!({"type": "direct", "username": "bobby"})),
        Some(&alice),
    )
    .await;
    let conversation = created.json["conversation"]["id"].as_str().unwrap();
    for n in 0..6 {
        let sent = call(
            &plain,
            Method::POST,
            &format!("/api/v1/conversations/{conversation}/messages"),
            Some(json!({"client_message_id": Uuid::now_v7(), "content": format!("m{n}")})),
            Some(&alice),
        )
        .await;
        assert_eq!(sent.status, axum::http::StatusCode::CREATED);
    }

    let (first, second) = (
        PgOutboxRepository::new(pool.clone()),
        PgOutboxRepository::new(pool.clone()),
    );
    let (a, b) = tokio::join!(first.pending_messages(3), second.pending_messages(3));
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!((a.len(), b.len()), (3, 3), "each instance claims a batch");
    let ids: HashSet<_> = a.iter().chain(b.iter()).map(|m| m.id).collect();
    assert_eq!(ids.len(), 6, "no event is claimed twice");
    assert!(
        first.pending_messages(10).await.unwrap().is_empty(),
        "claimed events are leased"
    );

    // If the claimer dies, the lease expires and another instance takes over.
    sqlx::query("UPDATE outbox_events SET claimed_until = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let retaken = second.pending_messages(10).await.unwrap();
    assert_eq!(retaken.len(), 6);
    for message in &retaken {
        second.mark_message_published(message.id).await.unwrap();
    }
    assert!(
        first.pending_messages(10).await.unwrap().is_empty(),
        "published events are done"
    );
}

#[sqlx::test(migrator = "MIGRATOR")]
async fn the_event_bus_reaches_other_instances_but_not_the_publisher(pool: PgPool) {
    let _serial = ONE_AT_A_TIME.lock().await;
    let received_a = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let received_b = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let recorder = |sink: &Arc<Mutex<Vec<(String, Value)>>>| -> rustchat_infrastructure::cluster::RemoteHandler {
        let sink = sink.clone();
        Arc::new(move |topic, payload| sink.lock().unwrap().push((topic.to_owned(), payload)))
    };
    let no_gap: rustchat_infrastructure::cluster::GapHandler = Arc::new(|| {});
    let bus_a = PgEventBus::start(
        pool.clone(),
        Uuid::now_v7(),
        recorder(&received_a),
        no_gap.clone(),
    );
    let _bus_b = PgEventBus::start(pool, Uuid::now_v7(), recorder(&received_b), no_gap);

    // The listeners subscribe asynchronously, so keep publishing until the other side hears one.
    let heard = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            bus_a.publish("typing", json!({"hello": "world"}));
            tokio::time::sleep(Duration::from_millis(100)).await;
            if !received_b.lock().unwrap().is_empty() {
                return;
            }
        }
    })
    .await;
    assert!(heard.is_ok(), "instance B never received the event");
    assert_eq!(
        received_b.lock().unwrap()[0],
        ("typing".to_owned(), json!({"hello": "world"}))
    );
    assert!(
        received_a.lock().unwrap().is_empty(),
        "an instance must not receive its own events"
    );

    // Oversized events are dropped instead of failing the database call.
    let before = received_b.lock().unwrap().len();
    bus_a.publish("typing", json!({"big": "x".repeat(10_000)}));
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        received_b
            .lock()
            .unwrap()
            .iter()
            .skip(before)
            .all(|(_, payload)| payload.get("big").is_none()),
        "an event over the notification limit must not be delivered"
    );
}
