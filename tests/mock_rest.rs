//! REST client against a local mock HTTP server: auth header, history
//! parsing, sending, ACKs, 401 handling, bucket limits and 429 retry.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Response, http};
use ferrocord::auth::secrets::Secret;
use ferrocord::error::Error;
use ferrocord::net::model::Snowflake;
use ferrocord::net::model::{Emoji, Presence};
use ferrocord::net::rest::{ReplyTarget, RestClient, SearchScope, UploadFile};
use serde_json::json;

const TOKEN: &str = "rest-test-token";

fn msg(id: u64) -> serde_json::Value {
    json!({"id": id.to_string(), "channel_id": "10", "content": format!("m{id}"),
           "author": {"id": "2", "username": "alice"}, "attachments": [], "embeds": [], "mentions": [], "type": 0})
}

#[tokio::test]
async fn sends_auth_header_and_parses_history_oldest_first() {
    let server = http(Arc::new(|req, _| {
        if req.path.starts_with("/channels/10/messages") {
            // Discord returns newest first.
            Response::json(200, json!([msg(30), msg(20), msg(10)]))
        } else {
            Response::json(404, json!({"message": "Unknown"}))
        }
    }))
    .await;
    let rest = RestClient::new(&server.base(), "http://cdn", Some(Secret::new(TOKEN))).unwrap();
    let msgs = rest
        .messages(Snowflake(10), Some(Snowflake(99)), 50)
        .await
        .unwrap();
    assert_eq!(
        msgs.iter().map(|m| m.id.0).collect::<Vec<_>>(),
        vec![10, 20, 30]
    );
    let r = &server.recorded()[0];
    assert_eq!(r.path, "/channels/10/messages?limit=50&before=99");
    // User tokens are sent raw (no "Bot " prefix).
    assert_eq!(r.header("authorization"), Some(TOKEN));
    assert_eq!(
        r.header("user-agent"),
        Some(ferrocord::net::rest::user_agent().as_str())
    );
}

#[tokio::test]
async fn send_message_typing_and_ack() {
    let server = http(Arc::new(|req, _| {
        match (req.method.as_str(), req.path.as_str()) {
            ("POST", "/channels/10/messages") => {
                let body: serde_json::Value = serde_json::from_str(&req.body).unwrap();
                let mut m = msg(77);
                m["content"] = body["content"].clone();
                m["nonce"] = body["nonce"].clone();
                Response::json(200, m)
            }
            ("POST", "/channels/10/typing") => Response::json(204, json!(null)),
            ("POST", p) if p.ends_with("/ack") => Response::json(200, json!({"token": null})),
            _ => Response::json(404, json!({})),
        }
    }))
    .await;
    let rest = RestClient::new(&server.base(), "http://cdn", Some(Secret::new(TOKEN))).unwrap();
    let m = rest
        .send_message(Snowflake(10), "hello **world**", "123", None)
        .await
        .unwrap();
    assert_eq!(m.content, "hello **world**");
    assert_eq!(m.nonce.as_deref(), Some("123"));
    rest.trigger_typing(Snowflake(10)).await.unwrap();
    rest.ack(Snowflake(10), Snowflake(77)).await.unwrap();
    let rec = server.recorded();
    let sent: serde_json::Value = serde_json::from_str(&rec[0].body).unwrap();
    assert_eq!(sent["nonce"], "123");
    assert_eq!(sent["tts"], false);
    assert_eq!(rec[2].path, "/channels/10/messages/77/ack");
}

#[tokio::test]
async fn unauthorized_maps_to_revoked_session() {
    let server = http(Arc::new(|_, _| {
        Response::json(401, json!({"message": "401: Unauthorized", "code": 0}))
    }))
    .await;
    let rest = RestClient::new(&server.base(), "http://cdn", Some(Secret::new(TOKEN))).unwrap();
    let e = rest.me().await.unwrap_err();
    assert!(matches!(e, Error::Auth(_)));
    assert!(e.is_session_revoked());
    let e = rest.trigger_typing(Snowflake(1)).await.unwrap_err();
    assert!(matches!(e, Error::Http { status: 401, .. }));
}

#[tokio::test]
async fn retries_after_429_with_retry_after() {
    let server = http(Arc::new(|_, n| {
        if n == 0 {
            Response::json(429, json!({"message": "You are being rate limited.", "retry_after": 0.3, "global": false}))
                .with_header("retry-after", "1")
        } else {
            Response::json(200, json!({"id": "1", "username": "me"}))
        }
    }))
    .await;
    let rest = RestClient::new(&server.base(), "http://cdn", Some(Secret::new(TOKEN))).unwrap();
    let start = Instant::now();
    let me = rest.me().await.unwrap();
    assert_eq!(me.username, "me");
    let rec = server.recorded();
    assert_eq!(rec.len(), 2);
    // The JSON body's retry_after (0.3 s) wins over the header (1 s).
    let gap = rec[1].at - rec[0].at;
    assert!(gap >= Duration::from_millis(290), "{gap:?}");
    assert!(start.elapsed() < Duration::from_millis(990));
}

#[tokio::test]
async fn gives_up_after_repeated_429() {
    let server = http(Arc::new(|_, _| {
        Response::json(429, json!({"retry_after": 0.05, "global": true}))
    }))
    .await;
    let rest = RestClient::new(&server.base(), "http://cdn", Some(Secret::new(TOKEN))).unwrap();
    let e = rest.trigger_typing(Snowflake(1)).await.unwrap_err();
    assert!(matches!(e, Error::Http { status: 429, .. }));
    assert_eq!(server.recorded().len(), 4);
}

#[tokio::test]
async fn exhausted_bucket_delays_next_request() {
    let server = http(Arc::new(|_, _| {
        Response::json(204, json!(null))
            .with_header("x-ratelimit-bucket", "typing")
            .with_header("x-ratelimit-limit", "1")
            .with_header("x-ratelimit-remaining", "0")
            .with_header("x-ratelimit-reset-after", "0.4")
    }))
    .await;
    let rest = RestClient::new(&server.base(), "http://cdn", Some(Secret::new(TOKEN))).unwrap();
    rest.trigger_typing(Snowflake(5)).await.unwrap();
    rest.trigger_typing(Snowflake(5)).await.unwrap();
    // A different channel is a different bucket instance: no wait.
    rest.trigger_typing(Snowflake(6)).await.unwrap();
    let rec = server.recorded();
    assert_eq!(rec.len(), 3);
    let gap = rec[1].at - rec[0].at;
    assert!(gap >= Duration::from_millis(380), "waited {gap:?}");
    let gap2 = rec[2].at - rec[1].at;
    assert!(gap2 < Duration::from_millis(300), "{gap2:?}");
}

#[tokio::test]
async fn media_downloads_are_capped_and_host_restricted() {
    let big = "x".repeat(ferrocord::net::rest::MAX_MEDIA_BYTES + 1);
    let server = http(Arc::new(move |req, _| {
        if req.path.starts_with("/big") {
            Response {
                status: 200,
                headers: vec![],
                body: big.clone(),
            }
        } else {
            Response {
                status: 200,
                headers: vec![],
                body: "small".into(),
            }
        }
    }))
    .await;
    // The mock server is the configured CDN, so it is allowed.
    let rest = RestClient::new("http://unused", &server.base(), None).unwrap();
    assert_eq!(
        rest.fetch_bytes(&format!("{}/ok.png", server.base()))
            .await
            .unwrap(),
        b"small"
    );
    assert!(
        rest.fetch_bytes(&format!("{}/big.png", server.base()))
            .await
            .is_err()
    );
    // Arbitrary hosts are never contacted.
    assert!(!rest.is_allowed_media_url("https://evil.example.com/a.png"));
    assert!(!rest.is_allowed_media_url("http://cdn.discordapp.com/a.png"));
    assert!(rest.is_allowed_media_url("https://cdn.discordapp.com/avatars/1/a.png"));
    assert!(rest.is_allowed_media_url("https://media.discordapp.net/attachments/1/2/x.png"));
    assert!(rest.is_allowed_media_url("https://images-ext-1.discordapp.net/external/x"));
    assert!(!rest.is_allowed_media_url("https://images-ext-1.discordapp.net.evil.com/x"));
    // GIF picker previews.
    assert!(rest.is_allowed_media_url("https://media.tenor.com/abc/preview.png"));
    assert!(!rest.is_allowed_media_url("http://media.tenor.com/abc/preview.png"));
    assert!(!rest.is_allowed_media_url("https://media.tenor.com.evil.com/x.png"));
    assert!(
        rest.fetch_bytes("https://evil.example.com/a.png")
            .await
            .is_err()
    );
}

#[test]
fn decompression_bombs_are_rejected() {
    use ferrocord::net::rest::check_image_dimensions;
    let mut ok = Vec::new();
    image::RgbaImage::new(4, 3)
        .write_to(&mut std::io::Cursor::new(&mut ok), image::ImageFormat::Png)
        .unwrap();
    assert_eq!(check_image_dimensions(&ok).unwrap(), (4, 3));
    // A PNG header claiming 100000 x 100000 pixels.
    let mut bomb = ok.clone();
    bomb[16..20].copy_from_slice(&100_000u32.to_be_bytes());
    bomb[20..24].copy_from_slice(&100_000u32.to_be_bytes());
    assert!(check_image_dimensions(&bomb).is_err());
    assert!(check_image_dimensions(b"not an image").is_err());
}

fn rest_for(server: &common::MockHttp) -> RestClient {
    RestClient::new(&server.base(), "http://cdn", Some(Secret::new(TOKEN))).unwrap()
}

#[tokio::test]
async fn reply_body_has_reference_and_optional_allowed_mentions() {
    let server = http(Arc::new(|_, _| Response::json(200, msg(80)))).await;
    let rest = rest_for(&server);
    let t = ReplyTarget {
        message: Snowflake(5),
        guild: Some(Snowflake(9)),
        mention: true,
    };
    rest.send_message(Snowflake(10), "yes", "n1", Some(t))
        .await
        .unwrap();
    rest.send_message(
        Snowflake(10),
        "quiet",
        "n2",
        Some(ReplyTarget {
            mention: false,
            ..t
        }),
    )
    .await
    .unwrap();
    let rec = server.recorded();
    let a: serde_json::Value = serde_json::from_str(&rec[0].body).unwrap();
    assert_eq!(a["message_reference"]["message_id"], "5");
    assert_eq!(a["message_reference"]["channel_id"], "10");
    assert_eq!(a["message_reference"]["guild_id"], "9");
    assert_eq!(a["message_reference"]["fail_if_not_exists"], false);
    assert!(a.get("allowed_mentions").is_none(), "ping on = default");
    let b: serde_json::Value = serde_json::from_str(&rec[1].body).unwrap();
    assert_eq!(b["allowed_mentions"]["replied_user"], false);
    assert_eq!(
        b["allowed_mentions"]["parse"],
        json!(["users", "roles", "everyone"])
    );
}

#[tokio::test]
async fn edit_and_delete_messages() {
    let server = http(Arc::new(|req, _| match req.method.as_str() {
        "PATCH" => {
            let body: serde_json::Value = serde_json::from_str(&req.body).unwrap();
            let mut m = msg(10);
            m["content"] = body["content"].clone();
            m["edited_timestamp"] = json!("2026-11-18T12:00:00+00:00");
            Response::json(200, m)
        }
        "DELETE" => Response::json(204, json!(null)),
        _ => Response::json(404, json!({})),
    }))
    .await;
    let rest = rest_for(&server);
    let m = rest
        .edit_message(Snowflake(10), Snowflake(10), "fixed")
        .await
        .unwrap();
    assert_eq!(m.content, "fixed");
    assert!(m.edited);
    rest.delete_message(Snowflake(10), Snowflake(10))
        .await
        .unwrap();
    let rec = server.recorded();
    assert_eq!(
        (rec[0].method.as_str(), rec[0].path.as_str()),
        ("PATCH", "/channels/10/messages/10")
    );
    assert_eq!(
        (rec[1].method.as_str(), rec[1].path.as_str()),
        ("DELETE", "/channels/10/messages/10")
    );
    // A forbidden delete is an error the UI can show.
    let denied = http(Arc::new(|_, _| {
        Response::json(
            403,
            json!({"message": "Missing Permissions", "code": 50013}),
        )
    }))
    .await;
    let e = rest_for(&denied)
        .delete_message(Snowflake(1), Snowflake(2))
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Http { status: 403, .. }));
}

#[tokio::test]
async fn reactions_use_encoded_emoji_paths() {
    let server = http(Arc::new(|_, _| Response::json(204, json!(null)))).await;
    let rest = rest_for(&server);
    let thumbs = Emoji::unicode("👍");
    rest.set_reaction(Snowflake(10), Snowflake(20), &thumbs, true)
        .await
        .unwrap();
    rest.set_reaction(Snowflake(10), Snowflake(20), &thumbs, false)
        .await
        .unwrap();
    let custom = Emoji {
        id: Some(Snowflake(77)),
        name: "ferris".into(),
        animated: false,
    };
    rest.set_reaction(Snowflake(10), Snowflake(20), &custom, true)
        .await
        .unwrap();
    let rec = server.recorded();
    assert_eq!(rec[0].method, "PUT");
    assert_eq!(
        rec[0].path,
        "/channels/10/messages/20/reactions/%F0%9F%91%8D/@me"
    );
    assert_eq!(rec[1].method, "DELETE");
    assert_eq!(
        rec[2].path,
        "/channels/10/messages/20/reactions/ferris:77/@me"
    );
}

#[tokio::test]
async fn jump_windows_and_status_setting() {
    let server = http(Arc::new(|req, _| {
        if req.path.starts_with("/channels/10/messages") {
            Response::json(200, json!([msg(30), msg(20), msg(10)]))
        } else {
            Response::json(200, json!({}))
        }
    }))
    .await;
    let rest = rest_for(&server);
    let around = rest
        .messages_around(Snowflake(10), Snowflake(20), 50)
        .await
        .unwrap();
    assert_eq!(
        around.iter().map(|m| m.id.0).collect::<Vec<_>>(),
        vec![10, 20, 30]
    );
    rest.messages_after(Snowflake(10), Snowflake(30), 50)
        .await
        .unwrap();
    rest.set_status_setting(Presence::Offline).await.unwrap();
    let rec = server.recorded();
    assert_eq!(rec[0].path, "/channels/10/messages?limit=50&around=20");
    assert_eq!(rec[1].path, "/channels/10/messages?limit=50&after=30");
    assert_eq!(
        (rec[2].method.as_str(), rec[2].path.as_str()),
        ("PATCH", "/users/@me/settings")
    );
    let b: serde_json::Value = serde_json::from_str(&rec[2].body).unwrap();
    // "Invisible" is spelled `invisible` on the wire.
    assert_eq!(b["status"], "invisible");
}

#[tokio::test]
async fn search_parses_hits_and_waits_for_the_index() {
    let server = http(Arc::new(|req, n| {
        assert!(
            req.path
                .starts_with("/guilds/9/messages/search?content=let%20chains&offset=25")
        );
        if n == 0 {
            // Index not ready yet.
            return Response::json(
                202,
                json!({"message": "Index not yet available.", "code": 110000, "retry_after": 0.2}),
            );
        }
        let mut hit = msg(55);
        hit["hit"] = json!(true);
        Response::json(
            200,
            json!({"total_results": 61, "messages": [[msg(54), hit, msg(56)], [{"broken": true}]]}),
        )
    }))
    .await;
    let rest = rest_for(&server);
    let start = Instant::now();
    let page = rest
        .search(SearchScope::Guild(Snowflake(9)), "let chains", 25)
        .await
        .unwrap();
    assert!(start.elapsed() >= Duration::from_millis(190));
    assert_eq!(page.total, 61);
    // The hit is picked out of its context group; the broken group is skipped.
    assert_eq!(page.messages.len(), 1);
    assert_eq!(page.messages[0].id.0, 55);
    assert_eq!(page.messages[0].guild_id, Some(Snowflake(9)));
    assert_eq!(server.recorded().len(), 2);
}

#[tokio::test]
async fn dm_search_uses_the_channel_endpoint() {
    let server = http(Arc::new(|req, _| {
        assert!(
            req.path
                .starts_with("/channels/10/messages/search?content=hi&offset=0")
        );
        Response::json(200, json!({"total_results": 0, "messages": []}))
    }))
    .await;
    let page = rest_for(&server)
        .search(SearchScope::Channel(Snowflake(10)), "hi", 0)
        .await
        .unwrap();
    assert_eq!(page.total, 0);
    assert!(page.messages.is_empty());
}

#[tokio::test]
async fn multipart_upload_streams_files_with_progress() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("notes.txt");
    let b = dir.path().join("log.json");
    // Big enough for several 64 KiB chunks.
    let a_body = "line of text\n".repeat(20_000);
    std::fs::write(&a, &a_body).unwrap();
    std::fs::write(&b, "{\"ok\":true}").unwrap();
    let files = vec![
        UploadFile {
            path: a.clone(),
            name: "notes.txt".into(),
            size: a_body.len() as u64,
        },
        UploadFile {
            path: b.clone(),
            name: "log.json".into(),
            size: 11,
        },
    ];
    let server = http(Arc::new(|_, _| {
        let mut m = msg(90);
        m["attachments"] = json!([{"id": "1", "filename": "notes.txt", "size": 1, "url": "https://cdn.discordapp.com/x"}]);
        Response::json(200, m)
    }))
    .await;
    let rest = rest_for(&server);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let reply = ReplyTarget {
        message: Snowflake(5),
        guild: None,
        mention: false,
    };
    let m = rest
        .send_with_files(Snowflake(10), "my caption", "nn", Some(reply), &files, tx)
        .await
        .unwrap();
    assert_eq!(m.attachments.len(), 1);

    let rec = &server.recorded()[0];
    assert_eq!(rec.path, "/channels/10/messages");
    let ct = rec.header("content-type").unwrap();
    let boundary = ct.strip_prefix("multipart/form-data; boundary=").unwrap();
    // The declared length is exactly the body we streamed.
    assert_eq!(
        rec.header("content-length")
            .unwrap()
            .parse::<usize>()
            .unwrap(),
        rec.body.len()
    );
    assert!(rec.body.starts_with(&format!("--{boundary}\r\n")));
    assert!(rec.body.ends_with(&format!("\r\n--{boundary}--\r\n")));
    assert!(rec.body.contains("name=\"payload_json\""));
    assert!(
        rec.body
            .contains("name=\"files[0]\"; filename=\"notes.txt\"")
    );
    assert!(
        rec.body
            .contains("name=\"files[1]\"; filename=\"log.json\"")
    );
    assert!(
        rec.body
            .contains("Content-Type: application/json\r\n\r\n{\"ok\":true}")
    );
    assert!(rec.body.contains(&a_body));
    // payload_json carries the caption, reply, nonce and attachment ids.
    let start = rec.body.find("\r\n\r\n").unwrap() + 4;
    let end = rec.body[start..].find("\r\n--").unwrap() + start;
    let payload: serde_json::Value = serde_json::from_str(&rec.body[start..end]).unwrap();
    assert_eq!(payload["content"], "my caption");
    assert_eq!(payload["nonce"], "nn");
    assert_eq!(payload["message_reference"]["message_id"], "5");
    assert_eq!(payload["attachments"][1]["filename"], "log.json");
    assert_eq!(payload["allowed_mentions"]["replied_user"], false);

    // Progress is monotonic and ends at the total length.
    let mut last = 0;
    let mut n = 0;
    while let Ok(v) = rx.try_recv() {
        assert!(v >= last);
        last = v;
        n += 1;
    }
    assert!(n > 2, "several progress reports, got {n}");
    assert_eq!(last as usize, rec.body.len());
}

#[tokio::test]
async fn upload_of_a_missing_file_fails_cleanly() {
    let server = http(Arc::new(|_, _| Response::json(200, msg(1)))).await;
    let files = vec![UploadFile {
        path: "/definitely/not/here.bin".into(),
        name: "here.bin".into(),
        size: 4,
    }];
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let r = tokio::time::timeout(
        Duration::from_secs(10),
        rest_for(&server).send_with_files(Snowflake(10), "", "n", None, &files, tx),
    )
    .await
    .expect("must not hang");
    assert!(r.is_err());
}

#[tokio::test]
async fn search_filters_become_query_parameters() {
    use ferrocord::net::extras::SearchQuery;
    let server = http(Arc::new(|_, _| {
        Response::json(200, json!({"total_results": 0, "messages": []}))
    }))
    .await;
    let rest = rest_for(&server);
    let q = SearchQuery {
        content: "let chains".into(),
        authors: vec![Snowflake(2)],
        has: vec!["image", "link"],
        channels: vec![Snowflake(10)],
    };
    rest.search_query(SearchScope::Guild(Snowflake(9)), &q, 25)
        .await
        .unwrap();
    // A filter-only search has no `content`; `in:` is ignored in DMs.
    let only = SearchQuery {
        has: vec!["file"],
        channels: vec![Snowflake(10)],
        ..Default::default()
    };
    rest.search_query(SearchScope::Channel(Snowflake(10)), &only, 0)
        .await
        .unwrap();
    let rec = server.recorded();
    assert_eq!(
        rec[0].path,
        "/guilds/9/messages/search?content=let%20chains&author_id=2&has=image&has=link&channel_id=10&offset=25&include_nsfw=true"
    );
    assert_eq!(
        rec[1].path,
        "/channels/10/messages/search?has=file&offset=0&include_nsfw=true"
    );
}

#[tokio::test]
async fn gifs_trending_and_search() {
    let server = http(Arc::new(|req, _| {
        let gif = |n: u32| {
            json!({"id": n.to_string(), "title": "", "url": format!("https://tenor.com/view/g-{n}"),
                   "src": format!("https://media.tenor.com/{n}.mp4"),
                   "gif_src": format!("https://media.tenor.com/{n}.gif"),
                   "preview": if n == 2 { String::new() } else { format!("https://media.tenor.com/{n}.png") },
                   "width": 220, "height": 120})
        };
        if req.path.starts_with("/gifs/trending-gifs") {
            Response::json(200, json!([gif(1)]))
        } else if req.path.starts_with("/gifs/search") {
            Response::json(200, json!([gif(2), {"url": "", "gif_src": "x"}]))
        } else {
            Response::json(404, json!({"message": "Unknown"}))
        }
    }))
    .await;
    let rest = RestClient::new(&server.base(), "http://cdn", Some(Secret::new(TOKEN))).unwrap();
    let trending = rest.gifs("  ").await.unwrap();
    assert_eq!(trending.len(), 1);
    assert_eq!(trending[0].url, "https://tenor.com/view/g-1");
    assert_eq!(trending[0].preview, "https://media.tenor.com/1.png");
    // No preview: falls back to the GIF; entries without a URL are dropped.
    let found = rest.gifs("happy cat").await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].preview, "https://media.tenor.com/2.gif");
    assert_eq!(found[0].gif_src, "https://media.tenor.com/2.gif");
    assert_eq!(found[0].src, "https://media.tenor.com/2.mp4");
    let rec = server.recorded();
    assert!(rec[0].path.starts_with("/gifs/trending-gifs?"));
    assert!(
        rec[1].path.starts_with("/gifs/search?q=happy%20cat&"),
        "{}",
        rec[1].path
    );
}

#[tokio::test]
async fn send_sticker_posts_sticker_ids() {
    let server = http(Arc::new(|req, _| {
        if req.method == "POST" && req.path == "/channels/10/messages" {
            let body: serde_json::Value = serde_json::from_str(&req.body).unwrap();
            let mut m = msg(77);
            m["content"] = json!("");
            m["sticker_items"] =
                json!([{"id": body["sticker_ids"][0], "name": "Wave", "format_type": 1}]);
            Response::json(200, m)
        } else {
            Response::json(404, json!({"message": "Unknown"}))
        }
    }))
    .await;
    let rest = RestClient::new(&server.base(), "http://cdn", Some(Secret::new(TOKEN))).unwrap();
    let m = rest
        .send_sticker(Snowflake(10), Snowflake(555), "n1", None)
        .await
        .unwrap();
    assert_eq!(m.stickers[0].id, Snowflake(555));
    let body: serde_json::Value = serde_json::from_str(&server.recorded()[0].body).unwrap();
    assert_eq!(body["sticker_ids"], json!(["555"]));
    assert_eq!(body["content"], json!(""));
    assert_eq!(body["nonce"], json!("n1"));
}

#[tokio::test]
async fn captcha_retry_sends_the_solution_headers() {
    let server = http(Arc::new(|req, n| {
        assert_eq!(req.path, "/users/@me/remote-auth/login");
        if n == 0 {
            Response::json(
                400,
                json!({
                    "captcha_key": ["captcha-required"],
                    "captcha_sitekey": "a9b5fb07-92ff-493f-86fe-352a2803b3df",
                    "captcha_service": "hcaptcha",
                    "captcha_rqdata": "rq-data",
                    "captcha_rqtoken": "rq-token",
                    "captcha_session_id": "sess-9"
                }),
            )
        } else {
            Response::json(200, json!({"encrypted_token": "ENC"}))
        }
    }))
    .await;
    let rest = RestClient::new(&server.base(), "http://cdn", None).unwrap();
    let err = rest
        .remote_auth_login_with("TICKET", Some("fp.1"), None)
        .await
        .unwrap_err();
    let Error::CaptchaRequired(challenge) = err else {
        panic!("{err:?}")
    };
    assert!(err_free(&challenge));
    let solution =
        ferrocord::net::captcha::CaptchaSolution::new("P1_user-solved", &challenge).unwrap();
    let enc = rest
        .remote_auth_login_with("TICKET", Some("fp.1"), Some(&solution))
        .await
        .unwrap();
    assert_eq!(enc, "ENC");
    let rec = server.recorded();
    assert_eq!(rec.len(), 2);
    assert!(rec[0].header("x-captcha-key").is_none());
    // The retry is the same request plus the official captcha headers.
    assert_eq!(rec[1].body, rec[0].body);
    assert_eq!(rec[1].header("x-captcha-key"), Some("P1_user-solved"));
    assert_eq!(rec[1].header("x-captcha-rqtoken"), Some("rq-token"));
    assert_eq!(rec[1].header("x-captcha-session-id"), Some("sess-9"));
    assert_eq!(rec[1].header("x-fingerprint"), Some("fp.1"));
}

fn err_free(c: &ferrocord::net::captcha::CaptchaChallenge) -> bool {
    c.is_solvable() && c.rqdata.as_deref() == Some("rq-data")
}

#[tokio::test]
async fn every_request_carries_the_honest_client_headers() {
    use base64::Engine;
    let server = http(Arc::new(|_, _| {
        Response::json(200, json!({"id": "1", "username": "me"}))
    }))
    .await;
    let rest = RestClient::new(&server.base(), "http://cdn", Some(Secret::new(TOKEN))).unwrap();
    rest.me().await.unwrap();
    let r = &server.recorded()[0];
    assert_eq!(
        r.header("user-agent"),
        Some(ferrocord::net::rest::user_agent().as_str())
    );
    let sp = base64::engine::general_purpose::STANDARD
        .decode(r.header("x-super-properties").unwrap())
        .unwrap();
    let sp: serde_json::Value = serde_json::from_slice(&sp).unwrap();
    assert_eq!(sp["browser"], "Chrome");
    assert_eq!(sp["browser_user_agent"], ferrocord::net::rest::user_agent());
    assert_eq!(
        r.header("x-discord-locale"),
        Some(ferrocord::net::identity::locale())
    );
    assert_eq!(
        r.header("x-discord-timezone"),
        ferrocord::net::identity::timezone()
    );
}

#[tokio::test]
async fn fingerprint_comes_from_experiments_and_rejects_garbage() {
    let server = http(Arc::new(|req, n| {
        assert_eq!(req.path, "/experiments");
        match n {
            0 => Response::json(200, json!({"fingerprint": "111.AbC-_", "assignments": []})),
            1 => Response::json(200, json!({"fingerprint": "bad\r\nvalue"})),
            2 => Response::json(200, json!({"assignments": []})),
            _ => Response::json(500, json!({})),
        }
    }))
    .await;
    let rest = RestClient::new(&server.base(), "http://cdn", None).unwrap();
    assert_eq!(rest.fingerprint().await.as_deref(), Some("111.AbC-_"));
    assert_eq!(rest.fingerprint().await, None);
    assert_eq!(rest.fingerprint().await, None);
    assert_eq!(rest.fingerprint().await, None);
}

#[test]
fn gif_entries_keep_every_rendition_whatever_the_host() {
    use ferrocord::net::rest::parse_gif;
    // Tenor serves from several hosts; the picker uses what the API says.
    let g = parse_gif(&json!({"url": "https://tenor.com/view/x-1",
        "src": "https://media1.tenor.com/m/abcdefghijkAAAPo/x.mp4",
        "gif_src": "https://media1.tenor.com/m/abcdefghijkAAAAC/x.gif",
        "preview": "https://media1.tenor.com/m/abcdefghijkAAAAe/x.png",
        "width": 498, "height": 0}))
    .unwrap();
    assert_eq!(
        g.preview,
        "https://media1.tenor.com/m/abcdefghijkAAAAe/x.png"
    );
    assert_eq!(g.height, 1);
    assert!(parse_gif(&json!({"url": "https://tenor.com/view/x", "src": "a.mp4"})).is_none());
    assert!(parse_gif(&json!("garbage")).is_none());
}

#[tokio::test]
async fn picker_media_needs_https() {
    let rest = RestClient::new("http://127.0.0.1:1", "http://cdn", None).unwrap();
    for bad in [
        "http://media1.tenor.com/m/a/x.png",
        "file:///etc/passwd",
        "not a url",
    ] {
        assert!(rest.fetch_picker_media(bad).await.is_err(), "{bad}");
    }
}

#[tokio::test]
async fn reactors_are_fetched_with_type_and_paging() {
    let server = http(Arc::new(|req, _| {
        if req.path.starts_with("/channels/10/messages/20/reactions/") {
            Response::json(
                200,
                json!([
                    {"id": "3", "username": "carol"},
                    {"id": "4", "username": null},
                    {"id": "5", "username": "eve", "global_name": "Eve"}
                ]),
            )
        } else {
            Response::json(404, json!({"message": "Unknown"}))
        }
    }))
    .await;
    let rest = RestClient::new(&server.base(), "http://cdn", Some(Secret::new(TOKEN))).unwrap();
    let thumbs = Emoji::unicode("\u{1F44D}");
    let users = rest
        .reactions(Snowflake(10), Snowflake(20), &thumbs, false, 3, None)
        .await
        .unwrap();
    // An unreadable user is skipped, not the page.
    assert_eq!(users.iter().map(|u| u.id.0).collect::<Vec<_>>(), vec![3, 5]);
    let custom = Emoji {
        id: Some(Snowflake(99)),
        name: "ferris".into(),
        animated: false,
    };
    rest.reactions(
        Snowflake(10),
        Snowflake(20),
        &custom,
        true,
        100,
        Some(Snowflake(5)),
    )
    .await
    .unwrap();
    let r = server.recorded();
    assert_eq!(
        r[0].path,
        "/channels/10/messages/20/reactions/%F0%9F%91%8D?limit=3&type=0"
    );
    assert_eq!(
        r[1].path,
        "/channels/10/messages/20/reactions/ferris:99?limit=100&type=1&after=5"
    );
}
