use super::api::session_user;
use crate::harbinger::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use rand_core::{OsRng, RngCore};
use serde_json::{json, Value};
use std::{collections::HashMap, time::Duration};

const PING_INTERVAL: Duration = Duration::from_secs(20);

/// Minimal Socket.IO v4 / Engine.IO v4 endpoint used by Audiobookshelf
/// clients. Clients connect over WebSocket, send `auth`, and wait for `init`
/// before treating the server as online.
pub(crate) async fn socket_io(
    State(state): State<AppState>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if query.get("transport").map(String::as_str) != Some("websocket") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    upgrade
        .on_upgrade(move |socket| handle_socket(socket, state, headers))
        .into_response()
}

async fn handle_socket(mut socket: WebSocket, state: AppState, headers: HeaderMap) {
    let sid = random_sid();
    let handshake = format!(
        "0{}",
        json!({
            "sid": sid,
            "upgrades": [],
            "pingInterval": 25000,
            "pingTimeout": 20000,
            "maxPayload": 1000000
        })
    );
    if socket.send(Message::Text(handshake.into())).await.is_err() {
        return;
    }
    let header_token = bearer_token(&headers);
    loop {
        match tokio::time::timeout(PING_INTERVAL, socket.recv()).await {
            Err(_) => {
                if socket.send(Message::Text("2".into())).await.is_err() {
                    return;
                }
            }
            Ok(None) => return,
            Ok(Some(Err(_))) => return,
            Ok(Some(Ok(message))) => match message {
                Message::Text(text) => {
                    if !handle_packet(
                        &mut socket,
                        &state,
                        text.as_str(),
                        &sid,
                        header_token.as_deref(),
                    )
                    .await
                    {
                        return;
                    }
                }
                Message::Ping(payload) => {
                    if socket.send(Message::Pong(payload)).await.is_err() {
                        return;
                    }
                }
                Message::Close(_) => return,
                _ => {}
            },
        }
    }
}

async fn handle_packet(
    socket: &mut WebSocket,
    state: &AppState,
    packet: &str,
    sid: &str,
    header_token: Option<&str>,
) -> bool {
    let packet = packet.trim();
    if packet == "2" {
        return socket.send(Message::Text("3".into())).await.is_ok();
    }
    if packet == "41" {
        return false;
    }
    if let Some(rest) = packet.strip_prefix("40") {
        if rest.is_empty() || rest.starts_with('{') {
            let connect = format!("40{}", json!({ "sid": sid }));
            if socket.send(Message::Text(connect.into())).await.is_err() {
                return false;
            }
        }
        return true;
    }
    let Some(payload) = packet
        .strip_prefix("42")
        .and_then(|payload| serde_json::from_str::<Value>(payload).ok())
    else {
        return true;
    };
    let Some(event) = payload.as_array() else {
        return true;
    };
    let Some(name) = event.first().and_then(Value::as_str) else {
        return true;
    };
    if name != "auth" {
        return true;
    }
    let token = event
        .get(1)
        .and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("token").and_then(Value::as_str))
        })
        .or(header_token);
    let Some(identity) = token.and_then(|token| session_user(state, token)) else {
        let failure = format!("42{}", json!(["auth_failed", "Invalid token"]));
        let _ = socket.send(Message::Text(failure.into())).await;
        return false;
    };
    let init = format!(
        "42{}",
        json!([
            "init",
            {
                "userId": identity.id,
                "username": identity.username,
                "usersOnline": []
            }
        ])
    );
    socket.send(Message::Text(init.into())).await.is_ok()
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::to_owned)
}

fn random_sid() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
