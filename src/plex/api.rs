use crate::harbinger::markers;
use crate::harbinger::*;
use crate::plex::plex_remux;
use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocketUpgrade},
        Path, Query, RawQuery, State,
    },
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode, Uri},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use isolang::Language;
use rand_core::{OsRng, RngCore};
use rusqlite::Connection;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    net::SocketAddr,
    path::PathBuf,
    sync::{atomic::Ordering, Arc, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tracing::{info, warn};

use axum::routing::{any, delete, get, post, put};

/// Routes owned by the Plex Media Server-compatible 32400 listener surface.
pub(crate) fn plex_routes() -> Router<AppState> {
    Router::new()
        .route("/identity", get(plex_identity))
        .route("/myplex/account", get(plex_myplex_account))
        .route("/myplex/claim", post(plex_myplex_claim))
        .route(
            "/myplex/refreshReachability",
            put(plex_refresh_reachability),
        )
        .route("/accounts", get(plex_accounts))
        .route("/devices", get(plex_devices))
        .route("/accounts/:id", get(plex_account).put(update_plex_account))
        .route("/activities", get(plex_activities))
        .route("/status/sessions", get(plex_sessions))
        .route("/status/sessions/history/all", get(plex_session_history))
        .route("/servers", get(plex_servers))
        .route("/media/providers", get(plex_media_providers))
        .route("/services/browse", get(plex_service_browse_root))
        .route("/services/browse/:path", get(plex_service_browse_path))
        .route("/security/resources", get(plex_security_resources))
        .route("/hubs", get(plex_global_hubs))
        .route("/hubs/promoted", get(plex_global_hubs))
        .route("/hubs/continueWatching", get(plex_continue_watching_hub))
        .route("/hubs/search", get(plex_hub_search))
        .route("/hubs/items", get(plex_hub_items))
        .route(
            "/actions/removeFromContinueWatching",
            put(plex_remove_from_continue_watching),
        )
        .route("/hubs/sections/:section", get(plex_section_hubs))
        .route(
            "/hubs/sections/:section/continueWatching/items",
            get(plex_section_continue_watching_items),
        )
        .route("/hubs/metadata/:id/related", get(plex_related_hubs))
        .route("/hubs/sections/:section/:hub", get(plex_section_hub))
        .route(
            "/library/sections",
            get(plex_sections).post(plex_create_section),
        )
        .route("/library/sections/prefs", get(plex_library_section_prefs))
        .route("/:root/plugins/:plugin/prefs", get(plex_agent_prefs))
        .route(
            "/library/sections/all",
            get(plex_sections).post(plex_create_section_official),
        )
        .route("/system/scanners/:section", get(plex_system_scanners))
        .route("/system/agents", get(plex_system_agents))
        .route(
            "/library/sections/:section",
            get(plex_section)
                .put(plex_update_section)
                .delete(plex_delete_section),
        )
        .route(
            "/library/sections/:section/refresh",
            get(plex_section_refresh).post(plex_section_refresh),
        )
        .route(
            "/library/sections/:section/collections",
            get(plex_section_collections),
        )
        .route(
            "/library/collections",
            get(plex_collections).post(plex_create_collection),
        )
        .route(
            "/library/collections/:id/items",
            get(plex_collection_items).put(plex_add_collection_item),
        )
        .route(
            "/library/collections/:id/items/:item_id",
            put(plex_remove_collection_item),
        )
        .route(
            "/library/sections/:section/prefs",
            get(plex_section_prefs).put(plex_update_section_prefs),
        )
        .route("/library/sections/:section/sorts", get(plex_section_sorts))
        .route(
            "/library/sections/:section/filters",
            get(plex_section_filters),
        )
        .route(
            "/library/sections/:section/firstCharacter",
            get(plex_section_first_characters),
        )
        .route(
            "/library/sections/:section/firstCharacter/:letter",
            get(plex_section_first_character_items),
        )
        .route(
            "/library/sections/:section/firstCharacters",
            get(plex_section_first_characters),
        )
        .route(
            "/library/sections/:section/intros",
            delete(plex_delete_section_markers),
        )
        .route("/library/search", get(plex_library_search))
        .route("/library/all", get(plex_library_all))
        .route("/library/sections/:section/all", get(plex_all))
        .route("/library/metadata/:id", get(plex_item))
        .route("/library/metadata/:id/children", get(plex_item))
        .route("/library/metadata/:ids/intro", put(plex_detect_markers))
        .route("/library/metadata/:ids/credits", put(plex_detect_markers))
        .route("/library/metadata/:ids/marker", post(plex_create_marker))
        .route(
            "/library/metadata/:ids/marker/:marker",
            put(plex_edit_marker).delete(plex_delete_marker),
        )
        .route("/library/metadata/:id/related", get(plex_related_hubs))
        .route("/library/metadata/:id/extras", get(plex_extras))
        .route("/library/metadata/:id/media/:part", get(plex_part))
        .route("/library/parts/:id", put(plex_set_streams))
        .route("/library/parts/:id/*path", get(plex_part_file))
        .route("/library/streams/:stream", get(plex_stream))
        .route("/playQueues", post(plex_create_play_queue))
        .route("/playQueues/:id", get(plex_get_play_queue))
        .route("/playlists", get(plex_playlists).post(plex_create_playlist))
        .route(
            "/playlists/:id",
            get(plex_get_playlist)
                .put(plex_update_playlist)
                .delete(plex_delete_playlist),
        )
        .route(
            "/playlists/:id/items",
            get(plex_playlist_items)
                .put(plex_add_playlist_item)
                .delete(plex_clear_playlist_items),
        )
        .route(
            "/playlists/:id/items/:item_id",
            delete(plex_delete_playlist_item),
        )
        .route(
            "/playlists/:id/items/:item_id/move",
            put(plex_move_playlist_item),
        )
        .route("/library/metadata/:id/thumb", get(plex_thumb))
        .route("/library/metadata/:id/art", get(plex_art))
        .route("/media/:id", any(stream_media))
}

pub(crate) const PLEX_DEFAULT_AUDIO_LANGUAGE: &str = "en";

pub(crate) fn plex_machine_identifier(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    digest[..20]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn plex_library_uuid(state: &AppState, library_name: &str, section: usize) -> String {
    let (stable_id, _, _) = library_publication_state(state, library_name, section);
    plex_library_uuid_for_id(&state.machine_identifier, stable_id)
}

pub(crate) fn plex_library_uuid_for_id(machine_identifier: &str, stable_id: i64) -> String {
    let digest = Sha256::digest(format!("{machine_identifier}:library:{stable_id}").as_bytes());
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0],
        digest[1],
        digest[2],
        digest[3],
        digest[4],
        digest[5],
        digest[6],
        digest[7],
        digest[8],
        digest[9],
        digest[10],
        digest[11],
        digest[12],
        digest[13],
        digest[14],
        digest[15]
    )
}

pub(crate) struct PlexRemuxSession {
    pub(crate) playback: Arc<plex_remux::RemuxedPlayback>,
    pub(crate) source_path: PathBuf,
    pub(crate) protocol: String,
    pub(crate) transcode_audio: bool,
    pub(crate) transcode_video: bool,
    pub(crate) offset_seconds: f64,
    pub(crate) expires_at: Instant,
}

pub(crate) const PLEX_REMUX_SESSION_TTL: Duration = Duration::from_secs(4 * 60 * 60);

pub(crate) static PLEX_REMUX_BUILD_LIMITER: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();

pub(crate) fn plex_library_section_details(
    state: &AppState,
    library_id: i64,
) -> (u64, Vec<serde_json::Value>) {
    let Ok(connection) = state.db.lock() else {
        return (0, Vec::new());
    };
    let created_at = connection
        .query_row(
            "SELECT added_at FROM library_groups WHERE id = ?1",
            [library_id],
            |row| row.get::<_, u64>(0),
        )
        .unwrap_or_default();
    let Ok(mut statement) = connection
        .prepare("SELECT id, path FROM library_folders WHERE library_id = ?1 ORDER BY id")
    else {
        return (created_at, Vec::new());
    };
    let Ok(locations) = statement.query_map([library_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    }) else {
        return (created_at, Vec::new());
    };
    let locations = locations
        .filter_map(Result::ok)
        .map(|(id, path)| serde_json::json!({"id": id, "path": path}))
        .collect();
    (created_at, locations)
}

#[derive(Deserialize)]
pub(crate) struct PlexPinResponse {
    pub(crate) id: i64,
    pub(crate) code: String,
}

#[derive(Deserialize, Default)]
pub(crate) struct PlexAuthStatusQuery {
    pub(crate) pin_id: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct PlexPinStatusResponse {
    #[serde(rename = "authToken")]
    pub(crate) auth_token: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct PlexClaimTokenResponse {
    pub(crate) token: String,
}

pub(crate) fn plex_auth_client_identifier(state: &AppState) -> String {
    format!("{}-auth", state.machine_identifier)
}

pub(crate) const PLEX_AUTH_PRODUCT: &str = "harbinger";

pub(crate) fn plex_auth_product(_state: &AppState) -> String {
    PLEX_AUTH_PRODUCT.to_owned()
}

pub(crate) fn plex_auth_url(
    client_identifier: &str,
    pin_code: &str,
    server_name: &str,
    callback: &str,
) -> String {
    format!(
        "https://app.plex.tv/auth#?clientID={}&code={}&context%5Bdevice%5D%5Bproduct%5D={}&context%5Bdevice%5D%5Bversion%5D={}&context%5Bdevice%5D%5Bplatform%5D=macOS&context%5Bdevice%5D%5BdeviceName%5D={}&forwardUrl={}",
        url_encode(client_identifier),
        url_encode(pin_code),
        url_encode(PLEX_AUTH_PRODUCT),
        env!("CARGO_PKG_VERSION"),
        url_encode(server_name),
        url_encode(callback)
    )
}

pub(crate) fn plex_pin_status_url(pin_id: i64, device_jwt: &str) -> reqwest::Url {
    let mut url = reqwest::Url::parse(&format!("https://clients.plex.tv/api/v2/pins/{pin_id}"))
        .expect("Plex PIN status URL is valid");
    url.query_pairs_mut().append_pair("deviceJWT", device_jwt);
    url
}

pub(crate) fn plex_pin_error_summary(body: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(body).ok()?;
    let message = value
        .get("error")
        .and_then(serde_json::Value::as_str)
        .or_else(|| value.get("message").and_then(serde_json::Value::as_str))
        .or_else(|| {
            value
                .get("errors")
                .and_then(serde_json::Value::as_array)
                .and_then(|errors| errors.first())
                .and_then(|error| error.get("message"))
                .and_then(serde_json::Value::as_str)
        })?;
    let summary = message
        .chars()
        .filter(|character| !character.is_control())
        .take(160)
        .collect::<String>();
    (!summary.is_empty()).then_some(summary)
}

pub(crate) fn plex_auth_pin_id(
    connection: &rusqlite::Connection,
    requested_id: Option<i64>,
) -> Option<i64> {
    if let Some(pin_id) = requested_id {
        return connection
            .query_row("SELECT id FROM plex_auth WHERE id = ?1", [pin_id], |row| {
                row.get(0)
            })
            .ok();
    }
    connection
        .query_row(
            "SELECT id FROM plex_auth ORDER BY created_at DESC, id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok()
}

pub(crate) fn plex_device_key(state: &AppState) -> Result<(SigningKey, String), String> {
    let connection = state
        .db
        .lock()
        .map_err(|_| "could not access Plex device key storage".to_owned())?;
    let existing = connection.query_row(
        "SELECT private_key, kid FROM plex_device_keys WHERE id = 1",
        [],
        |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?)),
    );
    if let Ok((private_key, kid)) = existing {
        let bytes: [u8; 32] = private_key
            .try_into()
            .map_err(|_| "stored Plex device key has an invalid length".to_owned())?;
        return Ok((SigningKey::from_bytes(&bytes), kid));
    }
    let signing_key = SigningKey::generate(&mut OsRng);
    let kid = format!("{}-jwk", state.machine_identifier);
    connection
        .execute(
            "INSERT INTO plex_device_keys (id, private_key, public_key, kid) VALUES (1, ?1, ?2, ?3)",
            (
                signing_key.to_bytes().as_slice(),
                signing_key.verifying_key().to_bytes().as_slice(),
                &kid,
            ),
        )
        .map_err(|_| "could not persist Plex device key".to_owned())?;
    Ok((signing_key, kid))
}

pub(crate) fn plex_device_jwk(signing_key: &SigningKey, kid: &str) -> serde_json::Value {
    serde_json::json!({
        "kty": "OKP",
        "crv": "Ed25519",
        "use": "sig",
        "x": URL_SAFE_NO_PAD.encode(signing_key.verifying_key().to_bytes()),
        "kid": kid,
        "alg": "EdDSA"
    })
}

pub(crate) fn plex_device_jwt(signing_key: &SigningKey, kid: &str, client_id: &str) -> String {
    let header = serde_json::json!({"alg": "EdDSA", "kid": kid, "typ": "JWT"});
    let issued_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let payload = serde_json::json!({
        "aud": "plex.tv",
        "iss": client_id,
        "iat": issued_at,
        "exp": issued_at + 3600,
        "scope": "username,email,friendly_name"
    });
    let encoded_header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap_or_default());
    let encoded_payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap_or_default());
    let signing_input = format!("{encoded_header}.{encoded_payload}");
    let signature = signing_key.sign(signing_input.as_bytes());
    format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    )
}

pub(crate) async fn plex_auth_callback(
    State(state): State<AppState>,
    Query(query): Query<PlexAuthStatusQuery>,
) -> Response {
    let Some(pin_id) = query.pin_id else {
        return Redirect::to(
            "/settings?error=Plex%20sign-in%20callback%20did%20not%20identify%20its%20PIN.",
        )
        .into_response();
    };
    for attempt in 0..15 {
        let status = plex_auth_status_for(state.clone(), Some(pin_id)).await;
        if status.status() == StatusCode::NOT_FOUND {
            if let Ok(connection) = state.db.lock() {
                let _ = connection.execute("DELETE FROM plex_auth WHERE id = ?1", [pin_id]);
            }
            return Redirect::to(
                "/settings?error=Plex%20rejected%20the%20PIN.%20Start%20sign-in%20again.",
            )
            .into_response();
        }
        let pending = state
            .db
            .lock()
            .ok()
            .and_then(|connection| {
                connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM plex_auth WHERE id = ?1)",
                        [pin_id],
                        |row| row.get::<_, bool>(0),
                    )
                    .ok()
            })
            .unwrap_or(false);
        if !pending {
            return Redirect::to("/settings").into_response();
        }
        if attempt < 14 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    Redirect::to("/settings?error=Plex%20sign-in%20was%20not%20confirmed.%20Please%20try%20again.")
        .into_response()
}

pub(crate) async fn plex_auth_status(State(state): State<AppState>) -> Response {
    plex_auth_status_for(state, None).await
}

pub(crate) async fn plex_auth_status_for(state: AppState, requested_id: Option<i64>) -> Response {
    let Some(pin_id) = state
        .db
        .lock()
        .ok()
        .and_then(|connection| plex_auth_pin_id(&connection, requested_id))
    else {
        return Json(serde_json::json!({"authorized": false})).into_response();
    };
    let Ok((signing_key, kid)) = plex_device_key(&state) else {
        return Json(serde_json::json!({"authorized": false})).into_response();
    };
    let device_jwt = plex_device_jwt(&signing_key, &kid, &plex_auth_client_identifier(&state));
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            warn!(%error, "could not initialize Plex PIN check");
            return Json(serde_json::json!({"authorized": false})).into_response();
        }
    };
    let response = client
        .get(plex_pin_status_url(pin_id, &device_jwt))
        .header("Accept", "application/json")
        .header("X-Plex-Product", plex_auth_product(&state))
        .header(
            "X-Plex-Client-Identifier",
            plex_auth_client_identifier(&state),
        )
        .send()
        .await;
    let Ok(response) = response else {
        warn!("Plex PIN check failed before a response was received");
        return Json(serde_json::json!({"authorized": false})).into_response();
    };
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let error = plex_pin_error_summary(&body);
        warn!(
            ?status,
            plex_error = error.as_deref().unwrap_or("unavailable"),
            client_identifier = %plex_auth_client_identifier(&state),
            product = "harbinger",
            "Plex PIN check was rejected"
        );
        return (status, Json(serde_json::json!({"authorized": false}))).into_response();
    }
    let Ok(pin) = response.json::<PlexPinStatusResponse>().await else {
        warn!("Plex PIN check returned an invalid response");
        return Json(serde_json::json!({"authorized": false})).into_response();
    };
    let Some(token) = pin.auth_token.filter(|token| !token.is_empty()) else {
        return Json(serde_json::json!({"authorized": false})).into_response();
    };
    let claim_result = claim_and_publish_plex_server(&state, &token).await;
    let (server_token, claimed, claim_error) = match claim_result {
        Ok((server_token, claimed, error)) => (server_token, claimed, error),
        Err(error) => {
            warn!(error = %error, "Plex server claim or resource publication failed");
            (token.clone(), false, Some(error))
        }
    };
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if connection
        .execute(
            "UPDATE server_identity SET plex_account_token = ?1, claimed = ?2, plex_last_error = ?3 WHERE id = 1",
            (&server_token, claimed, claim_error.as_deref()),
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let _ = connection.execute("DELETE FROM plex_auth WHERE id = ?1", [pin_id]);
    drop(connection);
    if claimed {
        let presence_state = state.clone();
        tokio::spawn(async move {
            let client = reqwest::Client::new();
            run_plex_presence(&client, &presence_state, &server_token).await;
        });
    }
    Json(serde_json::json!({
        "authorized": true,
        "claimed": claimed,
        "error": claim_error
    }))
    .into_response()
}

pub(crate) fn plex_startup_registration(
    connection: &rusqlite::Connection,
) -> Option<(String, bool)> {
    let (token, claimed) = connection
        .query_row(
            "SELECT plex_account_token, claimed FROM server_identity WHERE id = 1",
            [],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, bool>(1)?)),
        )
        .ok()?;
    Some((token?, claimed))
}

pub(crate) fn plex_resource_connection_summary(
    resources: &serde_json::Value,
    machine_identifier: &str,
) -> serde_json::Value {
    serde_json::Value::Array(
        resources
            .as_array()
            .into_iter()
            .flatten()
            .filter(|resource| {
                resource
                    .get("clientIdentifier")
                    .and_then(serde_json::Value::as_str)
                    == Some(machine_identifier)
            })
            .map(|resource| {
                let connections = resource
                    .get("connections")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|connection| {
                        serde_json::json!({
                            "address": connection.get("address"),
                            "port": connection.get("port"),
                            "protocol": connection.get("protocol"),
                            "local": connection.get("local"),
                            "uri": connection.get("uri")
                        })
                    })
                    .collect::<Vec<_>>();
                serde_json::json!({
                    "name": resource.get("name"),
                    "provides": resource.get("provides"),
                    "connections": connections
                })
            })
            .collect(),
    )
}

pub(crate) fn plex_account_resource_inventory(resources: &serde_json::Value) -> serde_json::Value {
    serde_json::Value::Array(
        resources
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|resource| {
                let provides = resource
                    .get("provides")
                    .and_then(serde_json::Value::as_str)?;
                if !provides
                    .split(',')
                    .any(|capability| capability.trim() == "server")
                {
                    return None;
                }
                Some(serde_json::json!({
                    "clientIdentifier": resource
                        .get("clientIdentifier")
                        .or_else(|| resource.get("machineIdentifier")),
                    "name": resource.get("name"),
                    "provides": provides,
                }))
            })
            .collect(),
    )
}

pub(crate) async fn plex_account_id(
    client: &reqwest::Client,
    state: &AppState,
    server_token: &str,
) -> Result<String, String> {
    let response = client
        .get("https://servers.plex.tv/api/v2/user")
        .query(&[("includeSubscriptions", "1"), ("includeProviders", "1")])
        .header("User-Agent", "PlexMediaServer/1.43.4.10903-e5521bd8c")
        .header("Accept", "*/*")
        .header("Accept-Encoding", "gzip")
        .header(
            "X-Plex-Client-Identifier",
            state.machine_identifier.as_ref(),
        )
        .header("X-Plex-Version", "1.43.4.10903-e5521bd8c")
        .header("X-Plex-Provides", "server")
        .header("X-Plex-Platform", "MacOSX")
        .header("X-Plex-Product", "Plex Media Server")
        .header("X-Plex-Platform-Version", std::env::consts::ARCH)
        .header("X-Plex-Device-Name", server_name(state))
        .header("X-Plex-Device", "MacOSX")
        .header("X-Plex-Model", std::env::consts::ARCH)
        .header("X-Plex-Device-Vendor", "Apple")
        .header("X-Plex-Token", server_token)
        .send()
        .await
        .map_err(|error| format!("could not read the Plex account for presence: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Plex account lookup for presence failed: {error}"))?;
    let body = response
        .text()
        .await
        .map_err(|error| format!("could not read the Plex account response: {error}"))?;
    let id = xml_attribute(&body, "id")
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| "Plex account response did not include a numeric id".to_owned())?;
    Ok(id)
}

pub(crate) async fn plex_connectivity_check(
    client: &reqwest::Client,
    state: &AppState,
    server_token: &str,
) -> Result<(), String> {
    let async_identifier = plex_async_identifier();
    let response = client
        .put(plex_connectivity_url(&state.machine_identifier))
        .query(&[
            ("X-Plex-Token", server_token),
            ("asyncIdentifier", async_identifier.as_str()),
        ])
        .header("User-Agent", "PlexMediaServer/1.43.4.10903-e5521bd8c")
        .header("Accept", "*/*")
        .header("Accept-Encoding", "gzip")
        .header(
            "X-Plex-Client-Identifier",
            state.machine_identifier.as_ref(),
        )
        .header("X-Plex-Version", "1.43.4.10903-e5521bd8c")
        .header("X-Plex-Provides", "server")
        .header("X-Plex-Platform", "MacOSX")
        .header("X-Plex-Product", "Plex Media Server")
        .header("X-Plex-Platform-Version", std::env::consts::ARCH)
        .header("X-Plex-Device-Name", server_name(state))
        .header("X-Plex-Device", "MacOSX")
        .header("X-Plex-Model", std::env::consts::ARCH)
        .header("X-Plex-Device-Vendor", "Apple")
        .body(Vec::new())
        .send()
        .await
        .map_err(|error| format!("could not request Plex connectivity check: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Plex rejected the connectivity check: {error}"))?;
    info!(status = %response.status(), "Plex connectivity check response");
    Ok(())
}

pub(crate) fn plex_presence_url(account_id: &str, machine_identifier: &str) -> String {
    format!("https://pubsub.plex.tv/sub/eventsource/{account_id}/{machine_identifier}")
}

pub(crate) fn plex_connectivity_url(machine_identifier: &str) -> String {
    format!("https://plex.tv/api/servers/{machine_identifier}/connectivity")
}

pub(crate) fn plex_async_identifier() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

pub(crate) fn plex_server_registration_url() -> &'static str {
    "https://plex.tv/servers.xml"
}

pub(crate) fn plex_device_announce_url(machine_identifier: &str) -> String {
    format!(
        "https://servers.plex.tv/devices/{}",
        url_encode(machine_identifier)
    )
}

pub(crate) fn plex_device_announce_query<'a>(
    urls: &'a [String],
    https_enabled: bool,
    server_token: &'a str,
) -> Vec<(&'static str, &'a str)> {
    urls.iter()
        .map(|url| ("Connection[][uri]", url.as_str()))
        .chain([
            ("httpsEnabled", if https_enabled { "1" } else { "0" }),
            ("httpsRequired", "0"),
            ("dnsRebindingProtection", "0"),
            ("X-Plex-Token", server_token),
        ])
        .collect()
}

pub(crate) fn plex_server_registration_xml(
    state: &AppState,
    configured_server_name: &str,
) -> Result<String, String> {
    let local_addresses = plex_local_addresses(state);
    let local_addresses = local_addresses
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let libraries = state
        .config
        .lock()
        .map_err(|_| "could not read harbinger libraries".to_owned())?
        .libraries
        .clone();
    let mut directories = String::new();
    for (index, library) in libraries.iter().enumerate() {
        let section = index + 1;
        directories.push_str(&format!(
            "<Directory key=\"{section}\" type=\"{}\" title=\"{}\" uuid=\"mplex-{section}\" />",
            xml_escape(plex_kind(&library.kind)),
            xml_escape(&library.name),
        ));
    }
    let mut registration_date = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock is before the Unix epoch".to_owned())?
        .as_micros() as i64
        & !0xf;
    registration_date ^= (registration_date >> 3) & 0x38;
    registration_date ^= (registration_date >> 3) & 0x7;
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><MediaContainer size=\"{}\"><Server name=\"{}\" host=\"\" localAddresses=\"{}\" machineIdentifier=\"{}\" version=\"1.43.4.10903-e5521bd8c\" date=\"{}\" relay=\"0\" />{directories}</MediaContainer>",
        libraries.len() + 1,
        xml_escape(configured_server_name),
        xml_escape(&local_addresses),
        xml_escape(&state.machine_identifier),
        registration_date,
    ))
}

pub(crate) fn plex_resources_contain(
    resources: &serde_json::Value,
    machine_identifier: &str,
) -> bool {
    resources.as_array().into_iter().flatten().any(|resource| {
        let identifier_matches = resource
            .get("clientIdentifier")
            .or_else(|| resource.get("machineIdentifier"))
            .and_then(serde_json::Value::as_str)
            == Some(machine_identifier);
        let provides_server = resource
            .get("provides")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|provides| {
                provides
                    .split(',')
                    .any(|capability| capability.trim() == "server")
            });
        let has_access_token = resource
            .get("accessToken")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|token| !token.is_empty());
        let has_connection = resource
            .get("connections")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|connections| {
                connections.iter().any(|connection| {
                    connection
                        .get("uri")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|uri| !uri.is_empty())
                        && connection
                            .get("address")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|address| !address.is_empty())
                        && connection
                            .get("protocol")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|protocol| !protocol.is_empty())
                        && connection
                            .get("port")
                            .and_then(serde_json::Value::as_u64)
                            .is_some_and(|port| port > 0)
                })
            });
        identifier_matches && provides_server && has_access_token && has_connection
    })
}

pub(crate) fn plex_resources_contain_local_https(
    resources: &serde_json::Value,
    machine_identifier: &str,
) -> bool {
    resources.as_array().into_iter().flatten().any(|resource| {
        resource
            .get("clientIdentifier")
            .and_then(serde_json::Value::as_str)
            == Some(machine_identifier)
            && resource
                .get("connections")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|connections| {
                    connections.iter().any(|connection| {
                        connection.get("local").and_then(serde_json::Value::as_bool) == Some(true)
                            && connection
                                .get("protocol")
                                .and_then(serde_json::Value::as_str)
                                == Some("https")
                            && connection
                                .get("uri")
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(|uri| {
                                    uri.starts_with("https://") && uri.contains(".plex.direct:")
                                })
                    })
                })
    })
}

pub(crate) fn plex_resources_contain_local_connection(
    resources: &serde_json::Value,
    machine_identifier: &str,
) -> bool {
    resources.as_array().into_iter().flatten().any(|resource| {
        resource
            .get("clientIdentifier")
            .and_then(serde_json::Value::as_str)
            == Some(machine_identifier)
            && resource
                .get("connections")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|connections| {
                    connections.iter().any(|connection| {
                        connection.get("local").and_then(serde_json::Value::as_bool) == Some(true)
                            && connection
                                .get("uri")
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(|uri| {
                                    uri.starts_with("http://") || uri.starts_with("https://")
                                })
                    })
                })
    })
}

pub(crate) fn plex_connection_urls(state: &AppState) -> Vec<String> {
    let bind = state
        .config
        .lock()
        .ok()
        .and_then(|config| config.bind.parse::<SocketAddr>().ok());
    let Some(bind) = bind else {
        return Vec::new();
    };
    plex_publish_urls(
        &plex_local_addresses(state),
        bind.port(),
        state.plex_tls.hash().as_deref(),
    )
}

pub(crate) fn plex_publish_urls(
    addresses: &[std::net::IpAddr],
    port: u16,
    plex_direct_hash: Option<&str>,
) -> Vec<String> {
    let mut urls =
        Vec::with_capacity(addresses.len() * if plex_direct_hash.is_some() { 2 } else { 1 });
    for address in addresses {
        urls.push(format!("http://{address}:{port}"));
        if let (Some(hash), std::net::IpAddr::V4(address)) = (plex_direct_hash, address) {
            let plex_direct_host = address.to_string().replace('.', "-");
            urls.push(format!(
                "https://{plex_direct_host}.{hash}.plex.direct:{port}"
            ));
        }
    }
    urls
}

pub(crate) fn plex_local_addresses(state: &AppState) -> Vec<std::net::IpAddr> {
    let bind = state
        .config
        .lock()
        .ok()
        .and_then(|config| config.bind.parse::<SocketAddr>().ok());
    let Some(bind) = bind else {
        return Vec::new();
    };
    if !bind.ip().is_unspecified() {
        return vec![bind.ip()];
    }
    let mut addresses = get_if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|interface| match interface.addr {
            get_if_addrs::IfAddr::V4(address)
                if !address.ip.is_loopback() && !address.ip.is_unspecified() =>
            {
                Some(std::net::IpAddr::V4(address.ip))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        let fallback = std::net::UdpSocket::bind("0.0.0.0:0")
            .ok()
            .and_then(|socket| {
                socket.connect("8.8.8.8:80").ok()?;
                socket.local_addr().ok().map(|address| address.ip())
            })
            .unwrap_or_else(|| std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
        addresses.push(fallback);
    }
    addresses
}

pub(crate) struct PlexTvShowGroup<'a> {
    pub(crate) title: String,
    pub(crate) rating_key: String,
    pub(crate) episodes: Vec<&'a Media>,
}

pub(crate) struct PlexTvEpisodeGroup<'a> {
    pub(crate) episode: Option<u32>,
    pub(crate) files: Vec<&'a Media>,
}

pub(crate) fn plex_tv_show_key(section: usize, title: &str) -> String {
    format!(
        "{section}-show-{}",
        plex_safe_numeric_id(&format!("tv-show:{section}:{}", title.to_lowercase()))
    )
}

pub(crate) fn plex_tv_season_key(show_key: &str, season: u32) -> String {
    format!("{show_key}-season-{season}")
}

pub(crate) fn plex_tv_show_groups<'a>(
    section: usize,
    roots: &[PathBuf],
    media: impl IntoIterator<Item = &'a Media>,
) -> Vec<PlexTvShowGroup<'a>> {
    let mut groups = BTreeMap::<String, PlexTvShowGroup<'a>>::new();
    for item in media {
        let Some(root) = roots.iter().find(|root| item.path.starts_with(root)) else {
            continue;
        };
        let title = tv_show_title(root, &item.path);
        let grouping_key = title.to_lowercase();
        let group = groups
            .entry(grouping_key)
            .or_insert_with(|| PlexTvShowGroup {
                rating_key: plex_tv_show_key(section, &title),
                title,
                episodes: Vec::new(),
            });
        group.episodes.push(item);
    }
    groups.into_values().collect()
}

pub(crate) fn plex_ranked_tv_shows<'a>(
    ranked_media: &[&'a Media],
    shows: &'a [PlexTvShowGroup<'a>],
) -> Vec<&'a PlexTvShowGroup<'a>> {
    let mut seen = BTreeSet::new();
    ranked_media
        .iter()
        .filter_map(|media| {
            shows
                .iter()
                .find(|show| show.episodes.iter().any(|episode| episode.id == media.id))
        })
        .filter(|show| seen.insert(show.rating_key.clone()))
        .collect()
}

pub(crate) fn plex_video_profile(value: &str) -> String {
    let profile = value
        .split('@')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase();
    match profile.as_str() {
        "constrained baseline" => "baseline".to_owned(),
        "high 10" | "high 10 intra" => "high10".to_owned(),
        "high 4:2:2" => "high422".to_owned(),
        "high 4:4:4" => "high444".to_owned(),
        "main 10" => "main10".to_owned(),
        _ => profile.replace([' ', '-'], ""),
    }
}

pub(crate) fn plex_video_level(value: &str) -> Option<u64> {
    let level = value.trim().parse::<f64>().ok()?;
    (level.is_finite() && level > 0.0).then(|| (level * 10.0).round() as u64)
}

pub(crate) const PLEX_MAX_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;

pub(crate) fn plex_safe_numeric_id(value: &str) -> u64 {
    let digest = Sha256::digest(value.as_bytes());
    let id = u64::from_be_bytes(digest[..8].try_into().unwrap_or_default()) & PLEX_MAX_SAFE_INTEGER;
    id.max(1)
}

pub(crate) fn plex_stream_id(media: &Media, path: &std::path::Path) -> u64 {
    plex_safe_numeric_id(&format!("stream:{}:{}", media.id, path.to_string_lossy()))
}

pub(crate) fn plex_part_id(media: &Media) -> u64 {
    plex_safe_numeric_id(&format!("part:{}", media.id))
}

pub(crate) fn plex_media_id(media: &Media) -> u64 {
    plex_safe_numeric_id(&format!("media:{}", media.id))
}

pub(crate) fn plex_rating_key(media: &Media) -> u64 {
    const ITEM_ID_BITS: u32 = 20;
    const ITEM_ID_MASK: u64 = (1 << ITEM_ID_BITS) - 1;
    const MAX_SECTION_ID: u64 = (i32::MAX as u64) >> ITEM_ID_BITS;

    media
        .id
        .split_once('-')
        .and_then(|(section, item)| {
            let section = section.parse::<u64>().ok()?;
            let item = item.parse::<u64>().ok()?;
            (section > 0 && section <= MAX_SECTION_ID && item > 0 && item <= ITEM_ID_MASK)
                .then_some((section << ITEM_ID_BITS) | item)
        })
        .unwrap_or_else(|| plex_safe_numeric_id(&format!("rating-key:{}", media.id)))
}

pub(crate) fn plex_external_subtitle_id(media: &Media, path: &std::path::Path) -> u64 {
    plex_stream_id(media, path)
}

pub(crate) fn plex_translated_subtitle_id(media: &Media, first: &str, second: &str) -> u64 {
    plex_safe_numeric_id(&format!("translated:{}:{}:{}", media.id, first, second))
}

pub(crate) fn plex_language(language: &str) -> (&'static str, &'static str) {
    let input = language.trim();
    if input.is_empty()
        || input.eq_ignore_ascii_case("und")
        || input.eq_ignore_ascii_case("unknown")
    {
        return ("und", "Unknown");
    }
    let input_code = input.split(['-', '_']).next().unwrap_or(input);
    let normalized = input_code.to_ascii_lowercase();
    let code = iso_639_2_bibliographic_alias(&normalized).unwrap_or(&normalized);
    let language = Language::from_639_1(code)
        .or_else(|| Language::from_639_3(code))
        .or_else(|| Language::from_name(input))
        .or_else(|| Language::from_name_lowercase(&input.to_ascii_lowercase()));
    language
        .map(|language| {
            (
                language.to_639_1().unwrap_or_else(|| language.to_639_3()),
                language.to_name(),
            )
        })
        .unwrap_or(("und", "Unknown"))
}

pub(crate) fn plex_language_code_3(code: &str) -> &'static str {
    if code == "und" {
        return "und";
    }
    Language::from_639_1(code)
        .or_else(|| Language::from_639_3(code))
        .map(|language| language.to_639_3())
        .unwrap_or("und")
}

pub(crate) fn plex_track_language(track: &serde_json::Value) -> (&'static str, &'static str) {
    let language = technical_text(track, "Language").unwrap_or("und");
    let normalized = plex_language(language);
    if normalized.0 != "und" {
        return normalized;
    }

    // Matroska files often store the human-readable language as the track name
    // while leaving the language tag unset.
    let title_language = technical_text(track, "Title")
        .unwrap_or_default()
        .split(|character: char| !character.is_ascii_alphabetic())
        .next()
        .unwrap_or_default();
    plex_language(title_language)
}

pub(crate) fn plex_video_resolution(height: u64) -> &'static str {
    match height {
        0..=480 => "480",
        481..=576 => "576",
        577..=720 => "720",
        721..=1080 => "1080",
        _ => "4k",
    }
}

pub(crate) fn plex_codec(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "avc" | "avc/h.264" | "h.264" => "h264".to_owned(),
        "hevc" | "h.265" | "hev1" => "hevc".to_owned(),
        "ac-3" | "ac3" => "ac3".to_owned(),
        "e-ac-3" | "eac3" => "eac3".to_owned(),
        "mpeg audio" | "mp3" => "mp3".to_owned(),
        _ => value.to_ascii_lowercase().replace(' ', ""),
    }
}

pub(crate) fn plex_track_label(track: &serde_json::Value, stream_type: u8) -> String {
    plex_track_label_with_fallback(track, stream_type, None)
}

pub(crate) fn plex_track_label_with_fallback(
    track: &serde_json::Value,
    stream_type: u8,
    fallback_language: Option<&str>,
) -> String {
    let (_, detected_language) = plex_track_language(track);
    let language = if detected_language == "Unknown" {
        fallback_language.unwrap_or(detected_language)
    } else {
        detected_language
    };
    let codec = match technical_text(track, "Format").unwrap_or("Unknown") {
        "AC-3" | "AC3" => "AC3",
        "E-AC-3" | "EAC3" => "EAC3",
        "AVC" => "H.264",
        "HEVC" => "H.265",
        codec => codec,
    };
    if stream_type == 1 {
        let height = technical_text(track, "Height")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_default();
        let resolution = match height {
            0..=576 => "SD",
            577..=720 => "720p",
            721..=1080 => "1080p",
            _ => "4K",
        };
        return format!("{resolution} ({codec})");
    }
    let detail = if stream_type == 2 {
        let count = technical_text(track, "Channels").and_then(|value| value.parse::<u8>().ok());
        let channels = match count {
            Some(6) => "5.1",
            Some(8) => "7.1",
            Some(2) => "Stereo",
            _ => technical_text(track, "ChannelLayout").unwrap_or_default(),
        };
        let channels = match channels {
            "6" => "5.1",
            "8" => "7.1",
            "2" => "Stereo",
            other => other,
        };
        if channels.is_empty() {
            codec.to_owned()
        } else {
            format!("{codec} {channels}")
        }
    } else {
        codec.to_owned()
    };
    if language == "Unknown" {
        detail
    } else {
        format!("{language} ({detail})")
    }
}

pub(crate) async fn plex_section_refresh(
    State(state): State<AppState>,
    Path(section): Path<usize>,
) -> Response {
    let Some(library_id) = section.checked_sub(1) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let library_exists = match state.config.lock() {
        Ok(config) => config.libraries.get(library_id).is_some(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if !library_exists {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Err(error) = scan_library_inventory(&state, library_id, false).await {
        warn!(section, %error, "Plex library scan failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let metadata_response = scan_library_metadata(
        State(state.clone()),
        Path(library_id),
        Query(ScanQuery {
            force: Some(false),
            path: None,
            refresh: None,
            assets_only: Some(false),
        }),
    )
    .await;
    if !metadata_response.status().is_success() && !metadata_response.status().is_redirection() {
        warn!(section, status = %metadata_response.status(), "Plex metadata refresh failed");
        return metadata_response;
    }
    StatusCode::OK.into_response()
}

#[derive(Deserialize)]
pub(crate) struct PlexTimelineQuery {
    #[serde(rename = "ratingKey")]
    pub(crate) rating_key: Option<String>,
    pub(crate) key: Option<String>,
    pub(crate) time: Option<i64>,
    pub(crate) duration: Option<i64>,
    #[serde(rename = "state")]
    pub(crate) playback_state: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct PlexScrobbleQuery {
    #[serde(rename = "ratingKey")]
    pub(crate) rating_key: Option<String>,
    pub(crate) key: Option<String>,
}

pub(crate) fn plex_query_media_id(rating_key: Option<&str>, key: Option<&str>) -> Option<String> {
    rating_key
        .filter(|key| !key.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            let key = key?;
            key.rsplit("/library/metadata/")
                .next()
                .filter(|id| !id.is_empty() && !id.contains('/'))
                .map(str::to_owned)
        })
}

pub(crate) fn plex_playback_media_from_query(
    state: &AppState,
    query: &BTreeMap<String, String>,
) -> Option<Media> {
    let path = query.get("path").or_else(|| query.get("key"));
    path.and_then(|path| {
        let part_path = path
            .find("/library/parts/")
            .map(|index| &path[index + "/library/parts/".len()..]);
        part_path
            .and_then(|path| path.split('/').next())
            .and_then(|part_id| find_media_part(state, part_id))
            .or_else(|| {
                plex_query_media_id(None, Some(path))
                    .and_then(|media_id| find_media(state, &media_id))
            })
    })
    .or_else(|| {
        query
            .get("ratingKey")
            .or_else(|| query.get("mediaId"))
            .and_then(|media_id| find_media(state, media_id))
    })
}

pub(crate) async fn plex_remux_start(
    state: &AppState,
    extension: &str,
    query: &BTreeMap<String, String>,
) -> Response {
    let (protocol, content_type) = if extension.eq_ignore_ascii_case("m3u8") {
        ("hls", "application/vnd.apple.mpegurl")
    } else if extension.eq_ignore_ascii_case("mpd") {
        ("dash", "application/dash+xml")
    } else {
        return StatusCode::NOT_IMPLEMENTED.into_response();
    };
    let direct_stream = query.get("directStream").map(String::as_str);
    if query
        .get("protocol")
        .is_some_and(|requested| !requested.eq_ignore_ascii_case(protocol))
        || !matches!(direct_stream, Some("0" | "1"))
    {
        warn!(
            protocol,
            reason = "unsupported_conversion",
            "Plex remux request rejected"
        );
        return plex_remux_unavailable(protocol);
    }
    let transcode_video = direct_stream == Some("0");
    let matroska_hls =
        protocol == "hls" && direct_stream == Some("1") && plex_profile_requests_hls_mkv(&query);

    let offset_seconds = match query.get("offset") {
        Some(offset) => match offset.parse::<f64>() {
            Ok(offset) if offset.is_finite() && offset >= 0.0 => offset,
            _ => return StatusCode::BAD_REQUEST.into_response(),
        },
        None => 0.0,
    };
    let Some(transcode_audio) = plex_remux_audio_mode(protocol, query) else {
        warn!(
            protocol,
            reason = "unsupported_conversion",
            "Plex remux request rejected"
        );
        return plex_remux_unavailable(protocol);
    };
    let Some(media) = plex_playback_media_from_query(state, query) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = media.path;
    prune_plex_remux_sessions(state);
    let existing_session = {
        let Ok(mut sessions) = state.plex_remux_sessions.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        sessions.iter_mut().find_map(|(session_id, session)| {
            if session.source_path == path
                && session.protocol == protocol
                && session.transcode_audio == transcode_audio
                && session.transcode_video == transcode_video
                && session.offset_seconds == offset_seconds
                && session.playback.matroska_hls() == matroska_hls
                && !session.playback.has_failed()
            {
                session.expires_at = Instant::now() + PLEX_REMUX_SESSION_TTL;
                Some((session_id.clone(), Arc::clone(&session.playback)))
            } else {
                None
            }
        })
    };
    if let Some((session_id, playback)) = existing_session {
        let manifest = if protocol == "hls" {
            playback.hls_manifest(&session_id)
        } else {
            playback.dash_manifest(&session_id)
        };
        return plex_protocol_response(
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, content_type)],
                manifest,
            )
                .into_response(),
        );
    }
    let preparation_path = path.clone();
    let playback = match tokio::task::spawn_blocking(move || {
        plex_remux::RemuxedPlayback::prepare(
            &preparation_path,
            transcode_audio,
            transcode_video,
            protocol == "dash",
            matroska_hls,
            offset_seconds,
        )
    })
    .await
    {
        Ok(Ok(playback)) => Arc::new(playback),
        Ok(Err(reason)) => {
            warn!(%reason, protocol, "Plex playback cannot be prepared");
            return plex_remux_unavailable(protocol);
        }
        Err(error) => {
            warn!(%error, protocol, "Plex playback preparation task failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let session_id = plex_async_identifier();
    let manifest = if protocol == "hls" {
        playback.hls_manifest(&session_id)
    } else {
        playback.dash_manifest(&session_id)
    };
    prune_plex_remux_sessions(state);
    let Ok(mut sessions) = state.plex_remux_sessions.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    sessions.insert(
        session_id,
        PlexRemuxSession {
            playback: Arc::clone(&playback),
            source_path: path.clone(),
            protocol: protocol.to_owned(),
            transcode_audio,
            transcode_video,
            offset_seconds,
            expires_at: Instant::now() + PLEX_REMUX_SESSION_TTL,
        },
    );
    drop(sessions);

    let worker_playback = Arc::clone(&playback);
    let limiter = Arc::clone(
        PLEX_REMUX_BUILD_LIMITER.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(1))),
    );
    let worker = tokio::spawn(async move {
        let Ok(permit) = Arc::clone(&limiter).acquire_owned().await else {
            return;
        };
        let build_limiter = Arc::clone(&limiter);
        let result = tokio::task::spawn_blocking(move || {
            worker_playback.build_with_permit(&path, build_limiter, permit)
        })
        .await;
        match result {
            Ok(Ok(())) => {}
            Ok(Err(reason)) => warn!(%reason, protocol, "Plex background remux failed"),
            Err(error) => warn!(%error, protocol, "Plex background remux task failed"),
        }
    });
    drop(worker);
    plex_protocol_response(
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, content_type)],
            manifest,
        )
            .into_response(),
    )
}

pub(crate) fn plex_remux_audio_mode(
    protocol: &str,
    query: &BTreeMap<String, String>,
) -> Option<bool> {
    match query.get("directStreamAudio").map(String::as_str) {
        Some("0") => Some(true),
        Some("1") => Some(false),
        None if protocol == "hls"
            && query.get("directPlay").map(String::as_str) == Some("0")
            && query.get("directStream").map(String::as_str) == Some("0") =>
        {
            Some(false)
        }
        None if protocol == "dash"
            && query.get("directPlay").map(String::as_str) == Some("0")
            && matches!(
                query.get("directStream").map(String::as_str),
                Some("0" | "1")
            ) =>
        {
            Some(true)
        }
        _ => None,
    }
}

pub(crate) fn plex_profile_requests_hls_mkv(query: &BTreeMap<String, String>) -> bool {
    query
        .get("X-Plex-Client-Profile-Extra")
        .is_some_and(|profile| {
            profile
                .split("add-transcode-target(")
                .skip(1)
                .filter_map(|target| target.split_once(')').map(|(target, _)| target))
                .any(|target| {
                    let mut hls = false;
                    let mut matroska = false;
                    for parameter in target.split('&') {
                        let Some((key, value)) = parameter.split_once('=') else {
                            continue;
                        };
                        hls |= key == "protocol" && value == "hls";
                        matroska |= key == "container" && value == "mkv";
                    }
                    hls && matroska
                })
        })
}

pub(crate) fn plex_remux_unavailable(protocol: &str) -> Response {
    let message = if protocol == "dash" {
        "DASH transcoding is not supported."
    } else {
        "HLS playback is not supported."
    };
    (
        StatusCode::NOT_IMPLEMENTED,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        message,
    )
        .into_response()
}

pub(crate) async fn plex_remux_resource(
    state: &AppState,
    resource: &str,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let Some((session_id, name)) = resource.split_once('/') else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let playback = {
        let Ok(mut sessions) = state.plex_remux_sessions.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let now = Instant::now();
        sessions.retain(|_, session| session.expires_at > now);
        let Some(session) = sessions.get_mut(session_id) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        session.expires_at = now + PLEX_REMUX_SESSION_TTL;
        Arc::clone(&session.playback)
    };
    if playback.resource(name).is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = match playback.wait_resource(name).await {
        Ok(path) => path,
        Err(reason) => {
            warn!(%reason, session_id, resource = name, "Plex playback resource not ready");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let Ok(metadata) = tokio::fs::metadata(&path).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let media = Media {
        id: session_id.to_owned(),
        title: "Plex playback segment".to_owned(),
        path,
        size: metadata.len(),
        container: if playback.matroska_hls() {
            "mkv"
        } else {
            "mp4"
        }
        .to_owned(),
        mime: if playback.matroska_hls() {
            "video/x-matroska"
        } else {
            "video/mp4"
        }
        .to_owned(),
        metadata_revision: 0,
    };
    plex_protocol_response(stream_file(&media, method, headers, false).await)
}

pub(crate) fn plex_progress_path(state: &AppState, media_id: &str) -> Option<String> {
    let media = find_media(state, media_id)?;
    let (_, library) = media_library(state, &media)?;
    let root = library
        .folders
        .iter()
        .find(|folder| media.path.starts_with(folder))?;
    Some(
        media
            .path
            .strip_prefix(root)
            .ok()?
            .to_string_lossy()
            .into_owned(),
    )
}

pub(crate) fn plex_playback_session_key(media_id: &str, headers: &HeaderMap) -> String {
    let session = [
        "X-Plex-Session-Identifier",
        "X-Plex-Client-Identifier",
        "X-Plex-Product",
    ]
    .iter()
    .find_map(|name| headers.get(*name).and_then(|value| value.to_str().ok()))
    .unwrap_or("unknown");
    format!("{session}:{media_id}")
}

pub(crate) fn plex_playback_webhook_event(
    state: &AppState,
    media_id: &str,
    playback_state: &str,
    headers: &HeaderMap,
) -> Option<&'static str> {
    let playback_state = playback_state.trim().to_ascii_lowercase();
    if !matches!(playback_state.as_str(), "playing" | "paused" | "stopped") {
        return None;
    }
    let key = plex_playback_session_key(media_id, headers);
    let mut states = state.plex_playback_states.lock().ok()?;
    let previous = states.insert(key.clone(), playback_state.clone());
    if previous.as_deref() == Some("stopped") && playback_state == "playing" {
        if let Ok(mut scrobbled) = state.plex_scrobbled_sessions.lock() {
            scrobbled.remove(&key);
        }
    }
    plex_webhook_event_for_transition(previous.as_deref(), &playback_state)
}

pub(crate) fn plex_scrobble_threshold_reached(position: i64, duration: i64) -> bool {
    position > 0 && duration > 0 && i128::from(position) * 100 >= i128::from(duration) * 90
}

pub(crate) fn plex_scrobble_webhook_once(
    state: &AppState,
    media_id: &str,
    headers: &HeaderMap,
) -> bool {
    state
        .plex_scrobbled_sessions
        .lock()
        .is_ok_and(|mut sessions| sessions.insert(plex_playback_session_key(media_id, headers)))
}

pub(crate) fn plex_webhook_event_for_transition(
    previous: Option<&str>,
    current: &str,
) -> Option<&'static str> {
    match current {
        "playing" if previous == Some("paused") => Some("media.resume"),
        "playing" if previous == Some("playing") => None,
        "playing" => Some("media.play"),
        "paused" if previous == Some("playing") => Some("media.pause"),
        "stopped" if matches!(previous, Some("playing" | "paused")) => Some("media.stop"),
        _ => None,
    }
}

pub(crate) fn plex_webhook_player(headers: &HeaderMap) -> serde_json::Value {
    let mut player = serde_json::json!({});
    for (field, header_name) in [
        ("title", "X-Plex-Device-Name"),
        ("uuid", "X-Plex-Client-Identifier"),
        ("platform", "X-Plex-Platform"),
        ("platformVersion", "X-Plex-Platform-Version"),
        ("device", "X-Plex-Device"),
    ] {
        if let Some(value) = headers
            .get(header_name)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
        {
            player[field] = serde_json::json!(value);
        }
    }
    if player.get("title").is_none() {
        if let Some(product) = headers
            .get("X-Plex-Product")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
        {
            player["title"] = serde_json::json!(product);
        }
    }
    if player.get("uuid").is_none() {
        if let Some(session) = headers
            .get("X-Plex-Session-Identifier")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
        {
            player["uuid"] = serde_json::json!(session);
        }
    }
    player
}

pub(crate) fn plex_webhook_server_token(state: &AppState) -> Option<String> {
    state.db.lock().ok().and_then(|connection| {
        connection
            .query_row(
                "SELECT plex_account_token FROM server_identity WHERE id = 1 AND claimed = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten()
            .filter(|token| !token.is_empty())
    })
}

#[derive(Clone)]
pub(crate) struct PlexWebhookDestination {
    pub(crate) user_id: String,
    pub(crate) owner: bool,
    pub(crate) url: reqwest::Url,
}

pub(crate) struct PlexWebhookConfiguration {
    pub(crate) owner_id: Option<String>,
    pub(crate) destinations: Vec<PlexWebhookDestination>,
}

pub(crate) fn plex_webhook_destinations_from_response(body: &str) -> PlexWebhookConfiguration {
    let lower = body.to_ascii_lowercase();
    let mut destinations = Vec::new();
    let mut owner_id = None;

    let mut seen = HashSet::new();
    let mut user_offset = 0;
    while let Some(relative_start) = lower[user_offset..].find("<user") {
        let user_start = user_offset + relative_start;
        let user_name_end = user_start + "<user".len();
        if !lower
            .as_bytes()
            .get(user_name_end)
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>'))
        {
            user_offset = user_name_end;
            continue;
        }
        let Some(user_tag_end_relative) = body[user_name_end..].find('>') else {
            break;
        };
        let user_tag_end = user_name_end + user_tag_end_relative + 1;
        let user_tag = &body[user_start..user_tag_end];
        let Some(user_id) = xml_attribute(user_tag, "id") else {
            user_offset = user_tag_end;
            continue;
        };
        let owner = xml_attribute(user_tag, "owner").is_some_and(|value| value == "1");
        if owner && owner_id.is_none() {
            owner_id = Some(user_id.clone());
        }

        let Some(user_block_end_relative) = lower[user_tag_end..].find("</user>") else {
            break;
        };
        let user_block_end = user_tag_end + user_block_end_relative;
        let user_block = &body[user_tag_end..user_block_end];
        let user_block_lower = &lower[user_tag_end..user_block_end];
        if let Some(webhooks_start) = user_block_lower.find("<webhooks") {
            if let Some(webhooks_tag_end_relative) = user_block[webhooks_start..].find('>') {
                let webhooks_body_start = webhooks_start + webhooks_tag_end_relative + 1;
                if let Some(webhooks_end_relative) =
                    user_block_lower[webhooks_body_start..].find("</webhooks>")
                {
                    let webhooks_body_end = webhooks_body_start + webhooks_end_relative;
                    let webhooks_body = &user_block[webhooks_body_start..webhooks_body_end];
                    let webhooks_lower = &user_block_lower[webhooks_body_start..webhooks_body_end];
                    let mut webhook_offset = 0;
                    while let Some(relative_webhook) =
                        webhooks_lower[webhook_offset..].find("<webhook")
                    {
                        let webhook_start = webhook_offset + relative_webhook;
                        let webhook_name_end = webhook_start + "<webhook".len();
                        if !webhooks_lower
                            .as_bytes()
                            .get(webhook_name_end)
                            .is_some_and(|byte| {
                                byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>')
                            })
                        {
                            webhook_offset = webhook_name_end;
                            continue;
                        }
                        let Some(webhook_tag_end_relative) =
                            webhooks_body[webhook_name_end..].find('>')
                        else {
                            break;
                        };
                        let webhook_tag_end = webhook_name_end + webhook_tag_end_relative + 1;
                        let tag = &webhooks_body[webhook_start..webhook_tag_end];
                        if let Some(raw_url) = xml_attribute(tag, "url") {
                            let raw_url = plex_xml_unescape(&raw_url);
                            if let Ok(url) = reqwest::Url::parse(&raw_url) {
                                if matches!(url.scheme(), "http" | "https")
                                    && url.host_str().is_some()
                                    && url.username().is_empty()
                                    && url.password().is_none()
                                    && seen.insert((user_id.clone(), url.as_str().to_owned()))
                                {
                                    destinations.push(PlexWebhookDestination {
                                        user_id: user_id.clone(),
                                        owner,
                                        url,
                                    });
                                }
                            }
                        }
                        webhook_offset = webhook_tag_end;
                    }
                }
            }
        }
        user_offset = user_block_end + "</user>".len();
    }
    PlexWebhookConfiguration {
        owner_id,
        destinations,
    }
}

pub(crate) fn plex_xml_unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub(crate) fn plex_webhook_destinations_for_account(
    destinations: Vec<PlexWebhookDestination>,
    account_id: Option<&str>,
) -> Vec<reqwest::Url> {
    let mut seen = HashSet::new();
    destinations
        .into_iter()
        .filter(|destination| destination.owner || Some(destination.user_id.as_str()) == account_id)
        .filter_map(|destination| {
            seen.insert(destination.url.as_str().to_owned())
                .then_some(destination.url)
        })
        .collect()
}

pub(crate) async fn plex_webhook_destinations(
    client: &reqwest::Client,
    state: &AppState,
    server_token: &str,
) -> Result<PlexWebhookConfiguration, ()> {
    let response = client
        .get("https://plex.tv/api/v2/server/webhooks")
        .query(&[("X-Plex-Token", server_token)])
        .header(header::ACCEPT, "application/xml")
        .header("X-Plex-Product", "Plex Media Server")
        .header(
            "X-Plex-Client-Identifier",
            state.machine_identifier.as_ref(),
        )
        .send()
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?;
    let body = response.text().await.map_err(|_| ())?;
    Ok(plex_webhook_destinations_from_response(&body))
}

pub(crate) async fn plex_webhook_account(
    client: &reqwest::Client,
    state: &AppState,
    user_token: &str,
) -> serde_json::Value {
    let Ok(response) = client
        .get("https://clients.plex.tv/api/v2/user")
        .header(header::ACCEPT, "application/json")
        .header("X-Plex-Product", "Plex Media Server")
        .header(
            "X-Plex-Client-Identifier",
            state.machine_identifier.as_ref(),
        )
        .header("X-Plex-Token", user_token)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
    else {
        return serde_json::json!({});
    };
    let Ok(account) = response.json::<serde_json::Value>().await else {
        return serde_json::json!({});
    };
    let mut profile = serde_json::json!({});
    for field in ["id", "title", "username", "thumb"] {
        if let Some(value) = account.get(field) {
            profile[field] = value.clone();
        }
    }
    if profile.get("title").is_none() {
        if let Some(username) = account.get("username").and_then(serde_json::Value::as_str) {
            profile["title"] = serde_json::json!(username);
        }
    }
    profile
}

pub(crate) async fn plex_webhook_access_tokens(
    client: &reqwest::Client,
    state: &AppState,
    server_token: &str,
) -> Option<String> {
    client
        .get("https://plex.tv/api/v2/server/access_tokens")
        .query(&[("X-Plex-Token", server_token)])
        .header(header::ACCEPT, "application/xml")
        .header("X-Plex-Product", "Plex Media Server")
        .header(
            "X-Plex-Client-Identifier",
            state.machine_identifier.as_ref(),
        )
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()
}

pub(crate) fn plex_webhook_account_from_access_tokens(
    body: &str,
    request_token: Option<&str>,
    server_token: &str,
    owner_id: Option<&str>,
) -> serde_json::Value {
    let owner_account = || {
        let mut account = serde_json::json!({});
        if let Some(id) = owner_id {
            account["id"] = id
                .parse::<u64>()
                .map(serde_json::Value::from)
                .unwrap_or_else(|_| serde_json::json!(id));
        }
        account
    };
    let Some(request_token) = request_token.filter(|token| *token != server_token) else {
        return owner_account();
    };
    let lower = body.to_ascii_lowercase();
    let mut offset = 0;
    while let Some(relative_start) = lower[offset..].find("<token") {
        let start = offset + relative_start;
        let name_end = start + "<token".len();
        if !lower
            .as_bytes()
            .get(name_end)
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>'))
        {
            offset = name_end;
            continue;
        }
        let Some(tag_end_relative) = body[name_end..].find('>') else {
            break;
        };
        let tag_end = name_end + tag_end_relative + 1;
        let tag = &body[start..tag_end];
        if xml_attribute(tag, "token").as_deref() != Some(request_token) {
            offset = tag_end;
            continue;
        }
        let token_type = xml_attribute(tag, "type").unwrap_or_default();
        if token_type.eq_ignore_ascii_case("server") {
            let Some(block_end_relative) = lower[tag_end..].find("</token>") else {
                return owner_account();
            };
            let block_end = tag_end + block_end_relative;
            let block = &body[tag_end..block_end];
            let block_lower = &lower[tag_end..block_end];
            if let Some(invited_start) = block_lower.find("<invited") {
                if let Some(invited_tag_end_relative) = block[invited_start..].find('>') {
                    let invited_tag_end = invited_start + invited_tag_end_relative + 1;
                    let invited = &block[invited_start..invited_tag_end];
                    if let Some(id) = xml_attribute(invited, "id") {
                        let mut account = serde_json::json!({
                            "id": id.parse::<u64>().map(serde_json::Value::from)
                                .unwrap_or_else(|_| serde_json::json!(id))
                        });
                        for field in ["title", "username", "thumb"] {
                            if let Some(value) = xml_attribute(invited, field) {
                                account[field] = serde_json::json!(plex_xml_unescape(&value));
                            }
                        }
                        return account;
                    }
                }
            }
            return owner_account();
        }
        return owner_account();
    }
    owner_account()
}

pub(crate) async fn plex_webhook_metadata(
    state: &AppState,
    media_id: &str,
    position: i64,
    duration: Option<i64>,
) -> Option<(Media, serde_json::Value)> {
    let media = find_media(state, media_id)?;
    let (section, library) = media_library(state, &media)?;
    let section_metadata = plex_section_metadata(state, section);
    let mut metadata = plex_video_json(&media, section, &library, section_metadata.get(&media.id));
    if library.kind == "tv" {
        let media_items = state
            .media
            .read()
            .ok()
            .map(|media_items| media_items.clone());
        if let Some(media_items) = media_items {
            if let Some(episode_metadata) = plex_tv_episode_json_for_media(
                state,
                media_items.as_slice(),
                &media,
                section,
                &library,
                &section_metadata,
            )
            .await
            {
                metadata = episode_metadata;
            }
        }
    }
    metadata["librarySectionType"] = serde_json::json!(if library.kind == "tv" {
        "show"
    } else {
        "movie"
    });
    metadata["librarySectionTitle"] = serde_json::json!(library.name);
    metadata["viewOffset"] = serde_json::json!(position);
    if let Some(duration) = duration {
        metadata["duration"] = serde_json::json!(duration);
    }
    if let Some(fields) = metadata.as_object_mut() {
        fields.remove("Media");
    }
    Some((media, metadata))
}

pub(crate) fn plex_timeline(
    state: &AppState,
    query: PlexTimelineQuery,
    headers: HeaderMap,
    request_token: Option<String>,
) -> Response {
    let Some(media_id) = plex_query_media_id(query.rating_key.as_deref(), query.key.as_deref())
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(path) = plex_progress_path(state, &media_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let position = query.time.unwrap_or_default().max(0);
    let duration = query.duration.filter(|duration| *duration > 0);
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if connection.execute(
        "INSERT INTO watch_progress (user_id, media_key, position_ms, duration_ms, watched, hidden_from_continue, updated_at) VALUES (?1, ?2, ?3, ?4, 0, 0, ?5) ON CONFLICT(user_id, media_key) DO UPDATE SET position_ms = excluded.position_ms, duration_ms = COALESCE(excluded.duration_ms, watch_progress.duration_ms), watched = 0, hidden_from_continue = 0, updated_at = excluded.updated_at",
        (USER_ID, path, position, duration, updated_at),
    ).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    drop(connection);
    if let Some(playback_state) = query.playback_state.as_deref() {
        if let Some(event) = plex_playback_webhook_event(state, &media_id, playback_state, &headers)
        {
            spawn_plex_webhook_delivery(
                state,
                event,
                media_id.clone(),
                position,
                duration,
                headers.clone(),
                request_token.clone(),
            );
        }
        if playback_state.eq_ignore_ascii_case("stopped") {
            if let Some(media) = find_media(state, &media_id) {
                cancel_streaming_translation_jobs(&media.id);
            }
        }
    }
    if duration.is_some_and(|duration| plex_scrobble_threshold_reached(position, duration))
        && plex_scrobble_webhook_once(state, &media_id, &headers)
    {
        spawn_plex_webhook_delivery(
            state,
            "media.scrobble",
            media_id,
            position,
            duration,
            headers,
            request_token,
        );
    }
    plex_json(serde_json::json!({"MediaContainer": {"size": 0}}))
}

pub(crate) fn plex_scrobble(
    state: &AppState,
    query: PlexScrobbleQuery,
    watched: bool,
    headers: HeaderMap,
    request_token: Option<String>,
) -> Response {
    let Some(media_id) = plex_query_media_id(query.rating_key.as_deref(), query.key.as_deref())
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(path) = plex_progress_path(state, &media_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let (position, duration, was_watched) = connection
        .query_row(
            "SELECT position_ms, duration_ms, watched FROM watch_progress WHERE user_id = ?1 AND media_key = ?2",
            (USER_ID, &path),
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, bool>(2)?)),
        )
        .unwrap_or((0, None, false));
    let result = if watched {
        connection.execute(
            "INSERT INTO watch_progress (user_id, media_key, position_ms, duration_ms, watched, hidden_from_continue, updated_at) VALUES (?1, ?2, 0, NULL, 1, 0, ?3) ON CONFLICT(user_id, media_key) DO UPDATE SET position_ms = 0, watched = 1, hidden_from_continue = 0, updated_at = excluded.updated_at",
            (USER_ID, path, updated_at),
        )
    } else {
        connection.execute(
            "INSERT INTO watch_progress (user_id, media_key, position_ms, duration_ms, watched, hidden_from_continue, updated_at) VALUES (?1, ?2, 0, NULL, 0, 0, ?3) ON CONFLICT(user_id, media_key) DO UPDATE SET position_ms = 0, watched = 0, hidden_from_continue = 0, updated_at = excluded.updated_at",
            (USER_ID, path, updated_at),
        )
    };
    if result.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    drop(connection);
    if watched {
        if !was_watched && plex_scrobble_webhook_once(state, &media_id, &headers) {
            spawn_plex_webhook_delivery(
                state,
                "media.scrobble",
                media_id,
                position,
                duration,
                headers,
                request_token,
            );
        }
    } else if let Ok(mut sessions) = state.plex_scrobbled_sessions.lock() {
        sessions.remove(&plex_playback_session_key(&media_id, &headers));
    }
    StatusCode::OK.into_response()
}

pub(crate) async fn plex_remove_from_continue_watching(
    State(state): State<AppState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Some(media_id) = plex_query_media_id(
        query.get("ratingKey").map(String::as_str),
        query.get("key").map(String::as_str),
    ) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(path) = plex_progress_path(&state, &media_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match connection.execute(
        "INSERT INTO watch_progress (user_id, media_key, position_ms, duration_ms, watched, hidden_from_continue, updated_at) VALUES (?1, ?2, 0, NULL, 0, 1, ?3) ON CONFLICT(user_id, media_key) DO UPDATE SET hidden_from_continue = 1, updated_at = excluded.updated_at",
        (USER_ID, path, updated_at),
    ) {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) fn plex_photo_source(source: &str) -> Option<(String, &'static str)> {
    let uri = Uri::try_from(source).ok()?;
    let segments = uri
        .path()
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let metadata_index = segments
        .windows(2)
        .position(|pair| pair == ["library", "metadata"])?;
    let id = *segments.get(metadata_index + 2)?;
    let filename = match *segments.get(metadata_index + 3)? {
        "thumb" => "cover.jpg",
        "art" => "banner.jpg",
        _ => return None,
    };
    Some((id.to_owned(), filename))
}

pub(crate) fn plex_user_avatar_source(source: &str) -> Option<String> {
    let uri = Uri::try_from(source).ok()?;
    let segments = uri
        .path()
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let ["users", user_id, "avatar"] = segments.as_slice() else {
        return None;
    };
    if user_id.is_empty()
        || user_id.len() > 64
        || !user_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return None;
    }
    Some((*user_id).to_owned())
}

pub(crate) async fn plex_user_avatar(state: &AppState, user_id: &str) -> Response {
    let token = state.db.lock().ok().and_then(|connection| {
        connection
            .query_row(
                "SELECT plex_account_token FROM server_identity WHERE id = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten()
    });
    let Some(token) = token else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let response = match reqwest::Client::new()
        .get(format!("https://plex.tv/users/{user_id}/avatar"))
        .header("Accept", "image/*")
        .header("X-Plex-Product", "Plex Media Server")
        .header(
            "X-Plex-Client-Identifier",
            state.machine_identifier.as_ref(),
        )
        .header("X-Plex-Token", token)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            warn!(%error, user_id, "Plex account avatar request failed");
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };
    if !response.status().is_success() {
        warn!(status = %response.status(), user_id, "Plex account avatar request failed");
        return StatusCode::BAD_GATEWAY.into_response();
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.starts_with("image/"))
        .unwrap_or("image/jpeg")
        .to_owned();
    let Ok(bytes) = response.bytes().await else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

pub(crate) async fn plex_cors(request: Request<Body>, next: Next) -> Response {
    if !plex_cors_path(request.uri().path()) {
        return next.run(request).await;
    }

    let origin = request.headers().get(header::ORIGIN).cloned();
    let preflight = request.method() == Method::OPTIONS
        && request
            .headers()
            .contains_key("access-control-request-method");
    if preflight {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::OK;
        let headers = response.headers_mut();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
        headers.insert("x-plex-protocol", HeaderValue::from_static("1.0"));
        headers.insert("cache-control", HeaderValue::from_static("no-cache"));
        if let Some(origin) = origin {
            headers.insert("access-control-allow-origin", origin);
            headers.insert(
                "access-control-allow-methods",
                HeaderValue::from_static("GET, POST, PUT, DELETE, HEAD, OPTIONS"),
            );
            if let Some(asked_headers) = request.headers().get("access-control-request-headers") {
                headers.insert("access-control-allow-headers", asked_headers.clone());
            } else {
                headers.insert(
                    "access-control-allow-headers",
                    HeaderValue::from_static(
                        "Accept, Accept-Language, Content-Type, Range, X-Plex-Token, X-Plex-Client-Identifier, X-Plex-Product, X-Plex-Version, X-Plex-Platform, X-Plex-Platform-Version, X-Plex-Device, X-Plex-Device-Name, X-Plex-Device-Vendor, X-Plex-Model, X-Plex-Language, X-Plex-Features, X-Plex-Provides, X-Plex-Container-Start, X-Plex-Container-Size, X-Plex-Client-Profile-Extra, X-Plex-Client-Profile-Name, X-Plex-Session-Identifier, X-Plex-Marketplace, X-Plex-Sync-Version",
                    ),
                );
            }
            headers.insert(
                "access-control-max-age",
                HeaderValue::from_static("1209600"),
            );
            headers.insert("vary", HeaderValue::from_static("Origin, X-Plex-Token"));
        }
        return response;
    }

    let mut response = next.run(request).await;
    if let Some(origin) = origin {
        let headers = response.headers_mut();
        headers.insert("access-control-allow-origin", origin);
        headers.insert(
            "access-control-expose-headers",
            HeaderValue::from_static(
                "Content-Length, Content-Range, Content-Type, Date, Location, X-Plex-Protocol, X-Plex-Container-Start, X-Plex-Container-Total-Size, X-Plex-Content-Original-Length, X-Plex-Content-Compressed-Length, X-Plex-Activity",
            ),
        );
        headers.insert("vary", HeaderValue::from_static("Origin, X-Plex-Token"));
    }
    response
}

pub(crate) fn plex_cors_path(path: &str) -> bool {
    // Keep the built-in UI same-origin while allowing Plex Web's file://
    // origin to call every Plex API operation, including queues and transcode.
    path != "/library"
        && path != "/web"
        && !path.starts_with("/web/")
        && !path.starts_with("/settings")
}

pub(crate) async fn plex_root(state: &AppState) -> Response {
    const ROOT_DIRECTORIES: &[&str] = &[
        "actions",
        "activities",
        "butler",
        "channels",
        "clients",
        "devices",
        "diagnostics",
        "hubs",
        "library",
        "livetv",
        "media",
        "neighborhood",
        "playQueues",
        "playlists",
        "resources",
        "search",
        "server",
        "servers",
        "statistics",
        "system",
        "transcode",
        "updater",
        "user",
    ];
    let root_directories_json = ROOT_DIRECTORIES
        .iter()
        .map(|name| serde_json::json!({"count": 1, "key": name, "title": name}))
        .collect::<Vec<_>>();
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": ROOT_DIRECTORIES.len(),
            "allowCameraUpload": false,
            "allowChannelAccess": false,
            "allowMediaDeletion": false,
            "allowSharing": false,
            "allowSync": false,
            "allowTuners": false,
            "apiVersion": "1.2.3",
            "backgroundProcessing": false,
            "certificate": state.plex_tls.hash().is_some(),
            "companionProxy": false,
            "countryCode": "usa",
            "diagnostics": "",
            "eventStream": false,
            "friendlyName": server_name(state),
            "hubSearch": true,
            "itemClusters": false,
            "livetv": 0,
            "machineIdentifier": state.machine_identifier.as_ref(),
            "mediaProviders": true,
            "multiuser": false,
            "musicAnalysis": 0,
            "myPlex": true,
            "myPlexMappingState": "mapped",
            "myPlexSigninState": "ok",
            "myPlexSubscription": false,
            "myPlexUsername": "",
            "offlineTranscode": 0,
            "ownerFeatures": "",
            "platform": "MacOSX",
            "platformVersion": std::env::consts::ARCH,
            "pluginHost": false,
            "pushNotifications": false,
            "readOnlyLibraries": false,
            "streamingBrainABRVersion": 3,
            "streamingBrainVersion": 2,
            "sync": false,
            "transcoderActiveVideoSessions": 0,
            "transcoderAudio": false,
            "transcoderLyrics": false,
            "transcoderPhoto": false,
            "transcoderSubtitles": false,
            "transcoderVideo": true,
            "updatedAt": updated_at,
            "updater": false,
            "version": "1.43.4.10903-e5521bd8c",
            "voiceSearch": false,
            "Directory": root_directories_json
        }
    }))
}

pub(crate) fn plex_json(body: serde_json::Value) -> Response {
    plex_protocol_response(Json(body).into_response())
}

pub(crate) fn plex_protocol_response(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert("X-Plex-Protocol", HeaderValue::from_static("1.0"));
    response
}

pub(crate) fn plex_request_token<'a>(headers: &'a HeaderMap, uri: &'a Uri) -> Option<&'a str> {
    headers
        .get("X-Plex-Token")
        .and_then(|value| value.to_str().ok())
        .or_else(|| plex_query_value(uri, "X-Plex-Token"))
}

pub(crate) fn plex_query_value<'a>(uri: &'a Uri, name: &str) -> Option<&'a str> {
    uri.query()?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        key.eq_ignore_ascii_case(name).then_some(value)
    })
}

pub(crate) async fn plex_identity(State(state): State<AppState>) -> Response {
    let claimed = state
        .db
        .lock()
        .ok()
        .and_then(|connection| {
            connection
                .query_row(
                    "SELECT claimed FROM server_identity WHERE id = 1",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .ok()
        })
        .unwrap_or(false);
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": 0,
            "apiVersion": "1.2.3",
            "claimed": claimed,
            "friendlyName": server_name(&state),
            "machineIdentifier": state.machine_identifier.as_ref(),
            "version": "1.43.4.10903-e5521bd8c"
        }
    }))
}

pub(crate) async fn plex_sections(State(state): State<AppState>) -> Response {
    let sections = plex_section_directories(&state);
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": sections.0,
            "allowSync": false,
            "title1": "Plex Library",
            "Directory": sections.1
        }
    }))
}

pub(crate) async fn plex_delete_section(
    State(state): State<AppState>,
    Path(section): Path<usize>,
) -> Response {
    let Some(library_index) = section.checked_sub(1) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(library) = config.libraries.get(library_index) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let library_name = library.name.clone();
    let mut updated_config = config.clone();
    updated_config.libraries.remove(library_index);

    let Ok(mut connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(transaction) = connection.transaction() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if transaction
        .execute(
            "DELETE FROM library_folders WHERE library_id = (SELECT id FROM library_groups WHERE name = ?1)",
            [&library_name],
        )
        .and_then(|_| {
            transaction.execute("DELETE FROM library_groups WHERE name = ?1", [&library_name])
        })
        .is_err()
        || update_library_catalog_timestamp(&transaction).is_err()
        || save_config(&state.config_path, &updated_config).is_err()
        || transaction.commit().is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    *config = updated_config;
    StatusCode::OK.into_response()
}

pub(crate) async fn plex_create_section_official(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let query = {
        let mut query = form_urlencoded::Serializer::new(String::new());
        for (key, value) in form_urlencoded::parse(raw_query.unwrap_or_default().as_bytes()) {
            match key.as_ref() {
                "type" => {
                    let value = match value.as_ref() {
                        "1" => "movie",
                        "2" => "show",
                        other => other,
                    };
                    query.append_pair("type", value);
                }
                "locations" => {
                    query.append_pair("location", &value);
                }
                _ => {
                    query.append_pair(&key, &value);
                }
            };
        }
        query.finish()
    };
    plex_create_section(State(state), RawQuery(Some(query)), headers).await
}

pub(crate) async fn plex_create_section(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let mut name = None;
    let mut library_type = None;
    let mut agent = None;
    let mut scanner = None;
    let mut language = None;
    let mut provider_group_id = None;
    let mut locations = Vec::new();
    let mut preferences = BTreeMap::new();
    for (key, value) in form_urlencoded::parse(raw_query.unwrap_or_default().as_bytes()) {
        match key.as_ref() {
            "name" => name = Some(value.into_owned()),
            "type" => library_type = Some(value.into_owned()),
            "agent" => agent = Some(value.into_owned()),
            "scanner" => scanner = Some(value.into_owned()),
            "language" => language = Some(value.into_owned()),
            "metadataAgentProviderGroupId" => {
                let Ok(id) = value.parse::<i64>() else {
                    return StatusCode::BAD_REQUEST.into_response();
                };
                provider_group_id = Some(id);
            }
            "location" | "location[]" | "locations" => locations.push(value.into_owned()),
            "prefs" => {
                let Ok(object) = serde_json::from_str::<serde_json::Value>(&value) else {
                    return StatusCode::BAD_REQUEST.into_response();
                };
                let Some(object) = object.as_object() else {
                    return StatusCode::BAD_REQUEST.into_response();
                };
                for (key, value) in object {
                    preferences.insert(
                        key.clone(),
                        value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string()),
                    );
                }
            }
            key => {
                if let Some(key) = key
                    .strip_prefix("prefs[")
                    .and_then(|key| key.strip_suffix(']'))
                    .filter(|key| !key.is_empty())
                {
                    preferences.insert(key.to_owned(), value.into_owned());
                }
            }
        }
    }

    let Some(name) = name
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let kind = match library_type.as_deref() {
        Some("movie") => "movie",
        Some("show" | "tv") => "tv",
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    let Some(agent) = agent.filter(|agent| !agent.is_empty()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let scanner = scanner
        .filter(|scanner| !scanner.is_empty())
        .unwrap_or_else(|| {
            if kind == "tv" {
                "Plex TV Series".to_owned()
            } else if agent == "tv.plex.agents.none" {
                "Plex Video Files".to_owned()
            } else {
                "Plex Movie".to_owned()
            }
        });
    let agents = if kind == "tv" {
        [
            "tv.plex.agents.series",
            "tv.plex.agents.none",
            "tv.plex.agents.nfo.tv",
        ]
    } else {
        [
            "tv.plex.agents.movie",
            "tv.plex.agents.none",
            "tv.plex.agents.nfo.movie",
        ]
    };
    let scanners = if kind == "tv" {
        ["Plex TV Series", "Plex Series Scanner"]
    } else {
        ["Plex Movie", "Plex Video Files"]
    };
    if !agents.contains(&agent.as_str()) || !scanners.contains(&scanner.as_str()) {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let mut canonical_locations = Vec::with_capacity(locations.len());
    for location in locations {
        let location = location.trim();
        let Ok(metadata) = tokio::fs::metadata(location).await else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if !metadata.is_dir() {
            return StatusCode::BAD_REQUEST.into_response();
        }
        let Ok(location) = tokio::fs::canonicalize(location).await else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if !canonical_locations.contains(&location) {
            canonical_locations.push(location);
        }
    }
    if canonical_locations.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let plex_language = Some(
        language
            .filter(|language| !language.trim().is_empty())
            .unwrap_or_else(|| "en-US".to_owned()),
    );
    let metadata_agent_provider_group_id = provider_group_id.or_else(|| {
        Some(if agent == "tv.plex.agents.none" {
            4
        } else if kind == "tv" {
            2
        } else {
            1
        })
    });
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if let Some(existing_index) = config
        .libraries
        .iter()
        .position(|library| library.name == name)
    {
        let existing = &config.libraries[existing_index];
        let default_agent = if existing.kind == "tv" {
            "tv.plex.agents.series"
        } else {
            "tv.plex.agents.movie"
        };
        let default_scanner = if existing.kind == "tv" {
            "Plex TV Series"
        } else {
            "Plex Movie"
        };
        let default_group_id = if existing.kind == "tv" { 2 } else { 1 };
        let same_request = existing.kind == kind
            && existing.folders == canonical_locations
            && existing.plex_agent.as_deref().unwrap_or(default_agent) == agent
            && existing.plex_scanner.as_deref().unwrap_or(default_scanner) == scanner
            && existing.plex_language.as_deref().unwrap_or("en-US")
                == plex_language.as_deref().unwrap_or("en-US")
            && existing
                .metadata_agent_provider_group_id
                .unwrap_or(default_group_id)
                == metadata_agent_provider_group_id.unwrap_or(default_group_id)
            && existing
                .plex_preferences
                .as_ref()
                .map_or(preferences.is_empty(), |stored| stored == &preferences);
        if !same_request {
            return StatusCode::BAD_REQUEST.into_response();
        }
        drop(config);
        let directories = plex_section_directories(&state).1;
        let Some(directory) = directories.get(existing_index) else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        return plex_create_section_response(directory, &headers);
    }
    let mut updated_config = config.clone();
    updated_config.libraries.push(ConfigLibrary {
        name,
        kind: kind.to_owned(),
        folders: canonical_locations,
        show_recently_added: true,
        show_continue_watching: true,
        scan_enabled: true,
        scan_interval_value: default_scan_interval_value(),
        scan_interval_unit: default_scan_interval_unit(),
        plex_agent: Some(agent),
        plex_scanner: Some(scanner),
        plex_language,
        metadata_agent_provider_group_id,
        plex_preferences: (!preferences.is_empty()).then_some(preferences),
    });

    let Ok(mut connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(transaction) = connection.transaction() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if sync_config_libraries(&transaction, &updated_config).is_err()
        || save_config(&state.config_path, &updated_config).is_err()
        || transaction.commit().is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    *config = updated_config;
    drop(connection);
    drop(config);

    let directories = plex_section_directories(&state).1;
    let Some(directory) = directories.last() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    plex_create_section_response(directory, &headers)
}

pub(crate) fn plex_create_section_response(
    directory: &serde_json::Value,
    headers: &HeaderMap,
) -> Response {
    let accepts_xml = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.to_ascii_lowercase().contains("application/xml"));
    if !accepts_xml {
        return plex_json(serde_json::json!({
            "MediaContainer": {
                "size": 1,
                "Directory": [directory]
            }
        }));
    }

    let body = plex_create_section_xml(directory);
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/xml"),
    );
    plex_protocol_response(response)
}

pub(crate) fn plex_create_section_xml(directory: &serde_json::Value) -> String {
    let attributes = [
        "allowSync",
        "filters",
        "refreshing",
        "key",
        "type",
        "title",
        "agent",
        "scanner",
        "language",
        "metadataAgentProviderGroupId",
        "uuid",
        "updatedAt",
        "createdAt",
        "scannedAt",
        "content",
        "directory",
        "contentChangedAt",
        "hidden",
    ]
    .iter()
    .filter_map(|name| {
        directory.get(*name).map(|value| {
            let value = match value {
                serde_json::Value::String(value) => value.clone(),
                serde_json::Value::Bool(value) => if *value { "1" } else { "0" }.to_owned(),
                _ => value.to_string(),
            };
            format!(" {}=\"{}\"", name, xml_escape(&value))
        })
    })
    .collect::<String>();
    let locations = directory
        .get("Location")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|location| {
            let attributes = ["id", "path"]
                .iter()
                .filter_map(|name| {
                    location.get(*name).map(|value| {
                        let value = match value {
                            serde_json::Value::String(value) => value.clone(),
                            _ => value.to_string(),
                        };
                        format!(" {}=\"{}\"", name, xml_escape(&value))
                    })
                })
                .collect::<String>();
            format!("<Location{attributes}/>")
        })
        .collect::<String>();

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><MediaContainer size=\"1\"><Directory{attributes}>{locations}</Directory></MediaContainer>"
    )
}

pub(crate) async fn plex_system_scanners(
    State(state): State<AppState>,
    Path(section): Path<usize>,
) -> Response {
    let Some(library) = section
        .checked_sub(1)
        .and_then(|index| state.config.lock().ok()?.libraries.get(index).cloned())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let scanners = if library.kind == "tv" {
        ["Plex TV Series", "Plex Series Scanner"]
    } else {
        ["Plex Movie", "Plex Video Files"]
    };
    let scanner_rows = scanners
        .iter()
        .map(|name| format!("<Scanner name=\"{name}\" />"))
        .collect::<String>();
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><MediaContainer>{scanner_rows}</MediaContainer>"
    );
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/xml"),
    );
    plex_protocol_response(response)
}

pub(crate) async fn plex_system_agents(RawQuery(raw_query): RawQuery) -> Response {
    let media_type = raw_query.as_deref().and_then(|query| {
        form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == "mediaType")
            .map(|(_, value)| value.into_owned())
    });
    if !matches!(media_type.as_deref(), Some("1" | "2")) {
        return StatusCode::BAD_REQUEST.into_response();
    }

    const LANGUAGES: &[(&str, &str)] = &[
        ("ar-SA", "Arabic (Saudi Arabia)"),
        ("bg-BG", "Bulgarian"),
        ("ca-ES", "Catalan"),
        ("zh-CN", "Chinese"),
        ("zh-HK", "Chinese (Hong Kong)"),
        ("zh-TW", "Chinese (Taiwan)"),
        ("hr-HR", "Croatian"),
        ("cs-CZ", "Czech"),
        ("da-DK", "Danish"),
        ("nl-NL", "Dutch"),
        ("en-US", "English"),
        ("en-AU", "English (Australia)"),
        ("en-CA", "English (Canada)"),
        ("en-GB", "English (UK)"),
        ("et-EE", "Estonian"),
        ("fi-FI", "Finnish"),
        ("fr-FR", "French"),
        ("fr-CA", "French (Canada)"),
        ("de-DE", "German"),
        ("el-GR", "Greek"),
        ("he-IL", "Hebrew"),
        ("hi-IN", "Hindi"),
        ("hu-HU", "Hungarian"),
        ("is-IS", "Icelandic"),
        ("id-ID", "Indonesian"),
        ("it-IT", "Italian"),
        ("ja-JP", "Japanese"),
        ("ko-KR", "Korean"),
        ("lv-LV", "Latvian"),
        ("lt-LT", "Lithuanian"),
        ("nb-NO", "Norwegian Bokm&#229;l"),
        ("fa-IR", "Persian"),
        ("pl-PL", "Polish"),
        ("pt-BR", "Portuguese"),
        ("pt-PT", "Portuguese (Portugal)"),
        ("ro-RO", "Romanian"),
        ("ru-RU", "Russian"),
        ("sk-SK", "Slovak"),
        ("es-ES", "Spanish"),
        ("es-MX", "Spanish (Mexico)"),
        ("sv-SE", "Swedish"),
        ("th-TH", "Thai"),
        ("tr-TR", "Turkish"),
        ("uk-UA", "Ukrainian"),
        ("vi-VN", "Vietnamese"),
    ];
    const AGENTS: &[(&str, &str, &str)] = &[
        ("tv.plex.agents.movie", "Plex Movie", "1"),
        ("tv.plex.agents.none", "Plex Personal Media", "4"),
        ("tv.plex.agents.nfo.movie", "Plex NFO Movie", "5"),
    ];

    let mut body =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><MediaContainer size=\"3\">");
    for (identifier, name, id) in AGENTS {
        body.push_str(&format!(
            "<Agent identifier=\"{identifier}\" name=\"{name}\" hasPrefs=\"0\" id=\"{id}\">"
        ));
        for (code, title) in LANGUAGES {
            if *code == "en-US" {
                body.push_str(&format!(
                    "<Language code=\"{code}\" title=\"{title}\" default=\"1\" />"
                ));
            } else {
                body.push_str(&format!("<Language code=\"{code}\" title=\"{title}\" />"));
            }
        }
        body.push_str("</Agent>");
    }
    body.push_str("</MediaContainer>");

    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/xml"),
    );
    plex_protocol_response(response)
}

pub(crate) async fn plex_myplex_account(State(state): State<AppState>) -> Response {
    let profile = plex_cloud_profile(&state).await;
    let mut account = serde_json::json!({
        "mappingState": "mapped",
        "signInState": "ok",
        "subscriptionState": "Unknown"
    });
    if let Some((name, thumb)) = profile {
        account["username"] = serde_json::json!(name);
        if let Some(thumb) = thumb {
            account["thumb"] = serde_json::json!(thumb);
        }
    }
    plex_json(serde_json::json!({"MyPlexAccount": account}))
}

pub(crate) async fn plex_myplex_claim(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let Some(account_token) = plex_request_token(&headers, &uri).filter(|token| !token.is_empty())
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    let (server_token, claimed, error) =
        match claim_and_publish_plex_server(&state, account_token).await {
            Ok(result) => result,
            Err(error) => return (StatusCode::BAD_GATEWAY, error).into_response(),
        };
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if connection
        .execute(
            "UPDATE server_identity SET plex_account_token = ?1, claimed = ?2, plex_last_error = ?3 WHERE id = 1",
            (&server_token, claimed, error.as_deref()),
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    drop(connection);
    if !claimed {
        return (
            StatusCode::BAD_GATEWAY,
            error.unwrap_or_else(|| "Plex server claim failed".to_owned()),
        )
            .into_response();
    }
    let response = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><MyPlex authToken=\"{}\" />",
        xml_escape(&server_token)
    );
    Response::builder()
        .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
        .body(Body::from(response))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

pub(crate) async fn plex_refresh_reachability(State(state): State<AppState>) -> Response {
    let server_token = state.db.lock().ok().and_then(|connection| {
        connection
            .query_row(
                "SELECT plex_account_token FROM server_identity WHERE id = 1 AND claimed = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok()
    });
    let Some(server_token) = server_token else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if let Err(error) =
        plex_connectivity_check(&reqwest::Client::new(), &state, &server_token).await
    {
        warn!(error = %error, "Plex reachability refresh failed");
        return StatusCode::BAD_GATEWAY.into_response();
    }
    Response::builder()
        .header(header::CONTENT_TYPE, "application/xml; charset=utf-8")
        .body(Body::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><MediaContainer size=\"0\" />",
        ))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

pub(crate) async fn plex_accounts(State(state): State<AppState>, headers: HeaderMap) -> Response {
    plex_account_value(
        plex_cloud_profile(&state).await,
        plex_account_preferences(&state),
        &headers,
    )
}

pub(crate) async fn plex_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if id != "1" {
        return StatusCode::NOT_FOUND.into_response();
    }
    plex_account_value(
        plex_cloud_profile(&state).await,
        plex_account_preferences(&state),
        &headers,
    )
}

pub(crate) async fn update_plex_account(
    State(state): State<AppState>,
    Path(id): Path<String>,
    RawQuery(raw_query): RawQuery,
) -> Response {
    if id != "1" {
        return StatusCode::NOT_FOUND.into_response();
    }
    let updates = form_urlencoded::parse(raw_query.unwrap_or_default().as_bytes())
        .filter(|(key, _)| {
            matches!(
                key.as_ref(),
                "defaultAudioLanguage"
                    | "autoSelectAudio"
                    | "defaultSubtitleLanguage"
                    | "subtitleMode"
            )
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    if updates.is_empty()
        || updates.iter().any(|(key, value)| match key.as_str() {
            "autoSelectAudio" => !matches!(value.as_str(), "0" | "1" | "true" | "false"),
            "subtitleMode" => {
                value.parse::<u8>().is_err() || value.parse::<u8>().is_ok_and(|mode| mode > 4)
            }
            _ => value.trim().is_empty(),
        })
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    for (key, value) in updates {
        config.plex_account_preferences.insert(key, value);
    }
    if save_config(&state.config_path, &config).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    StatusCode::OK.into_response()
}

pub(crate) async fn plex_cloud_profile(state: &AppState) -> Option<(String, Option<String>)> {
    let token = state.db.lock().ok().and_then(|connection| {
        connection
            .query_row(
                "SELECT plex_account_token FROM server_identity WHERE id = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .ok()
            .flatten()
    })?;
    let response = reqwest::Client::new()
        .get("https://plex.tv/api/v2/user")
        .header("Accept", "application/json")
        .header("X-Plex-Product", "Plex Media Server")
        .header(
            "X-Plex-Client-Identifier",
            state.machine_identifier.as_ref(),
        )
        .header("X-Plex-Token", token)
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json::<serde_json::Value>()
        .await
        .ok()?;
    let name = ["username", "title", "friendlyName"]
        .iter()
        .find_map(|key| {
            response
                .get(*key)
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
        })
        .map(str::to_owned);
    let thumb = response
        .get("thumb")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    name.or_else(|| thumb.as_ref().map(|_| "harbinger".to_owned()))
        .map(|name| (name, thumb))
}

pub(crate) fn plex_account_preferences(state: &AppState) -> BTreeMap<String, String> {
    state
        .config
        .lock()
        .map(|config| config.plex_account_preferences.clone())
        .unwrap_or_default()
}

pub(crate) fn plex_account_value(
    profile: Option<(String, Option<String>)>,
    preferences: BTreeMap<String, String>,
    headers: &HeaderMap,
) -> Response {
    let mut account = serde_json::json!({
        "id": 1,
        "key": "/accounts/1",
        "name": "harbinger",
        "defaultAudioLanguage": preferences
            .get("defaultAudioLanguage")
            .map(String::as_str)
            .unwrap_or(PLEX_DEFAULT_AUDIO_LANGUAGE),
        "autoSelectAudio": preferences
            .get("autoSelectAudio")
            .is_none_or(|value| matches!(value.as_str(), "1" | "true")),
        "defaultSubtitleLanguage": preferences
            .get("defaultSubtitleLanguage")
            .map(String::as_str)
            .unwrap_or("en"),
        "subtitleMode": preferences
            .get("subtitleMode")
            .and_then(|value| value.parse::<u8>().ok())
            .unwrap_or(2)
    });
    if let Some((name, thumb)) = profile {
        account["name"] = serde_json::json!(name);
        if let Some(thumb) = thumb {
            account["thumb"] = serde_json::json!(thumb);
        }
    }
    let container = serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "identifier": "com.plexapp.system.accounts",
            "Account": [account.clone()]
        }
    });
    if !headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("application/json"))
    {
        let fields = account
            .as_object()
            .into_iter()
            .flat_map(|account| account.iter())
            .map(|(key, value)| {
                let value = if key == "autoSelectAudio" {
                    if value.as_bool().unwrap_or(false) {
                        "1".to_owned()
                    } else {
                        "0".to_owned()
                    }
                } else {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string())
                };
                format!(" {key}=\"{}\"", xml_escape(&value))
            })
            .collect::<String>();
        return plex_protocol_response(
            (
                [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
                format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><MediaContainer size=\"1\" identifier=\"com.plexapp.system.accounts\"><Account{fields}/></MediaContainer>"),
            )
                .into_response(),
        );
    }
    plex_json(container)
}

pub(crate) async fn plex_activities() -> Response {
    plex_json(serde_json::json!({"MediaContainer": {"size": 0}}))
}

pub(crate) async fn plex_sessions() -> Response {
    plex_json(serde_json::json!({"MediaContainer": {"size": 0}}))
}

pub(crate) async fn plex_devices() -> Response {
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": 0,
            "identifier": "com.plexapp.system.devices",
            "Device": []
        }
    }))
}

pub(crate) async fn plex_session_history() -> Response {
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": 0,
            "totalSize": 0,
            "offset": 0,
            "Metadata": []
        }
    }))
}

pub(crate) fn plex_prefs(state: &AppState) -> Response {
    let settings = serde_json::json!([
        {"id":"FriendlyName","label":"Friendly name","summary":"Other computers on your network see this server under this name.","type":"text","default":"harbinger","value":server_name(state),"hidden":false,"advanced":false,"group":"general"},
        {"id":"OnDeckLimit","label":"On Deck limit","summary":"The maximum number of items shown in Continue Watching.","type":"int","default":40,"value":40,"hidden":false,"advanced":false,"group":"library"},
        {"id":"ScannerLowPriority","label":"Run scans at a lower priority","summary":"","type":"bool","default":false,"value":false,"hidden":false,"advanced":false,"group":"library"},
        {"id":"AllowLegacyAgents","label":"Allow legacy agents","summary":"","type":"bool","default":true,"value":true,"hidden":true,"advanced":false,"group":"library"},
        {"id":"CLDriver","label":"","summary":"","type":"text","default":"","value":"","hidden":true,"advanced":false,"group":"transcoder"},
        {"id":"LanNetworksBandwidth","label":"LAN Networks","summary":"","type":"text","default":"","value":"","hidden":false,"advanced":true,"group":"network"},
        {"id":"secureConnections","label":"Secure connections","summary":"","type":"int","default":1,"value":1,"hidden":false,"advanced":true,"group":"network","enumValues":"0:Disabled|1:Preferred|2:Required"},
        {"id":"PublishServerOnPlexOnline","label":"Show your server on plex.tv","summary":"","type":"bool","default":true,"value":true,"hidden":false,"advanced":false,"group":"network"}
    ]);
    plex_json(serde_json::json!({
        "MediaContainer": {"size": settings.as_array().map_or(0, Vec::len), "Setting": settings}
    }))
}

pub(crate) async fn plex_servers(State(state): State<AppState>) -> Response {
    let bind = state
        .config
        .lock()
        .ok()
        .and_then(|config| config.bind.parse::<SocketAddr>().ok())
        .map(|address| address.port())
        .unwrap_or(32400);
    let host = plex_connection_urls(&state)
        .first()
        .and_then(|url| url.strip_prefix("http://"))
        .and_then(|url| url.rsplit_once(':'))
        .map(|(host, _)| host.to_owned())
        .unwrap_or_else(|| "127.0.0.1".to_owned());
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "Server": [{
                "name": server_name(&state),
                "address": host,
                "port": bind,
                "machineIdentifier": state.machine_identifier.as_ref(),
                "version": "1.43.4.10903-e5521bd8c"
            }]
        }
    }))
}

pub(crate) async fn plex_media_providers(State(state): State<AppState>) -> Response {
    let updated_at = library_catalog_updated_at(&state);
    let mut directories = state
        .config
        .lock()
        .ok()
        .map(|config| {
            config
                .libraries
                .iter()
                .enumerate()
                .map(|(index, library)| {
                    let section = index + 1;
                    let (_, scanned_at, library_updated_at) =
                        library_publication_state(&state, &library.name, section);
                    let pivots = serde_json::json!([
                        {"id": "recommended", "key": format!("/hubs/sections/{section}"), "type": "hub", "title": "Recommended", "context": "content.discover", "symbol": "star"},
                        {"id": "library", "key": format!("/library/sections/{section}/all?type={}", if library.kind == "tv" { 2 } else { 1 }), "type": "list", "title": "Library", "context": "content.library", "symbol": "library"},
                        {"id": "collections", "key": format!("/library/sections/{section}/collections"), "type": "list", "title": "Collections", "context": "content.collections", "symbol": "stack"}
                    ]);
                    serde_json::json!({
                        "agent": if library.kind == "tv" { "tv.plex.agents.series" } else { "tv.plex.agents.movie" },
                        "scanner": if library.kind == "tv" { "Plex TV Series" } else { "Plex Movie" },
                        "language": "en-US",
                        "uuid": plex_library_uuid(&state, &library.name, section),
                        "id": section.to_string(),
                        "key": format!("/library/sections/{section}"),
                        "hubKey": format!("/hubs/sections/{section}"),
                        "type": plex_kind(&library.kind),
                        "title": library.name,
                        "refreshing": false,
                        "scannedAt": scanned_at,
                        "updatedAt": library_updated_at,
                        "Pivot": pivots
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    directories.insert(0, serde_json::json!({"title": "Home", "hubKey": "/hubs"}));
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "allowCameraUpload": false,
            "allowChannelAccess": false,
            "allowMediaDeletion": false,
            "allowSharing": false,
            "allowSync": false,
            "allowTuners": false,
            "apiVersion": "1.2.3",
            "backgroundProcessing": false,
            "certificate": state.plex_tls.hash().is_some(),
            "companionProxy": false,
            "countryCode": "usa",
            "diagnostics": "",
            "eventStream": false,
            "friendlyName": server_name(&state),
            "hubSearch": true,
            "itemClusters": false,
            "livetv": 0,
            "machineIdentifier": state.machine_identifier.as_ref(),
            "mediaProviders": true,
            "multiuser": false,
            "musicAnalysis": 0,
            "myPlex": true,
            "myPlexMappingState": "mapped",
            "myPlexSigninState": "ok",
            "myPlexSubscription": false,
            "myPlexUsername": "",
            "offlineTranscode": 0,
            "ownerFeatures": "",
            "platform": "MacOSX",
            "platformVersion": std::env::consts::ARCH,
            "pluginHost": false,
            "pushNotifications": false,
            "readOnlyLibraries": false,
            "streamingBrainABRVersion": 3,
            "streamingBrainVersion": 2,
            "sync": false,
            "transcoderActiveVideoSessions": 0,
            "transcoderAudio": false,
            "transcoderLyrics": false,
            "transcoderSubtitles": false,
            "transcoderVideo": true,
            "updatedAt": updated_at,
            "updater": false,
            "version": "1.43.4.10903-e5521bd8c",
            "voiceSearch": false,
            "MediaProvider": [{
                "identifier": "com.plexapp.plugins.library",
                "title": "Library",
                "types": "video",
                "protocols": "stream,download",
                "Feature": plex_library_provider_features(directories)
            }]
        }
    }))
}

pub(crate) fn plex_library_provider_features(
    directories: Vec<serde_json::Value>,
) -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "key": "/library/sections",
            "type": "content",
            "Directory": directories
        }),
        serde_json::json!({"key": "/hubs/search", "type": "search"}),
        serde_json::json!({"key": "/library/metadata", "type": "metadata"}),
        serde_json::json!({"key": "/hubs/promoted", "type": "promoted"}),
        serde_json::json!({
            "key": "/library/collections",
            "type": "collection"
        }),
        serde_json::json!({
            "key": "/hubs/continueWatching",
            "type": "continuewatching"
        }),
        serde_json::json!({
            "key": "/actions",
            "type": "actions",
            "Action": [{
                "id": "removeFromContinueWatching",
                "key": "/actions/removeFromContinueWatching"
            }]
        }),
        serde_json::json!({
            "key": "/library/search",
            "type": "universalsearch"
        }),
        serde_json::json!({
            "key": "/playlists",
            "type": "playlist",
            "flavor": "universal"
        }),
        serde_json::json!({
            "key": "/playQueues",
            "type": "playqueue",
            "flavor": "universal"
        }),
        serde_json::json!({
            "key": "/:/timeline",
            "type": "timeline",
            "scrobbleKey": "/:/scrobble",
            "unscrobbleKey": "/:/unscrobble"
        }),
        serde_json::json!({"type": "manage"}),
    ]
}

pub(crate) async fn plex_security_resources() -> Response {
    plex_json(serde_json::json!({"MediaContainer": {"size": 0}}))
}

pub(crate) async fn plex_global_hubs(
    State(state): State<AppState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    plex_hubs_response(&state, None, query.get("count"), None).await
}

pub(crate) async fn plex_continue_watching_hub(
    State(state): State<AppState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let only_section = query
        .get("section")
        .and_then(|section| section.parse::<usize>().ok());
    if query.contains_key("section") && only_section.is_none() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let count = query
        .get("count")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10)
        .clamp(1, 50);
    let progress = {
        let Ok(connection) = state.db.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let mut statement = match connection.prepare(
            "SELECT media_key, position_ms, duration_ms FROM watch_progress WHERE user_id = ?1 AND watched = 0 AND hidden_from_continue = 0 AND position_ms > 0 ORDER BY updated_at DESC, media_key",
        ) {
            Ok(statement) => statement,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        let rows = match statement.query_map([USER_ID], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        }) {
            Ok(rows) => rows,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        match rows.collect::<rusqlite::Result<Vec<_>>>() {
            Ok(progress) => progress,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    };
    let libraries = match state.config.lock() {
        Ok(config) => config.libraries.clone(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let media_index = media_snapshot(&state);
    if only_section.is_some_and(|section| section == 0 || section > libraries.len()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let progress_state = {
        let Ok(connection) = state.db.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let mut statement = match connection.prepare(
            "SELECT media_key, watched, hidden_from_continue, updated_at FROM watch_progress WHERE user_id = ?1",
        ) {
            Ok(statement) => statement,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        let rows = match statement.query_map([USER_ID], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, i64>(3)?,
            ))
        }) {
            Ok(rows) => rows,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        match rows.collect::<rusqlite::Result<Vec<_>>>() {
            Ok(progress) => progress,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    };
    let mut watched_paths = std::collections::HashSet::new();
    let mut hidden_paths = std::collections::HashSet::new();
    let mut last_activity = std::collections::HashMap::new();
    for (path, watched, hidden, updated_at) in progress_state {
        if watched {
            watched_paths.insert(path.clone());
        }
        if hidden {
            hidden_paths.insert(path);
            continue;
        }
        last_activity.insert(path, updated_at);
    }

    let mut tv_episode_paths = std::collections::HashSet::new();
    for library in libraries.iter().filter(|library| library.kind == "tv") {
        for root in &library.folders {
            for media in media_index.iter() {
                let Ok(relative) = media.path.strip_prefix(root) else {
                    continue;
                };
                if tv_episode_info(root, &media.path).is_some() {
                    tv_episode_paths.insert(relative.to_string_lossy().into_owned());
                }
            }
        }
    }
    let mut candidates = next_tv_episode_paths(
        &libraries,
        media_index.as_ref(),
        &watched_paths,
        &hidden_paths,
        &last_activity,
    );
    candidates.extend(
        progress
            .iter()
            .filter(|(path, _, _)| !tv_episode_paths.contains(path))
            .map(|(path, _, _)| {
                (
                    path.clone(),
                    last_activity.get(path).copied().unwrap_or_default(),
                )
            }),
    );
    candidates.sort_by(|(left_path, left_time), (right_path, right_time)| {
        right_time
            .cmp(left_time)
            .then_with(|| left_path.cmp(right_path))
    });
    let progress_by_path = progress
        .iter()
        .map(|(path, position, duration)| (path.clone(), (*position, *duration)))
        .collect::<std::collections::HashMap<_, _>>();
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    for (media_key, activity) in candidates {
        if !seen.insert(media_key.clone()) {
            continue;
        }
        let found = libraries.iter().enumerate().find_map(|(index, library)| {
            if only_section.is_some_and(|section| section != index + 1) {
                return None;
            }
            media_index.iter().find_map(|media| {
                if !media_in_library(media, library) {
                    return None;
                }
                let root = library
                    .folders
                    .iter()
                    .find(|folder| media.path.starts_with(folder))?;
                let relative = media.path.strip_prefix(root).ok()?;
                (relative.to_string_lossy() == media_key).then_some((media.id.clone(), index + 1))
            })
        });
        if let Some((media_id, section)) = found {
            let (position, duration) = progress_by_path
                .get(&media_key)
                .copied()
                .unwrap_or((0, None));
            items.push((media_id, position, duration, section, activity));
        }
    }
    let total_size = items.len();
    let mut metadata = Vec::new();
    for (media_id, position, duration, _, _) in items.into_iter().take(count) {
        if let Some(mut item) = plex_metadata_item_json(&state, &media_id).await {
            item["viewOffset"] = serde_json::json!(position);
            if let Some(duration) = duration {
                item["duration"] = serde_json::json!(duration);
            }
            metadata.push(item);
        }
    }
    let hub_key = only_section.map_or_else(
        || "/hubs/continueWatching".to_owned(),
        |section| format!("/hubs/sections/{section}/continueWatching/items"),
    );
    let hub = serde_json::json!({
        "key": hub_key,
        "hubKey": hub_key,
        "title": "Continue Watching",
        "type": "mixed",
        "hubIdentifier": "home.continue",
        "context": "hub.home.continue",
        "size": metadata.len(),
        "totalSize": total_size,

        "more": total_size > count,
        "style": "shelf",
        "promoted": true,
        "Metadata": metadata
    });
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "identifier": "com.plexapp.plugins.library",
            "allowSync": false,
            "Hub": [hub]
        }
    }))
}

pub(crate) fn plex_notification_response(upgrade: WebSocketUpgrade, state: AppState) -> Response {
    upgrade
        .on_upgrade(move |mut socket| async move {
            let mut notifications = state.plex_notifications.subscribe();
            let mut heartbeat = tokio::time::interval_at(
                tokio::time::Instant::now() + Duration::from_secs(10),
                Duration::from_secs(10),
            );
            loop {
                tokio::select! {
                    message = socket.recv() => match message {
                        Some(Ok(Message::Ping(payload))) => {
                            if socket.send(Message::Pong(payload)).await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(Message::Close(frame))) => {
                            info!(?frame, "Plex notification socket closed by client");
                            break;
                        }
                        Some(Ok(Message::Text(_) | Message::Binary(_) | Message::Pong(_))) => {}
                        Some(Err(error)) => {
                            info!(%error, "Plex notification socket read failed");
                            break;
                        }
                        None => {
                            info!("Plex notification socket ended by client");
                            break;
                        }
                    },
                    notification = notifications.recv() => match notification {
                        Ok(notification) => {
                            if socket.send(Message::Text(notification.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    },
                    _ = heartbeat.tick() => {
                        if socket.send(Message::Ping(vec![b'?'].into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        })
        .into_response()
}

pub(crate) async fn plex_section_hubs(
    State(state): State<AppState>,
    Path(section): Path<usize>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    plex_hubs_response(&state, Some(section), query.get("count"), None).await
}

pub(crate) async fn plex_section_hub(
    State(state): State<AppState>,
    Path((section, hub)): Path<(usize, String)>,
    Query(mut query): Query<BTreeMap<String, String>>,
) -> Response {
    if hub == "continueWatching" {
        query.insert("section".to_owned(), section.to_string());
        return plex_continue_watching_hub(State(state), Query(query)).await;
    }
    if !matches!(hub.as_str(), "recommended" | "recentlyAdded") {
        return StatusCode::NOT_FOUND.into_response();
    }
    plex_hubs_response(&state, Some(section), query.get("count"), Some(&hub)).await
}

pub(crate) async fn plex_hub_items(
    State(state): State<AppState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Some(identifier) = query.get("identifier") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let mut parts = identifier.split('.');
    let (Some(kind), Some(category), Some(section), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Ok(section) = section.parse::<usize>() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !matches!(category, "recommended" | "recentlyadded") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let libraries = match state.config.lock() {
        Ok(config) => config.libraries.clone(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Some(library) = section
        .checked_sub(1)
        .and_then(|index| libraries.get(index))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if plex_kind(&library.kind) != kind {
        return StatusCode::NOT_FOUND.into_response();
    }
    let hub_response =
        plex_hubs_response(&state, Some(section), query.get("count"), Some(category)).await;
    if !hub_response.status().is_success() {
        return hub_response;
    }
    let body = match axum::body::to_bytes(hub_response.into_body(), usize::MAX).await {
        Ok(body) => body,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let container = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(body) => body["MediaContainer"].clone(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Some(hub) = container["Hub"].as_array().and_then(|hubs| hubs.first()) else {
        return plex_json(serde_json::json!({
            "MediaContainer": {
                "size": 0,
                "totalSize": 0,
                "offset": 0,
                "allowSync": false,
                "identifier": "com.plexapp.plugins.library",
                "librarySectionID": section,
                "librarySectionTitle": library.name,
                "Metadata": []
            }
        }));
    };
    let metadata = hub["Metadata"].as_array().cloned().unwrap_or_default();
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": metadata.len(),
            "totalSize": hub["size"],
            "offset": 0,
            "allowSync": false,
            "identifier": "com.plexapp.plugins.library",
            "librarySectionID": section,
            "librarySectionTitle": library.name,
            "title1": library.name,
            "title2": hub["title"],
            "Metadata": metadata
        }
    }))
}

pub(crate) async fn plex_section_continue_watching_items(
    State(state): State<AppState>,
    Path(section): Path<usize>,
    Query(mut query): Query<BTreeMap<String, String>>,
) -> Response {
    let libraries = match state.config.lock() {
        Ok(config) => config.libraries.clone(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Some(library) = section
        .checked_sub(1)
        .and_then(|index| libraries.get(index))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let offset = query
        .get("X-Plex-Container-Start")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();
    let requested_count = query
        .get("X-Plex-Container-Size")
        .or_else(|| query.get("count"))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10)
        .clamp(1, 50);
    query.insert("section".to_owned(), section.to_string());
    query.insert(
        "count".to_owned(),
        offset
            .saturating_add(requested_count)
            .clamp(1, 50)
            .to_string(),
    );
    let response = plex_continue_watching_hub(State(state), Query(query)).await;
    if !response.status().is_success() {
        return response;
    }
    let body = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
        Ok(body) => body,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let hub = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(body) => body["MediaContainer"]["Hub"][0].clone(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let metadata = hub["Metadata"]
        .as_array()
        .map(|metadata| {
            metadata
                .iter()
                .skip(offset)
                .take(requested_count)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": metadata.len(),
            "totalSize": hub["totalSize"],
            "offset": offset,
            "allowSync": true,
            "identifier": "com.plexapp.plugins.library",
            "librarySectionID": section,
            "librarySectionTitle": library.name,
            "Metadata": metadata
        }
    }))
}

pub(crate) async fn plex_hubs_response(
    state: &AppState,
    only_section: Option<usize>,
    count: Option<&String>,
    only_hub: Option<&str>,
) -> Response {
    let count = count
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10)
        .clamp(1, 50);
    let libraries = state
        .config
        .lock()
        .map(|config| config.libraries.clone())
        .unwrap_or_default();
    if only_section.is_some_and(|section| section == 0 || section > libraries.len()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let media_index = media_snapshot(state);

    let mut hubs = Vec::new();
    for (index, library) in libraries.iter().enumerate() {
        let section = index + 1;
        if only_section.is_some_and(|requested| requested != section) {
            continue;
        }
        let mut media = media_index
            .iter()
            .filter(|media| media_in_library(media, library))
            .collect::<Vec<_>>();
        if media.is_empty() {
            continue;
        }
        let metadata = plex_section_metadata(state, section);
        let mut recently_added = media.clone();
        recently_added.sort_by_key(|item| std::cmp::Reverse(media_modified_at(item)));

        let rated_count = media
            .iter()
            .filter(|item| metadata.get(&item.id).and_then(plex_rating).is_some())
            .count();
        if rated_count > 0 {
            media.sort_by(|left, right| {
                let left_rating = metadata
                    .get(&left.id)
                    .and_then(plex_rating)
                    .unwrap_or_default();
                let right_rating = metadata
                    .get(&right.id)
                    .and_then(plex_rating)
                    .unwrap_or_default();
                right_rating
                    .partial_cmp(&left_rating)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| media_modified_at(right).cmp(&media_modified_at(left)))
            });
        } else {
            media.clone_from(&recently_added);
        }

        let recommendation = if rated_count > 0 {
            &media
        } else {
            &recently_added
        };
        let categories = plex_hub_categories(rated_count > 0, only_hub);
        for category in categories {
            if category == "recommended" {
                hubs.push(
                    plex_recommendation_hub(
                        state,
                        section,
                        library,
                        recommendation,
                        &metadata,
                        count,
                        rated_count > 0,
                        true,
                    )
                    .await,
                );
            } else {
                hubs.push(
                    plex_recommendation_hub(
                        state,
                        section,
                        library,
                        &recently_added,
                        &metadata,
                        count,
                        false,
                        false,
                    )
                    .await,
                );
            }
        }
    }

    if let Some(section) = only_section.filter(|_| only_hub.is_none()) {
        let mut query = BTreeMap::new();
        query.insert("section".to_owned(), section.to_string());
        let response = plex_continue_watching_hub(State(state.clone()), Query(query)).await;
        if !response.status().is_success() {
            return response;
        }
        let body = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
            Ok(body) => body,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        let continue_hub = match serde_json::from_slice::<serde_json::Value>(&body) {
            Ok(body) => body["MediaContainer"]["Hub"][0].clone(),
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        info!(
            section,
            item_count = continue_hub["Metadata"].as_array().map_or(0, Vec::len),
            "Plex section Continue Watching hub generated"
        );

        hubs.insert(0, continue_hub);
    }
    if only_hub == Some("recommended") {
        let hub_details = hubs
            .iter()
            .map(|hub| {
                (
                    hub["title"].as_str().unwrap_or_default(),
                    hub["size"].as_u64().unwrap_or_default(),
                    hub["Metadata"].as_array().map_or(0, Vec::len),
                )
            })
            .collect::<Vec<_>>();
        tracing::info!(
            section = ?only_section,
            hub_count = hubs.len(),
            hub_details = ?hub_details,
            "Plex recommended hubs generated"
        );
    }

    let mut container = serde_json::json!({"size": hubs.len(), "Hub": hubs});
    container["allowSync"] = serde_json::json!(false);
    container["identifier"] = serde_json::json!("com.plexapp.plugins.library");
    if let Some(section) = only_section {
        let library = &libraries[section - 1];
        container["librarySectionID"] = serde_json::json!(section);
        container["librarySectionTitle"] = serde_json::json!(library.name);
        container["librarySectionUUID"] =
            serde_json::json!(plex_library_uuid(&state, &library.name, section));
    }
    plex_json(serde_json::json!({"MediaContainer": container}))
}

pub(crate) fn plex_hub_categories(
    _has_rated_items: bool,
    requested: Option<&str>,
) -> Vec<&'static str> {
    match requested {
        Some("recommended") => vec!["recommended"],
        Some("recentlyAdded") => vec!["recentlyAdded"],
        None => vec!["recommended", "recentlyAdded"],
        Some(_) => Vec::new(),
    }
}

pub(crate) async fn plex_recommendation_hub(
    state: &AppState,
    section: usize,
    library: &ConfigLibrary,
    media: &[&Media],
    metadata: &HashMap<String, serde_json::Value>,
    count: usize,
    rank_by_rating: bool,
    recommended: bool,
) -> serde_json::Value {
    let kind = plex_kind(&library.kind);
    let (title, identifier, category, sort_key) = match (recommended, rank_by_rating) {
        (true, true) => (
            format!("Recommended in {}", library.name),
            format!("{}.recommended.{section}", kind),
            "recommended",
            "audienceRating:desc",
        ),
        (true, false) => (
            format!("Recommended in {}", library.name),
            format!("{}.recommended.{section}", kind),
            "recommended",
            "addedAt:desc",
        ),
        (false, _) => (
            format!("Recently Added in {}", library.name),
            format!("{}.recentlyadded.{section}", kind),
            "recentlyAdded",
            "addedAt:desc",
        ),
    };
    let media_index = media_snapshot(state);
    let tv_shows = (library.kind == "tv").then(|| {
        plex_tv_show_groups(
            section,
            &library.folders,
            media_index
                .iter()
                .filter(|media| media_in_library(media, library)),
        )
    });
    if let Some(tv_shows) = tv_shows.as_ref() {
        let ranked_shows = plex_ranked_tv_shows(media, tv_shows);
        let items = ranked_shows
            .iter()
            .take(count)
            .map(|show| plex_tv_show_json(show, section, library, metadata))
            .collect::<Vec<_>>();
        return serde_json::json!({
            "hubKey": format!("/library/sections/{section}/all?sort={sort_key}"),
            "key": format!("/hubs/items?identifier={identifier}"),
            "title": title,
            "type": kind,
            "hubIdentifier": identifier,
            "context": format!("hub.{kind}.{category}"),
            "size": ranked_shows.len(),
            "more": ranked_shows.len() > count,
            "style": "shelf",
            "promoted": true,
            "Metadata": items
        });
    }
    let mut items = Vec::new();
    for item in media.iter().take(count) {
        let mut video = plex_video_json(item, section, library, metadata.get(&item.id));
        add_plex_technical_metadata(state, &mut video, item).await;
        items.push(video);
    }
    serde_json::json!({
        "hubKey": format!("/library/sections/{section}/all?sort={sort_key}"),
        "key": format!("/hubs/items?identifier={identifier}"),
        "title": title,
        "type": kind,
        "hubIdentifier": identifier,
        "context": format!("hub.{kind}.{category}"),
        "size": media.len(),
        "more": media.len() > count,
        "style": "shelf",
        "promoted": true,
        "Metadata": items
    })
}

pub(crate) fn plex_rating(metadata: &serde_json::Value) -> Option<f64> {
    metadata
        .get("vote_average")
        .and_then(serde_json::Value::as_f64)
}

pub(crate) fn plex_section_metadata(
    state: &AppState,
    section: usize,
) -> HashMap<String, serde_json::Value> {
    let mut metadata = HashMap::new();
    let Ok(config) = state.config.lock().map(|config| config.clone()) else {
        return metadata;
    };
    let Some(library) = config.libraries.get(section.saturating_sub(1)).cloned() else {
        return metadata;
    };
    let media_index = media_snapshot(state);
    for media in media_index
        .iter()
        .filter(|media| media_in_library(media, &library))
    {
        let Some(root) = library
            .folders
            .iter()
            .find(|folder| media.path.starts_with(folder))
        else {
            continue;
        };
        let group_path = metadata_group_path(root, &media.path, &library.kind);
        let media_key = database_media_key(section, &media.id);
        let item = read_metadata_sidecar(state, &media_key, &group_path);
        let Some(mut item) = item.or_else(|| {
            (library.kind == "tv" && read_tv_episode_sidecar_metadata(root, media).is_some())
                .then(|| serde_json::json!({}))
        }) else {
            continue;
        };
        if library.kind == "tv" {
            if let Some(episode) = tv_episode_info(root, &media.path) {
                if let Some(object) = item.as_object_mut() {
                    object.insert("mplex_season".to_owned(), serde_json::json!(episode.season));
                    object.insert(
                        "mplex_episode".to_owned(),
                        serde_json::json!(episode.episode),
                    );
                    if let Some(end) = episode.episode_end {
                        object.insert("mplex_episode_end".to_owned(), serde_json::json!(end));
                    }
                    if let Some(title) = episode.episode_title {
                        object.insert("mplex_episode_title".to_owned(), serde_json::json!(title));
                    }
                    if let Some(details) = read_tv_episode_sidecar_metadata(root, media) {
                        object.insert("mplex_episode_metadata".to_owned(), details);
                    }
                }
            }
        }
        metadata.insert(media.id.clone(), item);
    }
    if let Ok(connection) = state.db.lock() {
        if let Ok(mut statement) = connection.prepare(
            "SELECT media_key, position_ms, duration_ms, watched, updated_at FROM watch_progress WHERE user_id = ?1 AND (hidden_from_continue = 0 OR watched = 1)",
        ) {
            if let Ok(rows) = statement.query_map([USER_ID], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            }) {
                let progress = rows
                    .filter_map(Result::ok)
                    .map(|(path, position, duration, watched, updated_at)| {
                        (path, (position, duration, watched, updated_at))
                    })
                    .collect::<HashMap<_, _>>();
                for media in media_index
                    .iter()
                    .filter(|media| media_in_library(media, &library))
                {
                    let Some(root) = library
                        .folders
                        .iter()
                        .find(|folder| media.path.starts_with(folder))
                    else {
                        continue;
                    };
                    let Some(path) = media.path.strip_prefix(root).ok() else {
                        continue;
                    };
                    let Some((position, duration, watched, updated_at)) =
                        progress.get(path.to_string_lossy().as_ref())
                    else {
                        continue;
                    };
                    let item = metadata
                        .entry(media.id.clone())
                        .or_insert_with(|| serde_json::json!({}));
                    if let Some(object) = item.as_object_mut() {
                        object.insert("mplex_view_offset".to_owned(), serde_json::json!(position));
                        object.insert("mplex_view_count".to_owned(), serde_json::json!(i64::from(*watched)));
                        object.insert(
                            "mplex_last_viewed_at".to_owned(),
                            serde_json::json!(if *watched { *updated_at } else { 0 }),
                        );
                        if let Some(duration) = duration {
                            object.insert("mplex_duration".to_owned(), serde_json::json!(duration));
                        }
                    }
                }
            }
        }
    }
    metadata
}

pub(crate) fn plex_genre_tokens(metadata: Option<&serde_json::Value>) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    if let Some(genres) = metadata
        .and_then(|metadata| metadata.get("genres"))
        .and_then(serde_json::Value::as_array)
    {
        tokens.extend(genres.iter().filter_map(|genre| {
            genre
                .get("id")
                .and_then(serde_json::Value::as_i64)
                .map(|id| format!("id:{id}"))
                .or_else(|| {
                    genre
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(|name| format!("name:{}", name.to_lowercase()))
                })
        }));
    }
    if let Some(ids) = metadata
        .and_then(|metadata| metadata.get("genre_ids"))
        .and_then(serde_json::Value::as_array)
    {
        tokens.extend(
            ids.iter()
                .filter_map(serde_json::Value::as_i64)
                .map(|id| format!("id:{id}")),
        );
    }
    tokens
}

pub(crate) async fn plex_detect_markers(
    State(state): State<AppState>,
    Path(ids): Path<String>,
    Query(query): Query<BTreeMap<String, String>>,
    uri: Uri,
) -> Response {
    let Some(media) = find_media(&state, &ids) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let kind = if uri.path().ends_with("/credits") {
        markers::MarkerKind::Credits
    } else {
        markers::MarkerKind::Intro
    };
    let force = query_bool(&query, "force");
    let threshold = markers::detection_threshold(
        query
            .get("threshold")
            .and_then(|value| value.parse::<f64>().ok()),
    );
    let activity_metrics = state.metrics.clone();
    let activity_key = format!("manual-markers:{}:{}", media.id, kind.as_str());
    let activity_kind = format!("{} detection (manual)", kind.as_str());
    let activity_title = media.title.clone();
    let activity_detail = format!("Library item {}", media.id);
    tokio::spawn(async move {
        let _activity = crate::harbinger::metrics::ActivityGuard::begin(
            &activity_metrics,
            activity_key,
            activity_kind,
            activity_title,
            activity_detail,
        );
        if let Err(error) = detect_markers_for_episode(state, media, kind, force, threshold).await {
            warn!(%error, marker_type = kind.as_str(), "marker detection failed");
        }
    });
    StatusCode::OK.into_response()
}

pub(crate) async fn plex_create_marker(
    State(state): State<AppState>,
    Path(ids): Path<String>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Some(media) = find_media(&state, &ids) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(kind) = query
        .get("type")
        .and_then(|value| markers::MarkerKind::from_marker_input(value))
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let (Some(start_ms), Some(end_ms)) = (
        query_i64(&query, "startTimeOffset"),
        query_i64(&query, "endTimeOffset"),
    ) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if start_ms < 0 || end_ms <= start_ms {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let duration_ms = media_duration_ms(&media).await.unwrap_or(i64::MAX);
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match markers::create_user_marker(&db, &media.id, kind, start_ms, end_ms, duration_ms) {
        Ok(marker) => marker_response(vec![marker]),
        Err(error) => {
            warn!(%error, "could not create marker");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub(crate) async fn plex_edit_marker(
    State(state): State<AppState>,
    Path((ids, marker_id)): Path<(String, String)>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Some(media) = find_media(&state, &ids) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(marker_id) = marker_id.parse::<i64>() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(kind) = query
        .get("type")
        .and_then(|value| markers::MarkerKind::from_marker_input(value))
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(start_ms) = query_i64(&query, "startTimeOffset") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let duration_ms = media_duration_ms(&media).await.unwrap_or(i64::MAX);
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(existing) = markers::markers_for_media(&db, &media.id) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(current) = existing.iter().find(|marker| marker.id == marker_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let end_ms = query_i64(&query, "endTimeOffset").unwrap_or(current.end_ms);
    if start_ms < 0 || end_ms <= start_ms {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match markers::update_marker(&db, marker_id, kind, start_ms, end_ms, duration_ms) {
        Ok(Some(marker)) => marker_response(vec![marker]),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            warn!(%error, "could not edit marker");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub(crate) async fn plex_delete_marker(
    State(state): State<AppState>,
    Path((ids, marker_id)): Path<(String, String)>,
) -> Response {
    let Some(media) = find_media(&state, &ids) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(marker_id) = marker_id.parse::<i64>() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Ok(db) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match markers::delete_marker(&db, &media.id, marker_id) {
        Ok(true) => StatusCode::OK.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            warn!(%error, "could not delete marker");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub(crate) async fn plex_delete_section_markers(
    State(state): State<AppState>,
    Path(section): Path<String>,
) -> Response {
    let Ok(section) = section.parse::<u64>() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match state
        .db
        .lock()
        .map_err(|_| ())
        .and_then(|db| markers::delete_section_markers(&db, section).map_err(|_| ()))
    {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) fn plex_library_for_metadata(
    state: &AppState,
    id: &str,
) -> Option<(usize, ConfigLibrary)> {
    let media_index = media_snapshot(state);
    find_media(state, id)
        .and_then(|media| media_library(state, &media))
        .or_else(|| {
            find_plex_tv_location(state, media_index.as_ref(), id)
                .map(|(section, library, _, _)| (section, library))
        })
}

pub(crate) async fn plex_related_hubs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Some((section, library)) = plex_library_for_metadata(&state, &id) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let count = query
        .get("count")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(10)
        .min(50);
    let media_index = media_snapshot(&state);
    let metadata = plex_section_metadata(&state, section);
    let source_groups = (library.kind == "tv").then(|| {
        plex_tv_show_groups(
            section,
            &library.folders,
            media_index
                .iter()
                .filter(|media| media_in_library(media, &library)),
        )
    });
    let source_group = source_groups.as_ref().and_then(|groups| {
        groups.iter().find(|show| {
            show.rating_key == id
                || show.episodes.iter().any(|episode| episode.id == id)
                || plex_tv_season_groups(show, &library.folders)
                    .keys()
                    .any(|season| plex_tv_season_key(&show.rating_key, *season) == id)
        })
    });
    let source_media = source_group
        .and_then(|show| show.episodes.first().copied())
        .or_else(|| media_index.iter().find(|media| media.id == id));
    let target_genres = plex_genre_tokens(source_media.and_then(|media| metadata.get(&media.id)));
    let source_group_key = source_group.map(|show| show.rating_key.clone());

    let (total, items) = if library.kind == "tv" {
        let groups = source_groups.unwrap_or_default();
        let mut candidates = groups
            .iter()
            .filter(|show| Some(show.rating_key.as_str()) != source_group_key.as_deref())
            .filter_map(|show| {
                let representative = show.episodes.first().copied()?;
                let item_metadata = metadata.get(&representative.id);
                let overlap = plex_genre_tokens(item_metadata)
                    .intersection(&target_genres)
                    .count();
                Some((
                    show,
                    representative,
                    overlap,
                    item_metadata.and_then(plex_rating),
                ))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .2
                .cmp(&left.2)
                .then_with(|| {
                    right
                        .3
                        .partial_cmp(&left.3)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| media_modified_at(right.1).cmp(&media_modified_at(left.1)))
        });
        let total = candidates.len();
        let mut related_items = Vec::new();
        for (show, _, _, _) in candidates.iter().take(count) {
            related_items.push(plex_tv_show_json(show, section, &library, &metadata));
        }
        (total, related_items)
    } else {
        let mut candidates = media_index
            .iter()
            .filter(|media| media_in_library(media, &library) && media.id != id)
            .map(|media| {
                let item_metadata = metadata.get(&media.id);
                let overlap = plex_genre_tokens(item_metadata)
                    .intersection(&target_genres)
                    .count();
                (media, overlap, item_metadata.and_then(plex_rating))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            right
                .1
                .cmp(&left.1)
                .then_with(|| {
                    right
                        .2
                        .partial_cmp(&left.2)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| media_modified_at(right.0).cmp(&media_modified_at(left.0)))
        });
        let total = candidates.len();
        let mut related_items = Vec::new();
        for (media, _, _) in candidates.iter().take(count) {
            let mut item = plex_video_json(*media, section, &library, metadata.get(&media.id));
            add_plex_technical_metadata(&state, &mut item, media).await;
            related_items.push(item);
        }
        (total, related_items)
    };
    let hubs = if query.get("onlyTransient").is_some_and(|value| value == "1") {
        Vec::new()
    } else {
        vec![serde_json::json!({
            "hubKey": format!("/library/sections/{section}/all"),
            "key": format!("/hubs/metadata/{id}/related"),
            "title": "Related",
            "type": plex_kind(&library.kind),
            "hubIdentifier": format!("{}.related.{section}", plex_kind(&library.kind)),
            "context": "hub.metadata.related",
            "size": items.len(),
            "more": total > items.len(),
            "style": "shelf",
            "promoted": false,
            "Metadata": items
        })]
    };
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": hubs.len(),
            "allowSync": false,
            "identifier": "com.plexapp.plugins.library",
            "librarySectionID": section,
            "librarySectionTitle": library.name,
            "librarySectionUUID": plex_library_uuid(&state, &library.name, section),
            "Hub": hubs
        }
    }))
}

pub(crate) fn plex_search_rank(query: &str, candidate: &str) -> Option<u8> {
    let query = query.trim().to_lowercase();
    let candidate = candidate.to_lowercase();
    if query.is_empty() {
        return None;
    }
    if candidate.contains(&query) {
        return Some(if candidate == query {
            0
        } else if candidate.starts_with(&query) {
            1
        } else {
            2
        });
    }

    let query_words = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let candidate_words = candidate
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    if query_words.is_empty() || candidate_words.is_empty() {
        return None;
    }

    let mut fuzzy_match = false;
    for query_word in query_words {
        if candidate_words
            .iter()
            .any(|candidate_word| candidate_word.contains(query_word))
        {
            continue;
        }
        let max_distance = if query_word.chars().count() >= 10 {
            2
        } else {
            1
        };
        if candidate_words.iter().any(|candidate_word| {
            plex_edit_distance(query_word, candidate_word).is_some_and(|distance| {
                distance <= max_distance
                    && (query_word.chars().count() as isize
                        - candidate_word.chars().count() as isize)
                        .unsigned_abs()
                        <= max_distance
            })
        }) {
            fuzzy_match = true;
        } else {
            return None;
        }
    }
    Some(if fuzzy_match { 3 } else { 2 })
}

pub(crate) fn plex_edit_distance(left: &str, right: &str) -> Option<usize> {
    let left = left.chars().collect::<Vec<_>>();
    let right = right.chars().collect::<Vec<_>>();
    if left.len().abs_diff(right.len()) > 2 {
        return None;
    }
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_character) in left.iter().enumerate() {
        current[0] = left_index + 1;
        let mut row_minimum = current[0];
        for (right_index, right_character) in right.iter().enumerate() {
            current[right_index + 1] = (previous[right_index + 1] + 1)
                .min(current[right_index] + 1)
                .min(previous[right_index] + usize::from(left_character != right_character));
            row_minimum = row_minimum.min(current[right_index + 1]);
        }
        if row_minimum > 2 {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    Some(previous[right.len()])
}

pub(crate) fn plex_episode_search_rank(
    query: &str,
    episode_title: &str,
    filename_title: &str,
    show_rank: Option<u8>,
) -> Option<u8> {
    [
        plex_search_rank(query, episode_title),
        plex_search_rank(query, filename_title),
        show_rank,
    ]
    .into_iter()
    .flatten()
    .min()
}

pub(crate) fn plex_search_type_matches(query: &BTreeMap<String, String>, kind: &str) -> bool {
    let Some(type_filter) = query.get("type") else {
        return true;
    };
    type_filter.split(',').any(|value| {
        matches!(
            (value.trim(), kind),
            ("1" | "movie", "movie") | ("2" | "show", "show") | ("4" | "episode", "episode")
        )
    })
}

pub(crate) async fn plex_search_items(
    state: &AppState,
    query: &BTreeMap<String, String>,
) -> Vec<serde_json::Value> {
    let Some(term) = query.get("query").map(|value| value.trim()) else {
        return Vec::new();
    };
    if term.is_empty() {
        return Vec::new();
    }
    let section_filter = query
        .get("sectionId")
        .or_else(|| query.get("sectionID"))
        .and_then(|section| section.parse::<usize>().ok());
    let limit = query
        .get("limit")
        .and_then(|limit| limit.parse::<usize>().ok())
        .unwrap_or(100)
        .clamp(1, 100);
    let libraries = state
        .config
        .lock()
        .map(|config| config.libraries.clone())
        .unwrap_or_default();
    let media_index = media_snapshot(state);
    let mut results = Vec::<(u8, String, serde_json::Value)>::new();

    for (index, library) in libraries.iter().enumerate() {
        let section = index + 1;
        if section_filter.is_some_and(|requested| requested != section) {
            continue;
        }
        let metadata = plex_section_metadata(state, section);
        if library.kind == "tv" {
            let shows = plex_tv_show_groups(
                section,
                &library.folders,
                media_index
                    .iter()
                    .filter(|media| media_in_library(media, library)),
            );
            for show in shows {
                let show_guid = plex_tv_show_guid(&show, &metadata);
                let representative = show.episodes.first().copied();
                let show_metadata = representative.and_then(|media| metadata.get(&media.id));
                let show_title = show_metadata
                    .and_then(|value| value.get("name"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(&show.title);
                let show_rank = [
                    show_title,
                    show.title.as_str(),
                    show_metadata
                        .and_then(|value| value.get("original_name"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default(),
                ]
                .into_iter()
                .filter_map(|candidate| plex_search_rank(term, candidate))
                .min();
                if plex_search_type_matches(query, "show") {
                    if let Some(rank) = show_rank {
                        results.push((
                            rank,
                            show_title.to_lowercase(),
                            plex_tv_show_json(&show, section, library, &metadata),
                        ));
                    }
                }
                if !plex_search_type_matches(query, "episode") {
                    continue;
                }
                let episode_groups = plex_tv_season_groups(&show, &library.folders);
                for (season, episodes) in episode_groups {
                    for (index, episode_group) in episodes.iter().enumerate() {
                        let Some(representative) = episode_group.files.first() else {
                            continue;
                        };
                        let episode_metadata = metadata.get(&representative.id);
                        let episode_filename_title = tv_episode_display_name(&representative.title);
                        let episode_title = episode_metadata
                            .and_then(|value| value.get("mplex_episode_title"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(&episode_filename_title);
                        let rank = episode_group
                            .files
                            .iter()
                            .filter_map(|file| {
                                let filename_title = tv_episode_display_name(&file.title);
                                let title = metadata
                                    .get(&file.id)
                                    .and_then(|value| value.get("mplex_episode_title"))
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or(&filename_title);
                                plex_episode_search_rank(
                                    term,
                                    title,
                                    filename_title.as_str(),
                                    show_rank,
                                )
                            })
                            .min();
                        let Some(rank) = rank else {
                            continue;
                        };
                        let item = plex_tv_episode_json(
                            state,
                            episode_group,
                            &show,
                            season,
                            episode_group.episode.unwrap_or(index as u32 + 1),
                            section,
                            library,
                            &metadata,
                            &show_guid,
                        )
                        .await;
                        results.push((rank, episode_title.to_lowercase(), item));
                    }
                }
            }
        } else if plex_search_type_matches(query, "movie") {
            for media in media_index
                .iter()
                .filter(|media| media_in_library(media, library))
            {
                let item_metadata = metadata.get(&media.id);
                let rank = [
                    media.title.as_str(),
                    item_metadata
                        .and_then(|value| value.get("title"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default(),
                    item_metadata
                        .and_then(|value| value.get("original_title"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default(),
                ]
                .into_iter()
                .filter_map(|candidate| plex_search_rank(term, candidate))
                .min();
                if let Some(rank) = rank {
                    let mut item = plex_video_json(media, section, library, item_metadata);
                    add_plex_technical_metadata(state, &mut item, media).await;
                    results.push((
                        rank,
                        item["title"].as_str().unwrap_or_default().to_lowercase(),
                        item,
                    ));
                }
            }
        }
    }

    results.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    results
        .into_iter()
        .take(limit)
        .map(|(_, _, item)| item)
        .collect()
}

pub(crate) fn plex_metadata_guid(media_type: &str, rating_key: &str) -> String {
    format!("com.plexapp.plugins.library://{media_type}/{rating_key}")
}

pub(crate) fn plex_tv_show_guid(
    show: &PlexTvShowGroup<'_>,
    metadata: &HashMap<String, serde_json::Value>,
) -> String {
    show.episodes
        .first()
        .and_then(|episode| metadata.get(&episode.id))
        .and_then(|metadata| metadata.get("id"))
        .and_then(serde_json::Value::as_i64)
        .map(|id| format!("tmdb://tv/{id}"))
        .unwrap_or_else(|| plex_metadata_guid("show", &show.rating_key))
}

pub(crate) async fn plex_library_search(
    State(state): State<AppState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    if !query.contains_key("query") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let items = plex_search_items(&state, &query).await;
    let hubs = plex_search_hubs(items.clone(), 3);
    // Plezy consumes the local-library response as SearchResult records whose
    // Metadata member is the actual playable library item. Keep the simpler
    // Metadata and Hub forms too for clients using those documented shapes.
    let search_results = plex_search_result_entries(&items);
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": items.len(),
            "totalSize": items.len(),
            "offset": 0,
            "identifier": "com.plexapp.plugins.library",
            "Hub": hubs,
            "SearchResult": search_results,
            "Metadata": items
        }
    }))
}

pub(crate) fn plex_search_result_entries(items: &[serde_json::Value]) -> Vec<serde_json::Value> {
    items
        .iter()
        .cloned()
        .map(|item| serde_json::json!({"Metadata": item, "score": 1.0}))
        .collect()
}

pub(crate) fn plex_search_hubs(
    items: Vec<serde_json::Value>,
    limit: usize,
) -> Vec<serde_json::Value> {
    let mut grouped = BTreeMap::<String, Vec<serde_json::Value>>::new();
    for item in items {
        let kind = item
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("movie")
            .to_owned();
        grouped.entry(kind).or_default().push(item);
    }
    grouped
        .into_iter()
        .map(|(kind, items)| {
            let title = match kind.as_str() {
                "show" => "Shows",
                "episode" => "Episodes",
                _ => "Movies",
            };
            let more = items.len() > limit;
            let items = items.into_iter().take(limit).collect::<Vec<_>>();
            let mut hub = serde_json::json!({
                "hubIdentifier": kind,
                "title": title,
                "type": kind,
                "size": items.len(),
                "more": more
            });
            let item_field = if kind == "show" {
                "Directory"
            } else {
                "Metadata"
            };
            hub[item_field] = serde_json::json!(items);
            hub
        })
        .collect()
}

pub(crate) async fn plex_hub_search(
    State(state): State<AppState>,
    Query(mut query): Query<BTreeMap<String, String>>,
) -> Response {
    if !query.contains_key("query") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let limit = query
        .get("limit")
        .and_then(|limit| limit.parse::<usize>().ok())
        .unwrap_or(3)
        .clamp(1, 50);
    query.insert("limit".to_owned(), "100".to_owned());
    let items = plex_search_items(&state, &query).await;
    let hubs = plex_search_hubs(items, limit);
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": hubs.len(),
            "Hub": hubs
        }
    }))
}

pub(crate) async fn plex_section(
    State(state): State<AppState>,
    Path(section): Path<usize>,
) -> Response {
    let Some(library) = state
        .config
        .lock()
        .ok()
        .and_then(|config| config.libraries.get(section.saturating_sub(1)).cloned())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let uuid = plex_library_uuid(&state, &library.name, section);
    let (_, scanned_at, updated_at) = library_publication_state(&state, &library.name, section);
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "updatedAt": library_catalog_updated_at(&state),
            "librarySectionID": section,
            "librarySectionTitle": library.name,
            "Directory": [{
                "key": format!("/library/sections/{section}/all"),
                "type": plex_kind(&library.kind),
                "title": library.name,
                "uuid": uuid,
                "scannedAt": scanned_at,
                "updatedAt": updated_at
            }]
        }
    }))
}

pub(crate) fn plex_section_update_response(
    state: &AppState,
    index: usize,
    headers: &HeaderMap,
) -> Response {
    let directories = plex_section_directories(state).1;
    let Some(directory) = directories.get(index) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    plex_create_section_response(directory, headers)
}

pub(crate) async fn plex_update_section(
    State(state): State<AppState>,
    Path(section): Path<usize>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let mut agent = None;
    let mut name = None;
    let mut scanner = None;
    let mut language = None;
    let mut provider_group_id = None;
    let mut locations = Vec::new();
    let mut preferences = BTreeMap::new();
    for (key, value) in form_urlencoded::parse(raw_query.unwrap_or_default().as_bytes()) {
        let value = value.into_owned();
        match key.as_ref() {
            "agent" => agent = Some(value),
            "name" => name = Some(value),
            "scanner" => scanner = Some(value),
            "language" => language = Some(value),
            "metadataAgentProviderGroupId" => {
                let Ok(id) = value.parse::<i64>() else {
                    return StatusCode::BAD_REQUEST.into_response();
                };
                provider_group_id = Some(id);
            }
            "locations" | "location" | "location[]" => locations.push(value),
            key => {
                if let Some(key) = key
                    .strip_prefix("prefs[")
                    .and_then(|key| key.strip_suffix(']'))
                    .filter(|key| !key.is_empty())
                {
                    preferences.insert(key.to_owned(), value);
                }
            }
        }
    }
    let Some(agent) = agent.filter(|agent| !agent.trim().is_empty()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let name = match name {
        Some(name) if !name.trim().is_empty() && name.trim().chars().count() <= 64 => {
            Some(name.trim().to_owned())
        }
        Some(_) => return StatusCode::BAD_REQUEST.into_response(),
        None => None,
    };
    let scanner = match scanner {
        Some(scanner) if !scanner.trim().is_empty() => Some(scanner.trim().to_owned()),
        Some(_) => return StatusCode::BAD_REQUEST.into_response(),
        None => None,
    };
    let language = match language {
        Some(language) if !language.trim().is_empty() => Some(language.trim().to_owned()),
        Some(_) => return StatusCode::BAD_REQUEST.into_response(),
        None => None,
    };
    let Some(index) = section.checked_sub(1) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(library) = state
        .config
        .lock()
        .ok()
        .and_then(|config| config.libraries.get(index).cloned())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let tv_library = library.kind == "tv";
    let allowed_agents = if tv_library {
        [
            "tv.plex.agents.series",
            "tv.plex.agents.none",
            "tv.plex.agents.nfo.tv",
        ]
    } else {
        [
            "tv.plex.agents.movie",
            "tv.plex.agents.none",
            "tv.plex.agents.nfo.movie",
        ]
    };
    let allowed_scanners = if tv_library {
        ["Plex TV Series", "Plex Series Scanner"]
    } else {
        ["Plex Movie", "Plex Video Files"]
    };
    if !allowed_agents.contains(&agent.as_str())
        || scanner
            .as_deref()
            .is_some_and(|scanner| !allowed_scanners.contains(&scanner))
    {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let mut requested_locations = Vec::new();
    for location in locations {
        let requested = PathBuf::from(location.trim());
        let Ok(metadata) = tokio::fs::metadata(&requested).await else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if !metadata.is_dir() {
            return StatusCode::BAD_REQUEST.into_response();
        }
        let Ok(canonical) = tokio::fs::canonicalize(requested).await else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if !requested_locations.contains(&canonical) {
            requested_locations.push(canonical);
        }
    }

    let mut existing_locations = Vec::new();
    for folder in &library.folders {
        existing_locations.push(
            tokio::fs::canonicalize(folder)
                .await
                .unwrap_or_else(|_| folder.clone()),
        );
    }

    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(current_library) = config.libraries.get(index) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if current_library.name != library.name
        || current_library.kind != library.kind
        || current_library.folders != library.folders
    {
        return StatusCode::CONFLICT.into_response();
    }
    if name.as_ref().is_some_and(|name| {
        config
            .libraries
            .iter()
            .enumerate()
            .any(|(other_index, other)| {
                other_index != index && other.name.eq_ignore_ascii_case(name)
            })
    }) {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let mut updated_config = config.clone();
    let updated_library = &mut updated_config.libraries[index];
    let mut changed = false;
    if updated_library.plex_agent.as_deref() != Some(agent.as_str()) {
        updated_library.plex_agent = Some(agent);
        changed = true;
    }
    if let Some(name) = name {
        if updated_library.name != name {
            updated_library.name = name;
            changed = true;
        }
    }
    if let Some(scanner) = scanner {
        if updated_library.plex_scanner.as_deref() != Some(scanner.as_str()) {
            updated_library.plex_scanner = Some(scanner);
            changed = true;
        }
    }
    if let Some(language) = language {
        if updated_library.plex_language.as_deref() != Some(language.as_str()) {
            updated_library.plex_language = Some(language);
            changed = true;
        }
    }
    if let Some(provider_group_id) = provider_group_id {
        if updated_library.metadata_agent_provider_group_id != Some(provider_group_id) {
            updated_library.metadata_agent_provider_group_id = Some(provider_group_id);
            changed = true;
        }
    }
    for location in requested_locations {
        if !existing_locations.contains(&location) {
            updated_library.folders.push(location.clone());
            existing_locations.push(location);
            changed = true;
        }
    }
    if !preferences.is_empty() {
        let stored_preferences = updated_library
            .plex_preferences
            .get_or_insert_with(BTreeMap::new);
        for (key, value) in preferences {
            if stored_preferences.get(&key) != Some(&value) {
                stored_preferences.insert(key, value);
                changed = true;
            }
        }
    }
    if !changed {
        drop(config);
        return plex_section_update_response(&state, index, &headers);
    }

    let old_name = library.name;
    let new_name = updated_library.name.clone();
    let Ok(mut connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(transaction) = connection.transaction() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if (old_name != new_name
        && transaction
            .execute(
                "UPDATE library_groups SET name = ?1, updated_at = MAX(updated_at + 1, CAST(strftime('%s', 'now') AS INTEGER)) WHERE name = ?2",
                (&new_name, &old_name),
            )
            .is_err())
        || sync_config_libraries(&transaction, &updated_config).is_err()
        || update_library_catalog_timestamp(&transaction).is_err()
        || save_config(&state.config_path, &updated_config).is_err()
        || transaction.commit().is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    drop(connection);
    *config = updated_config;
    drop(config);
    plex_section_update_response(&state, index, &headers)
}

pub(crate) fn plex_section_preference_settings(
    kind: &str,
    preferences: Option<&BTreeMap<String, String>>,
) -> Vec<serde_json::Value> {
    let make_setting = |id: &str, setting_type: &str, default: &str| {
        let value = preferences
            .and_then(|preferences| preferences.get(id))
            .map(String::as_str)
            .unwrap_or(default);
        let mut setting = serde_json::json!({
            "id": id,
            "label": id,
            "summary": "",
            "type": setting_type,
            "default": default,
            "value": value,
            "hidden": false,
            "advanced": false,
            "group": ""
        });
        let enum_values = match id {
            "hidden" => Some("0:Include on home screen and in global search|1:Exclude from home screen|2:Exclude from home screen and global search"),
            "episodeSort" => Some("-1:Oldest first|1:Newest first"),
            "showOrdering" => Some("tmdbAiring:The Movie Database|aired:TheTVDB"),
            "collectionMode" => Some("0:Hide collections and show items|1:Hide items in collections|2:Show both"),
            "flattenSeasons" => Some("0:Show|2:Hide for single-season series|1:Hide"),
            "enableAdMarkerGeneration" => Some("0:Disabled|1:For recorded items|2:For all items"),
            _ => None,
        };
        if let Some(enum_values) = enum_values {
            setting["enumValues"] = serde_json::json!(enum_values);
        }
        setting
    };
    let common_settings = [
        ("hidden", "int", "0"),
        ("country", "text", "US"),
        ("originalTitles", "bool", "false"),
        ("localizedArtwork", "bool", "true"),
        ("useLocalAssets", "bool", "true"),
        ("respectTags", "bool", "false"),
        ("useExternalExtras", "bool", "true"),
        ("skipNonTrailerExtras", "bool", "false"),
        ("useRedbandTrailers", "bool", "false"),
        ("includeExtrasWithLocalizedSubtitles", "bool", "false"),
        ("includeAdultContent", "bool", "false"),
        ("enableBIFGeneration", "bool", "true"),
        ("collectionMode", "int", "2"),
        ("enableIntroMarkerGeneration", "bool", "true"),
        ("enableCreditsMarkerGeneration", "bool", "true"),
        ("enableAdMarkerGeneration", "int", "1"),
        ("enableVoiceActivityGeneration", "bool", "true"),
        ("enableLoudnessAnalysis", "bool", "true"),
    ];
    let mut settings: Vec<serde_json::Value> = common_settings
        .iter()
        .map(|(id, setting_type, value)| make_setting(id, setting_type, value))
        .collect();
    if kind == "tv" {
        for (id, setting_type, value) in [
            ("episodeSort", "text", "-1"),
            ("showOrdering", "text", "tmdbAiring"),
            ("useSeasonTitles", "bool", "false"),
            ("flattenSeasons", "int", "0"),
        ] {
            settings.push(make_setting(id, setting_type, value));
        }
    }
    settings
}

pub(crate) fn plex_preferences_response(
    container: serde_json::Value,
    headers: &HeaderMap,
) -> Response {
    let accepts_xml = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.to_ascii_lowercase().contains("application/xml"));
    if !accepts_xml {
        return plex_json(serde_json::json!({"MediaContainer": container}));
    }
    let root_attributes = ["size", "librarySectionID", "librarySectionTitle"]
        .iter()
        .filter_map(|name| {
            container.get(*name).map(|value| {
                let value = value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string());
                format!(" {}=\"{}\"", name, xml_escape(&value))
            })
        })
        .collect::<String>();
    let settings = container
        .get("Setting")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|setting| {
            let attributes = setting
                .as_object()
                .into_iter()
                .flat_map(|setting| setting.iter())
                .map(|(name, value)| {
                    let value = value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string());
                    format!(" {name}=\"{}\"", xml_escape(&value))
                })
                .collect::<String>();
            format!("<Setting{attributes}/>")
        })
        .collect::<String>();
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><MediaContainer{root_attributes}>{settings}</MediaContainer>"
    );
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/xml"),
    );
    plex_protocol_response(response)
}

pub(crate) async fn plex_library_section_prefs(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let mut media_type = None;
    let mut agent = None;
    for (key, value) in form_urlencoded::parse(raw_query.unwrap_or_default().as_bytes()) {
        match key.as_ref() {
            "type" => {
                let Ok(value) = value.parse::<i64>() else {
                    return StatusCode::BAD_REQUEST.into_response();
                };
                media_type = Some(value);
            }
            "agent" => agent = Some(value.into_owned()),
            _ => {}
        }
    }
    let Some(media_type) = media_type else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let kind = match media_type {
        1 => "movie",
        2 => "tv",
        _ => {
            return plex_preferences_response(
                serde_json::json!({"size": 0, "Setting": []}),
                &headers,
            )
        }
    };
    let libraries = match state.config.lock() {
        Ok(config) => config.libraries.clone(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let library = libraries.iter().find(|library| {
        let library_kind_matches = if kind == "movie" {
            library.kind != "tv"
        } else {
            library.kind == "tv"
        };
        let default_agent = if library.kind == "tv" {
            "tv.plex.agents.series"
        } else {
            "tv.plex.agents.movie"
        };
        library_kind_matches
            && agent
                .as_deref()
                .is_none_or(|agent| library.plex_agent.as_deref().unwrap_or(default_agent) == agent)
    });
    let settings = plex_section_preference_settings(
        kind,
        library.and_then(|library| library.plex_preferences.as_ref()),
    );
    plex_preferences_response(
        serde_json::json!({"size": settings.len(), "Setting": settings}),
        &headers,
    )
}

pub(crate) async fn plex_agent_prefs(
    Path((root, plugin)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if root != ":" {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !matches!(
        plugin.as_str(),
        "tv.plex.agents.movie"
            | "tv.plex.agents.none"
            | "tv.plex.agents.series"
            | "tv.plex.agents.nfo.movie"
            | "tv.plex.agents.nfo.tv"
    ) {
        return StatusCode::NOT_FOUND.into_response();
    }
    plex_preferences_response(serde_json::json!({"size": 0, "Setting": []}), &headers)
}

pub(crate) fn plex_browse_include_files(raw_query: Option<&str>) -> Result<bool, StatusCode> {
    let include_files = raw_query
        .map(|query| {
            form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == "includeFiles")
                .map(|(_, value)| value.into_owned())
        })
        .flatten();
    match include_files.as_deref() {
        None | Some("0") => Ok(false),
        Some("1") => Ok(true),
        Some(_) => Err(StatusCode::BAD_REQUEST),
    }
}

pub(crate) async fn plex_service_browse_root(
    State(_state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let include_files = match plex_browse_include_files(raw_query.as_deref()) {
        Ok(include_files) => include_files,
        Err(status) => return status.into_response(),
    };
    plex_browse_directory(PathBuf::from("/"), include_files, &headers).await
}

pub(crate) async fn plex_service_browse_path(
    State(_state): State<AppState>,
    Path(encoded_path): Path<String>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let include_files = match plex_browse_include_files(raw_query.as_deref()) {
        Ok(include_files) => include_files,
        Err(status) => return status.into_response(),
    };
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded_path.as_bytes())
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(encoded_path.as_bytes()));
    let Ok(decoded) = decoded else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Ok(path) = String::from_utf8(decoded) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    plex_browse_directory(path, include_files, &headers).await
}

pub(crate) async fn plex_browse_directory(
    directory: PathBuf,
    include_files: bool,
    headers: &HeaderMap,
) -> Response {
    let Ok(metadata) = tokio::fs::metadata(&directory).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !metadata.is_dir() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(mut entries) = tokio::fs::read_dir(&directory).await else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let mut paths = Vec::new();
    let mut files = Vec::new();
    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        let Ok(file_type) = entry.file_type().await else {
            continue;
        };
        let child_path = entry.path();
        let encoded = URL_SAFE_NO_PAD.encode(child_path.to_string_lossy().as_bytes());
        let key = format!("/services/browse/{encoded}");
        let title = entry.file_name().to_string_lossy().into_owned();
        let item = serde_json::json!({
            "key": key,
            "path": child_path.to_string_lossy(),
            "title": title
        });
        if file_type.is_dir() {
            paths.push(item);
        } else if include_files && file_type.is_file() {
            files.push(item);
        }
    }
    paths.sort_by(|left, right| left["title"].as_str().cmp(&right["title"].as_str()));
    files.sort_by(|left, right| left["title"].as_str().cmp(&right["title"].as_str()));

    let size = paths.len() + files.len();
    let mut container = serde_json::json!({"size": size, "Path": paths});
    if !files.is_empty() {
        container["File"] = serde_json::json!(files);
    }
    let accepts_json = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.to_ascii_lowercase().contains("application/json"));
    if accepts_json {
        return plex_json(serde_json::json!({"MediaContainer": container}));
    }
    let mut body =
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><MediaContainer size=\"{size}\">");
    for (tag, items) in [
        ("Path", container["Path"].as_array()),
        ("File", container["File"].as_array()),
    ] {
        for item in items.into_iter().flatten() {
            let attributes = ["key", "path", "title"]
                .iter()
                .filter_map(|name| {
                    item.get(*name)
                        .and_then(serde_json::Value::as_str)
                        .map(|value| format!(" {name}=\"{}\"", xml_escape(value)))
                })
                .collect::<String>();
            body.push_str(&format!("<{tag}{attributes}/>"));
        }
    }
    body.push_str("</MediaContainer>");
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/xml"),
    );
    plex_protocol_response(response)
}

pub(crate) fn plex_section_preferences_response(
    section: usize,
    library: &ConfigLibrary,
    headers: &HeaderMap,
) -> Response {
    let settings =
        plex_section_preference_settings(&library.kind, library.plex_preferences.as_ref());
    plex_preferences_response(
        serde_json::json!({
            "size": settings.len(),
            "librarySectionID": section,
            "librarySectionTitle": library.name,
            "Setting": settings
        }),
        headers,
    )
}

pub(crate) async fn plex_section_prefs(
    State(state): State<AppState>,
    Path(section): Path<usize>,
    headers: HeaderMap,
) -> Response {
    let Some(library) = section.checked_sub(1).and_then(|index| {
        state
            .config
            .lock()
            .ok()
            .and_then(|config| config.libraries.get(index).cloned())
    }) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    plex_section_preferences_response(section, &library, &headers)
}

pub(crate) async fn plex_update_section_prefs(
    State(state): State<AppState>,
    Path(section): Path<usize>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    let mut preferences = BTreeMap::new();
    for (key, value) in form_urlencoded::parse(raw_query.unwrap_or_default().as_bytes()) {
        let key = key.as_ref();
        let value = value.into_owned();
        if key == "prefs" {
            let Ok(object) = serde_json::from_str::<serde_json::Value>(&value) else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            let Some(object) = object.as_object() else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            for (key, value) in object {
                preferences.insert(
                    key.clone(),
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string()),
                );
            }
        } else if let Some(key) = key
            .strip_prefix("prefs[")
            .and_then(|key| key.strip_suffix(']'))
            .filter(|key| !key.is_empty())
        {
            preferences.insert(key.to_owned(), value);
        } else if !key.starts_with("X-Plex-") && key != "token" {
            preferences.insert(key.to_owned(), value);
        }
    }
    if preferences.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(index) = section.checked_sub(1) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(mut config) = state.config.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(library) = config.libraries.get(index).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut updated_config = config.clone();
    let stored_preferences = updated_config.libraries[index]
        .plex_preferences
        .get_or_insert_with(BTreeMap::new);
    let mut changed = false;
    for (key, value) in preferences {
        if stored_preferences.get(&key) != Some(&value) {
            stored_preferences.insert(key, value);
            changed = true;
        }
    }
    if changed {
        let Ok(mut connection) = state.db.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let Ok(transaction) = connection.transaction() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        if transaction
            .execute(
                "UPDATE library_groups SET updated_at = MAX(updated_at + 1, CAST(strftime('%s', 'now') AS INTEGER)) WHERE name = ?1",
                [&library.name],
            )
            .is_err()
            || update_library_catalog_timestamp(&transaction).is_err()
            || save_config(&state.config_path, &updated_config).is_err()
            || transaction.commit().is_err()
        {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        drop(connection);
        *config = updated_config;
    }
    let updated_library = config.libraries[index].clone();
    drop(config);
    plex_section_preferences_response(section, &updated_library, &headers)
}

pub(crate) async fn plex_section_collections(
    State(state): State<AppState>,
    Path(section): Path<usize>,
) -> Response {
    let libraries = match state.config.lock() {
        Ok(config) => config.libraries.clone(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if section == 0 || section > libraries.len() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let library = &libraries[section - 1];
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(mut collections) = plex_collection_records(&connection) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    collections.retain(|collection| collection.section_id == section as i64);
    let Ok(mut body) = plex_collection_container(
        &connection,
        &collections,
        &libraries,
        &state.machine_identifier,
    ) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    body["MediaContainer"]["librarySectionID"] = serde_json::json!(section);
    body["MediaContainer"]["librarySectionTitle"] = serde_json::json!(library.name);
    let stable_id = connection
        .query_row(
            "SELECT id FROM library_groups WHERE name = ?1",
            [&library.name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(section as i64);
    body["MediaContainer"]["librarySectionUUID"] = serde_json::json!(plex_library_uuid_for_id(
        &state.machine_identifier,
        stable_id
    ));
    body["MediaContainer"]["title1"] = serde_json::json!(library.name);
    body["MediaContainer"]["title2"] = serde_json::json!("Collections");
    body["MediaContainer"]["viewGroup"] = serde_json::json!(plex_kind(&library.kind));
    plex_json(body)
}

pub(crate) async fn plex_section_sorts(
    State(state): State<AppState>,
    Path(section): Path<usize>,
) -> Response {
    let Some(library) = state
        .config
        .lock()
        .ok()
        .and_then(|config| config.libraries.get(section.saturating_sub(1)).cloned())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": 1,
            "identifier": "com.plexapp.plugins.library",
            "librarySectionID": section,
            "librarySectionTitle": library.name,
            "librarySectionUUID": plex_library_uuid(&state, &library.name, section),
            "title1": library.name,
            "Directory": [{
                "key": "titleSort",
                "descKey": "titleSort:desc",
                "default": "asc",
                "defaultDirection": "asc",
                "firstCharacterKey": format!("/library/sections/{section}/firstCharacter"),
                "title": "Title"
            }]
        }
    }))
}

pub(crate) async fn plex_section_filters(
    State(state): State<AppState>,
    Path(section): Path<usize>,
) -> Response {
    let Some(library) = state
        .config
        .lock()
        .ok()
        .and_then(|config| config.libraries.get(section.saturating_sub(1)).cloned())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": 0,
            "identifier": "com.plexapp.plugins.library",
            "librarySectionID": section,
            "librarySectionTitle": library.name,
            "librarySectionUUID": plex_library_uuid(&state, &library.name, section),
            "title1": library.name,
            "viewGroup": "secondary",
            "Directory": []
        }
    }))
}

pub(crate) fn plex_first_character(title: &str) -> char {
    title
        .chars()
        .find(|character| character.is_alphanumeric())
        .map(|character| character.to_ascii_uppercase())
        .filter(char::is_ascii)
        .unwrap_or('#')
}

pub(crate) async fn plex_section_first_characters(
    State(state): State<AppState>,
    Path(section): Path<usize>,
) -> Response {
    let Some(library) = state
        .config
        .lock()
        .ok()
        .and_then(|config| config.libraries.get(section.saturating_sub(1)).cloned())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let metadata = plex_section_metadata(&state, section);
    let media_index = media_snapshot(&state);
    let mut initials = BTreeSet::new();
    if library.kind == "tv" {
        for show in plex_tv_show_groups(
            section,
            &library.folders,
            media_index
                .iter()
                .filter(|media| media_in_library(media, &library)),
        ) {
            let title = show
                .episodes
                .first()
                .and_then(|episode| metadata.get(&episode.id))
                .and_then(|item| item.get("name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&show.title);
            initials.insert(plex_first_character(title));
        }
    } else {
        initials.extend(
            media_index
                .iter()
                .filter(|media| media_in_library(media, &library))
                .map(|media| plex_first_character(&media.title)),
        );
    }
    let directory = initials
        .into_iter()
        .map(|letter| {
            serde_json::json!({
                "key": format!("/library/sections/{section}/firstCharacter/{letter}"),
                "title": letter.to_string(),
                "type": "firstCharacter"
            })
        })
        .collect::<Vec<_>>();
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": directory.len(),
            "identifier": "com.plexapp.plugins.library",
            "librarySectionID": section,
            "librarySectionTitle": library.name,
            "librarySectionUUID": plex_library_uuid(&state, &library.name, section),
            "title1": library.name,
            "viewGroup": "secondary",
            "Directory": directory
        }
    }))
}

pub(crate) async fn plex_section_first_character_items(
    State(state): State<AppState>,
    Path((section, letter)): Path<(usize, String)>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let Some(library) = state
        .config
        .lock()
        .ok()
        .and_then(|config| config.libraries.get(section.saturating_sub(1)).cloned())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let metadata = plex_section_metadata(&state, section);
    let media_index = media_snapshot(&state);
    let initial = letter
        .chars()
        .next()
        .map(|character| character.to_ascii_uppercase())
        .unwrap_or('#');
    let mut items = Vec::new();
    if library.kind == "tv" {
        for show in plex_tv_show_groups(
            section,
            &library.folders,
            media_index
                .iter()
                .filter(|media| media_in_library(media, &library)),
        ) {
            let show_metadata = show
                .episodes
                .first()
                .and_then(|episode| metadata.get(&episode.id));
            let title = show_metadata
                .and_then(|item| item.get("name"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&show.title);
            if plex_first_character(title) == initial {
                items.push(plex_tv_show_json(&show, section, &library, &metadata));
            }
        }
    } else {
        for media in media_index
            .iter()
            .filter(|media| media_in_library(media, &library))
        {
            if plex_first_character(&media.title) == initial {
                let mut item = plex_video_json(media, section, &library, metadata.get(&media.id));
                add_plex_technical_metadata(&state, &mut item, media).await;
                items.push(item);
            }
        }
    }
    items.sort_by(|left, right| {
        left["title"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase()
            .cmp(&right["title"].as_str().unwrap_or_default().to_lowercase())
    });
    let total = items.len();
    let (offset, items) = plex_paged_children(&items, &headers, &uri);
    let mut container = plex_library_media_container(
        &library,
        section,
        &plex_library_uuid(&state, &library.name, section),
        items.len(),
        offset,
        total,
    );
    container["Metadata"] = serde_json::json!(items);
    container["viewGroup"] = serde_json::json!(plex_kind(&library.kind));
    let mut response = plex_json(serde_json::json!({"MediaContainer": container}));
    response.headers_mut().insert(
        "X-Plex-Container-Start",
        HeaderValue::from_str(&offset.to_string()).expect("numeric header value"),
    );
    response.headers_mut().insert(
        "X-Plex-Container-Total-Size",
        HeaderValue::from_str(&total.to_string()).expect("numeric header value"),
    );
    response
}

pub(crate) async fn plex_library_all(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<BTreeMap<String, String>>,
    uri: Uri,
) -> Response {
    let Ok(config) = state.config.lock().map(|config| config.clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let type_filter = query.get("type").map(String::as_str);
    let include_type = |numeric: &str, name: &str, default: bool| {
        type_filter.map_or(default, |filter| {
            filter
                .split(',')
                .any(|value| value.trim() == numeric || value.trim() == name)
        })
    };
    let mut items = Vec::new();
    let media_index = media_snapshot(&state);
    for (index, library) in config.libraries.iter().enumerate() {
        let section = index + 1;
        let metadata = plex_section_metadata(&state, section);
        if library.kind == "tv" {
            let shows = plex_tv_show_groups(
                section,
                &library.folders,
                media_index
                    .iter()
                    .filter(|media| media_in_library(media, library)),
            );
            for show in shows {
                let show_guid = plex_tv_show_guid(&show, &metadata);
                if include_type("2", "show", true) {
                    items.push(plex_tv_show_json(&show, section, library, &metadata));
                }
                let seasons = plex_tv_season_groups(&show, &library.folders);
                if include_type("3", "season", false) {
                    for (season, episodes) in &seasons {
                        items.push(plex_tv_season_json(
                            &show,
                            *season,
                            section,
                            library,
                            &metadata,
                            episodes.len(),
                        ));
                    }
                }
                if include_type("4", "episode", false) {
                    for (season, episodes) in &seasons {
                        for (index, episode) in episodes.iter().enumerate() {
                            let episode_number = episode.episode.unwrap_or(index as u32 + 1);
                            let item = plex_tv_episode_json(
                                &state,
                                episode,
                                &show,
                                *season,
                                episode_number,
                                section,
                                library,
                                &metadata,
                                &show_guid,
                            )
                            .await;
                            items.push(item);
                        }
                    }
                }
            }
        } else if include_type("1", "movie", true) {
            for media in media_index
                .iter()
                .filter(|media| media_in_library(media, library))
            {
                let mut item = plex_video_json(media, section, library, metadata.get(&media.id));
                add_plex_technical_metadata(&state, &mut item, media).await;
                items.push(item);
            }
        }
    }

    if let Some(title_filter) = query.get("title") {
        let exact = title_filter.starts_with('=');
        let title_filter = title_filter
            .strip_prefix('=')
            .unwrap_or(title_filter)
            .to_lowercase();
        items.retain(|item| {
            item.get("title")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|title| {
                    let title = title.to_lowercase();
                    if exact {
                        title == title_filter
                    } else {
                        title.contains(&title_filter)
                    }
                })
        });
    }

    let requested_sort = query
        .get("sort")
        .map(|sort| sort.split(',').next().unwrap_or_default())
        .unwrap_or_default();
    let mut sort_parts = requested_sort.split(':');
    let sort_key = sort_parts.next().unwrap_or_default();
    let descending = sort_parts.next() == Some("desc");
    items.sort_by(|left, right| {
        let ordering = match sort_key {
            "duration" | "addedAt" | "rating" | "audienceRating" => left
                .get(sort_key)
                .and_then(serde_json::Value::as_f64)
                .unwrap_or_default()
                .partial_cmp(
                    &right
                        .get(sort_key)
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or_default(),
                )
                .unwrap_or(std::cmp::Ordering::Equal),
            _ => left
                .get("titleSort")
                .or_else(|| left.get("title"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_lowercase()
                .cmp(
                    &right
                        .get("titleSort")
                        .or_else(|| right.get("title"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_lowercase(),
                ),
        };
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });

    let total = items.len();
    let (offset, end) = plex_container_page(&headers, &uri)
        .map(|(requested_offset, requested_size)| {
            let offset = requested_offset.min(total);
            (offset, offset.saturating_add(requested_size).min(total))
        })
        .unwrap_or((0, total));
    let metadata = items
        .into_iter()
        .skip(offset)
        .take(end.saturating_sub(offset))
        .collect::<Vec<_>>();
    let count = metadata.len();
    let mut response = plex_json(serde_json::json!({
        "MediaContainer": {
            "size": count,
            "totalSize": total,
            "offset": offset,
            "allowSync": false,
            "identifier": "com.plexapp.plugins.library",
            "content": "secondary",
            "title1": "Library",
            "title2": "All",
            "Metadata": metadata
        }
    }));
    response.headers_mut().insert(
        "X-Plex-Container-Start",
        HeaderValue::from_str(&offset.to_string()).expect("numeric header value"),
    );
    response.headers_mut().insert(
        "X-Plex-Container-Total-Size",
        HeaderValue::from_str(&total.to_string()).expect("numeric header value"),
    );
    response
}

pub(crate) async fn plex_all(
    State(s): State<AppState>,
    Path(section): Path<usize>,
    headers: HeaderMap,
    Query(query): Query<BTreeMap<String, String>>,
    uri: Uri,
) -> Response {
    let Some(library) = s
        .config
        .lock()
        .ok()
        .and_then(|config| config.libraries.get(section.saturating_sub(1)).cloned())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if library.kind == "tv" {
        return plex_tv_all(&s, section, library, &headers, &query, &uri).await;
    }
    let media_index = media_snapshot(&s);
    let mut media = media_index
        .iter()
        .filter(|media| media_in_library(media, &library))
        .collect::<Vec<_>>();
    let metadata_by_id = plex_section_metadata(&s, section);
    let requested_sort = query.get("sort").map(String::as_str).unwrap_or_default();
    let sort_key = requested_sort
        .split(',')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default();
    let descending = requested_sort
        .split(',')
        .next()
        .is_some_and(|sort| sort.ends_with(":desc"));
    if requested_sort.is_empty() || sort_key == "titleSort" {
        media.sort_by(|left, right| {
            let ordering = left.title.to_lowercase().cmp(&right.title.to_lowercase());
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    } else if sort_key == "addedAt" {
        media.sort_by_key(|item| {
            let time = media_modified_at(item);
            if descending {
                std::cmp::Reverse(time)
            } else {
                std::cmp::Reverse(u64::MAX - time)
            }
        });
    } else if sort_key == "audienceRating" || sort_key == "rating" {
        media.sort_by(|left, right| {
            let left_rating = metadata_by_id
                .get(&left.id)
                .and_then(plex_rating)
                .unwrap_or_default();
            let right_rating = metadata_by_id
                .get(&right.id)
                .and_then(plex_rating)
                .unwrap_or_default();
            let ordering = left_rating
                .partial_cmp(&right_rating)
                .unwrap_or(std::cmp::Ordering::Equal);
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }
    let total = media.len();
    let uuid = plex_library_uuid(&s, &library.name, section);
    let (offset, end) = plex_container_page(&headers, &uri)
        .map(|(requested_offset, requested_size)| {
            let offset = requested_offset.min(total);
            (offset, offset.saturating_add(requested_size).min(total))
        })
        .unwrap_or((0, total));
    let mut metadata = Vec::with_capacity(end.saturating_sub(offset));
    for item in media.iter().skip(offset).take(end.saturating_sub(offset)) {
        let mut video = plex_video_json(item, section, &library, metadata_by_id.get(&item.id));
        add_plex_technical_metadata(&s, &mut video, item).await;
        metadata.push(video);
    }
    let count = metadata.len();
    let mut container =
        plex_library_media_container(&library, section, &uuid, count, offset, total);
    container["Metadata"] = serde_json::json!(metadata);
    let mut response = plex_json(serde_json::json!({"MediaContainer": container}));
    response.headers_mut().insert(
        "X-Plex-Container-Start",
        HeaderValue::from_str(&offset.to_string()).expect("numeric header value"),
    );
    response.headers_mut().insert(
        "X-Plex-Container-Total-Size",
        HeaderValue::from_str(&total.to_string()).expect("numeric header value"),
    );
    response
}

pub(crate) async fn plex_tv_all(
    state: &AppState,
    section: usize,
    library: ConfigLibrary,
    headers: &HeaderMap,
    query: &BTreeMap<String, String>,
    uri: &Uri,
) -> Response {
    let metadata = plex_section_metadata(state, section);
    let media_index = media_snapshot(state);
    let mut shows = plex_tv_show_groups(
        section,
        &library.folders,
        media_index
            .iter()
            .filter(|media| media_in_library(media, &library)),
    );
    let requested_sort = query.get("sort").map(String::as_str).unwrap_or_default();
    let descending = requested_sort.ends_with(":desc");
    shows.sort_by(|left, right| {
        let ordering = left.title.to_lowercase().cmp(&right.title.to_lowercase());
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
    let total = shows.len();
    let (offset, end) = plex_container_page(headers, uri)
        .map(|(requested_offset, requested_size)| {
            let offset = requested_offset.min(total);
            (offset, offset.saturating_add(requested_size).min(total))
        })
        .unwrap_or((0, total));
    let items = shows
        .iter()
        .skip(offset)
        .take(end.saturating_sub(offset))
        .map(|show| plex_tv_show_json(show, section, &library, &metadata))
        .collect::<Vec<_>>();
    let mut container = plex_library_media_container(
        &library,
        section,
        &plex_library_uuid(state, &library.name, section),
        items.len(),
        offset,
        total,
    );
    container["Metadata"] = serde_json::json!(items);
    let mut response = plex_json(serde_json::json!({"MediaContainer": container}));
    response.headers_mut().insert(
        "X-Plex-Container-Start",
        HeaderValue::from_str(&offset.to_string()).expect("numeric header value"),
    );
    response.headers_mut().insert(
        "X-Plex-Container-Total-Size",
        HeaderValue::from_str(&total.to_string()).expect("numeric header value"),
    );
    response
}

pub(crate) fn plex_tv_season_groups<'a>(
    show: &PlexTvShowGroup<'a>,
    roots: &[PathBuf],
) -> BTreeMap<u32, Vec<PlexTvEpisodeGroup<'a>>> {
    let mut episode_groups = BTreeMap::<(u32, u32, String), Vec<&Media>>::new();
    for media in &show.episodes {
        let episode_info = roots
            .iter()
            .find(|root| media.path.starts_with(root))
            .and_then(|root| tv_episode_info(root, &media.path));
        let key = episode_info
            .map(|episode| (episode.season, episode.episode, String::new()))
            .unwrap_or_else(|| (1, u32::MAX, media.id.clone()));
        episode_groups.entry(key).or_default().push(media);
    }
    let mut seasons = BTreeMap::<u32, Vec<PlexTvEpisodeGroup>>::new();
    for ((season, episode, _), mut files) in episode_groups {
        files.sort_by_key(|media| media.id.clone());
        seasons.entry(season).or_default().push(PlexTvEpisodeGroup {
            episode: (episode != u32::MAX).then_some(episode),
            files,
        });
    }
    seasons
}

pub(crate) fn plex_tv_show_json(
    show: &PlexTvShowGroup<'_>,
    section: usize,
    library: &ConfigLibrary,
    metadata: &HashMap<String, serde_json::Value>,
) -> serde_json::Value {
    let Some(representative) = show.episodes.first() else {
        return serde_json::Value::Null;
    };
    let item_metadata = metadata.get(&representative.id);
    let mut item = plex_video_json(representative, section, library, item_metadata);
    let title = item_metadata
        .and_then(|metadata| metadata.get("name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&show.title);
    item["ratingKey"] = serde_json::json!(show.rating_key);
    item["key"] = serde_json::json!(format!("/library/metadata/{}/children", show.rating_key));
    item["type"] = serde_json::json!("show");
    item["title"] = serde_json::json!(title);
    item["titleSort"] = serde_json::json!(title.to_lowercase());
    item["originalTitle"] = serde_json::json!(title);
    let episode_groups = plex_tv_season_groups(show, &library.folders);
    item["childCount"] = serde_json::json!(episode_groups.len());
    let episode_groups = episode_groups.values().flatten().collect::<Vec<_>>();
    let leaf_count = episode_groups.len();
    item["leafCount"] = serde_json::json!(leaf_count);
    let viewed_episodes = episode_groups
        .iter()
        .filter(|group| {
            group.files.iter().any(|episode| {
                metadata
                    .get(&episode.id)
                    .and_then(|metadata| metadata.get("mplex_view_count"))
                    .and_then(serde_json::Value::as_i64)
                    .is_some_and(|view_count| view_count > 0)
            })
        })
        .count();
    item["viewedLeafCount"] = serde_json::json!(viewed_episodes);
    item["guid"] = serde_json::json!(plex_tv_show_guid(show, metadata));
    if let Some(object) = item.as_object_mut() {
        object.remove("Media");
        object.remove("duration");
        object.remove("viewOffset");
        object.remove("viewCount");
        object.remove("lastViewedAt");
        object.remove("parentIndex");
        object.remove("parentTitle");
        object.remove("grandparentTitle");
    }
    item
}

pub(crate) async fn plex_tv_episode_json(
    state: &AppState,
    group: &PlexTvEpisodeGroup<'_>,
    show: &PlexTvShowGroup<'_>,
    season: u32,
    episode_number: u32,
    section: usize,
    library: &ConfigLibrary,
    metadata: &HashMap<String, serde_json::Value>,
    show_guid: &str,
) -> serde_json::Value {
    let mut episode_item = None;
    let mut media_items = Vec::with_capacity(group.files.len());
    for file in &group.files {
        let mut file_item = plex_video_json(file, section, library, metadata.get(&file.id));
        add_plex_technical_metadata(state, &mut file_item, file).await;
        if episode_item.is_none() {
            episode_item = Some(file_item.clone());
        }
        if let Some(media) = file_item
            .get("Media")
            .and_then(serde_json::Value::as_array)
            .and_then(|media| media.first())
        {
            media_items.push(media.clone());
        }
    }
    let Some(mut item) = episode_item else {
        return serde_json::Value::Null;
    };
    item["Media"] = serde_json::json!(media_items);
    plex_tv_episode_parents(&mut item, show, season, episode_number, show_guid);
    if let Some(show_title) = group
        .files
        .first()
        .and_then(|file| metadata.get(&file.id))
        .and_then(|metadata| metadata.get("name"))
        .and_then(serde_json::Value::as_str)
    {
        item["grandparentTitle"] = serde_json::json!(show_title);
    }
    item
}

pub(crate) async fn plex_tv_episode_json_for_media(
    state: &AppState,
    media_items: &[Media],
    media: &Media,
    section: usize,
    library: &ConfigLibrary,
    metadata: &HashMap<String, serde_json::Value>,
) -> Option<serde_json::Value> {
    if library.kind != "tv" {
        return None;
    }
    let shows = plex_tv_show_groups(
        section,
        &library.folders,
        media_items
            .iter()
            .filter(|candidate| media_in_library(candidate, library)),
    );
    let show = shows
        .iter()
        .find(|show| show.episodes.iter().any(|episode| episode.id == media.id))?;
    let show_guid = plex_tv_show_guid(show, metadata);
    let seasons = plex_tv_season_groups(show, &library.folders);
    let (season, index, episode) = seasons.iter().find_map(|(season, episodes)| {
        episodes
            .iter()
            .enumerate()
            .find(|(_, episode)| episode.files.iter().any(|file| file.id == media.id))
            .map(|(index, episode)| (*season, index, episode))
    })?;
    Some(
        plex_tv_episode_json(
            state,
            episode,
            show,
            season,
            episode.episode.unwrap_or(index as u32 + 1),
            section,
            library,
            metadata,
            &show_guid,
        )
        .await,
    )
}

pub(crate) fn plex_tv_season_json(
    show: &PlexTvShowGroup<'_>,
    season: u32,
    section: usize,
    library: &ConfigLibrary,
    metadata: &HashMap<String, serde_json::Value>,
    episode_count: usize,
) -> serde_json::Value {
    let season_key = plex_tv_season_key(&show.rating_key, season);
    let representative = show
        .episodes
        .iter()
        .find(|media| {
            library
                .folders
                .iter()
                .find(|root| media.path.starts_with(root))
                .and_then(|root| tv_episode_info(root, &media.path))
                .map(|episode| episode.season == season)
                .unwrap_or(season == 1)
        })
        .or_else(|| show.episodes.first());
    let mut item = representative
        .map(|media| plex_video_json(media, section, library, metadata.get(&media.id)))
        .unwrap_or_else(|| serde_json::json!({}));
    let title = if season == 0 {
        "Specials".to_owned()
    } else {
        format!("Season {season}")
    };
    item["ratingKey"] = serde_json::json!(season_key);
    item["key"] = serde_json::json!(format!("/library/metadata/{season_key}/children"));
    item["guid"] = serde_json::json!(plex_metadata_guid("season", &season_key));
    item["parentGuid"] = serde_json::json!(plex_tv_show_guid(show, metadata));
    item["type"] = serde_json::json!("season");
    item["title"] = serde_json::json!(title);
    item["titleSort"] = serde_json::json!(title.to_lowercase());
    item["index"] = serde_json::json!(season);
    item["parentRatingKey"] = serde_json::json!(show.rating_key);
    item["parentKey"] = serde_json::json!(format!("/library/metadata/{}", show.rating_key));
    item["parentType"] = serde_json::json!("show");
    item["parentTitle"] = serde_json::json!(show.title);
    item["parentIndex"] = serde_json::json!(season);
    item["childCount"] = serde_json::json!(episode_count);
    item["leafCount"] = serde_json::json!(episode_count);
    if let Some(object) = item.as_object_mut() {
        object.remove("Media");
        object.remove("duration");
        object.remove("viewOffset");
        object.remove("viewCount");
        object.remove("lastViewedAt");
        object.remove("grandparentTitle");
        object.remove("grandparentRatingKey");
    }
    item
}

pub(crate) fn plex_tv_episode_parents(
    item: &mut serde_json::Value,
    show: &PlexTvShowGroup<'_>,
    season: u32,
    episode: u32,
    show_guid: &str,
) {
    let season_key = plex_tv_season_key(&show.rating_key, season);
    item["parentGuid"] = serde_json::json!(plex_metadata_guid("season", &season_key));
    item["grandparentGuid"] = serde_json::json!(show_guid);
    item["grandparentRatingKey"] = serde_json::json!(show.rating_key);
    item["grandparentKey"] = serde_json::json!(format!("/library/metadata/{}", show.rating_key));
    item["grandparentTitle"] = serde_json::json!(show.title);
    item["grandparentType"] = serde_json::json!("show");
    item["parentRatingKey"] = serde_json::json!(season_key);
    item["parentKey"] = serde_json::json!(format!("/library/metadata/{season_key}"));
    item["parentTitle"] = serde_json::json!(if season == 0 {
        "Specials".to_owned()
    } else {
        format!("Season {season}")
    });
    item["parentType"] = serde_json::json!("season");
    item["parentIndex"] = serde_json::json!(season);
    item["index"] = serde_json::json!(episode);
    if let Some(representative) = show.episodes.first() {
        let representative_key = plex_rating_key(representative);
        item["grandparentThumb"] =
            serde_json::json!(format!("/library/metadata/{representative_key}/thumb"));
        item["grandparentArt"] =
            serde_json::json!(format!("/library/metadata/{representative_key}/art"));
    }
}

pub(crate) fn plex_library_media_container(
    library: &ConfigLibrary,
    section: usize,
    library_uuid: &str,
    size: usize,
    offset: usize,
    total_size: usize,
) -> serde_json::Value {
    let view_group = plex_kind(&library.kind);
    serde_json::json!({
        "size": size,
        "totalSize": total_size,
        "offset": offset,
        "allowSync": false,
        "identifier": "com.plexapp.plugins.library",
        "content": "secondary",
        "librarySectionID": section,
        "librarySectionTitle": library.name,
        "librarySectionUUID": library_uuid,
        "title1": library.name,
        "title2": format!("All {}", library.name),
        "viewGroup": view_group
    })
}

pub(crate) fn plex_library_children_container(
    library: &ConfigLibrary,
    section: usize,
    library_uuid: &str,
    size: usize,
    offset: usize,
    total_size: usize,
    view_group: &str,
) -> serde_json::Value {
    let mut container =
        plex_library_media_container(library, section, library_uuid, size, offset, total_size);
    container["viewGroup"] = serde_json::json!(view_group);
    container
}

pub(crate) fn plex_container_page(headers: &HeaderMap, uri: &Uri) -> Option<(usize, usize)> {
    fn value(headers: &HeaderMap, uri: &Uri, name: &'static str) -> Option<usize> {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .or_else(|| plex_query_value(uri, name))
            .and_then(|value| value.parse().ok())
    }

    Some((
        value(headers, uri, "X-Plex-Container-Start")?,
        value(headers, uri, "X-Plex-Container-Size")?,
    ))
}

pub(crate) fn plex_paged_children<T: Clone>(
    items: &[T],
    headers: &HeaderMap,
    uri: &Uri,
) -> (usize, Vec<T>) {
    let total = items.len();
    let (offset, size) = plex_container_page(headers, uri).unwrap_or((0, 20));
    let offset = offset.min(total);
    let end = offset.saturating_add(size).min(total);
    (offset, items[offset..end].to_vec())
}

pub(crate) fn plex_children_json(
    items: Vec<serde_json::Value>,
    total: usize,
    offset: usize,
) -> serde_json::Value {
    serde_json::json!({
        "size": items.len(),
        "totalSize": total,
        "offset": offset,
        "Metadata": items
    })
}

pub(crate) async fn plex_item(
    State(s): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let wants_children = uri.path().ends_with("/children");
    let include_children = plex_query_value(&uri, "includeChildren") == Some("1");
    let include_markers = marker_include_requested(&uri);
    let media_index = media_snapshot(&s);
    if let Some((section, library, mut show, season)) =
        find_plex_tv_location(&s, media_index.as_ref(), &id)
    {
        let metadata = plex_section_metadata(&s, section);
        if let Some(show_title) = show
            .episodes
            .first()
            .and_then(|episode| metadata.get(&episode.id))
            .and_then(|metadata| metadata.get("name"))
            .and_then(serde_json::Value::as_str)
        {
            show.title = show_title.to_owned();
        }
        let show_guid = plex_tv_show_guid(&show, &metadata);
        let uuid = plex_library_uuid(&s, &library.name, section);
        if let Some(season) = season {
            let season_episodes = plex_tv_season_groups(&show, &library.folders)
                .remove(&season)
                .unwrap_or_default();
            let mut episodes = Vec::with_capacity(season_episodes.len());
            for (index, episode) in season_episodes.iter().enumerate() {
                let episode_number = episode.episode.unwrap_or(index as u32 + 1);
                let mut item = plex_tv_episode_json(
                    &s,
                    episode,
                    &show,
                    season,
                    episode_number,
                    section,
                    &library,
                    &metadata,
                    &show_guid,
                )
                .await;
                if let Some(file) = episode.files.first() {
                    attach_markers_to_item(&s, &mut item, &file.id, include_markers);
                }
                episodes.push(item);
            }
            if wants_children {
                let (offset, episodes) = plex_paged_children(&episodes, &headers, &uri);
                let mut container = plex_library_children_container(
                    &library,
                    section,
                    &uuid,
                    episodes.len(),
                    offset,
                    season_episodes.len(),
                    "episode",
                );
                container["Metadata"] = serde_json::json!(episodes);
                container["offset"] = serde_json::json!(offset);
                container["title2"] = serde_json::json!(if season == 0 {
                    "Specials".to_owned()
                } else {
                    format!("Season {season}")
                });
                return plex_json(serde_json::json!({"MediaContainer": container}));
            }
            let mut item = plex_tv_season_json(
                &show,
                season,
                section,
                &library,
                &metadata,
                season_episodes.len(),
            );
            if include_children {
                let (offset, episodes) = plex_paged_children(&episodes, &headers, &uri);
                item["Children"] = plex_children_json(episodes, season_episodes.len(), offset);
            }
            let mut container = plex_library_media_container(&library, section, &uuid, 1, 0, 1);
            container["Metadata"] = serde_json::json!([item]);
            return plex_json(serde_json::json!({"MediaContainer": container}));
        }
        let seasons = plex_tv_season_groups(&show, &library.folders);
        let season_items = seasons
            .iter()
            .map(|(season, episodes)| {
                plex_tv_season_json(&show, *season, section, &library, &metadata, episodes.len())
            })
            .collect::<Vec<_>>();
        if wants_children {
            let (offset, items) = plex_paged_children(&season_items, &headers, &uri);
            let mut container = plex_library_children_container(
                &library,
                section,
                &uuid,
                items.len(),
                offset,
                season_items.len(),
                "season",
            );
            container["Metadata"] = serde_json::json!(items);
            container["offset"] = serde_json::json!(offset);
            container["title2"] = serde_json::json!(show.title);
            return plex_json(serde_json::json!({"MediaContainer": container}));
        }
        let mut item = plex_tv_show_json(&show, section, &library, &metadata);
        if include_children {
            let (offset, items) = plex_paged_children(&season_items, &headers, &uri);
            item["Children"] = plex_children_json(items, season_items.len(), offset);
        }
        let mut container = plex_library_media_container(&library, section, &uuid, 1, 0, 1);
        container["Metadata"] = serde_json::json!([item]);
        return plex_json(serde_json::json!({"MediaContainer": container}));
    }
    let Some(media) = find_media(&s, &id) else {
        let media_key = id
            .split_once('-')
            .filter(|(section, item)| {
                section.parse::<usize>().is_ok() && item.parse::<u64>().is_ok()
            })
            .map(|_| id.clone())
            .or_else(|| {
                id.parse::<u64>().ok().and_then(|rating_key| {
                    let section = rating_key >> 20;
                    let item = rating_key & ((1 << 20) - 1);
                    (section > 0 && item > 0).then(|| format!("{section}-{item}"))
                })
            });
        let known_stale_key = media_key.as_ref().is_some_and(|key| {
            s.db.lock()
                .ok()
                .and_then(|connection| {
                    connection
                        .query_row(
                            "SELECT EXISTS(SELECT 1 FROM media_identity WHERE media_key = ?1)",
                            [key],
                            |row| row.get::<_, bool>(0),
                        )
                        .ok()
                })
                .unwrap_or(false)
        });
        let section = media_key
            .as_deref()
            .and_then(|key| key.split_once('-'))
            .and_then(|(section, _)| section.parse::<usize>().ok())
            .filter(|section| {
                *section > 0
                    && s.config
                        .lock()
                        .map(|config| *section <= config.libraries.len())
                        .unwrap_or(false)
            });
        if known_stale_key {
            if let Some(section) = section {
                if scan_library_inventory(&s, section - 1, false).await.is_ok() {
                    if let Ok(config) = s.config.lock().map(|config| config.clone()) {
                        if let Some(library) = config.libraries.get(section - 1) {
                            let mut container = plex_library_media_container(
                                library,
                                section,
                                &plex_library_uuid(&s, &library.name, section),
                                0,
                                0,
                                0,
                            );
                            container["Metadata"] = serde_json::json!([]);
                            return plex_json(serde_json::json!({"MediaContainer": container}));
                        }
                    }
                }
            }
        }
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((section, library)) = media_library(&s, &media) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let metadata_by_id = plex_section_metadata(&s, section);
    let mut video = if let Some(video) = plex_tv_episode_json_for_media(
        &s,
        media_index.as_ref(),
        &media,
        section,
        &library,
        &metadata_by_id,
    )
    .await
    {
        video
    } else {
        let mut video = plex_video_json(&media, section, &library, metadata_by_id.get(&media.id));
        add_plex_technical_metadata(&s, &mut video, &media).await;
        video
    };
    attach_markers_to_item(&s, &mut video, &media.id, include_markers);
    // Plex for Mac refetches the selected item after creating the queue and
    // bases its playability check on this metadata response as well.
    plex_mark_direct_play(&mut video);
    let mut container = plex_library_media_container(
        &library,
        section,
        &plex_library_uuid(&s, &library.name, section),
        1,
        0,
        1,
    );
    plex_mark_direct_play_decision(&mut container);
    container["Metadata"] = serde_json::json!([video]);
    plex_json(serde_json::json!({
        "MediaContainer": container
    }))
}

pub(crate) fn plex_empty_extras_response(
    library: &ConfigLibrary,
    section: usize,
    library_uuid: &str,
) -> serde_json::Value {
    let mut container = plex_library_media_container(library, section, library_uuid, 0, 0, 0);
    container["title2"] = serde_json::json!("Extras");
    container["Metadata"] = serde_json::json!([]);
    serde_json::json!({"MediaContainer": container})
}

pub(crate) async fn plex_extras(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let Some((section, library)) = plex_library_for_metadata(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    plex_json(plex_empty_extras_response(
        &library,
        section,
        &plex_library_uuid(&state, &library.name, section),
    ))
}

pub(crate) async fn plex_part(
    State(s): State<AppState>,
    Path((id, _part)): Path<(String, String)>,
) -> Response {
    plex_item(
        State(s),
        Path(id),
        HeaderMap::new(),
        Uri::from_static("/library/metadata/item"),
    )
    .await
}

pub(crate) fn plex_rating_key_from_source(source: &str) -> Option<String> {
    let source = source.split('?').next().unwrap_or(source);
    let id = source
        .rsplit_once("/library/metadata/")?
        .1
        .split('/')
        .next()?;
    (!id.is_empty()).then(|| id.to_owned())
}

#[derive(Clone)]
pub(crate) struct PlexPlaylistRecord {
    pub(crate) id: i64,
    pub(crate) title: String,
    pub(crate) playlist_type: String,
    pub(crate) smart: bool,
    pub(crate) added_at: i64,
    pub(crate) updated_at: i64,
}

#[derive(Clone)]
pub(crate) struct PlexCollectionRecord {
    pub(crate) id: i64,
    pub(crate) title: String,
    pub(crate) section_id: i64,
    pub(crate) added_at: i64,
    pub(crate) updated_at: i64,
}

pub(crate) fn plex_collection_record(
    connection: &Connection,
    id: i64,
) -> rusqlite::Result<PlexCollectionRecord> {
    connection.query_row(
        "SELECT id, title, section_id, added_at, updated_at FROM collections WHERE id = ?1",
        [id],
        |row| {
            Ok(PlexCollectionRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                section_id: row.get(2)?,
                added_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        },
    )
}

pub(crate) fn plex_collection_records(
    connection: &Connection,
) -> rusqlite::Result<Vec<PlexCollectionRecord>> {
    let mut statement = connection.prepare(
        "SELECT id, title, section_id, added_at, updated_at FROM collections ORDER BY title COLLATE NOCASE, id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(PlexCollectionRecord {
            id: row.get(0)?,
            title: row.get(1)?,
            section_id: row.get(2)?,
            added_at: row.get(3)?,
            updated_at: row.get(4)?,
        })
    })?;
    rows.collect()
}

pub(crate) fn plex_collection_summary(
    connection: &Connection,
    collection: &PlexCollectionRecord,
    libraries: &[ConfigLibrary],
    machine_identifier: &str,
) -> rusqlite::Result<serde_json::Value> {
    let item_count = connection.query_row(
        "SELECT COUNT(*) FROM collection_items WHERE collection_id = ?1",
        [collection.id],
        |row| row.get::<_, i64>(0),
    )?;
    let mut summary = serde_json::json!({
        "ratingKey": collection.id.to_string(),
        "key": format!("/library/collections/{}/items", collection.id),
        "type": "collection",
        "title": collection.title,
        "titleSort": collection.title.to_lowercase(),
        "summary": "",
        "smart": false,
        "childCount": item_count,
        "leafCount": item_count,
        "addedAt": collection.added_at,
        "updatedAt": collection.updated_at
    });
    if let Ok(section) = usize::try_from(collection.section_id) {
        if let Some(library) = section
            .checked_sub(1)
            .and_then(|index| libraries.get(index))
        {
            summary["librarySectionID"] = serde_json::json!(section);
            summary["librarySectionTitle"] = serde_json::json!(library.name);
            let stable_id = connection
                .query_row(
                    "SELECT id FROM library_groups WHERE name = ?1",
                    [&library.name],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(section as i64);
            summary["librarySectionUUID"] =
                serde_json::json!(plex_library_uuid_for_id(machine_identifier, stable_id));
            summary["librarySectionKey"] =
                serde_json::json!(format!("/library/sections/{section}"));
        }
    }
    Ok(summary)
}

pub(crate) fn plex_collection_container(
    connection: &Connection,
    collections: &[PlexCollectionRecord],
    libraries: &[ConfigLibrary],
    machine_identifier: &str,
) -> rusqlite::Result<serde_json::Value> {
    let metadata = collections
        .iter()
        .map(|collection| {
            plex_collection_summary(connection, collection, libraries, machine_identifier)
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(serde_json::json!({
        "MediaContainer": {
            "size": metadata.len(),
            "totalSize": metadata.len(),
            "offset": 0,
            "identifier": "com.plexapp.plugins.library",
            "Metadata": metadata
        }
    }))
}

pub(crate) async fn plex_collections(
    State(state): State<AppState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let libraries = match state.config.lock() {
        Ok(config) => config.libraries.clone(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let requested_section = match query.get("sectionId") {
        Some(value) => match value.parse::<usize>() {
            Ok(section) if section > 0 && section <= libraries.len() => Some(section as i64),
            Ok(_) => return StatusCode::NOT_FOUND.into_response(),
            Err(_) => return StatusCode::BAD_REQUEST.into_response(),
        },
        None => None,
    };
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(mut collections) = plex_collection_records(&connection) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if let Some(section) = requested_section {
        collections.retain(|collection| collection.section_id == section);
    }
    match plex_collection_container(
        &connection,
        &collections,
        &libraries,
        &state.machine_identifier,
    ) {
        Ok(body) => plex_json(body),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn plex_create_collection(
    State(state): State<AppState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Some(title) = query
        .get("title")
        .map(|title| title.trim())
        .filter(|title| !title.is_empty())
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if query
        .get("smart")
        .is_some_and(|smart| !matches!(smart.as_str(), "0" | "false"))
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let libraries = match state.config.lock() {
        Ok(config) => config.libraries.clone(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let section = match query.get("sectionId") {
        Some(value) => match value.parse::<usize>() {
            Ok(section) if section > 0 && section <= libraries.len() => section,
            Ok(_) => return StatusCode::NOT_FOUND.into_response(),
            Err(_) => return StatusCode::BAD_REQUEST.into_response(),
        },
        None if !libraries.is_empty() => 1,
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if connection
        .execute(
            "INSERT INTO collections (title, section_id, added_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            (title, section as i64, updated_at),
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let Ok(collection) = plex_collection_record(&connection, connection.last_insert_rowid()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(body) = plex_collection_container(
        &connection,
        &[collection],
        &libraries,
        &state.machine_identifier,
    ) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut response = plex_json(body);
    *response.status_mut() = StatusCode::CREATED;
    response
}

pub(crate) async fn plex_collection_items(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    let entries = {
        let Ok(connection) = state.db.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        if plex_collection_record(&connection, id).is_err() {
            return StatusCode::NOT_FOUND.into_response();
        }
        let mut statement = match connection.prepare(
            "SELECT id, media_key FROM collection_items WHERE collection_id = ?1 ORDER BY position, id",
        ) {
            Ok(statement) => statement,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        let rows = match statement.query_map([id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        }) {
            Ok(rows) => rows,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        match rows.collect::<rusqlite::Result<Vec<_>>>() {
            Ok(entries) => entries,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    };
    let mut metadata = Vec::new();
    for (item_id, media_key) in entries {
        if let Some(mut item) = plex_metadata_item_json(&state, &media_key).await {
            item["collectionItemID"] = serde_json::json!(item_id);
            metadata.push(item);
        }
    }
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": metadata.len(),
            "totalSize": metadata.len(),
            "offset": 0,
            "identifier": "com.plexapp.plugins.library",
            "Metadata": metadata
        }
    }))
}

pub(crate) async fn plex_add_collection_item(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let collection = {
        let Ok(connection) = state.db.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        match plex_collection_record(&connection, id) {
            Ok(collection) => collection,
            Err(_) => return StatusCode::NOT_FOUND.into_response(),
        }
    };
    let Some(source) = query.get("uri").or_else(|| query.get("key")) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(media_id) = plex_rating_key_from_source(source)
        .or_else(|| source.parse::<u64>().ok().map(|key| key.to_string()))
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(media) = find_media(&state, &media_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((section, _)) = media_library(&state, &media) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if collection.section_id != section as i64 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let libraries = match state.config.lock() {
        Ok(config) => config.libraries.clone(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let media_key = plex_rating_key(&media).to_string();
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let next_position = match connection.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM collection_items WHERE collection_id = ?1",
        [id],
        |row| row.get::<_, i64>(0),
    ) {
        Ok(position) => position,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if connection
        .execute(
            "INSERT OR IGNORE INTO collection_items (collection_id, media_key, position, added_at) VALUES (?1, ?2, ?3, ?4)",
            (id, media_key, next_position, updated_at),
        )
        .and_then(|_| {
            connection.execute(
                "UPDATE collections SET updated_at = ?1 WHERE id = ?2",
                (updated_at, id),
            )
        })
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let Ok(collection) = plex_collection_record(&connection, id) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match plex_collection_container(
        &connection,
        &[collection],
        &libraries,
        &state.machine_identifier,
    ) {
        Ok(body) => plex_json(body),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn plex_remove_collection_item(
    State(state): State<AppState>,
    Path((id, item_id)): Path<(i64, String)>,
) -> Response {
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if plex_collection_record(&connection, id).is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let media_key = find_media(&state, &item_id)
        .map(|media| plex_rating_key(&media).to_string())
        .unwrap_or_else(|| item_id.clone());
    let mut result = connection.execute(
        "DELETE FROM collection_items WHERE collection_id = ?1 AND media_key = ?2",
        (id, &media_key),
    );
    if matches!(result, Ok(0)) {
        if let Ok(item_id) = item_id.parse::<i64>() {
            result = connection.execute(
                "DELETE FROM collection_items WHERE collection_id = ?1 AND id = ?2",
                (id, item_id),
            );
        }
    }
    match result {
        Ok(0) => StatusCode::NOT_FOUND.into_response(),
        Ok(_) => {
            let updated_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or_default();
            if connection
                .execute(
                    "UPDATE collections SET updated_at = ?1 WHERE id = ?2",
                    (updated_at, id),
                )
                .is_err()
            {
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            } else {
                StatusCode::OK.into_response()
            }
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) fn plex_playlist_record(
    connection: &Connection,
    id: i64,
) -> rusqlite::Result<PlexPlaylistRecord> {
    connection.query_row(
        "SELECT id, title, playlist_type, smart, added_at, updated_at FROM playlists WHERE id = ?1",
        [id],
        |row| {
            Ok(PlexPlaylistRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                playlist_type: row.get(2)?,
                smart: row.get(3)?,
                added_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        },
    )
}

pub(crate) fn plex_playlist_records(
    connection: &Connection,
) -> rusqlite::Result<Vec<PlexPlaylistRecord>> {
    let mut statement = connection.prepare(
        "SELECT id, title, playlist_type, smart, added_at, updated_at FROM playlists ORDER BY title COLLATE NOCASE, id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(PlexPlaylistRecord {
            id: row.get(0)?,
            title: row.get(1)?,
            playlist_type: row.get(2)?,
            smart: row.get(3)?,
            added_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub(crate) fn plex_playlist_summary(
    connection: &Connection,
    playlist: &PlexPlaylistRecord,
) -> rusqlite::Result<serde_json::Value> {
    let leaf_count = connection.query_row(
        "SELECT COUNT(*) FROM playlist_items WHERE playlist_id = ?1",
        [playlist.id],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(serde_json::json!({
        "ratingKey": playlist.id.to_string(),
        "key": format!("/playlists/{}", playlist.id),
        "type": "playlist",
        "title": playlist.title,
        "titleSort": playlist.title.to_lowercase(),
        "playlistType": playlist.playlist_type,
        "smart": playlist.smart,
        "leafCount": leaf_count,
        "childCount": leaf_count,
        "addedAt": playlist.added_at,
        "updatedAt": playlist.updated_at
    }))
}

pub(crate) fn plex_playlist_container(
    connection: &Connection,
    playlists: &[PlexPlaylistRecord],
) -> rusqlite::Result<serde_json::Value> {
    let metadata = playlists
        .iter()
        .map(|playlist| plex_playlist_summary(connection, playlist))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(serde_json::json!({
        "MediaContainer": {
            "size": metadata.len(),
            "totalSize": metadata.len(),
            "identifier": "com.plexapp.plugins.library",
            "Metadata": metadata
        }
    }))
}

pub(crate) async fn plex_playlists(
    State(state): State<AppState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(mut playlists) = plex_playlist_records(&connection) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if let Some(playlist_type) = query.get("playlistType") {
        playlists.retain(|playlist| &playlist.playlist_type == playlist_type);
    }
    if let Some(smart) = query.get("smart") {
        let wants_smart = matches!(smart.as_str(), "1" | "true");
        playlists.retain(|playlist| playlist.smart == wants_smart);
    }
    match plex_playlist_container(&connection, &playlists) {
        Ok(body) => plex_json(body),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn plex_create_playlist(
    State(state): State<AppState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Some(title) = query
        .get("title")
        .map(|title| title.trim())
        .filter(|title| !title.is_empty())
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let playlist_type = query
        .get("playlistType")
        .or_else(|| query.get("type"))
        .map(String::as_str)
        .unwrap_or("video");
    if playlist_type != "video"
        || query
            .get("smart")
            .is_some_and(|smart| !matches!(smart.as_str(), "0" | "false"))
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if connection
        .execute(
            "INSERT INTO playlists (title, playlist_type, smart, added_at, updated_at) VALUES (?1, 'video', 0, ?2, ?2)",
            (title, updated_at),
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let playlist_id = connection.last_insert_rowid();
    let Ok(playlist) = plex_playlist_record(&connection, playlist_id) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(body) = plex_playlist_container(&connection, &[playlist]) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut response = plex_json(body);
    *response.status_mut() = StatusCode::CREATED;
    response
}

pub(crate) async fn plex_get_playlist(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Ok(playlist) = plex_playlist_record(&connection, id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match plex_playlist_container(&connection, &[playlist]) {
        Ok(body) => plex_json(body),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn plex_update_playlist(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if plex_playlist_record(&connection, id).is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(title) = query
        .get("title")
        .map(|title| title.trim())
        .filter(|title| !title.is_empty())
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    if connection
        .execute(
            "UPDATE playlists SET title = ?1, updated_at = ?2 WHERE id = ?3",
            (title, updated_at, id),
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let Ok(playlist) = plex_playlist_record(&connection, id) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match plex_playlist_container(&connection, &[playlist]) {
        Ok(body) => plex_json(body),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn plex_delete_playlist(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if plex_playlist_record(&connection, id).is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if connection
        .execute("DELETE FROM playlist_items WHERE playlist_id = ?1", [id])
        .and_then(|_| connection.execute("DELETE FROM playlists WHERE id = ?1", [id]))
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

pub(crate) async fn plex_metadata_item_json(
    state: &AppState,
    rating_key: &str,
) -> Option<serde_json::Value> {
    let uri = Uri::try_from(format!("/library/metadata/{rating_key}")).ok()?;
    let response = plex_item(
        State(state.clone()),
        Path(rating_key.to_owned()),
        HeaderMap::new(),
        uri,
    )
    .await;
    if response.status() != StatusCode::OK {
        return None;
    }
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .ok()?;
    serde_json::from_slice::<serde_json::Value>(&body)
        .ok()?
        .pointer("/MediaContainer/Metadata/0")
        .cloned()
}

pub(crate) async fn plex_playlist_items(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    let entries = {
        let Ok(connection) = state.db.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        if plex_playlist_record(&connection, id).is_err() {
            return StatusCode::NOT_FOUND.into_response();
        }
        let mut statement = match connection.prepare(
            "SELECT id, media_key FROM playlist_items WHERE playlist_id = ?1 ORDER BY position, id",
        ) {
            Ok(statement) => statement,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        let rows = match statement.query_map([id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        }) {
            Ok(rows) => rows,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
        match rows.collect::<rusqlite::Result<Vec<_>>>() {
            Ok(entries) => entries,
            Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        }
    };
    let mut metadata = Vec::new();
    for (item_id, rating_key) in entries {
        if let Some(mut item) = plex_metadata_item_json(&state, &rating_key).await {
            item["playlistItemID"] = serde_json::json!(item_id);
            metadata.push(item);
        }
    }
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": metadata.len(),
            "totalSize": metadata.len(),
            "offset": 0,
            "identifier": "com.plexapp.plugins.library",
            "Metadata": metadata
        }
    }))
}

pub(crate) async fn plex_add_playlist_item(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    {
        let Ok(connection) = state.db.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        if plex_playlist_record(&connection, id).is_err() {
            return StatusCode::NOT_FOUND.into_response();
        }
    }
    let Some(source) = query.get("uri").or_else(|| query.get("key")) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(rating_key) = plex_rating_key_from_source(source)
        .or_else(|| source.parse::<u64>().ok().map(|key| key.to_string()))
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(media) = find_media(&state, &rating_key) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let rating_key = plex_rating_key(&media).to_string();
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let next_position = match connection.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM playlist_items WHERE playlist_id = ?1",
        [id],
        |row| row.get::<_, i64>(0),
    ) {
        Ok(position) => position,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let added_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default();
    if connection
        .execute(
            "INSERT INTO playlist_items (playlist_id, media_key, position, added_at) VALUES (?1, ?2, ?3, ?4)",
            (id, rating_key, next_position, added_at),
        )
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let Ok(playlist) = plex_playlist_record(&connection, id) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match plex_playlist_container(&connection, &[playlist]) {
        Ok(body) => plex_json(body),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn plex_clear_playlist_items(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if plex_playlist_record(&connection, id).is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let result = if let Some(item_id) = query.get("playlistItemID") {
        let Ok(item_id) = item_id.parse::<i64>() else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        connection.execute(
            "DELETE FROM playlist_items WHERE id = ?1 AND playlist_id = ?2",
            (item_id, id),
        )
    } else {
        connection.execute("DELETE FROM playlist_items WHERE playlist_id = ?1", [id])
    };
    match result {
        Ok(0) if query.contains_key("playlistItemID") => StatusCode::NOT_FOUND.into_response(),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn plex_delete_playlist_item(
    State(state): State<AppState>,
    Path((playlist_id, item_id)): Path<(i64, i64)>,
) -> Response {
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if plex_playlist_record(&connection, playlist_id).is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match connection.execute(
        "DELETE FROM playlist_items WHERE playlist_id = ?1 AND id = ?2",
        (playlist_id, item_id),
    ) {
        Ok(0) => StatusCode::NOT_FOUND.into_response(),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) async fn plex_move_playlist_item(
    State(state): State<AppState>,
    Path((playlist_id, item_id)): Path<(i64, i64)>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Ok(connection) = state.db.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if plex_playlist_record(&connection, playlist_id).is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut statement = match connection
        .prepare("SELECT id FROM playlist_items WHERE playlist_id = ?1 ORDER BY position, id")
    {
        Ok(statement) => statement,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let rows = match statement.query_map([playlist_id], |row| row.get::<_, i64>(0)) {
        Ok(rows) => rows,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let mut item_ids = match rows.collect::<rusqlite::Result<Vec<_>>>() {
        Ok(item_ids) => item_ids,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let Some(current_index) = item_ids.iter().position(|id| *id == item_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let after = match query.get("after") {
        Some(after) => match after.parse::<i64>() {
            Ok(after) => after,
            Err(_) => return StatusCode::BAD_REQUEST.into_response(),
        },
        None => -1,
    };
    item_ids.remove(current_index);
    let insertion_index = if after < 0 {
        0
    } else if let Some(index) = item_ids.iter().position(|id| *id == after) {
        index + 1
    } else {
        return StatusCode::NOT_FOUND.into_response();
    };
    item_ids.insert(insertion_index, item_id);
    for (position, item_id) in item_ids.iter().enumerate() {
        if connection
            .execute(
                "UPDATE playlist_items SET position = ?1 WHERE id = ?2 AND playlist_id = ?3",
                (position as i64, item_id, playlist_id),
            )
            .is_err()
        {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

pub(crate) fn plex_play_queue_source_uri(library_uuid: &str, rating_key: &str) -> String {
    let item_path = format!("/library/metadata/{rating_key}");
    format!("library://{library_uuid}/item/{}", url_encode(&item_path))
}

pub(crate) fn plex_play_queue_response(
    id: u64,
    source_uri: &str,
    items: Vec<serde_json::Value>,
    selected_index: usize,
    include_direct_play_decision: bool,
) -> serde_json::Value {
    let selected = items.get(selected_index);
    let selected_item_id = selected
        .and_then(|item| item.get("playQueueItemID"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let selected_metadata_item_id = selected
        .and_then(|item| item.get("ratingKey"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let media_tag_version = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let mut response = serde_json::json!({
        "MediaContainer": {
            "identifier": "com.plexapp.plugins.library",
            "mediaTagPrefix": "/system/bundle/media/flags/",
            "mediaTagVersion": media_tag_version,
            "playQueueID": id,
            "playQueueSelectedItemID": selected_item_id,
            "playQueueSelectedItemOffset": 0,
            "playQueueSelectedMetadataItemID": selected_metadata_item_id,
            "playQueueShuffled": false,
            "playQueueSourceURI": source_uri,
            "playQueueTotalCount": items.len(),
            "playQueueVersion": 1,
            "size": items.len(),
            "Metadata": items
        }
    });
    if include_direct_play_decision {
        plex_mark_direct_play_decision(&mut response["MediaContainer"]);
    }
    response
}

pub(crate) fn plex_queue_item_without_playback_decision(item: &mut serde_json::Value) {
    let Some(media) = item
        .get_mut("Media")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|media| media.first_mut())
    else {
        return;
    };
    for field in ["protocol", "decision", "selected"] {
        media[field] = serde_json::Value::Null;
    }
    let Some(part) = media
        .get_mut("Part")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|parts| parts.first_mut())
    else {
        return;
    };
    for field in ["protocol", "decision", "selected"] {
        part[field] = serde_json::Value::Null;
    }
    if let Some(streams) = part
        .get_mut("Stream")
        .and_then(serde_json::Value::as_array_mut)
    {
        for stream in streams {
            stream["decision"] = serde_json::Value::Null;
            if stream["streamType"] == 1 || stream["selected"] == false {
                stream["selected"] = serde_json::Value::Null;
            }
        }
    }
}

pub(crate) fn plex_tv_show_playback_media(state: &AppState, rating_key: &str) -> Option<Media> {
    let libraries = state.config.lock().ok()?.libraries.clone();
    let media_index = media_snapshot(state);
    for (index, library) in libraries.iter().enumerate() {
        if library.kind != "tv" {
            continue;
        }
        let section = index + 1;
        let shows = plex_tv_show_groups(
            section,
            &library.folders,
            media_index
                .iter()
                .filter(|media| media_in_library(media, library)),
        );
        let Some(show) = shows.into_iter().find(|show| show.rating_key == rating_key) else {
            continue;
        };
        return plex_tv_season_groups(&show, &library.folders)
            .into_values()
            .flatten()
            .find_map(|episode| episode.files.into_iter().next().cloned());
    }
    None
}

pub(crate) async fn plex_create_play_queue(
    State(state): State<AppState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Some(source_uri) = query.get("uri") else {
        warn!(reason = "missing_uri", "Plex play queue request rejected");
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !matches!(query.get("type").map(String::as_str), Some("video")) {
        warn!(
            queue_type = query.get("type").map(String::as_str).unwrap_or("<missing>"),
            reason = "unsupported_type",
            "Plex play queue request rejected"
        );
        return StatusCode::BAD_REQUEST.into_response();
    }
    let rating_key = query
        .get("key")
        .and_then(|key| plex_rating_key_from_source(key))
        .or_else(|| plex_rating_key_from_source(source_uri));
    let Some(rating_key) = rating_key else {
        warn!(
            reason = "unrecognized_item_uri",
            "Plex play queue request rejected"
        );
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(media) = find_media(&state, &rating_key)
        .or_else(|| plex_tv_show_playback_media(&state, &rating_key))
    else {
        warn!(
            rating_key,
            reason = "item_not_found",
            "Plex play queue request rejected"
        );
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((section, library)) = media_library(&state, &media) else {
        warn!(
            rating_key,
            reason = "item_has_no_library",
            "Plex play queue request rejected"
        );
        return StatusCode::NOT_FOUND.into_response();
    };
    let metadata = plex_section_metadata(&state, section);
    let media_index = media_snapshot(&state);
    let mut item = if let Some(item) = plex_tv_episode_json_for_media(
        &state,
        media_index.as_ref(),
        &media,
        section,
        &library,
        &metadata,
    )
    .await
    {
        item
    } else {
        let mut item = plex_video_json(&media, section, &library, metadata.get(&media.id));
        add_plex_technical_metadata(&state, &mut item, &media).await;
        item
    };
    let include_markers = marker_include_value(query.get("includeMarkers").map(String::as_str));
    attach_markers_to_item(&state, &mut item, &media.id, include_markers);
    let direct_play = query.get("directPlay").map(String::as_str) != Some("0");
    if direct_play {
        plex_mark_direct_play(&mut item);
    } else {
        plex_queue_item_without_playback_decision(&mut item);
    }
    let queue_id = state
        .next_plex_play_queue_id
        .fetch_add(1, Ordering::Relaxed);
    // This queue currently contains one item, so its server-unique queue ID
    // also serves as the compact integer playQueueItemID expected by PMS clients.
    let queue_item_id = queue_id;
    item["playQueueItemID"] = serde_json::json!(queue_item_id);
    let library_uuid = plex_library_uuid(&state, &library.name, section);
    let plex_rating_key = plex_rating_key(&media).to_string();
    let normalized_source_uri = plex_play_queue_source_uri(&library_uuid, &plex_rating_key);
    let selected_metadata_item_id = plex_rating_key.parse::<u64>().unwrap_or_default();
    info!(
        queue_id,
        incoming_key = %rating_key,
        public_rating_key = %plex_rating_key,
        selected_metadata_item_id,
        queue_item_id,
        source_uri = %normalized_source_uri,
        "Plex play queue created"
    );
    let response =
        plex_play_queue_response(queue_id, &normalized_source_uri, vec![item], 0, direct_play);
    if let Ok(mut queues) = state.plex_play_queues.lock() {
        queues.insert(queue_id, response.clone());
    } else {
        warn!(queue_id, "Plex play queue response was not retained");
    }
    plex_json(response)
}

pub(crate) async fn plex_get_play_queue(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Response {
    state
        .plex_play_queues
        .lock()
        .ok()
        .and_then(|queues| queues.get(&id).cloned())
        .map(plex_json)
        .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

pub(crate) async fn plex_playback_decision(
    State(state): State<AppState>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let media_id = query
        .get("path")
        .or_else(|| query.get("key"))
        .and_then(|path| plex_query_media_id(None, Some(path)))
        .or_else(|| query.get("ratingKey").cloned())
        .or_else(|| query.get("mediaId").cloned());
    let Some(media_id) = media_id else {
        warn!(
            reason = "missing_media_id",
            "Plex playback decision rejected"
        );
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(media) = find_media(&state, &media_id) else {
        warn!(
            media_id,
            reason = "item_not_found",
            "Plex playback decision rejected"
        );
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((section, library)) = media_library(&state, &media) else {
        warn!(
            media_id,
            reason = "item_has_no_library",
            "Plex playback decision rejected"
        );
        return StatusCode::NOT_FOUND.into_response();
    };

    let metadata_by_id = plex_section_metadata(&state, section);
    let media_index = media_snapshot(&state);
    let mut video = if let Some(video) = plex_tv_episode_json_for_media(
        &state,
        media_index.as_ref(),
        &media,
        section,
        &library,
        &metadata_by_id,
    )
    .await
    {
        video
    } else {
        let mut video = plex_video_json(&media, section, &library, metadata_by_id.get(&media.id));
        add_plex_technical_metadata(&state, &mut video, &media).await;
        video
    };
    let library_uuid = plex_library_uuid(&state, &library.name, section);
    if query.get("directPlay").map(String::as_str) != Some("0") {
        plex_mark_direct_play(&mut video);
        info!(media_id, section, "Plex direct-play decision returned");
        return plex_json(plex_direct_play_decision_response(
            video,
            section,
            &library,
            &library_uuid,
        ));
    }

    let protocol = query.get("protocol").map(String::as_str).unwrap_or("http");
    let direct_stream = query.get("directStream").map(String::as_str);
    let audio_copy_disabled = query.get("directStreamAudio").map(String::as_str) == Some("0");
    let transcode_audio =
        audio_copy_disabled || (protocol == "dash" && query.get("directStreamAudio").is_none());
    if direct_stream == Some("0")
        && matches!(protocol, "hls" | "dash")
        && plex_can_transcode_video(&video)
        && (!transcode_audio || plex_can_transcode_audio(&video))
    {
        plex_mark_video_transcode_decision(&mut video, protocol, transcode_audio);
        info!(
            media_id,
            section, protocol, "Plex video conversion decision returned"
        );
        return plex_json(plex_audio_transcode_decision_response(
            video,
            section,
            &library,
            &library_uuid,
        ));
    }
    if direct_stream == Some("1")
        && audio_copy_disabled
        && matches!(protocol, "hls" | "dash")
        && plex_can_transcode_audio(&video)
    {
        let container = if protocol == "hls"
            && direct_stream == Some("1")
            && audio_copy_disabled
            && plex_profile_requests_hls_mkv(&query)
        {
            "mkv"
        } else {
            "mp4"
        };
        plex_mark_audio_transcode_decision(&mut video, protocol, container);
        info!(
            media_id,
            section, protocol, "Plex audio conversion decision returned"
        );
        return plex_json(plex_audio_transcode_decision_response(
            video,
            section,
            &library,
            &library_uuid,
        ));
    }

    warn!(
        media_id,
        section,
        protocol,
        reason = "transcoding_unavailable",
        "Plex playback conversion rejected"
    );
    StatusCode::NOT_IMPLEMENTED.into_response()
}

pub(crate) fn plex_direct_play_decision_response(
    video: serde_json::Value,
    section: usize,
    library: &ConfigLibrary,
    library_uuid: &str,
) -> serde_json::Value {
    let mut container = plex_library_media_container(library, section, library_uuid, 1, 0, 1);
    plex_mark_direct_play_decision(&mut container);
    // The client uses the overall decision fields to choose its playback
    // path. Returning a conversion failure here overrides the successful
    // direct-play result and produces Plex's "server is not powerful enough"
    // error before it requests the original media part. This response is a
    // direct-play decision; advertise that same successful result consistently.
    container["transcodeDecisionCode"] = serde_json::json!(1000);
    container["transcodeDecisionText"] = serde_json::json!("Direct play OK.");
    container["Metadata"] = serde_json::json!([video]);
    serde_json::json!({"MediaContainer": container})
}

pub(crate) fn plex_can_transcode_audio(video: &serde_json::Value) -> bool {
    let Some(streams) = video
        .pointer("/Media/0/Part/0/Stream")
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    let has_hevc_video = streams.iter().any(|stream| {
        stream.get("streamType").and_then(serde_json::Value::as_i64) == Some(1)
            && stream.get("codec").and_then(serde_json::Value::as_str) == Some("hevc")
    });
    let has_selected_eac3_audio = streams.iter().any(|stream| {
        stream.get("streamType").and_then(serde_json::Value::as_i64) == Some(2)
            && stream.get("codec").and_then(serde_json::Value::as_str) == Some("eac3")
            && stream.get("selected").and_then(serde_json::Value::as_bool) == Some(true)
    });
    let has_selected_subtitle = streams.iter().any(|stream| {
        stream.get("streamType").and_then(serde_json::Value::as_i64) == Some(3)
            && stream.get("selected").and_then(serde_json::Value::as_bool) == Some(true)
    });
    has_hevc_video && has_selected_eac3_audio && !has_selected_subtitle
}

pub(crate) fn plex_can_transcode_video(video: &serde_json::Value) -> bool {
    let Some(streams) = video
        .pointer("/Media/0/Part/0/Stream")
        .and_then(serde_json::Value::as_array)
    else {
        return false;
    };
    let has_hevc_video = streams.iter().any(|stream| {
        stream.get("streamType").and_then(serde_json::Value::as_i64) == Some(1)
            && stream.get("codec").and_then(serde_json::Value::as_str) == Some("hevc")
    });
    let unsupported_selected_audio = streams.iter().any(|stream| {
        stream.get("streamType").and_then(serde_json::Value::as_i64) == Some(2)
            && stream.get("selected").and_then(serde_json::Value::as_bool) == Some(true)
            && stream.get("codec").and_then(serde_json::Value::as_str) != Some("eac3")
    });
    let has_selected_subtitle = streams.iter().any(|stream| {
        stream.get("streamType").and_then(serde_json::Value::as_i64) == Some(3)
            && stream.get("selected").and_then(serde_json::Value::as_bool) == Some(true)
    });
    has_hevc_video && !unsupported_selected_audio && !has_selected_subtitle
}

pub(crate) fn plex_mark_audio_transcode_decision(
    video: &mut serde_json::Value,
    protocol: &str,
    container_name: &str,
) {
    let Some(media) = video
        .get_mut("Media")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|media| media.first_mut())
    else {
        return;
    };
    media["decision"] = serde_json::json!("transcode");
    media["selected"] = serde_json::json!(true);
    media["protocol"] = serde_json::json!(protocol);
    media["container"] = serde_json::json!(container_name);
    media["audioCodec"] = serde_json::json!("aac");
    media["audioChannels"] = serde_json::json!(2);
    let Some(part) = media
        .get_mut("Part")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|parts| parts.first_mut())
    else {
        return;
    };
    part["decision"] = serde_json::json!("transcode");
    part["protocol"] = serde_json::json!(protocol);
    part["container"] = serde_json::json!(container_name);
    if let Some(streams) = part
        .get_mut("Stream")
        .and_then(serde_json::Value::as_array_mut)
    {
        for stream in streams {
            match stream.get("streamType").and_then(serde_json::Value::as_i64) {
                Some(1) => stream["decision"] = serde_json::json!("copy"),
                Some(2)
                    if stream.get("selected").and_then(serde_json::Value::as_bool)
                        == Some(true) =>
                {
                    stream["decision"] = serde_json::json!("transcode");
                    stream["codec"] = serde_json::json!("aac");
                    stream["channels"] = serde_json::json!(2);
                }
                _ => {}
            }
        }
    }
}

pub(crate) fn plex_mark_video_transcode_decision(
    video: &mut serde_json::Value,
    protocol: &str,
    transcode_audio: bool,
) {
    let Some(media) = video
        .get_mut("Media")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|media| media.first_mut())
    else {
        return;
    };
    media["decision"] = serde_json::json!("transcode");
    media["selected"] = serde_json::json!(true);
    media["protocol"] = serde_json::json!(protocol);
    media["container"] = serde_json::json!("mp4");
    media["videoCodec"] = serde_json::json!("h264");
    if transcode_audio {
        media["audioCodec"] = serde_json::json!("aac");
        media["audioChannels"] = serde_json::json!(2);
    }
    let Some(part) = media
        .get_mut("Part")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|parts| parts.first_mut())
    else {
        return;
    };
    part["decision"] = serde_json::json!("transcode");
    part["protocol"] = serde_json::json!(protocol);
    part["container"] = serde_json::json!("mp4");
    if let Some(streams) = part
        .get_mut("Stream")
        .and_then(serde_json::Value::as_array_mut)
    {
        for stream in streams {
            match stream.get("streamType").and_then(serde_json::Value::as_i64) {
                Some(1) => {
                    stream["decision"] = serde_json::json!("transcode");
                    stream["codec"] = serde_json::json!("h264");
                }
                Some(2)
                    if stream.get("selected").and_then(serde_json::Value::as_bool)
                        == Some(true)
                        && transcode_audio =>
                {
                    stream["decision"] = serde_json::json!("transcode");
                    stream["codec"] = serde_json::json!("aac");
                    stream["channels"] = serde_json::json!(2);
                }
                Some(2)
                    if stream.get("selected").and_then(serde_json::Value::as_bool)
                        == Some(true) =>
                {
                    stream["decision"] = serde_json::json!("copy")
                }
                _ => {}
            }
        }
    }
}

pub(crate) fn plex_audio_transcode_decision_response(
    video: serde_json::Value,
    section: usize,
    library: &ConfigLibrary,
    library_uuid: &str,
) -> serde_json::Value {
    let mut response = plex_direct_play_decision_response(video, section, library, library_uuid);
    let container = &mut response["MediaContainer"];
    container["directPlayDecisionCode"] = serde_json::json!(3000);
    container["directPlayDecisionText"] = serde_json::json!("Direct play not available.");
    container["generalDecisionCode"] = serde_json::json!(1001);
    container["generalDecisionText"] =
        serde_json::json!("Direct play not available; Conversion OK.");
    container["transcodeDecisionCode"] = serde_json::json!(1001);
    container["transcodeDecisionText"] =
        serde_json::json!("Direct play not available; Conversion OK.");
    response
}

pub(crate) fn plex_mark_direct_play_decision(container: &mut serde_json::Value) {
    container["directPlayDecisionCode"] = serde_json::json!(1000);
    container["directPlayDecisionText"] = serde_json::json!("Direct play OK.");
    container["generalDecisionCode"] = serde_json::json!(1000);
    container["generalDecisionText"] = serde_json::json!("Direct play OK.");
}

pub(crate) fn plex_mark_direct_play(video: &mut serde_json::Value) {
    let Some(media) = video
        .get_mut("Media")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|media| media.first_mut())
    else {
        return;
    };
    media["decision"] = serde_json::json!("directplay");
    media["selected"] = serde_json::json!(true);
    media["protocol"] = serde_json::json!("http");
    if let Some(part) = media
        .get_mut("Part")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|parts| parts.first_mut())
    {
        part["decision"] = serde_json::json!("directplay");
        part["selected"] = serde_json::json!(true);
        part["protocol"] = serde_json::json!("http");
        if let Some(streams) = part
            .get_mut("Stream")
            .and_then(serde_json::Value::as_array_mut)
        {
            for stream in streams {
                stream["decision"] = serde_json::json!("directplay");
                if stream.get("streamType").and_then(serde_json::Value::as_i64) == Some(3) {
                    continue;
                }
                if stream.get("selected").is_none() {
                    stream["selected"] = serde_json::json!(true);
                }
            }
        }
    }
}

#[derive(Clone, Copy, Deserialize, Default)]
pub(crate) struct PlexStreamSelection {
    #[serde(rename = "audioStreamID")]
    pub(crate) audio_stream_id: Option<u64>,
    #[serde(rename = "subtitleStreamID")]
    pub(crate) subtitle_stream_id: Option<u64>,
}

pub(crate) async fn plex_set_streams(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(selection): Query<PlexStreamSelection>,
) -> Response {
    let Some(media) = find_media_part(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if selection.audio_stream_id.is_none() && selection.subtitle_stream_id.is_none() {
        return plex_json(serde_json::json!({"MediaContainer": {"size": 0}}));
    }
    let info = state
        .media_info
        .lock()
        .ok()
        .and_then(|cache| cache.get(&media.id).cloned());
    let info = match info {
        Some(info) => info,
        None => match inspect_media(&media.path).await {
            Some(info) => {
                if let Ok(mut cache) = state.media_info.lock() {
                    cache.insert(media.id.clone(), info.clone());
                }
                info
            }
            None => serde_json::json!({"media": {"track": []}}),
        },
    };
    let subtitle_paths = external_subtitle_files(&media.path).await;
    let audio_valid = selection.audio_stream_id.is_none_or(|selected| {
        technical_tracks(&info).iter().any(|track| {
            technical_text(track, "@type") == Some("Audio")
                && technical_track_id(track).saturating_add(1) == selected
        })
    });
    let subtitle_valid = selection.subtitle_stream_id.is_none_or(|selected| {
        selected == 0
            || technical_tracks(&info).iter().any(|track| {
                technical_text(track, "@type") == Some("Text")
                    && technical_track_id(track).saturating_add(1) == selected
            })
            || subtitle_paths
                .iter()
                .any(|path| plex_external_subtitle_id(&media, path) == selected)
            || (state.config.lock().ok().is_some_and(|config| {
                selected
                    == plex_translated_subtitle_id(
                        &media,
                        &config.subtitle_language_primary,
                        &config.subtitle_language_secondary,
                    )
            }))
    });
    if !audio_valid || !subtitle_valid {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if let Ok(mut selections) = state.plex_stream_selections.lock() {
        let current = selections.entry(plex_part_id(&media)).or_default();
        if selection.audio_stream_id.is_some() {
            current.audio_stream_id = selection.audio_stream_id;
        }
        if let Some(stream_id) = selection.subtitle_stream_id {
            current.subtitle_stream_id = Some(stream_id);
            let _ = web_subtitle_selection_broadcast().send((plex_part_id(&media), stream_id));
        }
    }
    plex_json(serde_json::json!({"MediaContainer": {"size": 0}}))
}

pub(crate) async fn plex_part_file(
    State(state): State<AppState>,
    Path((id, part_path)): Path<(String, String)>,
    uri: Uri,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let Some(media) = find_media_part(&state, &id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let expected = format!("file.{}", media.container);
    if part_path.rsplit('/').next() != Some(expected.as_str()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let download = plex_query_value(&uri, "download") == Some("1");
    stream_file(&media, method, headers, download).await
}

pub(crate) async fn plex_stream(
    State(state): State<AppState>,
    Path(stream): Path<String>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Some((stream_id, extension)) = plex_stream_id_and_format(&stream) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let media_index = media_snapshot(&state);
    let pair = state.config.lock().ok().and_then(|config| {
        config.bilingual_subtitles_enabled.then(|| {
            (
                config.subtitle_language_primary.clone(),
                config.subtitle_language_secondary.clone(),
            )
        })
    });
    let bilingual_enabled = pair.is_some();
    if let Some((first, second)) = pair.as_ref() {
        if let Some(media) = media_index
            .iter()
            .find(|media| plex_translated_subtitle_id(media, first, second) == stream_id)
        {
            let requested = query
                .get("format")
                .map(|format| format.to_ascii_lowercase())
                .unwrap_or_else(|| extension.unwrap_or("vtt").to_owned());
            return translated_subtitle_stream_response(&state, media, first, second, &requested)
                .await;
        }
    }
    for media in media_index.iter() {
        for subtitle_path in external_subtitle_files(&media.path).await {
            if !bilingual_enabled && external_subtitle_pair(&media.path, &subtitle_path).is_some() {
                continue;
            }
            if plex_external_subtitle_id(media, &subtitle_path) != stream_id {
                continue;
            }
            let Ok(contents) = tokio::fs::read_to_string(&subtitle_path).await else {
                return StatusCode::NOT_FOUND.into_response();
            };
            let subtitle_extension = subtitle_path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let requested_format = query
                .get("format")
                .map(|format| format.to_ascii_lowercase())
                .unwrap_or_else(|| extension.unwrap_or(&subtitle_extension).to_owned());
            let Some((content_type, contents)) =
                external_subtitle_content(&contents, &subtitle_extension, &requested_format)
            else {
                return StatusCode::NOT_FOUND.into_response();
            };
            return ([(header::CONTENT_TYPE, content_type)], contents).into_response();
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

pub(crate) fn plex_stream_id_and_format(stream: &str) -> Option<(u64, Option<&str>)> {
    let (id, extensions) = stream
        .split_once('.')
        .map(|(id, extensions)| (id, Some(extensions)))
        .unwrap_or((stream, None));
    let stream_id = id.parse::<u64>().ok()?;
    let Some(extensions) = extensions else {
        return Some((stream_id, None));
    };
    let mut requested_format = None;
    for extension in extensions.split('.') {
        if !matches!(extension, "srt" | "ass" | "vtt") {
            return None;
        }
        requested_format = Some(extension);
    }
    Some((stream_id, Some(requested_format?)))
}

pub(crate) async fn plex_thumb(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    plex_artwork(&state, &id, "cover.jpg").await
}

pub(crate) async fn plex_art(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    plex_artwork(&state, &id, "banner.jpg").await
}

pub(crate) async fn plex_artwork(state: &AppState, id: &str, filename: &str) -> Response {
    let Some(media) = find_media(state, id) else {
        info!(
            media_id = id,
            artwork = filename,
            "Plex artwork references unknown media"
        );
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((library_id, library)) = media_library(state, &media) else {
        info!(
            media_id = id,
            artwork = filename,
            "Plex artwork media has no library"
        );
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(root) = library
        .folders
        .iter()
        .find(|folder| media.path.starts_with(folder))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let config_mode = state
        .config
        .lock()
        .is_ok_and(|config| config.metadata_storage == "config");
    let filename = match filename {
        "cover.jpg" | "banner.jpg" => filename,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let group_path = metadata_group_path(root, &media.path, &library.kind);
    let media_index = media_snapshot(state);
    let representative = media_index
        .iter()
        .find(|candidate| {
            media_library(state, candidate).is_some_and(|(id, _)| id == library_id)
                && metadata_group_path(root, &candidate.path, &library.kind) == group_path
        })
        .unwrap_or(&media);
    let media_key = database_media_key(library_id, &representative.id);
    let config_artwork = config_metadata_sidecar_path(state, &media_key)
        .parent()
        .unwrap_or(state.subtitle_checkpoints_dir.as_path())
        .join(filename);
    let group_directory = if group_path.is_file() {
        group_path.parent().unwrap_or(root)
    } else {
        group_path.as_path()
    };
    let season_directory = media.path.parent().unwrap_or(root);
    let media_artwork = if library.kind == "tv" && filename == "cover.jpg" {
        [
            season_directory.join(".hbgr").join(filename),
            group_directory.join(".hbgr").join(filename),
        ]
    } else {
        [
            group_directory.join(".hbgr").join(filename),
            season_directory.join(".hbgr").join(filename),
        ]
    };
    let artwork_paths = if config_mode {
        std::iter::once(config_artwork)
            .chain(media_artwork)
            .collect::<Vec<_>>()
    } else {
        media_artwork
            .into_iter()
            .chain(std::iter::once(config_artwork))
            .collect::<Vec<_>>()
    };
    for artwork in &artwork_paths {
        if let Ok(bytes) = tokio::fs::read(artwork).await {
            return (
                [(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"))],
                bytes,
            )
                .into_response();
        }
    }
    if matches!(library.kind.as_str(), "movie" | "tv") {
        if let Ok(relative_group_path) = group_path.strip_prefix(root) {
            let _scan_guard = state.inventory_scan_lock.lock().await;
            let response = scan_library_metadata(
                State(state.clone()),
                Path(library_id - 1),
                Query(ScanQuery {
                    force: Some(false),
                    path: Some(relative_group_path.to_string_lossy().into_owned()),
                    refresh: None,
                    assets_only: Some(false),
                }),
            )
            .await;
            if !response.status().is_success() && !response.status().is_redirection() {
                warn!(media_id = %media.id, status = %response.status(), "metadata lookup after missing artwork failed");
            }
            for artwork in &artwork_paths {
                if let Ok(bytes) = tokio::fs::read(artwork).await {
                    return (
                        [(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"))],
                        bytes,
                    )
                        .into_response();
                }
            }
        }
    }
    if filename == "cover.jpg" {
        if let Some(bytes) = plex_frame_cover(state, &media).await {
            return (
                [(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"))],
                bytes,
            )
                .into_response();
        }
    }
    info!(
        media_id = id,
        artwork = filename,
        "Plex artwork unavailable"
    );
    StatusCode::NOT_FOUND.into_response()
}

pub(crate) async fn plex_frame_cover(state: &AppState, media: &Media) -> Option<Vec<u8>> {
    let cache_dir = state
        .config_path
        .parent()
        .unwrap_or(std::path::Path::new("config"))
        .join("artwork-cache");
    let cache_key = format!(
        "{:x}",
        Sha256::digest(format!(
            "{}:{}:{}",
            media.id,
            media.size,
            media_modified_at(media)
        ))
    );
    let cache_path = cache_dir.join(format!("{cache_key}.jpg"));
    if let Ok(bytes) = tokio::fs::read(&cache_path).await {
        return Some(bytes);
    }

    warn!(media_id = %media.id, "no cached cover; frame extraction requires an in-process decoder");
    None
}

pub(crate) fn plex_section_directories(state: &AppState) -> (usize, Vec<serde_json::Value>) {
    let libraries = state
        .config
        .lock()
        .map(|config| config.libraries.clone())
        .unwrap_or_default();
    let content_changed_at = library_catalog_updated_at(state);
    let directories_json = libraries
        .iter()
        .enumerate()
        .map(|(index, library)| {
            let section = index + 1;
            let (stable_id, scanned_at, updated_at) =
                library_publication_state(state, &library.name, section);
            let (created_at, locations) = plex_library_section_details(state, stable_id);
            serde_json::json!({
                "allowSync": true,
                "filters": true,
                "refreshing": false,
                "key": section.to_string(),
                "type": plex_kind(&library.kind),
                "title": library.name,
                "agent": library.plex_agent.as_deref().unwrap_or(if library.kind == "tv" { "tv.plex.agents.series" } else { "tv.plex.agents.movie" }),
                "scanner": library.plex_scanner.as_deref().unwrap_or(if library.kind == "tv" { "Plex TV Series" } else { "Plex Movie" }),
                "language": library.plex_language.as_deref().unwrap_or("en-US"),
                "metadataAgentProviderGroupId": library.metadata_agent_provider_group_id.unwrap_or(if library.kind == "tv" { 2 } else { 1 }),
                "uuid": plex_library_uuid(state, &library.name, section),
                "updatedAt": updated_at,
                "createdAt": created_at,
                "scannedAt": scanned_at,
                "content": true,
                "directory": true,
                "contentChangedAt": content_changed_at,
                "hidden": 0,
                "Location": locations
            })
        })
        .collect::<Vec<_>>();
    (libraries.len(), directories_json)
}

pub(crate) fn plex_kind(kind: &str) -> &'static str {
    match kind {
        "tv" => "show",
        "movie" => "movie",
        _ => "movie",
    }
}

pub(crate) fn plex_video_json(
    media: &Media,
    section: usize,
    library: &ConfigLibrary,
    metadata: Option<&serde_json::Value>,
) -> serde_json::Value {
    let rating_key = plex_rating_key(media).to_string();
    let media_type = if library.kind == "tv" {
        "episode"
    } else {
        plex_kind(&library.kind)
    };
    let playback_duration = metadata
        .and_then(|metadata| metadata.get("mplex_duration"))
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    let title = if media_type == "episode" {
        tv_episode_display_name(&media.title)
    } else {
        media.title.clone()
    };
    let modified_at = media_modified_at(media);
    let mut item = serde_json::json!({
        "ratingKey": rating_key,
        "key": format!("/library/metadata/{rating_key}"),
        "type": media_type,
        "guid": plex_metadata_guid(media_type, &rating_key),
        "title": title,
        "thumb": format!("/library/metadata/{rating_key}/thumb"),
        "art": format!("/library/metadata/{rating_key}/art"),
        "librarySectionID": section,
        "librarySectionKey": format!("/library/sections/{section}"),
        "librarySectionTitle": library.name,
        "originalTitle": media.title,
        "titleSort": media.title.to_lowercase(),
        "updatedAt": modified_at.max(media.metadata_revision),
        "addedAt": modified_at,
        "duration": playback_duration,
        "viewOffset": 0,
        "viewCount": 0,
        "lastViewedAt": 0,
        "Media": [{
            "id": plex_media_id(media),
            "duration": playback_duration,
            "container": media.container,
            "protocol": "http",
            "has64bitOffsets": media.size > i32::MAX as u64,
            "optimizedForStreaming": false,
            "videoResolution": "unknown",
            "Part": [{
                "id": plex_part_id(media),
                "key": format!("/library/parts/{}/{}/file.{}", plex_part_id(media), modified_at, media.container),
                "duration": playback_duration,
                "file": media.path,
                "size": media.size,
                "container": media.container,
                "exists": true,
                "accessible": true,
                "has64bitOffsets": media.size > i32::MAX as u64,
                "optimizedForStreaming": false
            }]
        }]
    });
    if let Some(metadata) = metadata {
        if let Some(position) = metadata
            .get("mplex_view_offset")
            .and_then(serde_json::Value::as_i64)
        {
            item["viewOffset"] = serde_json::json!(position);
        }
        if let Some(count) = metadata
            .get("mplex_view_count")
            .and_then(serde_json::Value::as_i64)
        {
            item["viewCount"] = serde_json::json!(count);
        }
        if let Some(last_viewed) = metadata
            .get("mplex_last_viewed_at")
            .and_then(serde_json::Value::as_i64)
        {
            item["lastViewedAt"] = serde_json::json!(last_viewed);
        }
    }
    if let Some(year) = title_year(&media.title) {
        item["year"] = serde_json::json!(year);
    }
    if let Some(metadata) = metadata {
        let metadata_title = match media_type {
            "show" => metadata.get("name").and_then(serde_json::Value::as_str),
            "episode" => metadata
                .get("mplex_episode_title")
                .and_then(serde_json::Value::as_str),
            _ => metadata.get("title").and_then(serde_json::Value::as_str),
        };
        if let Some(title) = metadata_title.filter(|title| !title.is_empty()) {
            item["title"] = serde_json::json!(title);
        }
        if let Some(summary) = metadata
            .get("overview")
            .and_then(serde_json::Value::as_str)
            .filter(|summary| !summary.is_empty())
        {
            item["summary"] = serde_json::json!(summary);
        }
        let date = match media_type {
            "show" => metadata.get("first_air_date"),
            "movie" => metadata.get("release_date"),
            _ => None,
        }
        .and_then(serde_json::Value::as_str)
        .filter(|date| !date.is_empty());
        if let Some(date) = date {
            item["originallyAvailableAt"] = serde_json::json!(date);
            if let Some(year) = date.get(..4).and_then(|year| year.parse::<i64>().ok()) {
                item["year"] = serde_json::json!(year);
            }
        }
        let original_title = metadata
            .get("original_title")
            .or_else(|| metadata.get("original_name"))
            .and_then(serde_json::Value::as_str);
        if let Some(original_title) = original_title.filter(|title| !title.is_empty()) {
            item["originalTitle"] = serde_json::json!(original_title);
        }
        if let Some(rating) = plex_rating(metadata) {
            item["rating"] = serde_json::json!(rating);
            item["audienceRating"] = serde_json::json!(rating);
        }
        let genre_names = metadata
            .get("genres")
            .and_then(serde_json::Value::as_array)
            .map(|genres| {
                genres
                    .iter()
                    .filter_map(|genre| genre.get("name").and_then(serde_json::Value::as_str))
                    .map(|name| serde_json::json!({"tag": name}))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| {
                metadata
                    .get("genre_ids")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_i64)
                    .filter_map(|id| {
                        tmdb_genre_name(
                            if media_type == "episode" {
                                "show"
                            } else {
                                media_type
                            },
                            id,
                        )
                    })
                    .map(|name| serde_json::json!({"tag": name}))
                    .collect()
            });
        if !genre_names.is_empty() {
            item["Genre"] = serde_json::json!(genre_names);
        }
        if media_type != "episode" {
            if let Some(tmdb_id) = metadata.get("id").and_then(serde_json::Value::as_i64) {
                item["guid"] = serde_json::json!(format!(
                    "tmdb://{}/{}",
                    if media_type == "show" { "tv" } else { "movie" },
                    tmdb_id
                ));
            }
        }
        if media_type == "show" {
            if let Some(show_title) = metadata.get("name").and_then(serde_json::Value::as_str) {
                item["grandparentTitle"] = serde_json::json!(show_title);
            }
            if let Some(season) = metadata
                .get("mplex_season")
                .and_then(serde_json::Value::as_u64)
            {
                item["parentIndex"] = serde_json::json!(season);
                item["parentTitle"] = serde_json::json!(format!("Season {season:02}"));
            }
            if let Some(episode) = metadata
                .get("mplex_episode")
                .and_then(serde_json::Value::as_u64)
            {
                item["index"] = serde_json::json!(episode);
            }
        }
    }
    item
}
