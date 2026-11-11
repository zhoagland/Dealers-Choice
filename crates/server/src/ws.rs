//! `/ws?table=<id>`: one socket per browser tab (web.md §3). Auth by the session cookie, or inside
//! the Discord Activity by `?token=` (cookies are unreliable in Discord's iframe, §13). The `Origin`
//! must be our own site (or Discord's Activity proxy). Messages are capped at 16 KB and rate-limited.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;

use protocol::{ClientMsg, ServerMsg};
use storage::Account;

use crate::table::Cmd;
use crate::{AppState, Shared, auth};

#[derive(Deserialize)]
pub struct WsQuery {
    table: String,
    token: Option<String>,
}

fn origin_ok(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        // Non-browser clients send no Origin; they still need a session.
        return true;
    };
    if state.config.dev_mode {
        return true;
    }
    let ours = state.config.public_url.trim_end_matches('/');
    if origin == ours {
        return true;
    }
    match &state.config.discord_client_id {
        Some(id) => origin == format!("https://{id}.discordsays.com"),
        None => false,
    }
}

pub async fn ws(
    State(state): State<Shared>,
    Query(q): Query<WsQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !origin_ok(&state, &headers) {
        return (StatusCode::FORBIDDEN, "bad origin").into_response();
    }
    let Some(account) = auth::account(&state, &headers, q.token.as_deref()) else {
        return (StatusCode::UNAUTHORIZED, "sign in first").into_response();
    };
    let Some(tx) = state
        .tables
        .lock()
        .unwrap()
        .get(&q.table)
        .map(|t| t.tx.clone())
    else {
        return (StatusCode::NOT_FOUND, "no such table").into_response();
    };
    let conn = state.next_conn.fetch_add(1, Ordering::Relaxed);
    upgrade
        .max_message_size(16 * 1024)
        .on_upgrade(move |socket| run(socket, account, tx, conn))
}

async fn run(socket: WebSocket, account: Account, tx: std::sync::mpsc::Sender<Cmd>, conn: u64) {
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
    if tx
        .send(Cmd::Join {
            conn,
            account,
            tx: out_tx,
        })
        .is_err()
    {
        return;
    }
    let (mut sink, mut stream) = socket.split();
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let text = serde_json::to_string(&msg).expect("server messages serialize");
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    // At most 20 messages a second per connection.
    let mut window = Instant::now();
    let mut count = 0u32;
    loop {
        let next = tokio::time::timeout(Duration::from_secs(90), stream.next()).await;
        let Ok(Some(Ok(msg))) = next else {
            break; // closed, errored, or silent for 90 s (clients ping every 25 s)
        };
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };
        if window.elapsed() > Duration::from_secs(1) {
            window = Instant::now();
            count = 0;
        }
        count += 1;
        if count > 20 {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<ClientMsg>(&text) else {
            continue;
        };
        if tx.send(Cmd::Msg { conn, msg }).is_err() {
            break;
        }
    }
    let _ = tx.send(Cmd::Leave { conn });
    writer.abort();
}
