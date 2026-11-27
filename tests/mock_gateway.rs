//! Gateway client against a local mock WebSocket server:
#![allow(clippy::result_large_err)] // tungstenite handshake callback signature
//! connect → HELLO → IDENTIFY → READY → MESSAGE_CREATE, heartbeats, zombie
//! detection, RECONNECT/RESUME, INVALID_SESSION, zlib-stream, close 4004.

use std::time::Duration;

use ferrocord::auth::secrets::Secret;
use ferrocord::net::gateway::{self, GatewayConfig, GatewayHandle, MemberSubscription};
use ferrocord::net::model::{Presence, Snowflake};
use ferrocord::net::types::ClientProperties;
use ferrocord::net::{ConnectionState, Event};
use flate2::{Compress, Compression, FlushCompress};
use futures::{SinkExt, Stream, StreamExt};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message as Ws;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

const TOKEN: &str = "test-token-not-real";
const STEP: Duration = Duration::from_secs(10);

type Server = WebSocketStream<TcpStream>;

async fn listen() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", l.local_addr().unwrap());
    (l, url)
}

/// Accepts one WebSocket and returns it with the request URI.
async fn accept(l: &TcpListener) -> (Server, String) {
    let (sock, _) = tokio::time::timeout(STEP, l.accept())
        .await
        .unwrap()
        .unwrap();
    let mut uri = String::new();
    let ws = tokio_tungstenite::accept_hdr_async(sock, |req: &Request, resp: Response| {
        uri = req.uri().to_string();
        Ok(resp)
    })
    .await
    .unwrap();
    (ws, uri)
}

async fn recv_json(ws: &mut Server) -> Value {
    loop {
        let m = tokio::time::timeout(STEP, ws.next())
            .await
            .expect("timed out waiting for client frame")
            .expect("client closed")
            .unwrap();
        if let Ws::Text(t) = m {
            return serde_json::from_str(t.as_str()).unwrap();
        }
    }
}

/// Waits for a frame with the given opcode, ignoring others (heartbeats).
async fn recv_op(ws: &mut Server, op: u64) -> Value {
    loop {
        let v = recv_json(ws).await;
        if v["op"] == op {
            return v;
        }
    }
}

async fn send(ws: &mut Server, v: Value) {
    ws.send(Ws::text(v.to_string())).await.unwrap();
}

fn hello(interval: u64) -> Value {
    json!({"op": 10, "d": {"heartbeat_interval": interval}})
}

fn ready(seq: u64, resume_url: &str) -> Value {
    let mut d: Value = serde_json::from_str(
        &std::fs::read_to_string(format!(
            "{}/tests/fixtures/ready.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap();
    d["resume_gateway_url"] = json!(resume_url);
    d["session_id"] = json!("session-abc");
    json!({"op": 0, "t": "READY", "s": seq, "d": d})
}

fn message(seq: u64, id: u64, content: &str) -> Value {
    json!({"op": 0, "t": "MESSAGE_CREATE", "s": seq, "d": {
        "id": id.to_string(), "channel_id": "4000000000000000002", "guild_id": "2000000000000000001",
        "content": content, "author": {"id": "1000000000000000002", "username": "alice.dev"},
        "attachments": [], "embeds": [], "mentions": [], "type": 0
    }})
}

fn client(url: &str, zlib: bool) -> impl Stream<Item = Event> + Unpin {
    client_with(url, zlib, GatewayHandle::default())
}

fn client_with(url: &str, zlib: bool, handle: GatewayHandle) -> impl Stream<Item = Event> + Unpin {
    Box::pin(gateway::spawn(GatewayConfig {
        url: url.to_owned(),
        token: Secret::new(TOKEN),
        zlib,
        properties: ClientProperties::default(),
        handle,
    }))
}

/// Next event that matches `pred`, skipping others.
async fn expect<S: Stream<Item = Event> + Unpin>(
    s: &mut S,
    what: &str,
    pred: impl Fn(&Event) -> bool,
) -> Event {
    let deadline = tokio::time::Instant::now() + STEP;
    loop {
        let ev = tokio::time::timeout_at(deadline, s.next())
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
            .unwrap_or_else(|| panic!("stream ended waiting for {what}"));
        if pred(&ev) {
            return ev;
        }
    }
}

#[tokio::test]
async fn connect_hello_identify_ready_message() {
    let (l, url) = listen().await;
    let mut c = client(&url, false);
    let server = tokio::spawn(async move {
        let (mut ws, uri) = accept(&l).await;
        assert!(uri.contains("v=10"), "{uri}");
        assert!(uri.contains("encoding=json"));
        assert!(!uri.contains("compress"));
        send(&mut ws, hello(45_000)).await;
        let id = recv_op(&mut ws, 2).await;
        assert_eq!(id["d"]["token"], TOKEN);
        assert_eq!(id["d"]["properties"]["browser"], "Chrome");
        send(&mut ws, ready(1, "ws://unused")).await;
        send(
            &mut ws,
            message(2, 5000000000000000100, "hello from the mock"),
        )
        .await;
        // Keep the socket open until the client is done.
        let _ = tokio::time::timeout(STEP, ws.next()).await;
    });

    expect(&mut c, "Connecting", |e| {
        matches!(e, Event::Connection(ConnectionState::Connecting))
    })
    .await;
    expect(&mut c, "Connected", |e| {
        matches!(e, Event::Connection(ConnectionState::Connected))
    })
    .await;
    let Event::Ready(r) = expect(&mut c, "READY", |e| matches!(e, Event::Ready(_))).await else {
        unreachable!()
    };
    assert_eq!(r.guilds.len(), 2);
    let Event::MessageCreate(m) = expect(&mut c, "MESSAGE_CREATE", |e| {
        matches!(e, Event::MessageCreate(_))
    })
    .await
    else {
        unreachable!()
    };
    assert_eq!(m.content, "hello from the mock");
    drop(c);
    server.await.unwrap();
}

#[tokio::test]
async fn heartbeat_ack_and_zombie_resume() {
    let (l, url) = listen().await;
    let resume_url = format!("{url}/resume");
    let mut c = client(&url, false);
    let ru = resume_url.clone();
    let server = tokio::spawn(async move {
        let (mut ws, _) = accept(&l).await;
        send(&mut ws, hello(1_000)).await;
        recv_op(&mut ws, 2).await;
        send(&mut ws, ready(3, &ru)).await;
        // First heartbeat carries the last sequence number.
        let hb = recv_op(&mut ws, 1).await;
        assert_eq!(hb["d"], 3);
        send(&mut ws, json!({"op": 11})).await;
        // Second heartbeat is NOT acknowledged: the client must treat the
        // connection as a zombie and resume on a new socket.
        recv_op(&mut ws, 1).await;
        let (mut ws2, uri) = accept(&l).await;
        assert!(uri.starts_with("/resume"), "resumed on {uri}");
        send(&mut ws2, hello(45_000)).await;
        let r = recv_op(&mut ws2, 6).await;
        assert_eq!(r["d"]["session_id"], "session-abc");
        assert_eq!(r["d"]["seq"], 3);
        assert_eq!(r["d"]["token"], TOKEN);
        send(&mut ws2, json!({"op": 0, "t": "RESUMED", "s": 4, "d": {}})).await;
        let _ = tokio::time::timeout(STEP, ws2.next()).await;
    });
    expect(&mut c, "READY", |e| matches!(e, Event::Ready(_))).await;
    expect(&mut c, "Reconnecting", |e| {
        matches!(e, Event::Connection(ConnectionState::Reconnecting { .. }))
    })
    .await;
    expect(&mut c, "RESUMED", |e| matches!(e, Event::Resumed)).await;
    drop(c);
    server.await.unwrap();
}

#[tokio::test]
async fn reconnect_opcode_resumes_and_replays() {
    let (l, url) = listen().await;
    let resume_url = format!("{url}/resume");
    let mut c = client(&url, false);
    let ru = resume_url.clone();
    let server = tokio::spawn(async move {
        let (mut ws, _) = accept(&l).await;
        send(&mut ws, hello(45_000)).await;
        recv_op(&mut ws, 2).await;
        send(&mut ws, ready(1, &ru)).await;
        send(&mut ws, message(5, 5000000000000000101, "before reconnect")).await;
        send(&mut ws, json!({"op": 7, "d": null})).await;
        let (mut ws2, uri) = accept(&l).await;
        assert!(uri.starts_with("/resume"));
        send(&mut ws2, hello(45_000)).await;
        let r = recv_op(&mut ws2, 6).await;
        assert_eq!(r["d"]["seq"], 5);
        // Replay a missed event, then RESUMED.
        send(
            &mut ws2,
            message(6, 5000000000000000102, "missed while away"),
        )
        .await;
        send(&mut ws2, json!({"op": 0, "t": "RESUMED", "s": 7, "d": {}})).await;
        let _ = tokio::time::timeout(STEP, ws2.next()).await;
    });
    expect(
        &mut c,
        "first message",
        |e| matches!(e, Event::MessageCreate(m) if m.content == "before reconnect"),
    )
    .await;
    expect(
        &mut c,
        "replayed message",
        |e| matches!(e, Event::MessageCreate(m) if m.content == "missed while away"),
    )
    .await;
    expect(&mut c, "RESUMED", |e| matches!(e, Event::Resumed)).await;
    drop(c);
    server.await.unwrap();
}

#[tokio::test]
async fn invalid_session_reidentifies() {
    let (l, url) = listen().await;
    let mut c = client(&url, false);
    let u2 = url.clone();
    let server = tokio::spawn(async move {
        let (mut ws, _) = accept(&l).await;
        send(&mut ws, hello(45_000)).await;
        recv_op(&mut ws, 2).await;
        send(&mut ws, ready(1, &format!("{u2}/resume"))).await;
        send(&mut ws, json!({"op": 9, "d": false})).await;
        // Not resumable: the client connects to the base URL and IDENTIFYs.
        let (mut ws2, uri) = accept(&l).await;
        assert!(!uri.starts_with("/resume"), "{uri}");
        send(&mut ws2, hello(45_000)).await;
        let id = recv_op(&mut ws2, 2).await;
        assert_eq!(id["d"]["token"], TOKEN);
        send(&mut ws2, ready(1, "ws://unused")).await;
        let _ = tokio::time::timeout(STEP, ws2.next()).await;
    });
    expect(&mut c, "first READY", |e| matches!(e, Event::Ready(_))).await;
    expect(&mut c, "second READY", |e| matches!(e, Event::Ready(_))).await;
    drop(c);
    server.await.unwrap();
}

#[tokio::test]
async fn zlib_stream_transport() {
    let (l, url) = listen().await;
    let mut c = client(&url, true);
    let server = tokio::spawn(async move {
        let (mut ws, uri) = accept(&l).await;
        assert!(uri.contains("compress=zlib-stream"), "{uri}");
        let mut z = Compress::new(Compression::default(), true);
        let mut compress = |v: Value| {
            let s = v.to_string();
            let mut out = Vec::with_capacity(s.len() + 128);
            z.compress_vec(s.as_bytes(), &mut out, FlushCompress::Sync)
                .unwrap();
            out
        };
        ws.send(Ws::binary(compress(hello(45_000)))).await.unwrap();
        recv_op(&mut ws, 2).await;
        let r = compress(ready(1, "ws://unused"));
        // Split READY across two frames.
        let (a, b) = r.split_at(r.len() / 2);
        ws.send(Ws::binary(a.to_vec())).await.unwrap();
        ws.send(Ws::binary(b.to_vec())).await.unwrap();
        ws.send(Ws::binary(compress(message(
            2,
            5000000000000000103,
            "compressed",
        ))))
        .await
        .unwrap();
        let _ = tokio::time::timeout(STEP, ws.next()).await;
    });
    expect(&mut c, "READY", |e| matches!(e, Event::Ready(_))).await;
    expect(
        &mut c,
        "message",
        |e| matches!(e, Event::MessageCreate(m) if m.content == "compressed"),
    )
    .await;
    drop(c);
    server.await.unwrap();
}

#[tokio::test]
async fn close_4004_is_fatal_auth_failure() {
    let (l, url) = listen().await;
    let mut c = client(&url, false);
    let server = tokio::spawn(async move {
        let (mut ws, _) = accept(&l).await;
        send(&mut ws, hello(45_000)).await;
        recv_op(&mut ws, 2).await;
        ws.send(Ws::Close(Some(CloseFrame {
            code: CloseCode::Library(4004),
            reason: "Authentication failed.".into(),
        })))
        .await
        .unwrap();
        // The client must not reconnect.
        assert!(
            tokio::time::timeout(Duration::from_secs(2), l.accept())
                .await
                .is_err()
        );
    });
    expect(&mut c, "AuthFailed", |e| matches!(e, Event::AuthFailed)).await;
    expect(&mut c, "Disconnected", |e| {
        matches!(e, Event::Connection(ConnectionState::Disconnected))
    })
    .await;
    assert!(
        tokio::time::timeout(STEP, c.next())
            .await
            .unwrap()
            .is_none()
    );
    server.await.unwrap();
}

#[tokio::test]
async fn server_down_reconnects_with_backoff() {
    // Bind then drop: nothing listens on this port.
    let (l, url) = listen().await;
    drop(l);
    let mut c = client(&url, false);
    let Event::Connection(ConnectionState::Reconnecting { attempt, delay_ms }) = expect(
        &mut c,
        "Reconnecting",
        |e| matches!(e, Event::Connection(ConnectionState::Reconnecting { delay_ms, .. }) if *delay_ms > 0),
    )
    .await
    else {
        unreachable!()
    };
    assert_eq!(attempt, 1);
    assert!((1_000..=2_000).contains(&delay_ms), "{delay_ms}");
}

#[tokio::test]
async fn status_and_member_subscription_are_sent_and_resent() {
    let (l, url) = listen().await;
    let resume_url = format!("{url}/resume");
    let handle = GatewayHandle::new(Presence::Dnd);
    let mut c = client_with(&url, false, handle.clone());
    let guild = Snowflake(2000000000000000001);
    let channel = Snowflake(4000000000000000002);
    let ru = resume_url.clone();
    let server = tokio::spawn(async move {
        let (mut ws, _) = accept(&l).await;
        send(&mut ws, hello(45_000)).await;
        // The status chosen before connecting is part of IDENTIFY.
        let id = recv_op(&mut ws, 2).await;
        assert_eq!(id["d"]["presence"]["status"], "dnd");
        send(&mut ws, ready(1, &ru)).await;
        // Op 14 with the requested range.
        let sub = recv_op(&mut ws, 14).await;
        assert_eq!(sub["d"]["guild_id"], "2000000000000000001");
        assert_eq!(
            sub["d"]["channels"]["4000000000000000002"],
            json!([[0, 99]])
        );
        // A status change while connected is an op 3.
        let st = recv_op(&mut ws, 3).await;
        assert_eq!(st["d"]["status"], "idle");
        assert_eq!(st["d"]["afk"], false);
        // Member list update reaches the client.
        send(
            &mut ws,
            json!({"op": 0, "t": "GUILD_MEMBER_LIST_UPDATE", "s": 2, "d": {
                "guild_id": "2000000000000000001", "id": "everyone", "member_count": 1,
                "online_count": 1, "groups": [{"id": "online", "count": 1}],
                "ops": [{"op": "SYNC", "range": [0, 99], "items": [
                    {"group": {"id": "online", "count": 1}},
                    {"member": {"user": {"id": "1000000000000000002", "username": "alice.dev"},
                                "roles": [], "presence": {"status": "online"}}}
                ]}]
            }}),
        )
        .await;
        // Drop the connection: the client resumes and re-sends op 14.
        send(&mut ws, json!({"op": 7, "d": null})).await;
        let (mut ws2, uri) = accept(&l).await;
        assert!(uri.starts_with("/resume"));
        send(&mut ws2, hello(45_000)).await;
        recv_op(&mut ws2, 6).await;
        send(&mut ws2, json!({"op": 0, "t": "RESUMED", "s": 3, "d": {}})).await;
        let again = recv_op(&mut ws2, 14).await;
        assert_eq!(again["d"]["guild_id"], "2000000000000000001");
        let _ = tokio::time::timeout(STEP, ws2.next()).await;
    });
    expect(&mut c, "ready", |e| matches!(e, Event::Ready(_))).await;
    handle.subscribe_members(MemberSubscription {
        guild,
        channel,
        ranges: vec![(0, 99)],
    });
    // Let the op 14 go out before changing the status (frames are ordered).
    tokio::time::sleep(Duration::from_millis(200)).await;
    handle.set_status(Presence::Idle);
    let ev = expect(&mut c, "member list", |e| matches!(e, Event::MemberList(_))).await;
    let Event::MemberList(u) = ev else { panic!() };
    assert_eq!(u.member_count, 1);
    expect(&mut c, "RESUMED", |e| matches!(e, Event::Resumed)).await;
    drop(c);
    server.await.unwrap();
}

#[test]
fn identical_subscription_is_not_resent() {
    let h = GatewayHandle::default();
    let sub = MemberSubscription {
        guild: Snowflake(1),
        channel: Snowflake(2),
        ranges: vec![(0, 99)],
    };
    h.subscribe_members(sub.clone());
    assert_eq!(h.take_pending().len(), 1);
    h.subscribe_members(sub.clone());
    assert!(h.take_pending().is_empty());
    h.subscribe_members(MemberSubscription {
        ranges: vec![(0, 99), (100, 199)],
        ..sub
    });
    assert_eq!(h.take_pending().len(), 1);
    h.set_status(Presence::Online); // unchanged
    assert!(h.take_pending().is_empty());
    h.set_status(Presence::Idle);
    assert_eq!(h.take_pending().len(), 1);
    h.clear_members();
    assert!(h.members().is_none());
}
