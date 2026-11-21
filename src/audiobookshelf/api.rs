use super::{
    catalog::{image_content_type, Catalog, Item, SERVER_VERSION},
    dto::{
        item_json, library_detail_json, library_json, login_envelope, media_progress_json,
        playback_session_json, user_json, PlaybackSession, USER_ID,
    },
};
use argon2::{password_hash::PasswordHash, Argon2, PasswordVerifier};
use axum::{
    body::{to_bytes, Body},
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand_core::{OsRng, RngCore};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

const TOKEN_TTL_SECONDS: i64 = 60 * 60 * 24 * 14;
const REFRESH_TTL_SECONDS: i64 = 60 * 60 * 24 * 30;
const MAX_BODY_BYTES: usize = 1024 * 1024;

static SESSIONS: OnceLock<Mutex<HashMap<String, PlaybackSession>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<String, PlaybackSession>> {
    SESSIONS.get_or_init(Default::default)
}

pub(crate) fn router(state: crate::harbinger::AppState) -> Router {
    let discovery = Router::new()
        .route("/login", post(login))
        .route("/auth/refresh", post(refresh))
        .route("/logout", post(logout))
        .route("/ping", get(ping))
        .route("/healthcheck", get(healthcheck))
        .route("/init", get(init))
        .route("/status", get(status))
        .with_state(state.clone());
    let api = Router::new()
        .route("/authorize", get(authorize))
        .route("/libraries", get(libraries))
        .route("/libraries/:id", get(library))
        .route("/libraries/:id/items", get(library_items))
        .route("/items/:id", get(item))
        .route("/items/:id/cover", get(cover))
        .route(
            "/items/:id/file/:ino",
            get(stream_file_by_ino).head(stream_file_by_ino),
        )
        .route("/items/:id/play", post(play))
        .route("/items/:id/play/:episode", post(play))
        .route("/me", get(me))
        .route("/me/items-in-progress", get(items_in_progress))
        .route("/me/progress", get(all_progress))
        .route("/me/progress/batch/update", patch(batch_progress))
        .route(
            "/me/progress/:id",
            get(get_progress)
                .patch(update_progress)
                .post(update_progress),
        )
        .route(
            "/me/progress/:id/:episode",
            get(get_progress_with_episode)
                .patch(update_progress_with_episode)
                .post(update_progress_with_episode),
        )
        .route("/session/:id", get(session))
        .route("/session/:id/sync", post(sync_session))
        .route("/session/:id/close", post(close_session))
        .route(
            "/s/item/:id/:filename",
            get(stream_track).head(stream_track),
        )
        .with_state(state.clone());
    let public = Router::new()
        .route(
            "/public/session/:session/track/:index",
            get(public_session_track).head(public_session_track),
        )
        .with_state(state);
    Router::new()
        .merge(public)
        .merge(discovery.clone())
        .merge(api.clone())
        .nest("/api", discovery.clone())
        .nest("/api", api.clone())
        .nest("/abs/api", discovery)
        .nest("/abs/api", api)
        .layer(middleware::from_fn(cors))
        .fallback(|| async { StatusCode::NOT_FOUND })
}

async fn cors(request: Request<Body>, next: Next) -> Response {
    if request.method() == Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("GET, POST, PATCH, DELETE, OPTIONS"),
        );
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("Authorization, Content-Type, X-Requested-With"),
        );
        return response;
    }
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    response
}

#[derive(Debug, Default, Deserialize)]
struct LoginRequest {
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
}

#[derive(Debug, Default, Deserialize)]
struct RefreshRequest {
    #[serde(default, alias = "refresh_token")]
    refresh_token: String,
    #[serde(default, alias = "token")]
    token: String,
}

#[derive(Debug, Default, Deserialize)]
struct ProgressRequest {
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    progress: Option<f64>,
    #[serde(default, alias = "currentTime")]
    current_time: Option<f64>,
    #[serde(default, alias = "isFinished")]
    is_finished: Option<bool>,
    #[serde(default, alias = "hideFromContinueListening")]
    hide_from_continue_listening: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct BatchProgressRequest {
    #[serde(default, alias = "libraryItemId")]
    library_item_id: String,
    #[serde(flatten)]
    progress: ProgressRequest,
}

#[derive(Debug, Default, Deserialize)]
struct SessionSyncRequest {
    #[serde(default, alias = "currentTime")]
    current_time: Option<f64>,
    #[serde(default, alias = "timeListened")]
    time_listened: Option<f64>,
    #[serde(default)]
    duration: Option<f64>,
}

async fn login(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Response {
    let return_tokens = headers
        .get("x-return-tokens")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("true"));
    let body = match read_body(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let credentials = parse_credentials(&body);
    if credentials.username.trim().is_empty()
        || credentials.password.is_empty()
        || !password_valid(&state, &credentials.password)
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let (access_token, refresh_token) = match issue_tokens(&state) {
        Ok(tokens) => tokens,
        Err(status) => return status.into_response(),
    };
    let catalog = Catalog::load(&state);
    let default_library_id = catalog.default_library_id();
    let user = user_json(
        &access_token,
        &default_library_id,
        progress_values(&state, &catalog),
        Some(&access_token),
        return_tokens.then_some(refresh_token.as_str()),
    );
    let mut response = Json(login_envelope(user, &default_library_id)).into_response();
    if !return_tokens {
        if let Ok(value) = HeaderValue::from_str(&format!(
            "refresh_token={refresh_token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={REFRESH_TTL_SECONDS}"
        )) {
            response.headers_mut().insert(header::SET_COOKIE, value);
        }
    }
    response
}

async fn refresh(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    request: Request<Body>,
) -> Response {
    let body = match read_body(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let payload = serde_json::from_slice::<RefreshRequest>(&body).unwrap_or_default();
    let token = headers
        .get("x-refresh-token")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            if !payload.refresh_token.is_empty() {
                Some(payload.refresh_token.clone())
            } else if !payload.token.is_empty() {
                Some(payload.token.clone())
            } else {
                None
            }
        });
    let Some(token) = token else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !token_valid(&state, &token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let (access_token, refresh_token) = match issue_tokens(&state) {
        Ok(tokens) => tokens,
        Err(status) => return status.into_response(),
    };
    let catalog = Catalog::load(&state);
    let default_library_id = catalog.default_library_id();
    let user = user_json(
        &access_token,
        &default_library_id,
        progress_values(&state, &catalog),
        Some(&access_token),
        Some(&refresh_token),
    );
    Json(login_envelope(user, &default_library_id)).into_response()
}

async fn logout(State(state): State<crate::harbinger::AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = bearer_from_headers(&headers) {
        revoke_token(&state, &token);
    }
    Json(json!({ "success": true })).into_response()
}

async fn ping() -> Response {
    Json(json!({
        "success": true,
        "server": "audiobookshelf",
        "version": SERVER_VERSION,
        "pong": true
    }))
    .into_response()
}

async fn healthcheck() -> Response {
    StatusCode::OK.into_response()
}

async fn init() -> Response {
    Json(json!({ "isInit": true })).into_response()
}

async fn status() -> Response {
    Json(json!({
        "app": "audiobookshelf",
        "serverVersion": SERVER_VERSION,
        "isInit": true,
        "language": "en-us",
        "authMethods": ["local"],
        "authFormData": {}
    }))
    .into_response()
}

async fn authorize(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let token = match require_auth(&state, &headers, &uri) {
        Ok(token) => token,
        Err(response) => return response,
    };
    let catalog = Catalog::load(&state);
    let default_library_id = catalog.default_library_id();
    let user = user_json(
        &token,
        &default_library_id,
        progress_values(&state, &catalog),
        None,
        None,
    );
    Json(login_envelope(user, &default_library_id)).into_response()
}

async fn libraries(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load(&state);
    Json(json!({
        "libraries": catalog.libraries.iter().map(library_json).collect::<Vec<_>>()
    }))
    .into_response()
}

async fn library(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load(&state);
    match catalog.find_library(&id) {
        Some(library) => Json(library_detail_json(library)).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn library_items(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load(&state);
    let Some(library) = catalog.find_library(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let page = query_value(&uri, "page")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1);
    let limit = query_value(&uri, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(50)
        .min(500);
    let sort_desc = query_value(&uri, "sortDesc")
        .or_else(|| query_value(&uri, "desc"))
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "True"));
    let minified = query_value(&uri, "minified")
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "True"));
    let sort_by = query_value(&uri, "sortBy")
        .or_else(|| query_value(&uri, "sort"))
        .unwrap_or_else(|| "media.metadata.title".to_owned());
    let filter_by = query_value(&uri, "filterBy")
        .or_else(|| query_value(&uri, "filter"))
        .unwrap_or_default();
    let mut items = library.items.iter().collect::<Vec<_>>();
    items.sort_by(|left, right| {
        let ordering = left
            .title
            .cmp(&right.title)
            .then_with(|| left.id.cmp(&right.id));
        if sort_desc {
            ordering.reverse()
        } else {
            ordering
        }
    });
    let total = items.len();
    let start = (page - 1).saturating_mul(limit).min(total);
    let end = start.saturating_add(limit).min(total);
    let results = items[start..end]
        .iter()
        .map(|item| item_json(item, minified))
        .collect::<Vec<_>>();
    Json(json!({
        "results": results,
        "total": total,
        "limit": limit,
        "page": page,
        "sortBy": sort_by,
        "sortDesc": sort_desc,
        "filterBy": filter_by,
        "mediaType": "book",
        "minified": minified,
        "collapseseries": false,
        "include": "",
        "offset": start
    }))
    .into_response()
}

async fn item(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load(&state);
    let Some(item) = catalog.find_item(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut response = item_json(item, false);
    let include = query_value(&uri, "include").unwrap_or_default();
    let expanded = query_value(&uri, "expanded")
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "True"));
    if expanded || include.contains("progress") {
        if let Some((current_time, duration)) = read_progress(&state, item) {
            let row = state.db.lock().ok().and_then(|db| {
                db.query_row(
                    "SELECT watched, hidden_from_continue, updated_at FROM watch_progress WHERE user_id = ?1 AND media_key = ?2",
                    params![USER_ID, item.id],
                    |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?, row.get::<_, i64>(2)?)),
                )
                .ok()
            });
            if let Some((watched, hidden, updated)) = row {
                response["userProgress"] = media_progress_json(
                    item,
                    current_time,
                    if duration > 0.0 {
                        duration
                    } else {
                        item.duration
                    },
                    watched,
                    hidden,
                    updated * 1000,
                );
            }
        }
    }
    Json(response).into_response()
}

async fn cover(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
) -> Response {
    let catalog = Catalog::load(&state);
    let Some(item) = catalog.find_item(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(path) = &item.cover_path else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(bytes) = std::fs::read(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = image_content_type(path.as_path());
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static(content_type))],
        Body::from(bytes),
    )
        .into_response()
}

async fn play(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load(&state);
    let Some(item) = catalog.find_item(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(library) = catalog.item_library(item) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let progress = read_progress(&state, item);
    let now = now_ms();
    let session = PlaybackSession {
        id: format!("play_{}", random_hex(16)),
        user_id: USER_ID.to_owned(),
        library_id: library.id.clone(),
        library_item_id: item.id.clone(),
        current_time: progress.as_ref().map(|row| row.0).unwrap_or(0.0),
        time_listened: 0.0,
        duration: if item.duration > 0.0 {
            item.duration
        } else {
            progress.as_ref().map(|row| row.1).unwrap_or(0.0)
        },
        started_at: now,
        updated_at: now,
    };
    if let Ok(mut sessions) = sessions().lock() {
        sessions.insert(session.id.clone(), session.clone());
    }
    Json(playback_session_json(&session, item)).into_response()
}

async fn me(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let token = match require_auth(&state, &headers, &uri) {
        Ok(token) => token,
        Err(response) => return response,
    };
    let catalog = Catalog::load(&state);
    Json(user_json(
        &token,
        &catalog.default_library_id(),
        progress_values(&state, &catalog),
        None,
        None,
    ))
    .into_response()
}

async fn all_progress(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load(&state);
    Json(progress_values(&state, &catalog)).into_response()
}

async fn items_in_progress(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let limit = query_value(&uri, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(25);
    let catalog = Catalog::load(&state);
    let mut results = progress_values(&state, &catalog)
        .into_iter()
        .filter(|progress| {
            progress["isFinished"].as_bool() == Some(false)
                && progress["hideFromContinueListening"].as_bool() == Some(false)
        })
        .collect::<Vec<_>>();
    results.truncate(limit);
    Json(json!({
        "results": results,
        "total": results.len(),
        "limit": limit,
        "page": 1
    }))
    .into_response()
}

async fn get_progress(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    progress_response(&state, &id, &headers, &uri).await
}

async fn get_progress_with_episode(
    State(state): State<crate::harbinger::AppState>,
    Path((id, episode)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let _ = episode;
    progress_response(&state, &id, &headers, &uri).await
}

async fn progress_response(
    state: &crate::harbinger::AppState,
    id: &str,
    headers: &HeaderMap,
    uri: &Uri,
) -> Response {
    if let Err(response) = require_auth(state, headers, uri) {
        return response;
    }
    let catalog = Catalog::load(state);
    let Some(item) = catalog.find_item(id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((current_time, duration)) = read_progress(state, item) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let row = state.db.lock().ok().and_then(|db| {
        db.query_row(
            "SELECT watched, hidden_from_continue, updated_at FROM watch_progress WHERE user_id = ?1 AND media_key = ?2",
            params![USER_ID, item.id],
            |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?, row.get::<_, i64>(2)?)),
        )
        .ok()
    });
    let Some((watched, hidden, updated)) = row else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(media_progress_json(
        item,
        current_time,
        if duration > 0.0 {
            duration
        } else {
            item.duration
        },
        watched,
        hidden,
        updated * 1000,
    ))
    .into_response()
}

async fn update_progress(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    request: Request<Body>,
) -> Response {
    update_progress_impl(state, id, request).await
}

async fn update_progress_with_episode(
    State(state): State<crate::harbinger::AppState>,
    Path((id, episode)): Path<(String, String)>,
    request: Request<Body>,
) -> Response {
    let _ = episode;
    update_progress_impl(state, id, request).await
}

async fn update_progress_impl(
    state: crate::harbinger::AppState,
    id: String,
    request: Request<Body>,
) -> Response {
    let headers = request.headers().clone();
    let uri = request.uri().clone();
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let body = match read_body(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let payload = parse_json_or_default::<ProgressRequest>(&body);
    let catalog = Catalog::load(&state);
    let Some(item) = catalog.find_item(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match save_progress(&state, item, &payload) {
        Ok(progress) => Json(progress).into_response(),
        Err(status) => status.into_response(),
    }
}

async fn batch_progress(
    State(state): State<crate::harbinger::AppState>,
    request: Request<Body>,
) -> Response {
    let headers = request.headers().clone();
    let uri = request.uri().clone();
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let body = match read_body(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let payloads = parse_json_or_default::<Vec<BatchProgressRequest>>(&body);
    if payloads.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let catalog = Catalog::load(&state);
    let mut results = Vec::new();
    for payload in payloads {
        let Some(item) = catalog.find_item(&payload.library_item_id) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        match save_progress(&state, item, &payload.progress) {
            Ok(progress) => results.push(progress),
            Err(status) => return status.into_response(),
        }
    }
    Json(json!({ "progresses": results })).into_response()
}

async fn session(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let Some(session) = sessions()
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&id).cloned())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let catalog = Catalog::load(&state);
    let Some(item) = catalog.find_item(&session.library_item_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(playback_session_json(&session, item)).into_response()
}

async fn sync_session(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    request: Request<Body>,
) -> Response {
    let headers = request.headers().clone();
    let uri = request.uri().clone();
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let body = match read_body(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let payload = parse_json_or_default::<SessionSyncRequest>(&body);
    let catalog = Catalog::load(&state);
    let Some(session) = sessions()
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&id).cloned())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(item) = catalog.find_item(&session.library_item_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut updated = session.clone();
    if let Some(current_time) = payload.current_time {
        updated.current_time = current_time.max(0.0);
    }
    if let Some(duration) = payload.duration {
        updated.duration = duration.max(0.0);
    }
    if let Some(time_listened) = payload.time_listened {
        updated.time_listened += time_listened.max(0.0);
    }
    updated.updated_at = now_ms();
    if let Ok(mut sessions) = sessions().lock() {
        sessions.insert(id.clone(), updated.clone());
    }
    let progress = save_progress(
        &state,
        item,
        &ProgressRequest {
            duration: Some(updated.duration),
            progress: None,
            current_time: Some(updated.current_time),
            is_finished: None,
            hide_from_continue_listening: None,
        },
    );
    if let Err(status) = progress {
        return status.into_response();
    }
    Json(playback_session_json(&updated, item)).into_response()
}

async fn close_session(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    request: Request<Body>,
) -> Response {
    let headers = request.headers().clone();
    let uri = request.uri().clone();
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let body = match read_body(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let payload = parse_json_or_default::<SessionSyncRequest>(&body);
    let catalog = Catalog::load(&state);
    let Some(session) = sessions()
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&id).cloned())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(item) = catalog.find_item(&session.library_item_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let current_time = payload
        .current_time
        .unwrap_or(session.current_time)
        .max(0.0);
    let duration = payload.duration.unwrap_or(session.duration).max(0.0);
    if let Ok(mut sessions) = sessions().lock() {
        sessions.remove(&id);
    }
    if let Err(status) = save_progress(
        &state,
        item,
        &ProgressRequest {
            duration: Some(duration),
            progress: None,
            current_time: Some(current_time),
            is_finished: None,
            hide_from_continue_listening: None,
        },
    ) {
        return status.into_response();
    }
    Json(json!({ "success": true })).into_response()
}

async fn stream_file_by_ino(
    State(state): State<crate::harbinger::AppState>,
    Path((id, ino)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    method: Method,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load(&state);
    let Some(item) = catalog.find_item(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(file) = item
        .audio_files
        .iter()
        .find(|file| file.index.to_string() == ino)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    serve_audio(item, file, method, headers).await
}

async fn public_session_track(
    State(state): State<crate::harbinger::AppState>,
    Path((session_id, index)): Path<(String, usize)>,
    headers: HeaderMap,
    method: Method,
) -> Response {
    let Some(session) = sessions()
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&session_id).cloned())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let catalog = Catalog::load(&state);
    let Some(item) = catalog.find_item(&session.library_item_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(file) = item.audio_files.iter().find(|file| file.index == index) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    serve_audio(item, file, method, headers).await
}

async fn stream_track(
    State(state): State<crate::harbinger::AppState>,
    Path((id, filename)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
    method: Method,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load(&state);
    let Some(item) = catalog.find_item(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(file) = item.audio_files.iter().find(|file| {
        file.path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == filename)
    }) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    serve_audio(item, file, method, headers).await
}

async fn serve_audio(
    item: &Item,
    file: &super::catalog::AudioFile,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let media = crate::harbinger::Media {
        id: item.id.clone(),
        title: file.title.clone(),
        path: file.path.clone(),
        size: file.size,
        container: file
            .path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_owned(),
        mime: file.mime.clone(),
        metadata_revision: file.modified_ms as u64,
    };
    crate::harbinger::stream_file(&media, method, headers, false).await
}

fn require_auth(
    state: &crate::harbinger::AppState,
    headers: &HeaderMap,
    uri: &Uri,
) -> Result<String, Response> {
    let token = bearer_from_headers(headers).or_else(|| {
        uri.query().and_then(|query| {
            form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == "token")
                .map(|(_, value)| value.into_owned())
        })
    });
    match token.filter(|token| token_valid(state, token)) {
        Some(token) => Ok(token),
        None => Err(StatusCode::UNAUTHORIZED.into_response()),
    }
}

fn bearer_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
}

fn token_valid(state: &crate::harbinger::AppState, token: &str) -> bool {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM web_sessions WHERE token_hash = ?1 AND expires_at > ?2)",
                params![hash_token(token), now_seconds()],
                |row| row.get::<_, bool>(0),
            )
            .ok()
        })
        .unwrap_or(false)
}

fn revoke_token(state: &crate::harbinger::AppState, token: &str) {
    if let Ok(db) = state.db.lock() {
        let _ = db.execute(
            "DELETE FROM web_sessions WHERE token_hash = ?1",
            params![hash_token(token)],
        );
    }
}

fn password_valid(state: &crate::harbinger::AppState, password: &str) -> bool {
    let hash = state.db.lock().ok().and_then(|db| {
        db.query_row(
            "SELECT password_hash FROM web_admin WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok()
    });
    match hash {
        Some(hash) => PasswordHash::new(&hash).ok().is_some_and(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        }),
        None => false,
    }
}

fn issue_tokens(state: &crate::harbinger::AppState) -> Result<(String, String), StatusCode> {
    let access = random_token();
    let refresh = random_token();
    let now = now_seconds();
    let db = state
        .db
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.execute(
        "INSERT INTO web_sessions (token_hash, expires_at) VALUES (?1, ?2)",
        params![hash_token(&access), now + TOKEN_TTL_SECONDS],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.execute(
        "INSERT INTO web_sessions (token_hash, expires_at) VALUES (?1, ?2)",
        params![hash_token(&refresh), now + REFRESH_TTL_SECONDS],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((access, refresh))
}

fn read_progress(state: &crate::harbinger::AppState, item: &Item) -> Option<(f64, f64)> {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            db.query_row(
                "SELECT position_ms, duration_ms FROM watch_progress WHERE user_id = ?1 AND media_key = ?2",
                params![USER_ID, item.id],
                |row| {
                    let position = row.get::<_, i64>(0)?;
                    let duration = row.get::<_, Option<i64>>(1)?;
                    Ok((position, duration))
                },
            )
            .ok()
        })
        .map(|(position, duration)| {
            (
                position as f64 / 1000.0,
                duration.unwrap_or_default() as f64 / 1000.0,
            )
        })
}

fn progress_values(state: &crate::harbinger::AppState, catalog: &Catalog) -> Vec<Value> {
    let rows = state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let mut statement = db
                .prepare(
                    "SELECT media_key, position_ms, duration_ms, watched, hidden_from_continue, updated_at
                       FROM watch_progress WHERE user_id = ?1 ORDER BY updated_at DESC",
                )
                .ok()?;
            let rows = statement
                .query_map(params![USER_ID], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, bool>(3)?,
                        row.get::<_, bool>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                })
                .ok()?;
            Some(rows.flatten().collect::<Vec<_>>())
        })
        .unwrap_or_default();
    rows.into_iter()
        .filter_map(|(key, position, duration, watched, hidden, updated)| {
            let item = catalog.find_item(&key)?;
            Some(media_progress_json(
                item,
                position as f64 / 1000.0,
                duration.unwrap_or_default() as f64 / 1000.0,
                watched,
                hidden,
                (updated as i128 * 1000) as i64,
            ))
        })
        .collect()
}

fn save_progress(
    state: &crate::harbinger::AppState,
    item: &Item,
    payload: &ProgressRequest,
) -> Result<Value, StatusCode> {
    let duration = payload.duration.unwrap_or(item.duration).max(0.0);
    let current_time = payload.current_time.unwrap_or(0.0).max(0.0);
    let ratio = if duration > 0.0 {
        (current_time / duration).clamp(0.0, 1.0)
    } else {
        payload.progress.unwrap_or(0.0).clamp(0.0, 1.0)
    };
    let finished = payload
        .is_finished
        .unwrap_or_else(|| ratio >= 0.995 || payload.progress.is_some_and(|value| value >= 0.995));
    let hidden = payload.hide_from_continue_listening.unwrap_or(false);
    let position_ms = (current_time * 1000.0).round() as i64;
    let duration_ms = if duration > 0.0 {
        Some((duration * 1000.0).round() as i64)
    } else {
        None
    };
    let updated = now_seconds();
    {
        let db = state
            .db
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        db.execute(
            "INSERT INTO watch_progress
                (user_id, media_key, position_ms, duration_ms, watched, hidden_from_continue, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(user_id, media_key) DO UPDATE SET
                position_ms = excluded.position_ms,
                duration_ms = COALESCE(excluded.duration_ms, watch_progress.duration_ms),
                watched = excluded.watched,
                hidden_from_continue = excluded.hidden_from_continue,
                updated_at = excluded.updated_at",
            params![
                USER_ID,
                item.id,
                position_ms,
                duration_ms,
                finished,
                hidden,
                updated
            ],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok(media_progress_json(
        item,
        current_time,
        duration,
        finished,
        hidden,
        updated * 1000,
    ))
}

async fn read_body(request: Request<Body>) -> Result<Vec<u8>, Response> {
    let (_, body) = request.into_parts();
    to_bytes(body, MAX_BODY_BYTES)
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|_| StatusCode::BAD_REQUEST.into_response())
}

fn parse_credentials(body: &[u8]) -> LoginRequest {
    serde_json::from_slice::<LoginRequest>(body)
        .ok()
        .filter(|credentials| !credentials.username.trim().is_empty())
        .or_else(|| {
            let form = form_urlencoded::parse(body).collect::<Vec<_>>();
            let username = form
                .iter()
                .find(|(key, _)| key == "username")
                .map(|(_, value)| value.to_string())
                .unwrap_or_default();
            let password = form
                .iter()
                .find(|(key, _)| key == "password")
                .map(|(_, value)| value.to_string())
                .unwrap_or_default();
            Some(LoginRequest { username, password })
        })
        .unwrap_or_default()
}

fn parse_json_or_default<T: Default + serde::de::DeserializeOwned>(body: &[u8]) -> T {
    if body.is_empty() {
        return T::default();
    }
    serde_json::from_slice(body).unwrap_or_default()
}

fn query_value(uri: &Uri, key: &str) -> Option<String> {
    uri.query().and_then(|query| {
        form_urlencoded::parse(query.as_bytes())
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.into_owned())
    })
}

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buffer);
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hash_token(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn clear_sessions() {
    if let Ok(mut sessions) = sessions().lock() {
        sessions.clear();
    }
}
