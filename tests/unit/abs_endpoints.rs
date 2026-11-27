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
    std::fs::write(
        book_dir.join("metadata.json"),
        json!({
            "title": "Harbor of Shadows",
            "subtitle": "The Lighthouse File",
            "authors": ["Nora Vale"],
            "narrators": ["Iris Quinn"],
            "series": ["Stormbound #2"],
            "genres": ["Mystery"],
            "tags": ["favorite"],
            "publishedYear": "2024",
            "publishedDate": "2024-05-17",
            "publisher": "Northwind Audio",
            "description": "A harbor town keeps a dangerous secret.",
            "isbn": "9780000000001",
            "asin": "B000000001",
            "language": "en",
            "explicit": false,
            "abridged": false,
            "chapters": [
                {"id": 1, "title": "Prologue", "start": 0.0, "end": 5.0},
                {"id": 2, "title": "The Arrival", "start": 5.0, "end": 16.0}
            ]
        })
        .to_string(),
    )
    .unwrap();
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
             expires_at INTEGER NOT NULL,
             user_id TEXT
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
    login_as(fixture, "root", PASSWORD).await
}

async fn login_as(fixture: &Fixture, username: &str, password: &str) -> String {
    let (status, body, _) = send(
        &fixture.app,
        Method::POST,
        "/login",
        None,
        Some(json!({"username": username, "password": password})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_slice(&body).unwrap();
    value["user"]["token"].as_str().unwrap().to_owned()
}

fn add_user(
    fixture: &Fixture,
    id: &str,
    username: &str,
    password: &str,
    user_type: &str,
    is_active: bool,
) {
    let db = rusqlite::Connection::open(fixture.root.join("config").join("state.sqlite")).unwrap();
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .unwrap();
    db.execute(
        "INSERT INTO users (id, name, username, password_hash, user_type, is_active)
         VALUES (?1, ?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id, username, hash.to_string(), user_type, is_active],
    )
    .unwrap();
}

async fn login_body(fixture: &Fixture, username: &str, password: &str) -> (StatusCode, Value) {
    let (status, body, _) = send(
        &fixture.app,
        Method::POST,
        "/login",
        None,
        Some(json!({"username": username, "password": password})),
        None,
    )
    .await;
    let value = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    (status, value)
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
async fn authorize_accepts_post_and_returns_the_current_user() {
    let fixture = fixture();
    let token = login(&fixture).await;

    for path in ["/api/authorize", "/abs/api/authorize"] {
        let (status, body, _) =
            send(&fixture.app, Method::POST, path, Some(&token), None, None).await;
        assert_eq!(status, StatusCode::OK);
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["user"]["username"], "root");
        assert!(value["serverSettings"].is_object());
        assert!(value["Source"].is_string());
    }

    let (status, _, _) = send(
        &fixture.app,
        Method::POST,
        "/api/authorize",
        None,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _, _) = send(
        &fixture.app,
        Method::GET,
        "/api/authorize",
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn login_uses_submitted_username_and_isolates_user_data() {
    let fixture = fixture();
    add_user(
        &fixture,
        "daniel-id",
        "daniel",
        "daniel-password",
        "guest",
        true,
    );
    add_user(
        &fixture,
        "disabled-id",
        "disabled",
        PASSWORD,
        "guest",
        false,
    );

    let (status, body) = login_body(&fixture, "daniel", "daniel-password").await;
    assert_eq!(status, StatusCode::OK);
    let user = &body["user"];
    assert_eq!(user["id"], "daniel-id");
    assert_eq!(user["username"], "daniel");
    assert_eq!(user["type"], "guest");
    let daniel_token = user["token"].as_str().unwrap().to_owned();

    // ABS looks up usernames case-insensitively.
    let (status, _) = login_body(&fixture, "DANIEL", "daniel-password").await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = login_body(&fixture, "daniel", "wrong-password").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = login_body(&fixture, "missing", PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = login_body(&fixture, "disabled", PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        "/api/me",
        Some(&daniel_token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let me: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(me["username"], "daniel");

    let root_token = login(&fixture).await;
    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        "/api/me",
        Some(&root_token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let me: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(me["username"], "root");
    assert_eq!(me["type"], "root");

    let (status, _, _) = send(
        &fixture.app,
        Method::PATCH,
        &format!("/api/me/progress/{}", fixture.item_id),
        Some(&daniel_token),
        Some(json!({"currentTime": 30.0, "duration": 100.0})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        &format!("/api/me/progress/{}", fixture.item_id),
        Some(&daniel_token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let progress: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(progress["userId"], "daniel-id");
    assert_eq!(progress["currentTime"], 30.0);

    let (status, _, _) = send(
        &fixture.app,
        Method::GET,
        &format!("/api/me/progress/{}", fixture.item_id),
        Some(&root_token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        "/api/me",
        Some(&root_token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let me: Value = serde_json::from_slice(&body).unwrap();
    assert!(me["mediaProgress"].as_array().unwrap().is_empty());
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
async fn library_details_include_every_filterdata_field() {
    let fixture = fixture();
    let token = login(&fixture).await;
    let library_id = catalog_library_id(&fixture, &token).await;
    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        &format!("/api/libraries/{library_id}?include=filterdata"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_slice(&body).unwrap();
    for key in [
        "authors",
        "genres",
        "tags",
        "series",
        "narrators",
        "languages",
        "publishers",
        "publishedDecades",
    ] {
        assert!(
            value["filterdata"][key].is_array(),
            "filterdata.{key} must be an array"
        );
    }
}

#[tokio::test]
async fn metadata_json_populates_abs_item_and_filter_data() {
    let fixture = fixture();
    let token = login(&fixture).await;
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
    let metadata = &value["media"]["metadata"];
    assert_eq!(metadata["title"], "Harbor of Shadows");
    assert_eq!(metadata["subtitle"], "The Lighthouse File");
    assert_eq!(metadata["authors"][0]["name"], "Nora Vale");
    assert_eq!(metadata["narrators"][0], "Iris Quinn");
    assert_eq!(metadata["series"][0]["name"], "Stormbound");
    assert_eq!(metadata["series"][0]["sequence"], "2");
    assert_eq!(metadata["genres"][0], "Mystery");
    assert_eq!(metadata["publishedYear"], "2024");
    assert_eq!(metadata["publisher"], "Northwind Audio");
    assert_eq!(
        metadata["description"],
        "A harbor town keeps a dangerous secret."
    );
    assert_eq!(metadata["isbn"], "9780000000001");
    assert_eq!(metadata["asin"], "B000000001");
    assert_eq!(metadata["language"], "en");
    assert_eq!(value["media"]["tags"][0], "favorite");
    assert_eq!(value["media"]["numChapters"], 2);
    assert_eq!(value["media"]["chapters"][0]["id"], 1);
    assert_eq!(value["media"]["chapters"][1]["id"], 2);
    assert_eq!(value["media"]["chapters"][1]["title"], "The Arrival");
    assert_eq!(value["media"]["chapters"][1]["start"], 5.0);

    let library_id = catalog_library_id(&fixture, &token).await;
    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        &format!("/api/libraries/{library_id}?include=filterdata"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_slice(&body).unwrap();
    let has_named_value = |key: &str, expected: &str| {
        value["filterdata"][key].as_array().is_some_and(|values| {
            values.iter().any(|entry| {
                let text = entry.as_str().unwrap_or_default();
                let name = entry["name"].as_str().unwrap_or_default();
                text == expected || name == expected
            })
        })
    };
    assert!(has_named_value("authors", "Nora Vale"));
    assert!(has_named_value("genres", "Mystery"));
    assert!(has_named_value("tags", "favorite"));
    assert!(has_named_value("series", "Stormbound"));
    assert!(has_named_value("narrators", "Iris Quinn"));
    assert!(has_named_value("publishers", "Northwind Audio"));
    assert!(has_named_value("publishedDecades", "2020s"));

    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        &format!("/api/libraries/{library_id}/items?minified=1"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_slice(&body).unwrap();
    let minified = &value["results"][0]["media"]["metadata"];
    assert_eq!(minified["narratorName"], "Iris Quinn");
    assert_eq!(minified["seriesName"], "Stormbound #2");
    assert_eq!(minified["genres"][0], "Mystery");
}

#[tokio::test]
async fn folder_structure_scan_matches_abs_book_layout() {
    let fixture = fixture();
    let library = fixture.root.join("library");
    let files = [
        (
            "Terry Goodkind/Sword of Truth/Vol 1 - 1994 - Wizards First Rule {Sam Tsoutsouvas}/Audio Track 1.mp3",
            b"track-one".as_slice(),
        ),
        (
            "Terry Goodkind/Sword of Truth/Vol 1 - 1994 - Wizards First Rule {Sam Tsoutsouvas}/Audio Track 2.mp3",
            b"track-two".as_slice(),
        ),
        (
            "Terry Goodkind/Sword of Truth/Vol 2 - 1995 - Stone of Tears/Audiobook.m4b",
            b"stone".as_slice(),
        ),
        (
            "Terry Goodkind/Heart of Black Ice - Sister of Darkness/Audio File.m4a",
            b"heart".as_slice(),
        ),
        (
            "Steven Levy/Hackers - Heroes of the Computer Revolution {Mike Chamberlain}/Audio File.m4a",
            b"hackers".as_slice(),
        ),
        ("1945 - Animal Farm/Audiobook.mp3", b"farm".as_slice()),
        ("Ender's Game.m4b", b"ender".as_slice()),
        ("Only Author/Only Author.m4a", b"author".as_slice()),
        ("Lee Child/No Metadata Book/Chapter 01.mp3", b"chapter".as_slice()),
        (
            "Lee Child/No Metadata Book/desc.txt",
            b"Description from desc.txt".as_slice(),
        ),
        (
            "Lee Child/No Metadata Book/reader.txt",
            b"Dick Hill\nIgnored second line".as_slice(),
        ),
        ("Disc Book/Disc 1/01.mp3", b"disc-one".as_slice()),
        ("Disc Book/Disc 2/01.mp3", b"disc-two".as_slice()),
        ("Disc Book/cover.jpg", b"disc-cover".as_slice()),
        ("Disc Book/art.jpg", b"wrong-cover".as_slice()),
    ];
    for (relative, contents) in files {
        let path = library.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    Catalog::clear_cache();

    let token = login(&fixture).await;
    let library_id = catalog_library_id(&fixture, &token).await;
    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        &format!("/api/libraries/{library_id}/items?limit=100&minified=1"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value: Value = serde_json::from_slice(&body).unwrap();
    let results = value["results"].as_array().unwrap();
    let find = |title: &str| {
        results
            .iter()
            .find(|item| item["media"]["metadata"]["title"] == title)
            .unwrap_or_else(|| panic!("missing item {title}"))
    };

    let wizards = find("Wizards First Rule");
    assert_eq!(wizards["media"]["metadata"]["authorName"], "Terry Goodkind");
    assert_eq!(
        wizards["media"]["metadata"]["seriesName"],
        "Sword of Truth #1"
    );
    assert_eq!(wizards["media"]["metadata"]["publishedYear"], "1994");
    assert_eq!(
        wizards["media"]["metadata"]["narratorName"],
        "Sam Tsoutsouvas"
    );
    assert_eq!(wizards["media"]["numAudioFiles"], 2);
    assert_eq!(wizards["isFile"], false);

    let stone = find("Stone of Tears");
    assert_eq!(
        stone["media"]["metadata"]["seriesName"],
        "Sword of Truth #2"
    );
    assert_eq!(stone["media"]["metadata"]["publishedYear"], "1995");

    let hackers = find("Hackers - Heroes of the Computer Revolution");
    assert_eq!(hackers["media"]["metadata"]["authorName"], "Steven Levy");
    assert_eq!(
        hackers["media"]["metadata"]["narratorName"],
        "Mike Chamberlain"
    );
    assert_eq!(hackers["media"]["metadata"]["seriesName"], "");

    let animal_farm = find("Animal Farm");
    assert_eq!(animal_farm["media"]["metadata"]["publishedYear"], "1945");
    assert_eq!(animal_farm["media"]["metadata"]["authorName"], "Unknown");

    let author_only = find("Only Author");
    assert_eq!(author_only["media"]["metadata"]["authorName"], "Unknown");

    let text_metadata = find("No Metadata Book");
    assert_eq!(
        text_metadata["media"]["metadata"]["authorName"],
        "Lee Child"
    );
    assert_eq!(
        text_metadata["media"]["metadata"]["narratorName"],
        "Dick Hill"
    );
    let text_metadata_id = text_metadata["id"].as_str().unwrap();
    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        &format!("/api/items/{text_metadata_id}"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let detail: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        detail["media"]["metadata"]["description"],
        "Description from desc.txt"
    );

    let ender = find("Ender's Game");
    assert_eq!(ender["isFile"], true);
    assert_eq!(ender["media"]["numAudioFiles"], 1);

    let disc_book = find("Disc Book");
    assert_eq!(disc_book["media"]["numAudioFiles"], 2);
    let disc_id = disc_book["id"].as_str().unwrap();
    let (status, body, _) = send(
        &fixture.app,
        Method::GET,
        &format!("/api/items/{disc_id}"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let detail: Value = serde_json::from_slice(&body).unwrap();
    let audio_files = detail["media"]["audioFiles"].as_array().unwrap();
    assert!(audio_files[0]["metadata"]["relPath"]
        .as_str()
        .unwrap()
        .contains("Disc 1"));
    assert!(audio_files[1]["metadata"]["relPath"]
        .as_str()
        .unwrap()
        .contains("Disc 2"));
    assert_eq!(audio_files[0]["discNumFromFilename"], 1);
    assert_eq!(audio_files[1]["discNumFromFilename"], 2);

    let (status, cover, _) = send(
        &fixture.app,
        Method::GET,
        &format!("/api/items/{disc_id}/cover"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cover, b"disc-cover");
}

#[tokio::test]
async fn library_items_use_zero_based_pages_and_limit_zero_means_all() {
    let fixture = fixture();
    let library = fixture.root.join("library");
    for name in [
        "Paged One.m4b",
        "Paged Two.m4b",
        "Paged Three.m4b",
        "Paged Four.m4b",
    ] {
        std::fs::write(library.join(name), b"audio").unwrap();
    }
    Catalog::clear_cache();
    let token = login(&fixture).await;
    let library_id = catalog_library_id(&fixture, &token).await;

    let first = send(
        &fixture.app,
        Method::GET,
        &format!("/api/libraries/{library_id}/items?limit=2&page=0&minified=1"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(first.0, StatusCode::OK);
    let first: Value = serde_json::from_slice(&first.1).unwrap();
    assert_eq!(first["page"], 0);
    assert_eq!(first["results"].as_array().unwrap().len(), 2);

    let second = send(
        &fixture.app,
        Method::GET,
        &format!("/api/libraries/{library_id}/items?limit=2&page=1&minified=1"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(second.0, StatusCode::OK);
    let second: Value = serde_json::from_slice(&second.1).unwrap();
    assert_eq!(second["page"], 1);
    assert_eq!(second["results"].as_array().unwrap().len(), 2);
    assert_ne!(
        first["results"][0]["id"], second["results"][0]["id"],
        "page 1 must not repeat page 0"
    );

    let all = send(
        &fixture.app,
        Method::GET,
        &format!("/api/libraries/{library_id}/items?limit=0&minified=1"),
        Some(&token),
        None,
        None,
    )
    .await;
    assert_eq!(all.0, StatusCode::OK);
    let all: Value = serde_json::from_slice(&all.1).unwrap();
    assert_eq!(all["limit"], 0);
    assert_eq!(
        all["results"].as_array().unwrap().len(),
        all["total"].as_u64().unwrap() as usize
    );
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
    assert_eq!(session["chapters"].as_array().unwrap().len(), 2);
    assert_eq!(session["chapters"][1]["title"], "The Arrival");
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
