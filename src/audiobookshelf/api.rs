use super::{
    catalog::{image_content_type, Catalog, Item, SERVER_VERSION},
    dto::{
        item_json, library_detail_json, library_json, login_envelope, media_progress_json,
        playback_session_json, user_json, PlaybackSession, UserIdentity,
    },
};
use argon2::{password_hash::PasswordHash, Argon2, PasswordVerifier};
use axum::{
    body::{to_bytes, Body},
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode, Uri},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
    Json, Router,
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD},
    Engine as _,
};
use chrono::{Datelike, TimeZone, Utc};
use rand_core::{OsRng, RngCore};
use rusqlite::params;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
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
        .route("/socket.io/", get(super::socket::socket_io))
        .route("/socket.io", get(super::socket::socket_io))
        .with_state(state.clone());
    let api = Router::new()
        .route("/authorize", post(authorize))
        .route("/libraries", get(libraries))
        .route("/libraries/:id", get(library))
        .route("/libraries/:id/items", get(library_items))
        .route("/libraries/:id/personalized", get(library_personalized))
        .route("/libraries/:id/search", get(library_search))
        .route("/libraries/:id/authors", get(library_authors))
        .route("/authors/:author_id", get(author_detail))
        .route("/libraries/:id/series", get(library_series))
        .route(
            "/libraries/:id/series/:series_id",
            get(library_series_detail),
        )
        .route("/series/:series_id", get(series_detail))
        .route("/libraries/:id/collections", get(library_collections))
        .route("/libraries/:id/playlists", get(library_playlists))
        .route("/libraries/:id/stats", get(library_stats))
        .route("/collections", get(collections).post(create_collection))
        .route(
            "/collections/:id",
            get(collection)
                .patch(update_collection)
                .delete(delete_collection),
        )
        .route("/collections/:id/book", post(add_collection_book))
        .route(
            "/collections/:id/book/:book_id",
            post(add_collection_book_path).delete(remove_collection_book),
        )
        .route("/collections/:id/batch/add", post(add_collection_batch))
        .route(
            "/collections/:id/batch/remove",
            post(remove_collection_batch),
        )
        .route("/playlists", get(playlists).post(create_playlist))
        .route(
            "/playlists/collection/:collection_id",
            post(create_playlist_from_collection),
        )
        .route(
            "/playlists/:id",
            get(playlist).patch(update_playlist).delete(delete_playlist),
        )
        .route("/playlists/:id/item", post(add_playlist_item))
        .route("/playlists/:id/item/:item_id", delete(remove_playlist_item))
        .route(
            "/playlists/:id/item/:item_id/:episode",
            delete(remove_playlist_item),
        )
        .route("/playlists/:id/batch/add", post(add_playlist_batch))
        .route("/playlists/:id/batch/remove", post(remove_playlist_batch))
        .route("/me/stats/year/:year", get(my_year_stats))
        .route("/stats/year/:year", get(admin_year_stats))
        .route("/items/:id", get(item))
        .route("/items/:id/cover", get(cover))
        .route(
            "/items/:id/file/:ino",
            get(stream_file_by_ino).head(stream_file_by_ino),
        )
        .route("/items/:id/play", post(play))
        .route("/items/:id/play/:episode", post(play))
        .route("/me", get(me))
        .route("/me/bookmarks", get(all_bookmarks))
        .route("/me/bookmarks/:item_id", get(item_bookmarks))
        .route(
            "/me/item/:id/bookmark",
            post(create_bookmark).patch(update_bookmark),
        )
        .route("/me/item/:id/bookmark/:time", delete(remove_bookmark))
        .route("/me/listening-stats", get(my_listening_stats))
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
        .layer(middleware::from_fn(log_requests))
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

async fn log_requests(request: Request<Body>, next: Next) -> Response {
    let request_id = crate::harbinger::next_http_request_id();
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let query = crate::harbinger::sanitized_query(request.uri());
    let started = Instant::now();
    // Log every ABS attempt, including unmatched routes: a silently dropped
    // playlist/collection call is impossible to diagnose otherwise.
    tracing::info!(
        tag = "abs",
        event = "http_request",
        id = request_id,
        method = %method,
        path = %path,
        query = ?query
    );
    let response = next.run(request).await;
    let status = response.status();
    let latency_ms = started.elapsed().as_millis();
    if status.is_client_error() || status.is_server_error() {
        tracing::warn!(
            tag = "abs",
            event = "http_response",
            id = request_id,
            method = %method,
            path = %path,
            status = status.as_u16(),
            latency_ms
        );
    } else {
        tracing::debug!(
            tag = "abs",
            event = "http_response",
            id = request_id,
            status = status.as_u16(),
            latency_ms
        );
    }
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
struct BookmarkRequest {
    #[serde(default)]
    time: Option<f64>,
    #[serde(default)]
    title: String,
}

#[derive(Debug, Default, Deserialize)]
struct CollectionCreateRequest {
    #[serde(default, alias = "libraryId")]
    library_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    books: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct CollectionUpdateRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    books: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct CollectionBookRequest {
    #[serde(default)]
    id: Option<String>,
    #[serde(default, alias = "libraryItemId")]
    library_item_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct BatchBooksRequest {
    #[serde(default)]
    books: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PlaylistItemRequest {
    #[serde(default, alias = "libraryItemId")]
    library_item_id: String,
    #[serde(default, alias = "episodeId")]
    episode_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PlaylistRequest {
    #[serde(default, alias = "libraryId")]
    library_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    items: Vec<PlaylistItemRequest>,
}

#[derive(Debug, Default, Deserialize)]
struct PlaylistBatchRequest {
    #[serde(default)]
    items: Vec<PlaylistItemRequest>,
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
    let Some(identity) = authenticate(&state, &credentials.username, &credentials.password) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let (access_token, refresh_token) = match issue_tokens(&state, &identity.id) {
        Ok(tokens) => tokens,
        Err(status) => return status.into_response(),
    };
    let catalog = Catalog::load_async(&state).await;
    let default_library_id = catalog.default_library_id();
    let user = user_json(
        &identity,
        &access_token,
        &default_library_id,
        progress_values(&state, &catalog, &identity.id),
        bookmark_values(&state, &identity.id),
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
    let Some(identity) = session_user(&state, &token) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let (access_token, refresh_token) = match issue_tokens(&state, &identity.id) {
        Ok(tokens) => tokens,
        Err(status) => return status.into_response(),
    };
    let catalog = Catalog::load_async(&state).await;
    let default_library_id = catalog.default_library_id();
    let user = user_json(
        &identity,
        &access_token,
        &default_library_id,
        progress_values(&state, &catalog, &identity.id),
        bookmark_values(&state, &identity.id),
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
    let Some((user, default_library_id)) = current_user_json(&state, &token).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
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
    let catalog = Catalog::load_async(&state).await;
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
    let catalog = Catalog::load_async(&state).await;
    match catalog.find_library(&id) {
        Some(library) => {
            let include = query_value(&uri, "include").unwrap_or_default();
            if include.split(',').any(|part| part.trim() == "filterdata") {
                Json(library_detail_json(library)).into_response()
            } else {
                Json(crate::audiobookshelf::dto::library_json(library)).into_response()
            }
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn library_items(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    let Some(library) = catalog.find_library(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let page = query_value(&uri, "page")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = query_value(&uri, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0);
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
    let (filter_group, filter_value) = filter_parts(&filter_by);
    let progress = (filter_group == "progress")
        .then(|| item_progress_map(&state, &identity.id))
        .unwrap_or_default();
    let mut items = library
        .items
        .iter()
        .filter(|item| item_matches_filter(item, &filter_group, &filter_value, &progress))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        let ordering = match sort_by.as_str() {
            "addedAt" => left.added_ms.cmp(&right.added_ms),
            "updatedAt" => left.updated_ms.cmp(&right.updated_ms),
            value if value.contains("series") => {
                let left_key = item_series_rank(left);
                let right_key = item_series_rank(right);
                left_key
                    .0
                    .cmp(&right_key.0)
                    .then_with(|| {
                        left_key
                            .1
                            .partial_cmp(&right_key.1)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| left.title.cmp(&right.title))
            }
            value if value.contains("author") => left
                .author
                .to_lowercase()
                .cmp(&right.author.to_lowercase())
                .then_with(|| left.title.cmp(&right.title)),
            _ => left
                .title
                .to_lowercase()
                .cmp(&right.title.to_lowercase())
                .then_with(|| left.id.cmp(&right.id)),
        };
        if sort_desc {
            ordering.reverse()
        } else {
            ordering
        }
    });
    let total = items.len();
    let start = limit
        .map(|limit| page.saturating_mul(limit).min(total))
        .unwrap_or(0);
    let end = limit
        .map(|limit| start.saturating_add(limit).min(total))
        .unwrap_or(total);
    let results = items[start..end]
        .iter()
        .map(|item| item_json(item, minified))
        .collect::<Vec<_>>();
    Json(json!({
        "results": results,
        "total": total,
        "limit": limit.unwrap_or(0),
        "page": page,
            "sortBy": sort_by,
            "sortDesc": sort_desc,
            "filterBy": if filter_by.is_empty() {
                "all".to_owned()
            } else {
                filter_by.clone()
            },
            "mediaType": "book",
        "minified": minified,
        "collapseseries": query_value(&uri, "collapseseries").is_some_and(|value| value == "1"),
        "include": "",
        "offset": start
    }))
    .into_response()
}

fn decode_filter_value(value: &str) -> String {
    let normalized = value.replace(' ', "+");
    for decoded in [
        URL_SAFE_NO_PAD.decode(&normalized).ok(),
        URL_SAFE.decode(&normalized).ok(),
        STANDARD.decode(&normalized).ok(),
    ]
    .into_iter()
    .flatten()
    {
        if let Ok(text) = String::from_utf8(decoded) {
            if !text.is_empty() {
                return text;
            }
        }
    }
    value.to_owned()
}

fn filter_parts(filter_by: &str) -> (String, String) {
    let filter = filter_by.trim();
    if filter.is_empty() || filter == "all" {
        return ("all".to_owned(), String::new());
    }
    if let Some((group, value)) = filter.split_once('.') {
        (group.to_ascii_lowercase(), decode_filter_value(value))
    } else {
        ("all".to_owned(), String::new())
    }
}

fn item_progress_map(
    state: &crate::harbinger::AppState,
    user_id: &str,
) -> HashMap<String, (i64, bool)> {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let mut statement = db
                .prepare(
                    "SELECT media_key, position_ms, watched FROM watch_progress WHERE user_id = ?1",
                )
                .ok()?;
            let rows = statement
                .query_map([user_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, bool>(2)?,
                    ))
                })
                .ok()?;
            Some(
                rows.filter_map(Result::ok)
                    .map(|(key, position, watched)| (key, (position, watched)))
                    .collect(),
            )
        })
        .unwrap_or_default()
}

fn item_matches_filter(
    item: &Item,
    filter_group: &str,
    filter_value: &str,
    progress: &HashMap<String, (i64, bool)>,
) -> bool {
    if filter_group == "all" || filter_group.is_empty() {
        return true;
    }
    let matches_name = |name: &str| {
        name.eq_ignore_ascii_case(filter_value)
            || crate::audiobookshelf::dto::author_id(name) == filter_value
            || crate::audiobookshelf::dto::series_id(name) == filter_value
    };
    match filter_group {
        "series" => {
            if filter_value.eq_ignore_ascii_case("no-series") {
                return item.metadata.series.is_empty();
            }
            item.metadata.series.iter().any(|label| {
                let (name, _) = super::scanner::split_series_label(label);
                name.eq_ignore_ascii_case(filter_value)
                    || crate::audiobookshelf::dto::series_id(name) == filter_value
            })
        }
        "authors" => {
            if item.metadata.authors.is_empty() {
                matches_name(&item.author)
            } else {
                item.metadata.authors.iter().any(|name| matches_name(name))
            }
        }
        "genres" => item
            .metadata
            .genres
            .iter()
            .any(|value| value.eq_ignore_ascii_case(filter_value)),
        "tags" => item
            .metadata
            .tags
            .iter()
            .any(|value| value.eq_ignore_ascii_case(filter_value)),
        "narrators" => item
            .metadata
            .narrators
            .iter()
            .any(|value| value.eq_ignore_ascii_case(filter_value)),
        "publishers" => item.metadata.publisher.eq_ignore_ascii_case(filter_value),
        "languages" => item.metadata.language.eq_ignore_ascii_case(filter_value),
        "progress" => {
            let (position, watched) = progress.get(&item.id).copied().unwrap_or((0, false));
            match filter_value {
                "finished" => watched,
                "not-finished" => !watched,
                "in-progress" => position > 0 && !watched,
                "not-started" => position == 0 && !watched,
                _ => true,
            }
        }
        _ => true,
    }
}

fn item_series_rank(item: &Item) -> (u8, f64, String) {
    item.metadata
        .series
        .first()
        .map(|label| {
            let (_, sequence) = super::scanner::split_series_label(label);
            sequence_rank(sequence)
        })
        .unwrap_or((2, 0.0, String::new()))
}

async fn item(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    let Some(item) = catalog.find_item(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut response = item_json(item, false);
    let include = query_value(&uri, "include").unwrap_or_default();
    let expanded = query_value(&uri, "expanded")
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "True"));
    if expanded || include.contains("progress") {
        if let Some((current_time, duration)) = read_progress(&state, item, &identity.id) {
            let row = state.db.lock().ok().and_then(|db| {
                db.query_row(
                    "SELECT watched, hidden_from_continue, updated_at FROM watch_progress WHERE user_id = ?1 AND media_key = ?2",
                    params![identity.id, item.id],
                    |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?, row.get::<_, i64>(2)?)),
                )
                .ok()
            });
            if let Some((watched, hidden, updated)) = row {
                response["userProgress"] = media_progress_json(
                    &identity.id,
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
    let catalog = Catalog::load_async(&state).await;
    let Some(item) = catalog.find_item(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let source_index = catalog
        .item_library(item)
        .map(|library| library.source_index)
        .unwrap_or_default();
    let path = match item.cover_path.clone().filter(|path| path.is_file()) {
        Some(path) => Some(path),
        None => {
            crate::audiobookshelf::catalog::ensure_embedded_cover_async(
                state.clone(),
                source_index,
                item.clone(),
            )
            .await
        }
    };
    let Some(path) = path else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = image_content_type(path.as_path());
    crate::harbinger::image_response(content_type, bytes, 300)
}

async fn play(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    let Some(item) = catalog.find_item(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(library) = catalog.item_library(item) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let progress = read_progress(&state, item, &identity.id);
    let now = now_ms();
    let session = PlaybackSession {
        id: format!("play_{}", random_hex(16)),
        user_id: identity.id.clone(),
        library_id: library.id.clone(),
        library_item_id: item.id.clone(),
        current_time: progress.as_ref().map(|row| row.0).unwrap_or(0.0),
        time_listened: 0.0,
        duration: if item.duration > 0.0 {
            item.duration
        } else {
            progress.as_ref().map(|row| row.1).unwrap_or(0.0)
        },
        start_time: progress.as_ref().map(|row| row.0).unwrap_or(0.0),
        started_at: now,
        updated_at: now,
    };
    if let Ok(mut sessions) = sessions().lock() {
        sessions.insert(session.id.clone(), session.clone());
    }
    persist_playback_session(&state, &session, false);
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
    let Some((user, _)) = current_user_json(&state, &token).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    Json(user).into_response()
}

async fn all_progress(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    Json(progress_values(&state, &catalog, &identity.id)).into_response()
}

#[derive(Clone, Debug)]
struct AbsBookmark {
    id: String,
    item_id: String,
    time: f64,
    title: String,
    created_at: i64,
    updated_at: i64,
}

fn bookmark_json(bookmark: &AbsBookmark) -> Value {
    json!({
        "id": bookmark.id,
        "libraryItemId": bookmark.item_id,
        "time": bookmark.time,
        "title": bookmark.title,
        "createdAt": bookmark.created_at,
        "updatedAt": bookmark.updated_at
    })
}

fn read_bookmarks(
    state: &crate::harbinger::AppState,
    user_id: &str,
    item_id: Option<&str>,
) -> Vec<AbsBookmark> {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let mut statement = db
                .prepare(
                    "SELECT id, item_id, CAST(time AS REAL), title,
                            CAST(created_at AS INTEGER), CAST(updated_at AS INTEGER)
                       FROM abs_bookmarks
                      WHERE user_id = ?1 AND (?2 IS NULL OR item_id = ?2)
                      ORDER BY time ASC",
                )
                .ok()?;
            let rows = statement
                .query_map(params![user_id, item_id], |row| {
                    Ok(AbsBookmark {
                        id: row.get(0)?,
                        item_id: row.get(1)?,
                        time: row.get(2)?,
                        title: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                })
                .ok()?;
            Some(rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default()
}

fn read_bookmark(
    state: &crate::harbinger::AppState,
    user_id: &str,
    item_id: &str,
    time: f64,
) -> Option<AbsBookmark> {
    read_bookmarks(state, user_id, Some(item_id))
        .into_iter()
        .find(|bookmark| bookmark.time == time)
}

fn bookmark_values(state: &crate::harbinger::AppState, user_id: &str) -> Vec<Value> {
    read_bookmarks(state, user_id, None)
        .iter()
        .map(bookmark_json)
        .collect()
}

async fn all_bookmarks(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    Json(json!({
        "bookmarks": read_bookmarks(&state, &identity.id, None)
            .iter()
            .map(bookmark_json)
            .collect::<Vec<_>>()
    }))
    .into_response()
}

async fn item_bookmarks(
    State(state): State<crate::harbinger::AppState>,
    Path(item_id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    if catalog.find_item(&item_id).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    Json(json!({
        "bookmarks": read_bookmarks(&state, &identity.id, Some(&item_id))
            .iter()
            .map(bookmark_json)
            .collect::<Vec<_>>()
    }))
    .into_response()
}

async fn create_bookmark(
    State(state): State<crate::harbinger::AppState>,
    Path(item_id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<BookmarkRequest>,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    if catalog.find_item(&item_id).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(time) = request.time.filter(|time| time.is_finite()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let title = request.title.trim();
    if title.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let now = now_ms();
    let existing = read_bookmark(&state, &identity.id, &item_id, time).is_some();
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if existing {
        let _ = db.execute(
            "UPDATE abs_bookmarks SET title = ?1, updated_at = ?2
              WHERE user_id = ?3 AND item_id = ?4 AND time = ?5",
            params![title, now, identity.id, item_id, time],
        );
    } else {
        let _ = db.execute(
            "INSERT INTO abs_bookmarks
                (id, user_id, item_id, time, title, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                format!("bm_{}", random_hex(16)),
                identity.id,
                item_id,
                time,
                title,
                now
            ],
        );
    }
    drop(db);
    let Some(bookmark) = read_bookmark(&state, &identity.id, &item_id, time) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    Json(bookmark_json(&bookmark)).into_response()
}

async fn update_bookmark(
    State(state): State<crate::harbinger::AppState>,
    Path(item_id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<BookmarkRequest>,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    if catalog.find_item(&item_id).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(time) = request.time.filter(|time| time.is_finite()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let title = request.title.trim();
    if title.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if read_bookmark(&state, &identity.id, &item_id, time).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let now = now_ms();
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let _ = db.execute(
        "UPDATE abs_bookmarks SET title = ?1, updated_at = ?2
          WHERE user_id = ?3 AND item_id = ?4 AND time = ?5",
        params![title, now, identity.id, item_id, time],
    );
    drop(db);
    let Some(bookmark) = read_bookmark(&state, &identity.id, &item_id, time) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(bookmark_json(&bookmark)).into_response()
}

async fn remove_bookmark(
    State(state): State<crate::harbinger::AppState>,
    Path((item_id, time)): Path<(String, f64)>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    if !time.is_finite() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let catalog = Catalog::load_async(&state).await;
    if catalog.find_item(&item_id).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if read_bookmark(&state, &identity.id, &item_id, time).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let _ = db.execute(
        "DELETE FROM abs_bookmarks WHERE user_id = ?1 AND item_id = ?2 AND time = ?3",
        params![identity.id, item_id, time],
    );
    StatusCode::OK.into_response()
}

async fn items_in_progress(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let limit = query_value(&uri, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(25);
    let catalog = Catalog::load_async(&state).await;
    let mut results = progress_values(&state, &catalog, &identity.id)
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
    let identity = match require_user(state, headers, uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(state).await;
    let Some(item) = catalog.find_item(id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((current_time, duration)) = read_progress(state, item, &identity.id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let row = state.db.lock().ok().and_then(|db| {
        db.query_row(
            "SELECT watched, hidden_from_continue, updated_at FROM watch_progress WHERE user_id = ?1 AND media_key = ?2",
            params![identity.id, item.id],
            |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?, row.get::<_, i64>(2)?)),
        )
        .ok()
    });
    let Some((watched, hidden, updated)) = row else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(media_progress_json(
        &identity.id,
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
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let body = match read_body(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let payload = parse_json_or_default::<ProgressRequest>(&body);
    let catalog = Catalog::load_async(&state).await;
    let Some(item) = catalog.find_item(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match save_progress(&state, item, &payload, &identity.id) {
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
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let body = match read_body(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let payloads = parse_json_or_default::<Vec<BatchProgressRequest>>(&body);
    if payloads.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let catalog = Catalog::load_async(&state).await;
    let mut results = Vec::new();
    for payload in payloads {
        let Some(item) = catalog.find_item(&payload.library_item_id) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        match save_progress(&state, item, &payload.progress, &identity.id) {
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
    let catalog = Catalog::load_async(&state).await;
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
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let body = match read_body(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let payload = parse_json_or_default::<SessionSyncRequest>(&body);
    let catalog = Catalog::load_async(&state).await;
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
    persist_playback_session(&state, &updated, false);
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
        &identity.id,
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
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let body = match read_body(request).await {
        Ok(body) => body,
        Err(response) => return response,
    };
    let payload = parse_json_or_default::<SessionSyncRequest>(&body);
    let catalog = Catalog::load_async(&state).await;
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
    let mut closed = session.clone();
    closed.current_time = current_time;
    closed.duration = duration;
    closed.time_listened += payload.time_listened.unwrap_or(0.0).max(0.0);
    closed.updated_at = now_ms();
    if let Ok(mut sessions) = sessions().lock() {
        sessions.remove(&id);
    }
    persist_playback_session(&state, &closed, true);
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
        &identity.id,
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
    let catalog = Catalog::load_async(&state).await;
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
    let catalog = Catalog::load_async(&state).await;
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
    let catalog = Catalog::load_async(&state).await;
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

fn require_user(
    state: &crate::harbinger::AppState,
    headers: &HeaderMap,
    uri: &Uri,
) -> Result<UserIdentity, Response> {
    let token = require_auth(state, headers, uri)?;
    session_user(state, &token).ok_or_else(|| StatusCode::UNAUTHORIZED.into_response())
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
    session_user(state, token).is_some()
}

fn revoke_token(state: &crate::harbinger::AppState, token: &str) {
    if let Ok(db) = state.db.lock() {
        let _ = db.execute(
            "DELETE FROM web_sessions WHERE token_hash = ?1",
            params![hash_token(token)],
        );
    }
}

fn authenticate(
    state: &crate::harbinger::AppState,
    username: &str,
    password: &str,
) -> Option<UserIdentity> {
    let username = username.trim();
    if username.is_empty() || password.is_empty() {
        return None;
    }
    let db = state.db.lock().ok()?;
    let (identity, hash) = db
        .query_row(
            "SELECT id, COALESCE(username, name), COALESCE(user_type, 'guest'), created_at, last_seen, password_hash
               FROM users
              WHERE lower(username) = lower(?1) AND is_active = 1
              LIMIT 1",
            [username],
            |row| {
                Ok((
                    UserIdentity {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        user_type: row.get(2)?,
                        created_at: row.get(3)?,
                        last_seen: row.get(4)?,
                    },
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .ok()?;
    let hash = hash?;
    let parsed = PasswordHash::new(&hash).ok()?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .ok()?;
    Some(identity)
}

pub(crate) fn session_user(
    state: &crate::harbinger::AppState,
    token: &str,
) -> Option<UserIdentity> {
    state.db.lock().ok().and_then(|db| {
        db.query_row(
            "SELECT users.id, COALESCE(users.username, users.name),
                        COALESCE(users.user_type, 'guest'), users.created_at, users.last_seen
                   FROM web_sessions
                   JOIN users ON users.id = COALESCE(web_sessions.user_id, 'admin')
                  WHERE web_sessions.token_hash = ?1
                    AND web_sessions.expires_at > ?2
                    AND users.is_active = 1",
            params![hash_token(token), now_seconds()],
            |row| {
                Ok(UserIdentity {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    user_type: row.get(2)?,
                    created_at: row.get(3)?,
                    last_seen: row.get(4)?,
                })
            },
        )
        .ok()
    })
}

async fn current_user_json(
    state: &crate::harbinger::AppState,
    token: &str,
) -> Option<(Value, String)> {
    let identity = session_user(state, token)?;
    let catalog = Catalog::load_async(state).await;
    let default_library_id = catalog.default_library_id();
    let user = user_json(
        &identity,
        token,
        &default_library_id,
        progress_values(state, &catalog, &identity.id),
        bookmark_values(state, &identity.id),
        None,
        None,
    );
    Some((user, default_library_id))
}

fn issue_tokens(
    state: &crate::harbinger::AppState,
    user_id: &str,
) -> Result<(String, String), StatusCode> {
    let access = random_token();
    let refresh = random_token();
    let now = now_seconds();
    let db = state
        .db
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.execute(
        "INSERT INTO web_sessions (token_hash, expires_at, user_id) VALUES (?1, ?2, ?3)",
        params![hash_token(&access), now + TOKEN_TTL_SECONDS, user_id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db.execute(
        "INSERT INTO web_sessions (token_hash, expires_at, user_id) VALUES (?1, ?2, ?3)",
        params![hash_token(&refresh), now + REFRESH_TTL_SECONDS, user_id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((access, refresh))
}

fn read_progress(
    state: &crate::harbinger::AppState,
    item: &Item,
    user_id: &str,
) -> Option<(f64, f64)> {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            db.query_row(
                "SELECT position_ms, duration_ms FROM watch_progress WHERE user_id = ?1 AND media_key = ?2",
                params![user_id, item.id],
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

fn progress_values(
    state: &crate::harbinger::AppState,
    catalog: &Catalog,
    user_id: &str,
) -> Vec<Value> {
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
                .query_map(params![user_id], |row| {
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
                user_id,
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
    user_id: &str,
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
                (user_id, media_key, position_ms, duration_ms, watched, hidden_from_continue, updated_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, CASE WHEN ?5 THEN ?7 ELSE NULL END)
             ON CONFLICT(user_id, media_key) DO UPDATE SET
                position_ms = excluded.position_ms,
                duration_ms = COALESCE(excluded.duration_ms, watch_progress.duration_ms),
                finished_at = CASE
                    WHEN excluded.watched = 1 THEN COALESCE(watch_progress.finished_at, excluded.updated_at)
                    ELSE NULL
                END,
                watched = excluded.watched,
                hidden_from_continue = excluded.hidden_from_continue,
                updated_at = excluded.updated_at",
            params![
                user_id,
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
        user_id,
        item,
        current_time,
        duration,
        finished,
        hidden,
        updated * 1000,
    ))
}

/// Persist ABS playback sessions so year statistics can report listening time
/// and the books heard during a year after the in-memory session is closed.
fn persist_playback_session(
    state: &crate::harbinger::AppState,
    session: &PlaybackSession,
    closed: bool,
) {
    let Ok(db) = state.db.lock() else {
        return;
    };
    let started_at = (session.started_at / 1000).max(0);
    let updated_at = (session.updated_at / 1000).max(0);
    if db
        .execute(
            "INSERT INTO abs_playback_sessions
                (id, user_id, library_id, item_id, duration, current_time, time_listened, start_time, started_at, updated_at, closed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                duration = excluded.duration,
                current_time = excluded.current_time,
                time_listened = excluded.time_listened,
                start_time = excluded.start_time,
                updated_at = excluded.updated_at,
                closed_at = COALESCE(excluded.closed_at, abs_playback_sessions.closed_at)",
            params![
                session.id,
                session.user_id,
                session.library_id,
                session.library_item_id,
                session.duration,
                session.current_time,
                session.time_listened,
                session.start_time,
                started_at,
                updated_at,
                closed.then_some(updated_at)
            ],
        )
        .is_err()
    {
        tracing::warn!(
            tag = "abs",
            session = %session.id,
            "could not persist playback session for statistics"
        );
    }
}

#[derive(Clone, Debug)]
struct AbsCollectionRecord {
    id: String,
    library_id: String,
    name: String,
    description: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Clone, Debug)]
struct AbsPlaylistRecord {
    id: String,
    library_id: String,
    user_id: String,
    name: String,
    description: String,
    created_at: i64,
    updated_at: i64,
}

fn read_collection(state: &crate::harbinger::AppState, id: &str) -> Option<AbsCollectionRecord> {
    state.db.lock().ok().and_then(|db| {
        db.query_row(
            "SELECT id, library_id, name, description, created_at, updated_at
               FROM abs_collections WHERE id = ?1",
            [id],
            |row| {
                Ok(AbsCollectionRecord {
                    id: row.get(0)?,
                    library_id: row.get(1)?,
                    name: row.get(2)?,
                    description: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        )
        .ok()
    })
}

fn read_playlist(state: &crate::harbinger::AppState, id: &str) -> Option<AbsPlaylistRecord> {
    state.db.lock().ok().and_then(|db| {
        db.query_row(
            "SELECT id, library_id, user_id, name, description, created_at, updated_at
               FROM abs_playlists WHERE id = ?1",
            [id],
            |row| {
                Ok(AbsPlaylistRecord {
                    id: row.get(0)?,
                    library_id: row.get(1)?,
                    user_id: row.get(2)?,
                    name: row.get(3)?,
                    description: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )
        .ok()
    })
}

fn read_ordered_items(
    state: &crate::harbinger::AppState,
    table: &str,
    owner_column: &str,
    owner: &str,
) -> Vec<String> {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let sql = format!(
                "SELECT item_id FROM {table} WHERE {owner_column} = ?1 ORDER BY position, added_at"
            );
            let mut statement = db.prepare(&sql).ok()?;
            let rows = statement
                .query_map([owner], |row| row.get::<_, String>(0))
                .ok()?;
            Some(rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default()
}

fn collection_json(
    state: &crate::harbinger::AppState,
    catalog: &Catalog,
    record: &AbsCollectionRecord,
) -> Value {
    let item_ids = read_ordered_items(state, "abs_collection_items", "collection_id", &record.id);
    let books = item_ids
        .iter()
        .filter_map(|id| catalog.find_item(id))
        .map(|item| item_json(item, false))
        .collect::<Vec<_>>();
    json!({
        "id": record.id,
        "libraryId": record.library_id,
        "name": record.name,
        "description": record.description,
        "books": books,
        "lastUpdate": record.updated_at * 1000,
        "createdAt": record.created_at * 1000
    })
}

fn playlist_json(
    state: &crate::harbinger::AppState,
    catalog: &Catalog,
    record: &AbsPlaylistRecord,
) -> Value {
    let item_ids = read_ordered_items(state, "abs_playlist_items", "playlist_id", &record.id);
    let items = item_ids
        .iter()
        .filter_map(|id| {
            let item = catalog.find_item(id)?;
            Some(json!({
                "libraryItemId": item.id,
                "libraryItem": item_json(item, false)
            }))
        })
        .collect::<Vec<_>>();
    json!({
        "id": record.id,
        "libraryId": record.library_id,
        "userId": record.user_id,
        "name": record.name,
        "description": record.description,
        "items": items,
        "lastUpdate": record.updated_at * 1000,
        "createdAt": record.created_at * 1000
    })
}

fn collection_item_position(state: &crate::harbinger::AppState, collection_id: &str) -> i64 {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            db.query_row(
                "SELECT COALESCE(MAX(position), 0) + 1 FROM abs_collection_items WHERE collection_id = ?1",
                [collection_id],
                |row| row.get::<_, i64>(0),
            )
            .ok()
        })
        .unwrap_or(1)
}

fn playlist_item_position(state: &crate::harbinger::AppState, playlist_id: &str) -> i64 {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            db.query_row(
                "SELECT COALESCE(MAX(position), 0) + 1 FROM abs_playlist_items WHERE playlist_id = ?1",
                [playlist_id],
                |row| row.get::<_, i64>(0),
            )
            .ok()
        })
        .unwrap_or(1)
}

fn item_in_library<'a>(catalog: &'a Catalog, library_id: &str, item_id: &str) -> Option<&'a Item> {
    catalog
        .find_item(item_id)
        .filter(|item| item.library_id == library_id)
}

struct SeriesGroup<'a> {
    name: String,
    books: Vec<(Option<String>, &'a Item)>,
}

fn sequence_rank(sequence: Option<&str>) -> (u8, f64, String) {
    match sequence.and_then(|value| value.trim().parse::<f64>().ok()) {
        Some(value) => (0, value, String::new()),
        None => (1, 0.0, sequence.unwrap_or_default().to_ascii_lowercase()),
    }
}

fn series_groups(library: &crate::audiobookshelf::catalog::Library) -> Vec<SeriesGroup<'_>> {
    let mut grouped = std::collections::BTreeMap::<String, Vec<(Option<String>, &Item)>>::new();
    for item in &library.items {
        let mut seen = std::collections::BTreeSet::new();
        for label in &item.metadata.series {
            let (name, sequence) = super::scanner::split_series_label(label);
            if !seen.insert(name.to_owned()) {
                continue;
            }
            grouped
                .entry(name.to_owned())
                .or_default()
                .push((sequence.map(str::to_owned), item));
        }
    }
    grouped
        .into_iter()
        .map(|(name, mut books)| {
            books.sort_by(|left, right| {
                let left_key = sequence_rank(left.0.as_deref());
                let right_key = sequence_rank(right.0.as_deref());
                left_key
                    .0
                    .cmp(&right_key.0)
                    .then_with(|| {
                        left_key
                            .1
                            .partial_cmp(&right_key.1)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then_with(|| left_key.2.cmp(&right_key.2))
                    .then_with(|| left.1.title.cmp(&right.1.title))
            });
            SeriesGroup { name, books }
        })
        .collect()
}

fn series_group_json(group: &SeriesGroup<'_>, library_id: &str) -> Value {
    let books = group
        .books
        .iter()
        .map(|(_, item)| item_json(item, true))
        .collect::<Vec<_>>();
    let added_at = group
        .books
        .iter()
        .map(|(_, item)| item.added_ms)
        .min()
        .unwrap_or_default();
    let updated_at = group
        .books
        .iter()
        .map(|(_, item)| item.updated_ms)
        .max()
        .unwrap_or_default();
    let total_duration = group
        .books
        .iter()
        .map(|(_, item)| item.duration)
        .sum::<f64>();
    json!({
        "id": crate::audiobookshelf::dto::series_id(&group.name),
        "name": group.name,
        "nameIgnorePrefix": group.name,
        "description": "",
        "addedAt": added_at,
        "updatedAt": updated_at,
        "libraryId": library_id,
        "books": books,
        "numBooks": group.books.len(),
        "totalDuration": total_duration
    })
}

struct AuthorGroup<'a> {
    name: String,
    items: Vec<&'a Item>,
}

fn author_groups(library: &crate::audiobookshelf::catalog::Library) -> Vec<AuthorGroup<'_>> {
    let mut grouped = std::collections::BTreeMap::<String, Vec<&Item>>::new();
    for item in &library.items {
        let mut names = if item.metadata.authors.is_empty() {
            vec![item.author.clone()]
        } else {
            item.metadata.authors.clone()
        };
        names.sort();
        names.dedup();
        for name in names {
            if name.trim().is_empty() {
                continue;
            }
            grouped.entry(name).or_default().push(item);
        }
    }
    grouped
        .into_iter()
        .map(|(name, items)| AuthorGroup { name, items })
        .collect()
}

async fn library_authors(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load_async(&state).await;
    let Some(library) = catalog.find_library(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut groups = author_groups(library);
    let sort_desc = query_value(&uri, "desc")
        .or_else(|| query_value(&uri, "sortDesc"))
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "True"));
    let sort_by = query_value(&uri, "sort")
        .or_else(|| query_value(&uri, "sortBy"))
        .unwrap_or_else(|| "name".to_owned());
    groups.sort_by(|left, right| {
        let ordering = match sort_by.as_str() {
            "numBooks" => left.items.len().cmp(&right.items.len()),
            "addedAt" => left
                .items
                .iter()
                .map(|item| item.added_ms)
                .min()
                .cmp(&right.items.iter().map(|item| item.added_ms).min()),
            _ => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
        };
        if sort_desc {
            ordering.reverse()
        } else {
            ordering
        }
        .then_with(|| left.name.cmp(&right.name))
    });
    let limit = query_value(&uri, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0);
    let page = query_value(&uri, "page")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let total = groups.len();
    let start = limit
        .map(|limit| page.saturating_mul(limit).min(total))
        .unwrap_or(0);
    let end = limit
        .map(|limit| start.saturating_add(limit).min(total))
        .unwrap_or(total);
    let results = groups[start..end]
        .iter()
        .map(|group| {
            crate::audiobookshelf::dto::author_json(&group.name, &library.id, group.items.len())
        })
        .collect::<Vec<_>>();
    // Real ABS returns `{authors: [...]}` for an unpaginated request and the
    // paged envelope only when both limit and page are present.
    if query_value(&uri, "limit").is_some() && query_value(&uri, "page").is_some() {
        Json(json!({
            "results": results,
            "total": total,
            "limit": limit.unwrap_or(0),
            "page": page,
            "sortBy": sort_by,
            "sortDesc": sort_desc,
            "filterBy": query_value(&uri, "filter")
                .or_else(|| query_value(&uri, "filterBy"))
                .unwrap_or_else(|| "all".to_owned()),
            "minified": false,
            "include": query_value(&uri, "include").unwrap_or_default()
        }))
        .into_response()
    } else {
        Json(json!({ "authors": results })).into_response()
    }
}

fn author_group_for_id<'a>(
    catalog: &'a Catalog,
    author_id: &str,
) -> Option<(&'a crate::audiobookshelf::catalog::Library, AuthorGroup<'a>)> {
    for library in &catalog.libraries {
        for group in author_groups(library) {
            if crate::audiobookshelf::dto::author_id(&group.name) == author_id {
                return Some((library, group));
            }
        }
    }
    None
}

async fn author_detail(
    State(state): State<crate::harbinger::AppState>,
    Path(author_id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    let Some((library, group)) = author_group_for_id(&catalog, &author_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut author =
        crate::audiobookshelf::dto::author_json(&group.name, &library.id, group.items.len());
    let include = query_value(&uri, "include").unwrap_or_default();
    let includes = |name: &str| include.split(',').any(|part| part.trim() == name);
    if includes("items") {
        author["libraryItems"] = json!(group
            .items
            .iter()
            .map(|item| item_json(item, true))
            .collect::<Vec<_>>());
        if includes("series") {
            let mut series =
                std::collections::BTreeMap::<String, Vec<(Option<String>, &Item)>>::new();
            for item in &group.items {
                let mut seen = std::collections::BTreeSet::new();
                for label in &item.metadata.series {
                    let (name, sequence) = super::scanner::split_series_label(label);
                    if !seen.insert(name.to_owned()) {
                        continue;
                    }
                    series
                        .entry(name.to_owned())
                        .or_default()
                        .push((sequence.map(str::to_owned), item));
                }
            }
            let mut series_json = Vec::new();
            for (name, mut books) in series {
                books.sort_by(|left, right| {
                    let left_key = sequence_rank(left.0.as_deref());
                    let right_key = sequence_rank(right.0.as_deref());
                    left_key
                        .0
                        .cmp(&right_key.0)
                        .then_with(|| {
                            left_key
                                .1
                                .partial_cmp(&right_key.1)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .then_with(|| left_key.2.cmp(&right_key.2))
                        .then_with(|| left.1.title.cmp(&right.1.title))
                });
                let items = books
                    .into_iter()
                    .map(|(sequence, item)| {
                        let mut minified = item_json(item, true);
                        minified["media"]["metadata"]["series"] = json!({
                            "id": crate::audiobookshelf::dto::series_id(&name),
                            "name": name,
                            "nameIgnorePrefix": name,
                            "sequence": sequence
                        });
                        minified
                    })
                    .collect::<Vec<_>>();
                series_json.push(json!({
                    "id": crate::audiobookshelf::dto::series_id(&name),
                    "name": name,
                    "items": items
                }));
            }
            series_json.sort_by(|left, right| {
                left["name"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(right["name"].as_str().unwrap_or_default())
            });
            author["series"] = json!(series_json);
        }
    }
    if identity.id.is_empty() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(author).into_response()
}

fn series_group_for_id<'a>(
    catalog: &'a Catalog,
    series_id: &str,
) -> Option<(&'a crate::audiobookshelf::catalog::Library, SeriesGroup<'a>)> {
    for library in &catalog.libraries {
        for group in series_groups(library) {
            if crate::audiobookshelf::dto::series_id(&group.name) == series_id {
                return Some((library, group));
            }
        }
    }
    None
}

async fn series_detail(
    State(state): State<crate::harbinger::AppState>,
    Path(series_id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    let Some((library, group)) = series_group_for_id(&catalog, &series_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut response = series_group_json(&group, &library.id);
    let include = query_value(&uri, "include").unwrap_or_default();
    if include.split(',').any(|part| part.trim() == "progress") {
        let item_ids = group
            .books
            .iter()
            .map(|(_, item)| item.id.clone())
            .collect::<Vec<_>>();
        let watched = watched_item_ids(&state, &identity.id);
        let finished = item_ids
            .iter()
            .filter(|id| watched.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        response["progress"] = json!({
            "libraryItemIds": item_ids,
            "libraryItemIdsFinished": finished,
            "isFinished": !finished.is_empty() && finished.len() >= group.books.len()
        });
    }
    Json(response).into_response()
}

async fn library_search(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load_async(&state).await;
    let Some(library) = catalog.find_library(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let needle = query_value(&uri, "q")
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let limit = query_value(&uri, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0 && *value <= 50)
        .unwrap_or(12);
    if needle.is_empty() {
        return Json(json!({
            "book": [],
            "podcast": [],
            "narrators": [],
            "tags": [],
            "genres": [],
            "series": [],
            "authors": []
        }))
        .into_response();
    }
    let books = library
        .items
        .iter()
        .filter(|item| {
            item.title.to_lowercase().contains(&needle)
                || item.author.to_lowercase().contains(&needle)
                || item
                    .metadata
                    .series
                    .iter()
                    .any(|series| series.to_lowercase().contains(&needle))
        })
        .take(limit)
        .map(|item| json!({ "libraryItem": item_json(item, false) }))
        .collect::<Vec<_>>();
    let authors = author_groups(library)
        .into_iter()
        .filter(|group| group.name.to_lowercase().contains(&needle))
        .take(limit)
        .map(|group| {
            crate::audiobookshelf::dto::author_json(&group.name, &library.id, group.items.len())
        })
        .collect::<Vec<_>>();
    let series = series_groups(library)
        .into_iter()
        .filter(|group| group.name.to_lowercase().contains(&needle))
        .take(limit)
        .map(|group| {
            let object = series_group_json(&group, &library.id);
            let books = object["books"].clone();
            json!({ "series": object, "books": books })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "book": books,
        "podcast": [],
        "narrators": [],
        "tags": [],
        "genres": [],
        "series": series,
        "authors": authors
    }))
    .into_response()
}

async fn library_personalized(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    let Some(library) = catalog.find_library(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let limit = query_value(&uri, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(10);
    let mut shelves = vec![
        json!({"id": "continue-listening", "label": "Continue Listening", "labelStringKey": "LabelContinueListening", "type": "book", "entities": [], "total": 0}),
        json!({"id": "continue-series", "label": "Continue Series", "labelStringKey": "LabelContinueSeries", "type": "book", "entities": [], "total": 0}),
        json!({"id": "newest", "label": "Newest", "labelStringKey": "LabelNewest", "type": "book", "entities": [], "total": 0}),
        json!({"id": "recent-series", "label": "Recent Series", "labelStringKey": "LabelRecentSeries", "type": "series", "entities": [], "total": 0}),
        json!({"id": "discover", "label": "Discover", "labelStringKey": "LabelDiscover", "type": "book", "entities": [], "total": 0}),
        json!({"id": "listen-again", "label": "Listen Again", "labelStringKey": "LabelListenAgain", "type": "book", "entities": [], "total": 0}),
    ];

    let mut in_progress = Vec::new();
    if let Ok(db) = state.db.lock() {
        if let Ok(mut statement) = db.prepare(
            "SELECT media_key FROM watch_progress
              WHERE user_id = ?1 AND watched = 0 AND hidden_from_continue = 0 AND position_ms > 0
              ORDER BY updated_at DESC LIMIT ?2",
        ) {
            if let Ok(rows) = statement.query_map(params![identity.id, limit as i64], |row| {
                row.get::<_, String>(0)
            }) {
                for media_key in rows.filter_map(Result::ok) {
                    if let Some(item) = library.items.iter().find(|item| item.id == media_key) {
                        in_progress.push(item_json(item, true));
                    }
                }
            }
        }
    }
    shelves[0]["entities"] = json!(in_progress);
    shelves[0]["total"] = json!(in_progress.len());

    let mut newest = library.items.iter().collect::<Vec<_>>();
    newest.sort_by(|left, right| right.added_ms.cmp(&left.added_ms));
    let newest = newest
        .into_iter()
        .take(limit)
        .map(|item| item_json(item, true))
        .collect::<Vec<_>>();
    shelves[2]["entities"] = json!(newest);
    shelves[2]["total"] = json!(newest.len());

    let mut groups = series_groups(library);
    groups.sort_by(|left, right| {
        let left_added = left.books.iter().map(|(_, item)| item.added_ms).max();
        let right_added = right.books.iter().map(|(_, item)| item.added_ms).max();
        right_added.cmp(&left_added)
    });
    let recent_series = groups
        .iter()
        .take(limit)
        .map(|group| series_group_json(group, &library.id))
        .collect::<Vec<_>>();
    shelves[3]["entities"] = json!(recent_series);
    shelves[3]["total"] = json!(recent_series.len());

    let mut discover = library.items.iter().collect::<Vec<_>>();
    discover.sort_by(|left, right| left.title.to_lowercase().cmp(&right.title.to_lowercase()));
    let discover = discover
        .into_iter()
        .take(limit)
        .map(|item| item_json(item, true))
        .collect::<Vec<_>>();
    shelves[4]["entities"] = json!(discover);
    shelves[4]["total"] = json!(discover.len());

    Json(json!(shelves)).into_response()
}

async fn library_series(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load_async(&state).await;
    let Some(library) = catalog.find_library(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut groups = series_groups(library);
    let sort_desc = query_value(&uri, "desc")
        .or_else(|| query_value(&uri, "sortDesc"))
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "True"));
    let sort_by = query_value(&uri, "sortBy")
        .or_else(|| query_value(&uri, "sort"))
        .unwrap_or_else(|| "name".to_owned());
    groups.sort_by(|left, right| {
        let ordering = match sort_by.as_str() {
            "numBooks" => left.books.len().cmp(&right.books.len()),
            "addedAt" => left
                .books
                .iter()
                .map(|(_, item)| item.added_ms)
                .min()
                .cmp(&right.books.iter().map(|(_, item)| item.added_ms).min()),
            "totalDuration" => {
                let left_total = left
                    .books
                    .iter()
                    .map(|(_, item)| item.duration)
                    .sum::<f64>();
                let right_total = right
                    .books
                    .iter()
                    .map(|(_, item)| item.duration)
                    .sum::<f64>();
                left_total
                    .partial_cmp(&right_total)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
            _ => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
        };
        if sort_desc {
            ordering.reverse()
        } else {
            ordering
        }
        .then_with(|| left.name.cmp(&right.name))
    });
    let page = query_value(&uri, "page")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = query_value(&uri, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0);
    let total = groups.len();
    let start = limit
        .map(|limit| page.saturating_mul(limit).min(total))
        .unwrap_or(0);
    let end = limit
        .map(|limit| start.saturating_add(limit).min(total))
        .unwrap_or(total);
    let results = groups[start..end]
        .iter()
        .map(|group| series_group_json(group, &library.id))
        .collect::<Vec<_>>();
    Json(json!({
        "results": results,
        "total": total,
        "limit": limit.unwrap_or(0),
        "page": page,
        "sortBy": sort_by,
        "sortDesc": sort_desc,
        "filterBy": query_value(&uri, "filter")
            .or_else(|| query_value(&uri, "filterBy"))
            .unwrap_or_else(|| "all".to_owned()),
        "minified": false,
        "include": query_value(&uri, "include").unwrap_or_default()
    }))
    .into_response()
}

async fn library_series_detail(
    State(state): State<crate::harbinger::AppState>,
    Path((id, series_id)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    let Some(library) = catalog.find_library(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(group) = series_groups(library)
        .into_iter()
        .find(|group| crate::audiobookshelf::dto::series_id(&group.name) == series_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut response = series_group_json(&group, &library.id);
    if query_value(&uri, "include").is_some_and(|include| include.contains("progress")) {
        let item_ids = group
            .books
            .iter()
            .map(|(_, item)| item.id.clone())
            .collect::<Vec<_>>();
        let watched = watched_item_ids(&state, &identity.id);
        let finished = item_ids
            .iter()
            .filter(|id| watched.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        response["progress"] = json!({
            "libraryItemIds": item_ids,
            "libraryItemIdsFinished": finished,
            "isFinished": !finished.is_empty() && finished.len() >= group.books.len()
        });
    }
    Json(response).into_response()
}

fn watched_item_ids(
    state: &crate::harbinger::AppState,
    user_id: &str,
) -> std::collections::HashSet<String> {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let mut statement = db
                .prepare("SELECT media_key FROM watch_progress WHERE user_id = ?1 AND watched = 1")
                .ok()?;
            let rows = statement
                .query_map([user_id], |row| row.get::<_, String>(0))
                .ok()?;
            Some(rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default()
}

async fn collections(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load_async(&state).await;
    let records = state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let mut statement = db
                .prepare(
                    "SELECT id, library_id, name, description, created_at, updated_at
                       FROM abs_collections ORDER BY name",
                )
                .ok()?;
            let rows = statement
                .query_map([], |row| {
                    Ok(AbsCollectionRecord {
                        id: row.get(0)?,
                        library_id: row.get(1)?,
                        name: row.get(2)?,
                        description: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                })
                .ok()?;
            Some(rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .unwrap_or_default();
    Json(json!({
        "collections": records.iter().map(|record| collection_json(&state, &catalog, record)).collect::<Vec<_>>()
    }))
    .into_response()
}

async fn library_collections(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load_async(&state).await;
    if catalog.find_library(&id).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let records = state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let mut statement = db
                .prepare(
                    "SELECT id, library_id, name, description, created_at, updated_at
                       FROM abs_collections WHERE library_id = ?1 ORDER BY name",
                )
                .ok()?;
            let rows = statement
                .query_map([&id], |row| {
                    Ok(AbsCollectionRecord {
                        id: row.get(0)?,
                        library_id: row.get(1)?,
                        name: row.get(2)?,
                        description: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                })
                .ok()?;
            Some(rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .unwrap_or_default();
    let page = query_value(&uri, "page")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = query_value(&uri, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0);
    let total = records.len();
    let start = limit
        .map(|limit| page.saturating_mul(limit).min(total))
        .unwrap_or(0);
    let end = limit
        .map(|limit| start.saturating_add(limit).min(total))
        .unwrap_or(total);
    let results = records[start..end]
        .iter()
        .map(|record| collection_json(&state, &catalog, record))
        .collect::<Vec<_>>();
    Json(json!({
        "results": results,
        "total": total,
        "limit": limit.unwrap_or(0),
        "page": page,
        "sortBy": query_value(&uri, "sortBy").unwrap_or_default(),
        "sortDesc": query_value(&uri, "desc").is_some_and(|value| value == "1"),
        "filterBy": query_value(&uri, "filter")
            .or_else(|| query_value(&uri, "filterBy"))
            .unwrap_or_else(|| "all".to_owned()),
        "minified": false,
        "include": query_value(&uri, "include").unwrap_or_default()
    }))
    .into_response()
}

async fn collection(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_auth(&state, &headers, &uri) {
        return response;
    }
    let Some(record) = read_collection(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let catalog = Catalog::load_async(&state).await;
    Json(collection_json(&state, &catalog, &record)).into_response()
}

async fn create_collection(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<CollectionCreateRequest>,
) -> Response {
    if let Err(response) = require_user(&state, &headers, &uri) {
        return response;
    }
    let name = request.name.trim();
    if name.is_empty() || request.library_id.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let catalog = Catalog::load_async(&state).await;
    if catalog.find_library(&request.library_id).is_none() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let books = request
        .books
        .iter()
        .filter(|id| item_in_library(&catalog, &request.library_id, id).is_some())
        .cloned()
        .collect::<Vec<_>>();
    if books.is_empty() || books.len() != request.books.len() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let id = format!("col_{}", random_hex(16));
    let now = now_seconds();
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if db
        .execute(
            "INSERT INTO abs_collections (id, library_id, name, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![
                id,
                request.library_id,
                name,
                request.description.unwrap_or_default(),
                now
            ],
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    for (index, book_id) in books.iter().enumerate() {
        if db
            .execute(
                "INSERT INTO abs_collection_items (collection_id, item_id, position, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, book_id, (index + 1) as i64, now],
            )
            .is_err()
        {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    drop(db);
    let Some(record) = read_collection(&state, &id) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    info_abs_collection_created(&id, name, books.len());
    Json(collection_json(&state, &catalog, &record)).into_response()
}

fn info_abs_collection_created(id: &str, name: &str, books: usize) {
    tracing::info!(tag = "abs", collection = %id, name = %name, books, "ABS collection created");
}

async fn update_collection(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<CollectionUpdateRequest>,
) -> Response {
    if let Err(response) = require_user(&state, &headers, &uri) {
        return response;
    }
    let Some(record) = read_collection(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let catalog = Catalog::load_async(&state).await;
    let existing = read_ordered_items(&state, "abs_collection_items", "collection_id", &id);
    let now = now_seconds();
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let name = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&record.name)
        .to_owned();
    let description = request.description.unwrap_or(record.description.clone());
    if db
        .execute(
            "UPDATE abs_collections SET name = ?1, description = ?2, updated_at = ?3 WHERE id = ?4",
            params![name, description, now, id],
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if !request.books.is_empty() {
        let mut ordered = request
            .books
            .iter()
            .filter(|book_id| existing.contains(book_id))
            .cloned()
            .collect::<Vec<_>>();
        for book_id in existing {
            if !ordered.contains(&book_id) {
                ordered.push(book_id);
            }
        }
        let _ = db.execute(
            "DELETE FROM abs_collection_items WHERE collection_id = ?1",
            [&id],
        );
        for (index, book_id) in ordered.iter().enumerate() {
            let _ = db.execute(
                "INSERT INTO abs_collection_items (collection_id, item_id, position, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, book_id, (index + 1) as i64, now],
            );
        }
    }
    drop(db);
    let Some(record) = read_collection(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(collection_json(&state, &catalog, &record)).into_response()
}

async fn delete_collection(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_user(&state, &headers, &uri) {
        return response;
    }
    if read_collection(&state, &id).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let _ = db.execute(
        "DELETE FROM abs_collection_items WHERE collection_id = ?1",
        [&id],
    );
    let _ = db.execute("DELETE FROM abs_collections WHERE id = ?1", [&id]);
    info_abs_collection_deleted(&id);
    StatusCode::OK.into_response()
}

fn info_abs_collection_deleted(id: &str) {
    tracing::info!(tag = "abs", collection = %id, "ABS collection deleted");
}

async fn add_collection_book(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<CollectionBookRequest>,
) -> Response {
    let Some(book_id) = request.id.or(request.library_item_id) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    add_collection_book_to(state, id, book_id, headers, uri).await
}

async fn add_collection_book_path(
    State(state): State<crate::harbinger::AppState>,
    Path((id, book_id)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    add_collection_book_to(state, id, book_id, headers, uri).await
}

async fn add_collection_book_to(
    state: crate::harbinger::AppState,
    id: String,
    book_id: String,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_user(&state, &headers, &uri) {
        return response;
    }
    let Some(record) = read_collection(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let catalog = Catalog::load_async(&state).await;
    if item_in_library(&catalog, &record.library_id, &book_id).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let existing = read_ordered_items(&state, "abs_collection_items", "collection_id", &id);
    if existing.contains(&book_id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let position = collection_item_position(&state, &id);
    let now = now_seconds();
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if db
        .execute(
            "INSERT INTO abs_collection_items (collection_id, item_id, position, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, book_id, position, now],
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let _ = db.execute(
        "UPDATE abs_collections SET updated_at = ?1 WHERE id = ?2",
        params![now, id],
    );
    drop(db);
    info_abs_collection_book_added(&id, &book_id);
    let Some(record) = read_collection(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(collection_json(&state, &catalog, &record)).into_response()
}

fn info_abs_collection_book_added(collection: &str, book: &str) {
    tracing::info!(tag = "abs", collection = %collection, book = %book, "ABS book added to collection");
}

async fn remove_collection_book(
    State(state): State<crate::harbinger::AppState>,
    Path((id, book_id)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    remove_collection_items(state, id, vec![book_id], headers, uri).await
}

async fn add_collection_batch(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<BatchBooksRequest>,
) -> Response {
    if let Err(response) = require_user(&state, &headers, &uri) {
        return response;
    }
    let Some(record) = read_collection(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let catalog = Catalog::load_async(&state).await;
    let existing = read_ordered_items(&state, "abs_collection_items", "collection_id", &id);
    let now = now_seconds();
    let mut position = collection_item_position(&state, &id);
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    for book_id in request
        .books
        .iter()
        .filter(|book_id| item_in_library(&catalog, &record.library_id, book_id).is_some())
        .filter(|book_id| !existing.contains(book_id))
    {
        let _ = db.execute(
            "INSERT OR IGNORE INTO abs_collection_items (collection_id, item_id, position, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, book_id, position, now],
        );
        position += 1;
    }
    let _ = db.execute(
        "UPDATE abs_collections SET updated_at = ?1 WHERE id = ?2",
        params![now, id],
    );
    drop(db);
    let Some(record) = read_collection(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(collection_json(&state, &catalog, &record)).into_response()
}

async fn remove_collection_batch(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<BatchBooksRequest>,
) -> Response {
    remove_collection_items(state, id, request.books, headers, uri).await
}

async fn remove_collection_items(
    state: crate::harbinger::AppState,
    id: String,
    book_ids: Vec<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_user(&state, &headers, &uri) {
        return response;
    }
    if read_collection(&state, &id).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let catalog = Catalog::load_async(&state).await;
    let existing = read_ordered_items(&state, "abs_collection_items", "collection_id", &id);
    let remaining = existing
        .into_iter()
        .filter(|book_id| !book_ids.contains(book_id))
        .collect::<Vec<_>>();
    let now = now_seconds();
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let _ = db.execute(
        "DELETE FROM abs_collection_items WHERE collection_id = ?1",
        [&id],
    );
    for (index, book_id) in remaining.iter().enumerate() {
        let _ = db.execute(
            "INSERT INTO abs_collection_items (collection_id, item_id, position, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, book_id, (index + 1) as i64, now],
        );
    }
    let _ = db.execute(
        "UPDATE abs_collections SET updated_at = ?1 WHERE id = ?2",
        params![now, id],
    );
    drop(db);
    let Some(record) = read_collection(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(collection_json(&state, &catalog, &record)).into_response()
}

async fn playlists(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    let records = state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let mut statement = db
                .prepare(
                    "SELECT id, library_id, user_id, name, description, created_at, updated_at
                       FROM abs_playlists WHERE user_id = ?1 ORDER BY name",
                )
                .ok()?;
            let rows = statement
                .query_map([&identity.id], |row| {
                    Ok(AbsPlaylistRecord {
                        id: row.get(0)?,
                        library_id: row.get(1)?,
                        user_id: row.get(2)?,
                        name: row.get(3)?,
                        description: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                })
                .ok()?;
            Some(rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .unwrap_or_default();
    Json(json!({
        "playlists": records.iter().map(|record| playlist_json(&state, &catalog, record)).collect::<Vec<_>>()
    }))
    .into_response()
}

async fn library_playlists(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    if catalog.find_library(&id).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let records = state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let mut statement = db
                .prepare(
                    "SELECT id, library_id, user_id, name, description, created_at, updated_at
                       FROM abs_playlists WHERE user_id = ?1 AND library_id = ?2 ORDER BY name",
                )
                .ok()?;
            let rows = statement
                .query_map(params![identity.id, id], |row| {
                    Ok(AbsPlaylistRecord {
                        id: row.get(0)?,
                        library_id: row.get(1)?,
                        user_id: row.get(2)?,
                        name: row.get(3)?,
                        description: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                })
                .ok()?;
            Some(rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .unwrap_or_default();
    let page = query_value(&uri, "page")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = query_value(&uri, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0);
    let total = records.len();
    let start = limit
        .map(|limit| page.saturating_mul(limit).min(total))
        .unwrap_or(0);
    let end = limit
        .map(|limit| start.saturating_add(limit).min(total))
        .unwrap_or(total);
    let results = records[start..end]
        .iter()
        .map(|record| playlist_json(&state, &catalog, record))
        .collect::<Vec<_>>();
    Json(json!({
        "results": results,
        "total": total,
        "limit": limit.unwrap_or(0),
        "page": page
    }))
    .into_response()
}

async fn playlist(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(record) = read_playlist(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if record.user_id != identity.id {
        return StatusCode::FORBIDDEN.into_response();
    }
    let catalog = Catalog::load_async(&state).await;
    Json(playlist_json(&state, &catalog, &record)).into_response()
}

async fn create_playlist(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<PlaylistRequest>,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let name = request.name.trim();
    if name.is_empty() || request.library_id.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let catalog = Catalog::load_async(&state).await;
    if catalog.find_library(&request.library_id).is_none() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let items = request
        .items
        .iter()
        .filter(|item| {
            item.episode_id.is_none()
                && item_in_library(&catalog, &request.library_id, &item.library_item_id).is_some()
        })
        .collect::<Vec<_>>();
    if items.len() != request.items.len() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let id = format!("pl_{}", random_hex(16));
    let now = now_seconds();
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if db
        .execute(
            "INSERT INTO abs_playlists (id, library_id, user_id, name, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                id,
                request.library_id,
                identity.id,
                name,
                request.description.unwrap_or_default(),
                now
            ],
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    for (index, item) in items.iter().enumerate() {
        if db
            .execute(
                "INSERT INTO abs_playlist_items (playlist_id, item_id, position, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, item.library_item_id, (index + 1) as i64, now],
            )
            .is_err()
        {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    drop(db);
    let Some(record) = read_playlist(&state, &id) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    info_abs_playlist_created(&id, name, items.len());
    Json(playlist_json(&state, &catalog, &record)).into_response()
}

fn info_abs_playlist_created(id: &str, name: &str, items: usize) {
    tracing::info!(tag = "abs", playlist = %id, name = %name, items, "ABS playlist created");
}

async fn create_playlist_from_collection(
    State(state): State<crate::harbinger::AppState>,
    Path(collection_id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(collection) = read_collection(&state, &collection_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let catalog = Catalog::load_async(&state).await;
    let item_ids = read_ordered_items(
        &state,
        "abs_collection_items",
        "collection_id",
        &collection_id,
    );
    if item_ids.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let id = format!("pl_{}", random_hex(16));
    let now = now_seconds();
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if db
        .execute(
            "INSERT INTO abs_playlists (id, library_id, user_id, name, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                id,
                collection.library_id,
                identity.id,
                collection.name,
                collection.description,
                now
            ],
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    for (index, item_id) in item_ids.iter().enumerate() {
        let _ = db.execute(
            "INSERT INTO abs_playlist_items (playlist_id, item_id, position, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, item_id, (index + 1) as i64, now],
        );
    }
    drop(db);
    let Some(record) = read_playlist(&state, &id) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    Json(playlist_json(&state, &catalog, &record)).into_response()
}

async fn update_playlist(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<PlaylistRequest>,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(record) = read_playlist(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if record.user_id != identity.id {
        return StatusCode::FORBIDDEN.into_response();
    }
    let catalog = Catalog::load_async(&state).await;
    let existing = read_ordered_items(&state, "abs_playlist_items", "playlist_id", &id);
    let now = now_seconds();
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let name = request
        .name
        .trim()
        .is_empty()
        .then(|| record.name.clone())
        .unwrap_or_else(|| request.name.trim().to_owned());
    let description = request
        .description
        .clone()
        .unwrap_or(record.description.clone());
    let _ = db.execute(
        "UPDATE abs_playlists SET name = ?1, description = ?2, updated_at = ?3 WHERE id = ?4",
        params![name, description, now, id],
    );
    if !request.items.is_empty() {
        let mut ordered = request
            .items
            .iter()
            .map(|item| item.library_item_id.clone())
            .filter(|item_id| existing.contains(item_id))
            .collect::<Vec<_>>();
        for item_id in existing {
            if !ordered.contains(&item_id) {
                ordered.push(item_id);
            }
        }
        let _ = db.execute(
            "DELETE FROM abs_playlist_items WHERE playlist_id = ?1",
            [&id],
        );
        for (index, item_id) in ordered.iter().enumerate() {
            let _ = db.execute(
                "INSERT INTO abs_playlist_items (playlist_id, item_id, position, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, item_id, (index + 1) as i64, now],
            );
        }
    }
    drop(db);
    let Some(record) = read_playlist(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(playlist_json(&state, &catalog, &record)).into_response()
}

async fn delete_playlist(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(record) = read_playlist(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if record.user_id != identity.id {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let _ = db.execute(
        "DELETE FROM abs_playlist_items WHERE playlist_id = ?1",
        [&id],
    );
    let _ = db.execute("DELETE FROM abs_playlists WHERE id = ?1", [&id]);
    info_abs_playlist_deleted(&id);
    StatusCode::OK.into_response()
}

fn info_abs_playlist_deleted(id: &str) {
    tracing::info!(tag = "abs", playlist = %id, "ABS playlist deleted");
}

async fn add_playlist_item(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<PlaylistItemRequest>,
) -> Response {
    add_playlist_items(state, id, vec![request], headers, uri).await
}

async fn add_playlist_batch(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<PlaylistBatchRequest>,
) -> Response {
    add_playlist_items(state, id, request.items, headers, uri).await
}

async fn add_playlist_items(
    state: crate::harbinger::AppState,
    id: String,
    items: Vec<PlaylistItemRequest>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(record) = read_playlist(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if record.user_id != identity.id {
        return StatusCode::FORBIDDEN.into_response();
    }
    let catalog = Catalog::load_async(&state).await;
    let existing = read_ordered_items(&state, "abs_playlist_items", "playlist_id", &id);
    let now = now_seconds();
    let mut position = playlist_item_position(&state, &id);
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    for item in items {
        if item.episode_id.is_some()
            || item_in_library(&catalog, &record.library_id, &item.library_item_id).is_none()
            || existing.contains(&item.library_item_id)
        {
            continue;
        }
        let _ = db.execute(
            "INSERT OR IGNORE INTO abs_playlist_items (playlist_id, item_id, position, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, item.library_item_id, position, now],
        );
        position += 1;
    }
    let _ = db.execute(
        "UPDATE abs_playlists SET updated_at = ?1 WHERE id = ?2",
        params![now, id],
    );
    drop(db);
    let Some(record) = read_playlist(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    info_abs_playlist_item_added(&id);
    Json(playlist_json(&state, &catalog, &record)).into_response()
}

fn info_abs_playlist_item_added(id: &str) {
    tracing::info!(tag = "abs", playlist = %id, "ABS playlist item added");
}

async fn remove_playlist_item(
    State(state): State<crate::harbinger::AppState>,
    Path((id, item_id)): Path<(String, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    remove_playlist_items(state, id, vec![item_id], headers, uri).await
}

async fn remove_playlist_batch(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<PlaylistBatchRequest>,
) -> Response {
    remove_playlist_items(
        state,
        id,
        request
            .items
            .into_iter()
            .map(|item| item.library_item_id)
            .collect(),
        headers,
        uri,
    )
    .await
}

async fn remove_playlist_items(
    state: crate::harbinger::AppState,
    id: String,
    item_ids: Vec<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some(record) = read_playlist(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if record.user_id != identity.id {
        return StatusCode::FORBIDDEN.into_response();
    }
    let catalog = Catalog::load_async(&state).await;
    let existing = read_ordered_items(&state, "abs_playlist_items", "playlist_id", &id);
    if !existing.iter().any(|item_id| item_ids.contains(item_id)) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let remaining = existing
        .into_iter()
        .filter(|item_id| !item_ids.contains(item_id))
        .collect::<Vec<_>>();
    let now = now_seconds();
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let _ = db.execute(
        "DELETE FROM abs_playlist_items WHERE playlist_id = ?1",
        [&id],
    );
    for (index, item_id) in remaining.iter().enumerate() {
        let _ = db.execute(
            "INSERT INTO abs_playlist_items (playlist_id, item_id, position, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, item_id, (index + 1) as i64, now],
        );
    }
    if remaining.is_empty() {
        let _ = db.execute("DELETE FROM abs_playlists WHERE id = ?1", [&id]);
        drop(db);
        return Json(playlist_json(&state, &catalog, &record)).into_response();
    }
    let _ = db.execute(
        "UPDATE abs_playlists SET updated_at = ?1 WHERE id = ?2",
        params![now, id],
    );
    drop(db);
    let Some(record) = read_playlist(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(playlist_json(&state, &catalog, &record)).into_response()
}

fn year_bounds(year: u32) -> Option<(i64, i64)> {
    if !(2000..=9999).contains(&year) {
        return None;
    }
    let start = Utc
        .with_ymd_and_hms(year as i32, 1, 1, 0, 0, 0)
        .single()?
        .timestamp();
    let end = Utc
        .with_ymd_and_hms(year as i32 + 1, 1, 1, 0, 0, 0)
        .single()?
        .timestamp();
    Some((start, end))
}

fn top_times(map: std::collections::HashMap<String, f64>, limit: usize, field: &str) -> Vec<Value> {
    let mut values = map
        .into_iter()
        .map(|(name, time)| (name, time.round() as i64))
        .collect::<Vec<_>>();
    values.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    values
        .into_iter()
        .take(limit)
        .map(|(name, time)| json!({ field: name, "time": time }))
        .collect()
}

fn most_time(map: std::collections::HashMap<String, f64>, field: &str) -> Value {
    let Some((name, time)) = map.into_iter().max_by(|left, right| {
        left.1
            .total_cmp(&right.1)
            .then_with(|| right.0.cmp(&left.0))
    }) else {
        return Value::Null;
    };
    json!({ field: name, "time": time.round() as i64 })
}

fn listening_rollup(
    catalog: &Catalog,
    rows: &[(String, f64, i64)],
) -> (
    i64,
    std::collections::HashMap<String, f64>,
    std::collections::HashMap<String, f64>,
    std::collections::HashMap<String, f64>,
    std::collections::HashMap<String, f64>,
    std::collections::HashMap<String, f64>,
    Vec<String>,
) {
    let mut total = 0.0;
    let mut authors = std::collections::HashMap::new();
    let mut narrators = std::collections::HashMap::new();
    let mut genres = std::collections::HashMap::new();
    let mut months = std::collections::HashMap::new();
    let mut books = std::collections::HashMap::new();
    let mut covers = Vec::new();
    for (item_id, time, started_at) in rows {
        total += time;
        let month = chrono::Local
            .timestamp_opt(*started_at, 0)
            .single()
            .map(|date| date.month0().to_string())
            .unwrap_or_default();
        *months.entry(month).or_insert(0.0) += time;
        *books.entry(item_id.clone()).or_insert(0.0) += time;
        let Some(item) = catalog.find_item(item_id) else {
            continue;
        };
        if item.cover_path.is_some() && covers.len() < 25 && !covers.contains(item_id) {
            covers.push(item_id.clone());
        }
        let names = if item.metadata.authors.is_empty() {
            vec![item.author.clone()]
        } else {
            item.metadata.authors.clone()
        };
        for author in names {
            *authors.entry(author).or_insert(0.0) += time;
        }
        for narrator in &item.metadata.narrators {
            *narrators.entry(narrator.clone()).or_insert(0.0) += time;
        }
        for genre in &item.metadata.genres {
            if genre.to_ascii_lowercase().contains("audiobook")
                || genre.to_ascii_lowercase().contains("audio book")
            {
                continue;
            }
            *genres.entry(genre.clone()).or_insert(0.0) += time;
        }
    }
    (
        total.round() as i64,
        authors,
        narrators,
        genres,
        months,
        books,
        covers,
    )
}

fn read_listening_rows(
    state: &crate::harbinger::AppState,
    user: Option<&str>,
    start: i64,
    end: i64,
) -> (usize, Vec<(String, f64, i64)>) {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let mut statement = db
                .prepare(
                    "SELECT item_id, time_listened, started_at FROM abs_playback_sessions
                      WHERE (?1 IS NULL OR user_id = ?1)
                        AND started_at >= ?2 AND started_at < ?3",
                )
                .ok()?;
            let rows = statement
                .query_map(params![user, start, end], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .ok()?;
            let rows = rows.filter_map(Result::ok).collect::<Vec<_>>();
            Some((rows.len(), rows))
        })
        .unwrap_or((0, Vec::new()))
}

#[derive(Clone, Debug)]
struct ListeningSessionRow {
    id: String,
    item_id: String,
    time_listened: f64,
    current_time: f64,
    duration: f64,
    start_time: f64,
    started_at: i64,
    updated_at: i64,
}

fn read_user_sessions(
    state: &crate::harbinger::AppState,
    user_id: &str,
) -> Vec<ListeningSessionRow> {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let mut statement = db
                .prepare(
                    "SELECT id, item_id,
                            CAST(time_listened AS REAL),
                            CAST(current_time AS REAL),
                            CAST(duration AS REAL),
                            CAST(start_time AS REAL),
                            CAST(started_at AS INTEGER),
                            CAST(updated_at AS INTEGER)
                       FROM abs_playback_sessions
                      WHERE user_id = ?1
                      ORDER BY updated_at DESC",
                )
                .ok()?;
            let rows = statement
                .query_map([user_id], |row| {
                    Ok(ListeningSessionRow {
                        id: row.get(0)?,
                        item_id: row.get(1)?,
                        time_listened: row.get(2)?,
                        current_time: row.get(3)?,
                        duration: row.get(4)?,
                        start_time: row.get(5)?,
                        started_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                })
                .ok()?;
            Some(rows.filter_map(Result::ok).collect())
        })
        .unwrap_or_default()
}

/// Best-effort history for playback that happened before ABS session
/// persistence existed, or when a client only wrote progress. Each saved
/// progress row becomes one synthetic session based on the last update.
fn progress_sessions(
    state: &crate::harbinger::AppState,
    catalog: &Catalog,
    user: Option<&str>,
    start: Option<i64>,
    end: Option<i64>,
) -> Vec<ListeningSessionRow> {
    state
        .db
        .lock()
        .ok()
        .and_then(|db| {
            let mut statement = db
                .prepare(
                    "SELECT media_key, position_ms, duration_ms, watched,
                            CAST(updated_at AS INTEGER)
                       FROM watch_progress
                      WHERE (?1 IS NULL OR user_id = ?1)
                        AND (?2 IS NULL OR updated_at >= ?2)
                        AND (?3 IS NULL OR updated_at < ?3)
                      ORDER BY updated_at DESC",
                )
                .ok()?;
            let rows = statement
                .query_map(params![user, start, end], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, bool>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })
                .ok()?;
            let mut sessions = Vec::new();
            for (media_key, position_ms, duration_ms, watched, updated_at) in
                rows.filter_map(Result::ok)
            {
                let Some(item) = catalog.find_item(&media_key) else {
                    continue;
                };
                let duration = duration_ms
                    .map(|duration| duration as f64 / 1000.0)
                    .filter(|duration| *duration > 0.0)
                    .unwrap_or(item.duration);
                let current_time = position_ms.max(0) as f64 / 1000.0;
                let listened = if watched {
                    duration.max(current_time)
                } else {
                    current_time.min(duration.max(current_time))
                };
                if listened <= 0.0 {
                    continue;
                }
                sessions.push(ListeningSessionRow {
                    id: format!("progress_{}", item.id),
                    item_id: item.id.clone(),
                    time_listened: listened,
                    current_time,
                    duration,
                    start_time: 0.0,
                    started_at: updated_at,
                    updated_at,
                });
            }
            Some(sessions)
        })
        .unwrap_or_default()
}

async fn my_listening_stats(
    State(state): State<crate::harbinger::AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let catalog = Catalog::load_async(&state).await;
    let mut sessions = read_user_sessions(&state, &identity.id);
    if sessions.is_empty() {
        sessions = progress_sessions(&state, &catalog, Some(&identity.id), None, None);
    }
    let today_date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut total_time = 0.0;
    let mut days = std::collections::HashMap::<String, f64>::new();
    let mut day_of_week = std::collections::HashMap::<String, f64>::new();
    let mut items = serde_json::Map::new();
    for session in &sessions {
        total_time += session.time_listened;
        if session.time_listened > 0.0 {
            if let Some(date) = chrono::Local.timestamp_opt(session.started_at, 0).single() {
                let day = date.format("%Y-%m-%d").to_string();
                *days.entry(day).or_insert(0.0) += session.time_listened;
                *day_of_week
                    .entry(date.format("%A").to_string())
                    .or_insert(0.0) += session.time_listened;
            }
        }
        let Some(item) = catalog.find_item(&session.item_id) else {
            continue;
        };
        let metadata = item_json(item, false)["media"]["metadata"].clone();
        let entry = items.entry(session.item_id.clone()).or_insert_with(|| {
            json!({
                "id": session.item_id,
                "timeListening": 0.0,
                "mediaMetadata": metadata
            })
        });
        let current = entry["timeListening"].as_f64().unwrap_or_default();
        entry["timeListening"] = json!(current + session.time_listened);
    }
    for entry in items.values_mut() {
        let time = entry["timeListening"].as_f64().unwrap_or_default().round() as i64;
        entry["timeListening"] = json!(time);
    }
    let recent_sessions = sessions
        .iter()
        .take(10)
        .filter_map(|session| {
            let item = catalog.find_item(&session.item_id)?;
            let metadata = item_json(item, false)["media"]["metadata"].clone();
            let date = chrono::Local
                .timestamp_opt(session.started_at, 0)
                .single()
                .map(|date| date.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            let day_of_week = chrono::Local
                .timestamp_opt(session.started_at, 0)
                .single()
                .map(|date| date.format("%A").to_string())
                .unwrap_or_default();
            Some(json!({
                "id": session.id,
                "userId": identity.id,
                "libraryId": item.library_id,
                "libraryItemId": item.id,
                "bookId": item.id,
                "episodeId": null,
                "mediaType": "book",
                "mediaMetadata": metadata,
                "chapters": [],
                "displayTitle": item.title,
                "displayAuthor": item.author,
                "coverPath": format!("/api/items/{}/cover", item.id),
                "duration": session.duration,
                "playMethod": 0,
                "mediaPlayer": "html5",
                "deviceInfo": null,
                "serverVersion": SERVER_VERSION,
                "date": date,
                "dayOfWeek": day_of_week,
                "timeListening": session.time_listened.round() as i64,
                "startTime": session.start_time,
                "currentTime": session.current_time,
                "startedAt": session.started_at * 1000,
                "updatedAt": session.updated_at * 1000
            }))
        })
        .collect::<Vec<_>>();
    let days = days
        .into_iter()
        .map(|(date, time)| (date, json!(time.round() as i64)))
        .collect::<serde_json::Map<String, Value>>();
    let day_of_week = day_of_week
        .into_iter()
        .map(|(day, time)| (day, json!(time.round() as i64)))
        .collect::<serde_json::Map<String, Value>>();
    let today = days.get(&today_date).and_then(Value::as_i64).unwrap_or(0);
    Json(json!({
        "totalTime": total_time.round() as i64,
        "items": items,
        "days": days,
        "dayOfWeek": day_of_week,
        "today": today,
        "recentSessions": recent_sessions
    }))
    .into_response()
}

async fn library_stats(
    State(state): State<crate::harbinger::AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Err(response) = require_user(&state, &headers, &uri) {
        return response;
    }
    let catalog = Catalog::load_async(&state).await;
    let Some(library) = catalog.find_library(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut author_counts = std::collections::HashMap::<String, usize>::new();
    let mut genre_counts = std::collections::HashMap::<String, usize>::new();
    let mut total_size = 0u64;
    let mut total_duration = 0.0;
    let mut num_audio_tracks = 0usize;
    let mut largest = library
        .items
        .iter()
        .map(|item| (item.size, item))
        .collect::<Vec<_>>();
    let mut longest = library
        .items
        .iter()
        .map(|item| (item.duration, item))
        .collect::<Vec<_>>();
    for item in &library.items {
        total_size += item.size;
        total_duration += item.duration;
        num_audio_tracks += item.audio_files.len();
        if item.metadata.authors.is_empty() {
            *author_counts.entry(item.author.clone()).or_default() += 1;
        } else {
            for author in &item.metadata.authors {
                *author_counts.entry(author.clone()).or_default() += 1;
            }
        }
        for genre in &item.metadata.genres {
            *genre_counts.entry(genre.clone()).or_default() += 1;
        }
    }
    largest.sort_by(|left, right| right.0.cmp(&left.0));
    longest.sort_by(|left, right| {
        right
            .0
            .partial_cmp(&left.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let authors_with_count = {
        let mut values = author_counts
            .into_iter()
            .map(|(name, count)| (name, count))
            .collect::<Vec<_>>();
        values.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        values
            .into_iter()
            .take(10)
            .map(|(name, count)| {
                json!({
                    "id": crate::audiobookshelf::dto::author_id(&name),
                    "name": name,
                    "count": count
                })
            })
            .collect::<Vec<_>>()
    };
    let total_authors = {
        let mut unique = std::collections::HashSet::new();
        for item in &library.items {
            if item.metadata.authors.is_empty() {
                unique.insert(item.author.clone());
            } else {
                for author in &item.metadata.authors {
                    unique.insert(author.clone());
                }
            }
        }
        unique.len()
    };
    let genres_with_count = {
        let mut values = genre_counts.into_iter().collect::<Vec<_>>();
        values.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        values
            .into_iter()
            .map(|(genre, count)| json!({ "genre": genre, "count": count }))
            .collect::<Vec<_>>()
    };
    Json(json!({
        "largestItems": largest.into_iter().take(10).map(|(size, item)| json!({
            "id": item.id,
            "title": item.title,
            "size": size
        })).collect::<Vec<_>>(),
        "totalAuthors": total_authors,
        "authorsWithCount": authors_with_count,
        "totalGenres": genres_with_count.len(),
        "genresWithCount": genres_with_count,
        "totalItems": library.items.len(),
        "longestItems": longest.into_iter().take(10).map(|(duration, item)| json!({
            "id": item.id,
            "title": item.title,
            "duration": duration
        })).collect::<Vec<_>>(),
        "totalSize": total_size,
        "totalDuration": total_duration,
        "numAudioTracks": num_audio_tracks
    }))
    .into_response()
}

async fn my_year_stats(
    State(state): State<crate::harbinger::AppState>,
    Path(year): Path<u32>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    let Some((start, end)) = year_bounds(year) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let catalog = Catalog::load_async(&state).await;
    let (mut num_sessions, mut rows) = read_listening_rows(&state, Some(&identity.id), start, end);
    if rows.is_empty() {
        let progress =
            progress_sessions(&state, &catalog, Some(&identity.id), Some(start), Some(end));
        num_sessions = progress.len();
        rows = progress
            .into_iter()
            .map(|session| (session.item_id, session.time_listened, session.started_at))
            .collect();
    }
    let (total, authors, narrators, genres, months, books, covers) =
        listening_rollup(&catalog, &rows);

    let mut finished = Vec::new();
    if let Ok(db) = state.db.lock() {
        if let Ok(mut statement) = db.prepare(
            "SELECT media_key, COALESCE(finished_at, updated_at) FROM watch_progress
              WHERE user_id = ?1 AND watched = 1
                AND COALESCE(finished_at, updated_at) >= ?2
                AND COALESCE(finished_at, updated_at) < ?3
              ORDER BY COALESCE(finished_at, updated_at) DESC",
        ) {
            if let Ok(rows) = statement.query_map(params![identity.id, start, end], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            }) {
                finished = rows.filter_map(Result::ok).collect();
            }
        }
    }
    let mut longest: Option<(String, String, i64, i64)> = None;
    let mut finished_covers = Vec::new();
    let mut num_finished = 0;
    for (item_id, finished_at) in &finished {
        let Some(item) = catalog.find_item(item_id) else {
            continue;
        };
        num_finished += 1;
        if item.cover_path.is_some() && finished_covers.len() < 5 {
            finished_covers.push(item.id.clone());
        }
        let duration = item.duration.round() as i64;
        if longest
            .as_ref()
            .is_none_or(|(_, _, current, _)| duration > *current)
        {
            longest = Some((
                item.id.clone(),
                item.title.clone(),
                duration,
                finished_at.saturating_mul(1000),
            ));
        }
    }
    let covers = covers
        .into_iter()
        .filter(|item_id| !finished_covers.contains(item_id))
        .collect::<Vec<_>>();
    let longest = longest
        .map(|(id, title, duration, finished_at)| {
            json!({
                "id": id,
                "title": title,
                "duration": duration,
                "finishedAt": chrono::Utc
                    .timestamp_millis_opt(finished_at)
                    .single()
                    .map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
            })
        })
        .unwrap_or(Value::Null);
    let most_narrator = most_time(narrators, "name");
    let most_month = months
        .into_iter()
        .filter_map(|(month, time)| {
            month
                .parse::<u32>()
                .ok()
                .map(|month| (month, time.round() as i64))
        })
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
        .map(|(month, time)| json!({ "month": month, "time": time }))
        .unwrap_or(Value::Null);
    Json(json!({
        "totalListeningSessions": num_sessions,
        "totalListeningTime": total,
        "totalBookListeningTime": total,
        "totalPodcastListeningTime": 0,
        "topAuthors": top_times(authors, 3, "name"),
        "topGenres": top_times(genres, 3, "genre"),
        "mostListenedNarrator": most_narrator,
        "mostListenedMonth": most_month,
        "numBooksFinished": num_finished,
        "numBooksListened": books.len(),
        "longestAudiobookFinished": longest,
        "booksWithCovers": covers,
        "finishedBooksWithCovers": finished_covers
    }))
    .into_response()
}

async fn admin_year_stats(
    State(state): State<crate::harbinger::AppState>,
    Path(year): Path<u32>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let identity = match require_user(&state, &headers, &uri) {
        Ok(identity) => identity,
        Err(response) => return response,
    };
    if identity.user_type != "admin" {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some((start, end)) = year_bounds(year) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let catalog = Catalog::load_async(&state).await;
    let (mut num_sessions, mut rows) = read_listening_rows(&state, None, start, end);
    if rows.is_empty() {
        let progress = progress_sessions(&state, &catalog, None, Some(start), Some(end));
        num_sessions = progress.len();
        rows = progress
            .into_iter()
            .map(|session| (session.item_id, session.time_listened, session.started_at))
            .collect();
    }
    let (total, authors, narrators, genres, _, _, _) = listening_rollup(&catalog, &rows);

    let mut num_added = 0;
    let mut total_added_size = 0u64;
    let mut total_added_duration = 0.0;
    let mut authors_added = std::collections::HashSet::new();
    let mut added_covers = Vec::new();
    let mut total_size = 0u64;
    let mut total_duration = 0.0;
    let mut num_books = 0;
    for library in &catalog.libraries {
        for item in &library.items {
            num_books += 1;
            total_size += item.size;
            total_duration += item.duration;
            let added = item.added_ms / 1000;
            if added >= start && added < end {
                num_added += 1;
                total_added_size += item.size;
                total_added_duration += item.duration;
                if item.metadata.authors.is_empty() {
                    authors_added.insert(item.author.clone());
                } else {
                    for author in &item.metadata.authors {
                        authors_added.insert(author.clone());
                    }
                }
                if item.cover_path.is_some() && added_covers.len() < 25 {
                    added_covers.push(item.id.clone());
                }
            }
        }
    }
    Json(json!({
        "numListeningSessions": num_sessions,
        "numBooksAdded": num_added,
        "numAuthorsAdded": authors_added.len(),
        "totalBooksAddedSize": total_added_size,
        "totalBooksAddedDuration": total_added_duration.round() as i64,
        "booksAddedWithCovers": added_covers,
        "totalBooksSize": total_size,
        "totalBooksDuration": total_duration.round() as i64,
        "totalListeningTime": total,
        "numBooks": num_books,
        "topAuthors": top_times(authors, 3, "name"),
        "topNarrators": top_times(narrators, 3, "name"),
        "topGenres": top_times(genres, 3, "genre")
    }))
    .into_response()
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
