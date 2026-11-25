use super::{api, catalog::Catalog};
use crate::harbinger::{metrics, AppState, Config, ConfigLibrary, SkipMode};
use crate::plex::plex_tls::PlexTls;
use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    Router,
};
use rand_core::OsRng;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};
use tower::ServiceExt;

const PASSWORD: &str = "abs-test-password";

static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

struct Fixture {
    app: Router,
    root: PathBuf,
    item_id: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn fixture() -> Fixture {
    api::clear_sessions();
    Catalog::clear_cache();
    let id = NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "mplex-abs-endpoint-tests-{}-{id}",
        std::process::id()
    ));
    let config_dir = root.join("config");
    let library_dir = root.join("library");
    let movie_dir = root.join("movies");
    let book_dir = library_dir.join("Author One").join("Book One");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&book_dir).unwrap();
    std::fs::create_dir_all(&movie_dir).unwrap();
    let track_path = book_dir.join("Track 01.mp3");
    std::fs::write(&track_path, b"0123456789abcdef").unwrap();
    std::fs::write(book_dir.join("cover.jpg"), b"cover-bytes").unwrap();
    std::fs::write(movie_dir.join("Movie Theme.mp3"), b"movie-theme").unwrap();

    let db = rusqlite::Connection::open(config_dir.join("state.sqlite")).unwrap();
    db.execute_batch(
        "CREATE TABLE users (
             id TEXT PRIMARY KEY,
             name TEXT NOT NULL,
             username TEXT UNIQUE,
             password_hash TEXT,
             user_type TEXT NOT NULL DEFAULT 'admin',
             is_active INTEGER NOT NULL DEFAULT 1,
             created_at INTEGER NOT NULL DEFAULT 0,
             last_seen INTEGER NOT NULL DEFAULT 0,
             permissions TEXT NOT NULL DEFAULT '{}',
             libraries TEXT NOT NULL DEFAULT '[]'
         );
         CREATE TABLE web_sessions (
             token_hash TEXT PRIMARY KEY,
             expires_at INTEGER NOT NULL
         );
         CREATE TABLE watch_progress (
             user_id TEXT NOT NULL,
             media_key TEXT NOT NULL,
             position_ms INTEGER NOT NULL DEFAULT 0,
             duration_ms INTEGER,
             watched INTEGER NOT NULL DEFAULT 0,
             hidden_from_continue INTEGER NOT NULL DEFAULT 0,
             updated_at INTEGER NOT NULL,
             PRIMARY KEY (user_id, media_key)
         );",
    )
    .unwrap();
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(PASSWORD.as_bytes(), &salt)
        .unwrap();
    db.execute(
        "INSERT INTO users (id, name, username, password_hash, user_type) VALUES (?1, 'root', 'root', ?2, 'admin')",
        ["abs-test-user", &hash.to_string()],
    )
    .unwrap();
    let config = Config {
        bind: "127.0.0.1:0".to_owned(),
        libraries: vec![
            ConfigLibrary {
                name: "Audiobooks".to_owned(),
                kind: "audiobooks".to_owned(),
                folders: vec![library_dir.clone()],
                show_recently_added: true,
                show_continue_watching: true,
                scan_enabled: true,
                scan_interval_value: 5,
                scan_interval_unit: "minutes".to_owned(),
                plex_agent: None,
                plex_scanner: None,
                plex_language: None,
                metadata_agent_provider_group_id: None,
                plex_preferences: None,
            },
            ConfigLibrary {
                name: "Movies".to_owned(),
                kind: "movie".to_owned(),
                folders: vec![movie_dir],
                show_recently_added: true,
                show_continue_watching: true,
                scan_enabled: true,
                scan_interval_value: 5,
                scan_interval_unit: "minutes".to_owned(),
                plex_agent: None,
                plex_scanner: None,
                plex_language: None,
                metadata_agent_provider_group_id: None,
                plex_preferences: None,
            },
        ],
        metadata_storage: "config".to_owned(),
        tmdb_bearer_token: None,
        audio_language_primary: "en".to_owned(),
        audio_language_secondary: None,
        subtitle_language_primary: "en".to_owned(),
        subtitle_language_secondary: "vi".to_owned(),
        bilingual_subtitles_enabled: true,
        skip_intro_mode: SkipMode::Manual,
        skip_credits_mode: SkipMode::Manual,
        marker_detection_enabled: true,
        plex_account_preferences: BTreeMap::new(),
    };
    let (plex_notifications, _) = tokio::sync::broadcast::channel(64);
    let state = AppState {
        config: Arc::new(Mutex::new(config)),
        subtitle_checkpoints_dir: Arc::new(config_dir.join("translated_subtitles")),
        config_path: Arc::new(config_dir.join("config.toml")),
        db: Arc::new(Mutex::new(db)),
        server_name: Arc::new(RwLock::new("harbinger".to_owned())),
        machine_identifier: Arc::from(format!("abs-test-{id}")),
        root: Arc::new(library_dir),
        media: Arc::new(RwLock::new(Arc::new(Vec::new()))),
        inventory_scan_lock: Arc::new(tokio::sync::Mutex::new(())),
        plex_notifications,
        media_info: Arc::new(Mutex::new(HashMap::new())),
        media_info_pending: Arc::new(Mutex::new(HashSet::new())),
        media_info_inspection_lock: Arc::new(tokio::sync::Mutex::new(())),
        plex_stream_selections: Arc::new(Mutex::new(HashMap::new())),
        plex_play_queues: Arc::new(Mutex::new(HashMap::new())),
        next_plex_play_queue_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        plex_playback_states: Arc::new(Mutex::new(HashMap::new())),
        plex_remux_sessions: Arc::new(Mutex::new(HashMap::new())),
        plex_scrobbled_sessions: Arc::new(Mutex::new(HashSet::new())),
        plex_webhook_delivery_lock: Arc::new(tokio::sync::Mutex::new(())),
        metrics: metrics::Metrics::new(),
        plex_tls: PlexTls::load(config_dir.join("certs")).unwrap(),
    };
    let catalog = Catalog::load(&state);
    let item_id = catalog.libraries[0].items[0].id.clone();
    Fixture {
        app: super::router(state),
        root,
        item_id,
    }
}

async fn send(
    app: &Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
    range: Option<&str>,
) -> (StatusCode, Vec<u8>, axum::http::HeaderMap) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(range) = range {
        builder = builder.header(header::RANGE, range);
    }
    let payload = body
        .as_ref()
        .map(|body| body.to_string())
        .unwrap_or_default();
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let request = builder
        .body(Body::from(payload))
        .expect("request should build");
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("route should respond");
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body should read")
        .to_vec();
    (status, body, headers)
}

async fn login(fixture: &Fixture) -> String {
    let (status, body, _) = send(
        &fixture.app,
        Method::POST,
        "/login",
        None,
        Some(json!({"username": "root", "password": PASSWORD})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_slice(&body).unwrap();
    value["user"]["token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn discovery_endpoints_are_public() {
    let fixture = fixture();
    let (status, body, _) = send(&fixture.app, Method::GET, "/ping", None, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["success"],
        true
    );

    let (status, body, _) = send(&fixture.app, Method::GET, "/status", None, None, None).await;
    assert_eq!(status, StatusCode::OK);
    let status = serde_json::from_slice::<Value>(&body).unwrap();
    assert_eq!(status["app"], "audiobookshelf");
    assert!(status["serverVersion"].as_str().unwrap() >= "2.26.0");

    let (status, _, _) = send(&fixture.app, Method::GET, "/healthcheck", None, None, None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn login_rejects_bad_credentials_and_accepts_admin_password() {
    let fixture = fixture();
    let (status, _, _) = send(
        &fixture.app,
        Method::POST,
        "/login",
        None,
        Some(json!({"username": "root", "password": "wrong"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(!login(&fixture).await.is_empty());
}

#[tokio::test]
async fn libraries_browse_and_cover_routes_work_with_prefix_aliases() {
    let fixture = fixture();
    let token = login(&fixture).await;
    for path in ["/api/libraries", "/libraries", "/abs/api/libraries"] {
        let (status, body, _) =
            send(&fixture.app, Method::GET, path, Some(&token), None, None).await;
        assert_eq!(status, StatusCode::OK, "path {path}");
        let value: Value = serde_json::from_slice(&body).unwrap();
        let libraries = value["libraries"].as_array().unwrap();
        assert_eq!(libraries.len(), 1);
        assert_eq!(libraries[0]["name"], "Audiobooks");
    }
    let items_path = format!(
        "/api/libraries/{}/items?minified=1",
        catalog_library_id(&fixture, &token).await
    );
    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        &items_path,
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["total"], 1);
    assert_eq!(value["results"][0]["id"], fixture.item_id);
    assert_eq!(value["results"][0]["media"]["numAudioFiles"], 1);
    assert!(value["results"][0]["media"]["audioFiles"].is_null());

    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        &format!("/api/items/{}", fixture.item_id),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["media"]["audioFiles"].as_array().unwrap().len(), 1);
    assert_eq!(value["media"]["tracks"].as_array().unwrap().len(), 1);

    let (status, body, headers) = send(
        &fixture.app,
        Method::GET,
        &format!("/api/items/{}/cover", fixture.item_id),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, b"cover-bytes");
    assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
}

#[tokio::test]
async fn playback_streams_ranges_and_syncs_progress() {
    let fixture = fixture();
    let token = login(&fixture).await;
    let (status, body, _) = send(
        &fixture.app,
        Method::POST,
        &format!("/api/items/{}/play", fixture.item_id),
        Some(&token),
        Some(json!({"forceDirectPlay": true})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session: Value = serde_json::from_slice(&body).unwrap();
    let session_id = session["id"].as_str().unwrap();
    let content_url = session["audioTracks"][0]["contentUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(content_url.starts_with(&format!("/public/session/{session_id}/track/")));

    let (status, body, headers) = send(
        &fixture.app,
        Method::GET,
        &content_url,
        None,
        None,
        Some("bytes=0-3"),
    )
    .await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(body, b"0123");
    assert_eq!(headers[header::CONTENT_RANGE], "bytes 0-3/16");

    let (status, _, _) = send(
        &fixture.app,
        Method::GET,
        &content_url,
        None,
        None,
        Some("bytes=99-"),
    )
    .await;
    assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE);

    let (status, _, _) = send(
        &fixture.app,
        Method::POST,
        &format!("/api/session/{session_id}/sync"),
        Some(&token),
        Some(json!({"currentTime": 12.0, "duration": 120.0, "timeListened": 5.0})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        &format!("/api/me/progress/{}", fixture.item_id),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let progress: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(progress["currentTime"], 12.0);
    assert_eq!(progress["duration"], 120.0);
    assert_eq!(progress["progress"], 0.1);

    let (status, body, _) = send(
        &fixture.app,
        Method::POST,
        &format!("/api/session/{session_id}/close"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap()["success"],
        true
    );
}

#[tokio::test]
async fn protected_routes_reject_missing_auth_and_unknown_ids() {
    let fixture = fixture();
    let (status, _, _) = send(
        &fixture.app,
        Method::GET,
        "/api/libraries",
        None,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let token = login(&fixture).await;
    let (status, _, _) = send(
        &fixture.app,
        Method::GET,
        "/api/items/li_unknown",
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(
        &fixture.app,
        Method::GET,
        "/api/libraries/lib_unknown",
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

async fn catalog_library_id(fixture: &Fixture, token: &str) -> String {
    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        "/api/libraries",
        Some(token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_slice(&body).unwrap();
    value["libraries"][0]["id"].as_str().unwrap().to_owned()
}
