use crate::plex::plex_tls;
#[path = "api.rs"]
mod api;
pub(crate) mod introdb;
pub(crate) mod markers;
pub(crate) mod metrics;
mod subtitles;

pub(crate) use api::{
    clean_relative_path, image_response, is_hidden_path, media_cover_url, media_progress_markup,
    settings_error, stream_for_id, web_auth_gate,
};
#[cfg(test)]
pub(crate) use api::{
    contains_hidden_component, media_card, media_card_action_script, ordered_continue_paths,
    render_metadata_info_panel, render_season_description_panel, render_toml_details,
    render_tv_show_metadata_details, sidebar_nav_with_library_links, sidebar_nav_with_settings,
    unwatched_episode_count, web_title_sort_key,
};

pub(crate) use crate::plex::api::{
    plex_account_id, plex_account_resource_inventory, plex_artwork, plex_auth_callback,
    plex_auth_client_identifier, plex_auth_product, plex_auth_status, plex_auth_url, plex_codec,
    plex_connection_urls, plex_connectivity_check, plex_cors, plex_device_announce_query,
    plex_device_announce_url, plex_device_jwk, plex_device_key, plex_external_subtitle_id,
    plex_frame_cover, plex_json, plex_language, plex_language_code_3, plex_machine_identifier,
    plex_media_id, plex_notification_response, plex_part_id, plex_photo_source,
    plex_playback_decision, plex_playback_media_from_query, plex_prefs, plex_presence_url,
    plex_profile_requests_hls_mkv, plex_protocol_response, plex_query_value, plex_rating_key,
    plex_remux_resource, plex_remux_start, plex_request_token, plex_resource_connection_summary,
    plex_resources_contain, plex_resources_contain_local_connection,
    plex_resources_contain_local_https, plex_root, plex_scrobble, plex_section_metadata,
    plex_server_registration_url, plex_server_registration_xml, plex_startup_registration,
    plex_timeline, plex_track_label, plex_track_label_with_fallback, plex_track_language,
    plex_translated_subtitle_id, plex_tv_season_groups, plex_tv_season_key, plex_tv_show_groups,
    plex_user_avatar, plex_user_avatar_source, plex_video_level, plex_video_profile,
    plex_video_resolution, plex_webhook_access_tokens, plex_webhook_account,
    plex_webhook_account_from_access_tokens, plex_webhook_destinations,
    plex_webhook_destinations_for_account, plex_webhook_metadata, plex_webhook_player,
    plex_webhook_server_token, PlexClaimTokenResponse, PlexPinResponse, PlexRemuxSession,
    PlexScrobbleQuery, PlexStreamSelection, PlexTimelineQuery, PlexTvShowGroup,
    PLEX_DEFAULT_AUDIO_LANGUAGE,
};
#[cfg(test)]
pub(crate) use crate::plex::api::{
    plex_auth_pin_id, plex_children_json, plex_connectivity_url,
    plex_direct_play_decision_response, plex_empty_extras_response, plex_episode_search_rank,
    plex_genre_tokens, plex_hub_categories, plex_library_children_container,
    plex_library_media_container, plex_library_provider_features, plex_mark_direct_play,
    plex_paged_children, plex_pin_error_summary, plex_pin_status_url, plex_play_queue_response,
    plex_play_queue_source_uri, plex_publish_urls, plex_query_media_id,
    plex_queue_item_without_playback_decision, plex_ranked_tv_shows, plex_rating_key_from_source,
    plex_remux_audio_mode, plex_safe_numeric_id, plex_scrobble_threshold_reached, plex_search_hubs,
    plex_search_rank, plex_search_result_entries, plex_search_type_matches, plex_stream_id,
    plex_stream_id_and_format, plex_tv_episode_parents, plex_tv_season_json, plex_tv_show_json,
    plex_video_json, plex_webhook_destinations_from_response, plex_webhook_event_for_transition,
    PlexPinStatusResponse,
};

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocketUpgrade},
        DefaultBodyLimit, Form, FromRequestParts, Path, Query, RawForm, State,
    },
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode, Uri},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use clap::Parser;
use matroska_demuxer::{Frame as MatroskaFrame, MatroskaFile, TrackType};
use plex_tls::PlexTls;
use rand_core::{OsRng, RngCore};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    io::{self, Write},
    net::SocketAddr,
    path::{Component, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, OnceLock, RwLock,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt, SeekFrom},
};
use tokio_util::io::ReaderStream;
use tracing::{
    debug,
    field::{Field, Visit},
    info, warn, Event, Subscriber,
};
use tracing_subscriber::{
    filter::EnvFilter,
    fmt::{
        format::{FormatEvent, FormatFields, Writer},
        time::FormatTime,
        FmtContext,
    },
    layer::SubscriberExt,
    registry::LookupSpan,
    reload,
    util::SubscriberInitExt,
};

const POSTER_CARD_MIN_WIDTH: usize = 140;
pub(crate) const WEB_LIBRARY_PAGE_SIZE: usize = 60;
const WEB_UI_PORT: u16 = 4826;
const ABS_PORT: u16 = 13378;
pub(crate) const USER_ID: &str = "admin";

static MARKER_DETECTION_GROUPS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn begin_marker_detection_group(
    state: &AppState,
    key: String,
    library_name: String,
    show: &str,
    season: u32,
    kind: markers::MarkerKind,
) -> Option<metrics::ActivityGuard> {
    let inserted = MARKER_DETECTION_GROUPS
        .get_or_init(Default::default)
        .lock()
        .map(|mut groups| groups.insert(key.clone()))
        .unwrap_or(false);
    if !inserted {
        return None;
    }
    Some(metrics::ActivityGuard::begin(
        &state.metrics,
        format!("markers:{key}"),
        format!("{} detection", kind.as_str()),
        format!("{show} · Season {season}"),
        format!("Library: {library_name}"),
    ))
}

fn begin_movie_marker_detection(
    state: &AppState,
    key: String,
    library_name: String,
    title: &str,
) -> Option<metrics::ActivityGuard> {
    let inserted = MARKER_DETECTION_GROUPS
        .get_or_init(Default::default)
        .lock()
        .map(|mut groups| groups.insert(key.clone()))
        .unwrap_or(false);
    if !inserted {
        return None;
    }
    Some(metrics::ActivityGuard::begin(
        &state.metrics,
        format!("markers:{key}"),
        "markers detection",
        title.to_owned(),
        format!("Library: {library_name}"),
    ))
}

fn end_marker_detection_group(key: &str) {
    if let Ok(mut groups) = MARKER_DETECTION_GROUPS.get_or_init(Default::default).lock() {
        groups.remove(key);
    }
}

#[derive(Parser, Debug, Clone)]
struct Args {
    #[arg(long, value_name = "DIRECTORY")]
    config: Option<PathBuf>,
    #[arg(long)]
    bind: Option<SocketAddr>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SkipMode {
    Automatic,
    Manual,
    Off,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Config {
    #[serde(default = "default_bind")]
    pub(crate) bind: String,
    #[serde(default)]
    pub(crate) libraries: Vec<ConfigLibrary>,
    #[serde(default = "default_metadata_storage")]
    pub(crate) metadata_storage: String,
    #[serde(default)]
    pub(crate) tmdb_bearer_token: Option<String>,
    #[serde(default = "default_audio_language_primary")]
    pub(crate) audio_language_primary: String,
    #[serde(default)]
    pub(crate) audio_language_secondary: Option<String>,
    #[serde(default = "default_subtitle_language_primary")]
    pub(crate) subtitle_language_primary: String,
    #[serde(default = "default_subtitle_language_secondary")]
    pub(crate) subtitle_language_secondary: String,
    #[serde(default = "default_true")]
    pub(crate) bilingual_subtitles_enabled: bool,
    #[serde(default = "default_skip_mode")]
    pub(crate) skip_intro_mode: SkipMode,
    #[serde(default = "default_skip_mode")]
    pub(crate) skip_credits_mode: SkipMode,
    #[serde(default = "default_true")]
    pub(crate) marker_detection_enabled: bool,
    #[serde(default)]
    pub(crate) plex_account_preferences: BTreeMap<String, String>,
}

impl SkipMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Manual => "manual",
            Self::Off => "off",
        }
    }
}

fn parse_skip_mode(value: &str) -> Option<SkipMode> {
    match value.trim() {
        "automatic" => Some(SkipMode::Automatic),
        "manual" => Some(SkipMode::Manual),
        "off" => Some(SkipMode::Off),
        _ => None,
    }
}

fn skip_mode_options(selected: SkipMode) -> String {
    [
        (SkipMode::Automatic, "Automatic"),
        (SkipMode::Manual, "Manual"),
        (SkipMode::Off, "Off"),
    ]
    .into_iter()
    .map(|(mode, label)| {
        format!(
            "<option value=\"{}\"{}>{}</option>",
            mode.as_str(),
            if mode == selected { " selected" } else { "" },
            label
        )
    })
    .collect()
}

fn default_skip_mode() -> SkipMode {
    SkipMode::Manual
}

fn default_audio_language_primary() -> String {
    PLEX_DEFAULT_AUDIO_LANGUAGE.to_owned()
}

fn default_subtitle_language_primary() -> String {
    "en".to_owned()
}
fn default_subtitle_language_secondary() -> String {
    "vi".to_owned()
}

fn normalize_preferred_language(language: &str) -> Option<String> {
    let trimmed = language.trim();
    if trimmed.is_empty() {
        return None;
    }
    let code = plex_language(trimmed).0;
    let normalized = if code == "und" {
        trimmed.to_ascii_lowercase()
    } else {
        code.to_owned()
    };
    (!normalized.is_empty()).then_some(normalized)
}

pub(crate) fn configured_bilingual_languages(state: &AppState) -> (bool, String, String) {
    state
        .config
        .lock()
        .ok()
        .map(|config| {
            (
                config.bilingual_subtitles_enabled && config.subtitle_language_primary != "off",
                config.subtitle_language_primary.clone(),
                config.subtitle_language_secondary.clone(),
            )
        })
        .unwrap_or_else(|| {
            (
                false,
                default_subtitle_language_primary(),
                default_subtitle_language_secondary(),
            )
        })
}

pub(crate) fn effective_plex_language_settings(
    state: &AppState,
) -> (bool, String, Option<String>, String, String) {
    state
        .config
        .lock()
        .ok()
        .map(|config| {
            let preferred = |key: &str, fallback: &str| {
                config
                    .plex_account_preferences
                    .get(key)
                    .and_then(|language| normalize_preferred_language(language))
                    .unwrap_or_else(|| {
                        normalize_preferred_language(fallback)
                            .unwrap_or_else(|| fallback.to_owned())
                    })
            };
            let audio_secondary = config
                .audio_language_secondary
                .as_deref()
                .and_then(normalize_preferred_language);
            let subtitle_primary =
                preferred("defaultSubtitleLanguage", &config.subtitle_language_primary);
            let subtitle_secondary =
                normalize_preferred_language(&config.subtitle_language_secondary)
                    .unwrap_or_else(|| config.subtitle_language_secondary.clone());
            (
                config.bilingual_subtitles_enabled && subtitle_primary != "off",
                preferred("defaultAudioLanguage", &config.audio_language_primary),
                audio_secondary,
                subtitle_primary,
                subtitle_secondary,
            )
        })
        .unwrap_or_else(|| {
            (
                false,
                default_audio_language_primary(),
                None,
                default_subtitle_language_primary(),
                default_subtitle_language_secondary(),
            )
        })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ConfigLibrary {
    pub(crate) name: String,
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) folders: Vec<PathBuf>,
    #[serde(default = "default_true")]
    pub(crate) show_recently_added: bool,
    #[serde(default = "default_true")]
    pub(crate) show_continue_watching: bool,
    #[serde(default = "default_true")]
    pub(crate) scan_enabled: bool,
    #[serde(default = "default_scan_interval_value")]
    pub(crate) scan_interval_value: u32,
    #[serde(default = "default_scan_interval_unit")]
    pub(crate) scan_interval_unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) plex_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) plex_scanner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) plex_language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) metadata_agent_provider_group_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) plex_preferences: Option<BTreeMap<String, String>>,
}

fn default_bind() -> String {
    "0.0.0.0:32400".to_owned()
}
fn default_metadata_storage() -> String {
    "config".to_owned()
}
fn default_true() -> bool {
    true
}

pub(crate) fn default_scan_interval_value() -> u32 {
    5
}

pub(crate) fn default_scan_interval_unit() -> String {
    "minutes".to_owned()
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) config: Arc<Mutex<Config>>,
    pub(crate) config_path: Arc<PathBuf>,
    pub(crate) subtitle_checkpoints_dir: Arc<PathBuf>,
    pub(crate) db: Arc<Mutex<Connection>>,
    pub(crate) server_name: Arc<RwLock<String>>,
    pub(crate) machine_identifier: Arc<str>,
    pub(crate) root: Arc<PathBuf>,
    pub(crate) media: Arc<RwLock<Arc<Vec<Media>>>>,
    pub(crate) inventory_scan_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) plex_notifications: tokio::sync::broadcast::Sender<String>,
    pub(crate) media_info: Arc<Mutex<HashMap<String, serde_json::Value>>>,
    pub(crate) media_info_pending: Arc<Mutex<HashSet<String>>>,
    pub(crate) media_info_inspection_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) plex_stream_selections: Arc<Mutex<HashMap<u64, PlexStreamSelection>>>,
    pub(crate) plex_play_queues: Arc<Mutex<HashMap<u64, serde_json::Value>>>,
    pub(crate) next_plex_play_queue_id: Arc<AtomicU64>,
    pub(crate) plex_playback_states: Arc<Mutex<HashMap<String, String>>>,
    pub(crate) plex_remux_sessions: Arc<Mutex<HashMap<String, PlexRemuxSession>>>,
    pub(crate) plex_scrobbled_sessions: Arc<Mutex<HashSet<String>>>,
    pub(crate) plex_webhook_delivery_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) metrics: metrics::Metrics,

    pub(crate) plex_tls: PlexTls,
}

static TRANSLATION_CACHE_LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
    OnceLock::new();
static ACTIVE_TRANSLATION_JOBS: OnceLock<Mutex<HashMap<String, ActiveTranslationJob>>> =
    OnceLock::new();

struct ActiveTranslationJob {
    media_id: String,
    explicit: bool,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

const BILINGUAL_INCOMPLETE_NOTICE: &str =
    "Google Translate rate limit reached. Wait a while and try again.";
const BILINGUAL_GENERATING_NOTICE: &str = "Generating bilingual subtitles, please reload";

pub(crate) fn bilingual_translation_job_key(media: &Media, first: &str, second: &str) -> String {
    format!(
        "prepared:{}:{}:{}:{}",
        media.id,
        media.path.display(),
        first,
        second
    )
}

fn start_translation_job(
    job_key: String,
    media_id: String,
    explicit: bool,
) -> Option<Arc<std::sync::atomic::AtomicBool>> {
    let mut jobs = ACTIVE_TRANSLATION_JOBS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()?;
    if jobs.contains_key(&job_key) {
        return None;
    }
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    jobs.insert(
        job_key,
        ActiveTranslationJob {
            media_id,
            explicit,
            cancelled: cancelled.clone(),
        },
    );
    Some(cancelled)
}

fn finish_translation_job(job_key: &str) {
    if let Some(jobs) = ACTIVE_TRANSLATION_JOBS.get() {
        if let Ok(mut jobs) = jobs.lock() {
            jobs.remove(job_key);
        }
    }
}

pub(crate) fn cancel_streaming_translation_jobs(media_id: &str) {
    if let Some(jobs) = ACTIVE_TRANSLATION_JOBS.get() {
        if let Ok(jobs) = jobs.lock() {
            let mut cancelled = 0;
            for job in jobs
                .values()
                .filter(|job| job.media_id == media_id && !job.explicit)
            {
                if !job
                    .cancelled
                    .swap(true, std::sync::atomic::Ordering::AcqRel)
                {
                    cancelled += 1;
                }
            }
            if cancelled > 0 {
                info!(
                    media_id,
                    jobs = cancelled,
                    "playback stopped; cancelling streaming subtitle translation"
                );
            }
        }
    }
}

fn cancel_all_translation_jobs() {
    if let Some(jobs) = ACTIVE_TRANSLATION_JOBS.get() {
        if let Ok(jobs) = jobs.lock() {
            for job in jobs.values() {
                job.cancelled
                    .store(true, std::sync::atomic::Ordering::Release);
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct Media {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) path: PathBuf,
    pub(crate) size: u64,
    pub(crate) container: String,
    pub(crate) mime: String,
    pub(crate) metadata_revision: u64,
}

pub(crate) fn media_snapshot(state: &AppState) -> Arc<Vec<Media>> {
    state
        .media
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

pub(crate) fn load_cached_media(
    db: &Connection,
    config: &Config,
) -> Result<Vec<Media>, rusqlite::Error> {
    let metadata_revision = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let mut result = Vec::new();
    for (library_index, library) in config.libraries.iter().enumerate() {
        let folders = library
            .folders
            .iter()
            .filter_map(|configured| {
                std::fs::canonicalize(configured)
                    .ok()
                    .map(|canonical| (configured.clone(), canonical))
            })
            .collect::<Vec<_>>();
        let mut statement = db.prepare(
            "SELECT i.media_key, i.canonical_path, m.title
               FROM media_identity i
               JOIN media m ON m.media_key = i.media_key
              WHERE i.library_section = ?1
              ORDER BY i.canonical_path",
        )?;
        let rows = statement.query_map([library_index as i64 + 1], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (media_key, canonical_path, title) = row?;
            let canonical_path = PathBuf::from(canonical_path);
            let path = folders
                .iter()
                .find_map(|(configured, canonical)| {
                    canonical_path
                        .strip_prefix(canonical)
                        .ok()
                        .map(|relative| configured.join(relative))
                })
                .unwrap_or(canonical_path);
            let Some((container, mime)) = media_type(&path) else {
                continue;
            };
            result.push(Media {
                id: media_key,
                title,
                size: std::fs::metadata(&path)
                    .map(|metadata| metadata.len())
                    .unwrap_or_default(),
                path,
                container: container.to_owned(),
                mime: mime.to_owned(),
                metadata_revision,
            });
        }
    }
    Ok(result)
}

struct LocalSecondTimer;

impl FormatTime for LocalSecondTimer {
    fn format_time(&self, writer: &mut Writer<'_>) -> std::fmt::Result {
        write!(
            writer,
            "{}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S %:z")
        )
    }
}

fn default_log_tag(target: &str) -> &'static str {
    if target.starts_with("harbinger::audiobookshelf") {
        "abs"
    } else if target.starts_with("harbinger::plex") {
        "plex"
    } else {
        "hb"
    }
}

fn logfmt_needs_quotes(value: &str) -> bool {
    value.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control() || matches!(ch, '"' | '=' | '\\'))
}

fn logfmt_value(value: &str) -> String {
    use std::fmt::Write as _;

    if !logfmt_needs_quotes(value) {
        return value.to_owned();
    }
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control() => {
                let _ = write!(output, "\\u{{{:x}}}", ch as u32);
            }
            ch => output.push(ch),
        }
    }
    output.push('"');
    output
}

#[derive(Default)]
struct LogfmtFields {
    values: Vec<(String, String)>,
}

impl LogfmtFields {
    fn push(&mut self, field: &Field, value: String) {
        self.values.push((field.name().to_owned(), value));
    }
}

impl Visit for LogfmtFields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.push(field, format!("{value:?}"));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.push(field, value.to_owned());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.push(field, value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.push(field, value.to_string());
    }

    fn record_i128(&mut self, field: &Field, value: i128) {
        self.push(field, value.to_string());
    }

    fn record_u128(&mut self, field: &Field, value: u128) {
        self.push(field, value.to_string());
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.push(field, value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.push(field, value.to_string());
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.push(field, value.to_string());
    }
}

struct LogfmtFormat<T> {
    timer: T,
}

impl<S, N, T> FormatEvent<S, N> for LogfmtFormat<T>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
    T: FormatTime,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let mut timestamp = String::new();
        self.timer.format_time(&mut Writer::new(&mut timestamp))?;
        write!(writer, "ts={}", logfmt_value(&timestamp))?;

        let mut recorded = LogfmtFields::default();
        event.record(&mut recorded);
        let mut source_tag = None;
        let mut message = None;
        let mut fields = Vec::new();
        for (name, value) in recorded.values {
            match name.as_str() {
                "tag" => source_tag = Some(value),
                "message" => message = Some(value),
                _ => fields.push((name, value)),
            }
        }

        let level = event.metadata().level().as_str().to_ascii_lowercase();
        let source =
            source_tag.unwrap_or_else(|| default_log_tag(event.metadata().target()).to_owned());
        write!(writer, " level={}", logfmt_value(&level))?;
        write!(writer, " tag={}", logfmt_value(&source))?;
        for (name, value) in fields {
            write!(writer, " {}={}", name, logfmt_value(&value))?;
        }
        if let Some(message) = message {
            write!(writer, " message={}", logfmt_value(&message))?;
        }
        writeln!(writer)
    }
}

const MAX_LOG_LINES: usize = 1000;
/// The UI keeps a short live buffer; downloads retain a much longer history so
/// a slow startup is still available after the live buffer has rolled over.
const MAX_LOG_HISTORY_LINES: usize = 20_000;
const MAX_LOG_LINE_BYTES: usize = 4096;
static LOG_BUFFER: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
static LOG_HISTORY: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
static LOG_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static HTTP_REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static LOG_BROADCAST: OnceLock<tokio::sync::broadcast::Sender<LogEvent>> = OnceLock::new();

#[derive(Clone)]
pub(crate) enum LogEvent {
    Entry(u64, String),
    Cleared(u64),
}

fn log_broadcast() -> &'static tokio::sync::broadcast::Sender<LogEvent> {
    LOG_BROADCAST.get_or_init(|| tokio::sync::broadcast::channel(1024).0)
}

static WEB_SUBTITLE_SELECTION_BROADCAST: OnceLock<tokio::sync::broadcast::Sender<(u64, u64)>> =
    OnceLock::new();

pub(crate) fn web_subtitle_selection_broadcast(
) -> &'static tokio::sync::broadcast::Sender<(u64, u64)> {
    WEB_SUBTITLE_SELECTION_BROADCAST.get_or_init(|| tokio::sync::broadcast::channel(128).0)
}

#[derive(Clone)]
struct LogBufferMakeWriter;

struct LogBufferWriter {
    pending: Vec<u8>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBufferMakeWriter {
    type Writer = LogBufferWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogBufferWriter {
            pending: Vec::new(),
        }
    }
}

impl Write for LogBufferWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        io::stdout().lock().write_all(bytes)?;
        self.pending.extend_from_slice(bytes);
        while let Some(newline) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line = self.pending.drain(..=newline).collect::<Vec<_>>();
            push_log_line(String::from_utf8_lossy(&line[..line.len() - 1]).into_owned());
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stdout().flush()?;
        if !self.pending.is_empty() {
            push_log_line(String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into_owned());
        }
        Ok(())
    }
}

impl Drop for LogBufferWriter {
    fn drop(&mut self) {
        if !self.pending.is_empty() {
            push_log_line(String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into_owned());
        }
    }
}

fn strip_ansi_sequences(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\u{1b}' {
            output.push(character);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for sequence_character in chars.by_ref() {
                    if ('@'..='~').contains(&sequence_character) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(sequence_character) = chars.next() {
                    if sequence_character == '\u{7}'
                        || (sequence_character == '\u{1b}' && chars.next() == Some('\\'))
                    {
                        break;
                    }
                }
            }
            Some(_) => {}
            None => break,
        }
    }
    output
}

fn push_log_line(line: String) {
    let line = strip_ansi_sequences(&line)
        .chars()
        .take(MAX_LOG_LINE_BYTES)
        .collect::<String>();
    let mut logs = LOG_BUFFER
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if logs.len() == MAX_LOG_LINES {
        logs.pop_front();
    }
    logs.push_back(line.clone());
    let mut history = LOG_HISTORY
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if history.len() == MAX_LOG_HISTORY_LINES {
        history.pop_front();
    }
    history.push_back(line.clone());
    let sequence = LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let _ = log_broadcast().send(LogEvent::Entry(sequence, line));
}

pub(crate) fn clear_log_lines() {
    if let Some(logs) = LOG_BUFFER.get() {
        logs.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
    if let Some(history) = LOG_HISTORY.get() {
        history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
    let sequence = LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let _ = log_broadcast().send(LogEvent::Cleared(sequence));
}

fn recent_log_lines() -> Vec<String> {
    LOG_BUFFER
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .map(|logs| logs.iter().cloned().collect())
        .unwrap_or_default()
}

pub(crate) fn recent_log_history() -> Vec<String> {
    LOG_HISTORY
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .map(|logs| logs.iter().cloned().collect())
        .unwrap_or_default()
}

fn recent_log_snapshot() -> (u64, Vec<String>) {
    let logs = LOG_BUFFER
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    (
        LOG_SEQUENCE.load(Ordering::Relaxed),
        logs.iter().cloned().collect(),
    )
}

type LogFilterHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;
static LOG_FILTER: OnceLock<LogFilterHandle> = OnceLock::new();
static DEBUG_LOGGING: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_log_level(level: &str) -> Result<(), String> {
    let (level, debug_enabled) = match level.trim().to_ascii_lowercase().as_str() {
        "debug" => ("debug", true),
        "info" | "" => ("info", false),
        _ => return Err("Unknown log level".to_owned()),
    };
    DEBUG_LOGGING.store(debug_enabled, Ordering::Relaxed);
    let Some(handle) = LOG_FILTER.get() else {
        return Ok(());
    };
    handle
        .modify(|filter| *filter = EnvFilter::new(level))
        .map_err(|error| error.to_string())
}

pub(crate) fn debug_logging_enabled() -> bool {
    DEBUG_LOGGING.load(Ordering::Relaxed)
}

pub(super) async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (filter, handle) = reload::Layer::new(
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
    );
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(LogBufferMakeWriter)
                .event_format(LogfmtFormat {
                    timer: LocalSecondTimer,
                }),
        )
        .init();
    let _ = LOG_FILTER.set(handle);
    if let Ok(build_date) = std::env::var("IMAGE_BUILD_DATE") {
        if build_date != "unknown" {
            info!(image_build_date = %build_date, "image build date");
        }
    }
    let args = Args::parse();
    if run_server(args).await? == RunOutcome::Reload {
        restart_process()?;
    }
    Ok(())
}

#[derive(PartialEq)]
enum RunOutcome {
    Reload,
    Shutdown,
}

fn config_path_in_dir(config_dir: &std::path::Path) -> Result<PathBuf, std::io::Error> {
    let new_path = config_dir.join("config.toml");
    let old_path = config_dir.join("mplex.toml");
    if !new_path.exists() && old_path.exists() {
        std::fs::rename(old_path, &new_path)?;
    }
    Ok(new_path)
}

async fn run_server(args: Args) -> Result<RunOutcome, Box<dyn std::error::Error>> {
    let startup_started = Instant::now();
    let config_arg = args.config.unwrap_or_else(|| PathBuf::from("config"));
    let config_path = if config_arg
        .extension()
        .is_some_and(|extension| extension == "toml")
    {
        if config_arg.is_absolute() {
            config_arg
        } else {
            std::env::current_dir()?.join(config_arg)
        }
    } else {
        let config_dir = if config_arg.is_absolute() {
            config_arg
        } else {
            std::env::current_dir()?.join(config_arg)
        };
        config_path_in_dir(&config_dir)?
    };
    let mut config = load_config(&config_path)?;
    if let Some(bind) = args.bind {
        config.bind = bind.to_string();
    }
    let (db, machine_identifier) = open_database(&config_path)?;
    apply_stored_settings(&db, &mut config);
    if let Some(level) = explicit_setting_from_db(&db, "log_level") {
        let _ = set_log_level(&level);
    }
    save_config(&config_path, &config)?;
    let server_name = db
        .query_row(
            "SELECT server_name FROM server_identity WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "harbinger".to_owned());
    migrate_library_model(&db)?;
    sync_config_libraries(&db, &config)?;
    let root = config
        .libraries
        .iter()
        .flat_map(|library| library.folders.iter())
        .find_map(|path| std::fs::canonicalize(path).ok())
        .unwrap_or_else(|| {
            config_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf()
        });
    let media = load_cached_media(&db, &config)?;
    info!(
        items = media.len(),
        elapsed_ms = startup_started.elapsed().as_millis(),
        "loaded cached media inventory"
    );

    let gdm_machine_identifier = machine_identifier.clone();
    let cert_dir = config_path
        .parent()
        .unwrap_or(std::path::Path::new("config"))
        .join("certs");
    let plex_tls = PlexTls::load(cert_dir).map_err(std::io::Error::other)?;
    let (plex_notifications, _) = tokio::sync::broadcast::channel(64);
    let state = AppState {
        config: Arc::new(Mutex::new(config.clone())),
        subtitle_checkpoints_dir: Arc::new(
            config_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("translated_subtitles"),
        ),
        config_path: Arc::new(config_path),
        db: Arc::new(Mutex::new(db)),
        server_name: Arc::new(RwLock::new(server_name.clone())),
        machine_identifier: Arc::from(machine_identifier),
        root: Arc::new(root),
        media: Arc::new(RwLock::new(Arc::new(media))),
        inventory_scan_lock: Arc::new(tokio::sync::Mutex::new(())),
        plex_notifications,
        media_info: Arc::new(Mutex::new(HashMap::new())),
        media_info_pending: Arc::new(Mutex::new(HashSet::new())),
        media_info_inspection_lock: Arc::new(tokio::sync::Mutex::new(())),
        plex_stream_selections: Arc::new(Mutex::new(HashMap::new())),
        plex_play_queues: Arc::new(Mutex::new(HashMap::new())),
        next_plex_play_queue_id: Arc::new(AtomicU64::new(1)),
        plex_playback_states: Arc::new(Mutex::new(HashMap::new())),
        plex_remux_sessions: Arc::new(Mutex::new(HashMap::new())),
        plex_scrobbled_sessions: Arc::new(Mutex::new(HashSet::new())),
        plex_webhook_delivery_lock: Arc::new(tokio::sync::Mutex::new(())),
        metrics: metrics::Metrics::new(),

        plex_tls,
    };
    let migration_started = Instant::now();
    migrate_legacy_database_metadata(&state);
    info!(
        elapsed_ms = migration_started.elapsed().as_millis(),
        "startup metadata migration finished"
    );
    let scan_state = state.clone();
    let inventory_scan_task = tokio::spawn(library_scan_scheduler(scan_state));
    let metrics_task = tokio::spawn(metrics::run_sampler(state.metrics.clone()));
    let remux_cleanup_state = state.clone();
    let remux_cleanup_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(300)).await;
            prune_plex_remux_sessions(&remux_cleanup_state);
        }
    });
    let server_tls = state.plex_tls.clone();
    let publication_state = state.clone();
    let startup_server_name = server_name.clone();
    let publication_task = tokio::spawn(async move {
        let saved_registration = publication_state
            .db
            .lock()
            .ok()
            .and_then(|connection| plex_startup_registration(&connection));
        if let Some((server_token, was_claimed)) = saved_registration {
            if !was_claimed {
                info!("retrying Plex server registration with the saved server token");
            }
            let client = reqwest::Client::new();
            if let Err(error) = publication_state
                .plex_tls
                .ensure_certificate(
                    &client,
                    &publication_state.machine_identifier,
                    &startup_server_name,
                    &server_token,
                )
                .await
            {
                warn!(error = %error, "could not ensure Plex TLS certificate at startup");
            }
            match publish_plex_connections(
                &client,
                &publication_state,
                &startup_server_name,
                &server_token,
            )
            .await
            {
                Ok(()) => {
                    if let Ok(connection) = publication_state.db.lock() {
                        if let Err(error) = set_plex_registration_state(&connection, true, None) {
                            warn!(%error, "could not save successful Plex registration state");
                        }
                    }
                    run_plex_presence(&client, &publication_state, &server_token).await;
                }
                Err(error) => {
                    if let Ok(connection) = publication_state.db.lock() {
                        if let Err(save_error) =
                            set_plex_registration_state(&connection, false, Some(&error))
                        {
                            warn!(%save_error, "could not save failed Plex registration state");
                        }
                    }
                    warn!(error = %error, "could not refresh Plex connection URLs at startup");
                }
            }
        }
    });
    let has_audiobooks = state.config.lock().is_ok_and(|config| {
        config
            .libraries
            .iter()
            .any(|library| is_audiobook_library(&library.kind))
    });
    if has_audiobooks {
        let warm_state = state.clone();
        tokio::task::spawn_blocking(move || {
            let _ = crate::audiobookshelf::catalog::Catalog::load(&warm_state);
        });
    }
    let plex_app = build_plex_listener_app(state.clone());
    let web_app = build_web_listener_app(state.clone());
    let abs_app = crate::audiobookshelf::router(state.clone());
    let listener = tokio::net::TcpListener::bind(config.bind.parse::<SocketAddr>()?).await?;
    let address = listener.local_addr()?;
    let web_address = SocketAddr::new(address.ip(), WEB_UI_PORT);
    let web_listener = tokio::net::TcpListener::bind(web_address).await?;
    let abs_address = SocketAddr::new(address.ip(), ABS_PORT);
    let abs_listener = match tokio::net::TcpListener::bind(abs_address).await {
        Ok(listener) => Some(listener),
        Err(error) => {
            warn!(%error, address = %abs_address, "could not bind Audiobookshelf compatibility listener");
            None
        }
    };
    info!(
        elapsed_ms = startup_started.elapsed().as_millis(),
        "startup listeners are ready"
    );
    let gdm_task = tokio::spawn(run_gdm_responder(
        gdm_machine_identifier,
        address.port(),
        state.server_name.clone(),
    ));
    let host = if address.ip().is_unspecified() {
        "127.0.0.1".to_owned()
    } else {
        format!("{}", address.ip())
    };
    print!("\n");
    print_startup_banner();
    println!(
        "harbinger Plex API: http://{}:{}/\nHarbinger web UI: http://{}:{}/\nHarbinger Audiobookshelf API: http://{}:{}/\n",
        host,
        address.port(),
        host,
        web_address.port(),
        host,
        abs_address.port()
    );
    #[cfg(unix)]
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let (web_shutdown_tx, web_shutdown_rx) = tokio::sync::oneshot::channel();
    let (abs_shutdown_tx, abs_shutdown_rx) = tokio::sync::oneshot::channel();
    let plex_server = server_tls.serve_until(listener, plex_app, async move {
        let _ = shutdown_rx.await;
    });
    let web_server = async move {
        axum::serve(web_listener, web_app)
            .with_graceful_shutdown(async move {
                let _ = web_shutdown_rx.await;
            })
            .await
    };
    let abs_server = async move {
        match abs_listener {
            Some(listener) => {
                axum::serve(listener, abs_app)
                    .with_graceful_shutdown(async move {
                        let _ = abs_shutdown_rx.await;
                    })
                    .await
            }
            None => Ok(()),
        }
    };
    tokio::pin!(plex_server);
    tokio::pin!(web_server);
    tokio::pin!(abs_server);
    #[cfg(unix)]
    let outcome = tokio::select! {
        result = &mut plex_server => {
            result?;
            RunOutcome::Shutdown
        }
        result = &mut web_server => {
            result?;
            RunOutcome::Shutdown
        }
        result = &mut abs_server => {
            result?;
            RunOutcome::Shutdown
        }
        _ = hangup.recv() => {
            info!("SIGHUP received; gracefully reloading Harbinger");
            RunOutcome::Reload
        }
        _ = tokio::signal::ctrl_c() => {
            info!("shutdown signal received");
            RunOutcome::Shutdown
        }
    };
    #[cfg(not(unix))]
    let outcome = tokio::select! {
        result = &mut plex_server => {
            result?;
            RunOutcome::Shutdown
        }
        result = &mut web_server => {
            result?;
            RunOutcome::Shutdown
        }
        result = &mut abs_server => {
            result?;
            RunOutcome::Shutdown
        }
        _ = tokio::signal::ctrl_c() => {
            info!("shutdown signal received");
            RunOutcome::Shutdown
        }
    };
    let _ = shutdown_tx.send(());
    let _ = web_shutdown_tx.send(());
    let _ = abs_shutdown_tx.send(());
    let plex_drain = tokio::time::timeout(Duration::from_secs(5), &mut plex_server);
    let web_drain = tokio::time::timeout(Duration::from_secs(5), &mut web_server);
    let abs_drain = tokio::time::timeout(Duration::from_secs(5), &mut abs_server);
    let (plex_result, web_result, abs_result) = tokio::join!(plex_drain, web_drain, abs_drain);
    if plex_result.is_err() || web_result.is_err() || abs_result.is_err() {
        warn!("timed out waiting for active connections to close during shutdown");
    }
    gdm_task.abort();
    publication_task.abort();
    inventory_scan_task.abort();
    metrics_task.abort();
    remux_cleanup_task.abort();
    Ok(outcome)
}

#[cfg(unix)]
fn restart_process() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::process::CommandExt;

    let executable = std::env::current_exe()?;
    let error = Command::new(executable)
        .args(std::env::args_os().skip(1))
        .exec();
    Err(error.into())
}

#[cfg(not(unix))]
fn restart_process() -> Result<(), Box<dyn std::error::Error>> {
    Err("SIGHUP process reload is only available on Unix".into())
}

#[cfg(test)]
fn build_app(state: AppState) -> Router {
    build_base_app(state).layer(middleware::from_fn(
        |request: Request<Body>, next: Next| async move {
            log_http_exchange("plex", request, next).await
        },
    ))
}

/// Build the shared Plex/web router without the request-log layer so listener
/// variants can put their own surface gate *inside* request logging. That way
/// a request sent to the wrong port is still logged before it is rejected.
fn build_base_app(state: AppState) -> Router {
    Router::new()
        .merge(api::web_routes())
        .merge(crate::plex::api::plex_routes())
        .fallback(log_not_found)
        .layer(middleware::from_fn(plex_cors))
        .layer(middleware::from_fn_with_state(state.clone(), web_auth_gate))
        .with_state(state)
}

pub(crate) fn next_http_request_id() -> u64 {
    HTTP_REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1
}

async fn log_http_exchange(tag: &'static str, request: Request<Body>, next: Next) -> Response {
    if request.uri().path() == "/settings/logs/socket" {
        return next.run(request).await;
    }
    let request_id = next_http_request_id();
    let remote_ip = request
        .extensions()
        .get::<SocketAddr>()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let query = sanitized_query(request.uri());
    let accept = request.headers().get(header::ACCEPT).cloned();
    let plex_product = request
        .headers()
        .get("X-Plex-Product")
        .and_then(|value| value.to_str().ok())
        .or_else(|| plex_query_value(request.uri(), "X-Plex-Product"))
        .map(str::to_owned);
    let client_profile = request
        .headers()
        .get("X-Plex-Client-Profile-Name")
        .and_then(|value| value.to_str().ok())
        .or_else(|| plex_query_value(request.uri(), "X-Plex-Client-Profile-Name"))
        .map(str::to_owned);
    let client_platform = request
        .headers()
        .get("X-Plex-Platform")
        .and_then(|value| value.to_str().ok())
        .or_else(|| plex_query_value(request.uri(), "X-Plex-Platform"))
        .map(str::to_owned);
    let token_present = plex_request_token(request.headers(), request.uri()).is_some();
    // Artwork and favicon requests are high-volume and rarely useful at the
    // default level; keep them available when debug logging is enabled.
    let noisy = path == "/favicon.ico"
        || path == "/web/ping"
        || path == "/web/artwork"
        || path.starts_with("/web/artwork/")
        || path.starts_with("/web/episode-artwork/");
    if noisy {
        debug!(
            tag,
            event = "http_request",
            id = request_id,
            remote_ip = %remote_ip,
            method = %method,
            path = %path,
            query = ?query,
            accept = ?accept,
            plex_product = ?plex_product,
            client_profile = ?client_profile,
            client_platform = ?client_platform,
            token_present
        );
    } else {
        info!(
            tag,
            event = "http_request",
            id = request_id,
            remote_ip = %remote_ip,
            method = %method,
            path = %path,
            query = ?query,
            accept = ?accept,
            plex_product = ?plex_product,
            client_profile = ?client_profile,
            client_platform = ?client_platform,
            token_present
        );
    }
    let started = Instant::now();
    let response = next.run(request).await;
    if response
        .extensions()
        .get::<SuppressHttpResponseLog>()
        .is_none()
    {
        if noisy {
            debug!(
                tag,
                event = "http_response",
                id = request_id,
                status = response.status().as_u16(),
                latency_ms = started.elapsed().as_millis()
            );
        } else {
            info!(
                tag,
                event = "http_response",
                id = request_id,
                status = response.status().as_u16(),
                latency_ms = started.elapsed().as_millis()
            );
        }
    }
    response
}

#[derive(Clone, Copy)]
pub(crate) enum ListenerSurface {
    Web,
    Plex,
}

fn build_web_listener_app(state: AppState) -> Router {
    build_base_app(state)
        .layer(middleware::from_fn(
            |mut request: Request<Body>, next: Next| async move {
                if !is_web_surface_path(request.uri().path()) {
                    return StatusCode::NOT_FOUND.into_response();
                }
                request.extensions_mut().insert(ListenerSurface::Web);
                next.run(request).await
            },
        ))
        .layer(middleware::from_fn(
            |request: Request<Body>, next: Next| async move {
                log_http_exchange("hb", request, next).await
            },
        ))
}

pub(crate) fn sanitized_query(uri: &Uri) -> Option<String> {
    let query = uri.query()?;
    let mut parts = Vec::new();
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        let key = key.into_owned();
        let sensitive = matches!(
            key.to_ascii_lowercase().as_str(),
            "token"
                | "x-plex-token"
                | "password"
                | "auth"
                | "access_token"
                | "refresh_token"
                | "pin"
                | "code"
        );
        parts.push(if sensitive {
            format!("{key}=[redacted]")
        } else {
            format!("{key}={value}")
        });
    }
    Some(parts.join("&"))
}

fn build_plex_listener_app(state: AppState) -> Router {
    build_base_app(state)
        .layer(middleware::from_fn(
            |mut request: Request<Body>, next: Next| async move {
                if is_web_surface_path(request.uri().path())
                    && request.uri().path() != "/"
                    && request.uri().path() != "/web/index.html"
                    && request.uri().path() != "/web/"
                {
                    return StatusCode::NOT_FOUND.into_response();
                }
                request.extensions_mut().insert(ListenerSurface::Plex);
                next.run(request).await
            },
        ))
        .layer(middleware::from_fn(
            |request: Request<Body>, next: Next| async move {
                log_http_exchange("plex", request, next).await
            },
        ))
}

fn is_web_surface_path(path: &str) -> bool {
    path == "/"
        || path == "/favicon.ico"
        || path == "/auth"
        || path.starts_with("/auth/")
        || path == "/web"
        || path.starts_with("/web/")
        || path == "/settings"
        || path.starts_with("/settings/")
}

async fn run_gdm_responder(
    machine_identifier: String,
    port: u16,
    configured_name: Arc<RwLock<String>>,
) {
    let multicast = std::net::Ipv4Addr::new(239, 0, 0, 250);
    for gdm_port in [32410, 32412, 32413, 32414] {
        let socket = match std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, gdm_port)) {
            Ok(socket) => socket,
            Err(error) => {
                warn!(%error, gdm_port, "Plex GDM discovery port unavailable");
                continue;
            }
        };
        if let Err(error) = socket.join_multicast_v4(&multicast, &std::net::Ipv4Addr::UNSPECIFIED) {
            warn!(%error, gdm_port, "could not join Plex GDM multicast group");
        }
        if let Err(error) = socket.set_nonblocking(true) {
            warn!(%error, gdm_port, "could not configure Plex GDM socket");
            continue;
        }
        let Ok(socket) = tokio::net::UdpSocket::from_std(socket) else {
            warn!(gdm_port, "could not hand Plex GDM socket to Tokio");
            continue;
        };
        let configured_name = configured_name.clone();
        let machine_identifier = machine_identifier.clone();
        tokio::spawn(async move {
            let mut buffer = [0u8; 2048];
            loop {
                let Ok((size, peer)) = socket.recv_from(&mut buffer).await else {
                    return;
                };
                if buffer[..size].starts_with(b"M-SEARCH * HTTP/1.") {
                    let name = configured_name
                        .read()
                        .map(|name| name.clone())
                        .unwrap_or_else(|_| "harbinger".to_owned());
                    let response = gdm_message("HTTP/1.0 200 OK", &machine_identifier, port, &name);
                    let _ = socket.send_to(response.as_bytes(), peer).await;
                }
            }
        });
    }

    let name = configured_name
        .read()
        .map(|name| name.clone())
        .unwrap_or_else(|_| "harbinger".to_owned());
    let announcement = gdm_message("HELLO", &machine_identifier, port, &name);
    let announcement_socket = std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, 0));
    if let Ok(announcement_socket) = announcement_socket {
        let _ = announcement_socket.send_to(announcement.as_bytes(), (multicast, 32413));
    }
}

fn gdm_message(prefix: &str, machine_identifier: &str, port: u16, name: &str) -> String {
    let updated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!(
        "{prefix}\r\nContent-Type: plex/media-server\r\nResource-Identifier: {machine_identifier}\r\nName: {name}\r\nPort: {port}\r\nUpdated-At: {updated_at}\r\nVersion: 1.43.4.10903-e5521bd8c\r\n\r\n"
    )
}

fn startup_banner() -> &'static str {
    r#"
|              |    o
|---.,---.,---.|---..,---.,---.,---.,---.
|   |,---||    |   |||   ||   ||---'|
`   '`---^`    `---'``   '`---|`---'`
                          `---'`
"#
}

fn print_startup_banner() {
    println!("{}", startup_banner());
}

async fn scan_media(dir: &PathBuf) -> Result<Vec<Media>, Box<dyn std::error::Error>> {
    let mut directories = vec![dir.clone()];
    let mut paths = Vec::new();
    while let Some(directory) = directories.pop() {
        let mut entries = tokio::fs::read_dir(&directory).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if is_hidden_path(&path) {
                continue;
            }
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                directories.push(path);
            } else if file_type.is_file() && media_type(&path).is_some() {
                paths.push(path);
            }
        }
    }
    paths.sort_by_key(|path| path.file_name().map(|name| name.to_ascii_lowercase()));

    let metadata_revision = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let mut result = Vec::with_capacity(paths.len());
    for (index, path) in paths.into_iter().enumerate() {
        let metadata = tokio::fs::metadata(&path).await?;
        let (container, mime) = media_type(&path).expect("media type was checked during scan");
        let title = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Video")
            .to_owned();
        result.push(Media {
            id: (index + 1).to_string(),
            title,
            path,
            size: metadata.len(),
            container: container.to_owned(),
            mime: mime.to_owned(),
            metadata_revision,
        });
    }
    Ok(result)
}

fn media_type(path: &std::path::Path) -> Option<(&'static str, &'static str)> {
    match path
        .extension()
        .and_then(|x| x.to_str())
        .map(|x| x.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp4") | Some("m4v") => Some(("mp4", "video/mp4")),
        Some("mkv") => Some(("mkv", "video/x-matroska")),
        Some("webm") => Some(("webm", "video/webm")),
        _ => None,
    }
}

pub(crate) fn find_media(state: &AppState, id: &str) -> Option<Media> {
    let public_rating_key = id.parse::<u64>().ok();
    media_snapshot(state)
        .iter()
        .find(|item| item.id == id || public_rating_key == Some(plex_rating_key(item)))
        .cloned()
}

fn persist_media_records(
    connection: &Connection,
    root: &std::path::Path,
    media: &[Media],
) -> Result<(), rusqlite::Error> {
    let transaction = connection.unchecked_transaction()?;
    for item in media {
        let path = item
            .path
            .strip_prefix(root)
            .unwrap_or(&item.path)
            .to_string_lossy();
        transaction.execute(
            "INSERT INTO media (media_key, path, title) VALUES (?1, ?2, ?3)
             ON CONFLICT(media_key) DO UPDATE SET path = excluded.path, title = excluded.title",
            (&item.id, path.as_ref(), &item.title),
        )?;
    }
    transaction.commit()
}

fn assign_media_ids(
    connection: &Connection,
    library_section: usize,
    canonical_root: &std::path::Path,
    configured_root: &std::path::Path,
    media: &mut [Media],
) -> Result<(), rusqlite::Error> {
    const MAX_ITEM_ID: u64 = (1 << 20) - 1;

    let transaction = connection.unchecked_transaction()?;
    let section = library_section as i64;
    let id_prefix = format!("{library_section}-");
    let mut identity_by_path = HashMap::<String, String>::new();
    let mut legacy_by_path = HashMap::<String, Vec<String>>::new();
    let mut reserved_keys = std::collections::HashSet::new();
    let mut assigned_keys = std::collections::HashSet::new();
    let mut next_id = 1_u64;
    {
        let mut statement = transaction.prepare(
            "SELECT canonical_path, media_key FROM media_identity WHERE library_section = ?1",
        )?;
        let rows = statement.query_map([section], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (path, media_key) = row?;
            if let Some(item_id) = media_key
                .strip_prefix(&id_prefix)
                .and_then(|value| value.parse::<u64>().ok())
            {
                next_id = next_id.max(item_id.saturating_add(1));
            }
            reserved_keys.insert(media_key.clone());
            identity_by_path.insert(path, media_key);
        }
    }
    {
        let mut statement =
            transaction.prepare("SELECT media_key, path FROM media WHERE media_key LIKE ?1")?;
        let rows = statement.query_map([format!("{id_prefix}%")], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (media_key, relative_path) = row?;
            if let Some(item_id) = media_key
                .strip_prefix(&id_prefix)
                .and_then(|value| value.parse::<u64>().ok())
            {
                next_id = next_id.max(item_id.saturating_add(1));
            }
            reserved_keys.insert(media_key.clone());
            legacy_by_path
                .entry(relative_path)
                .or_default()
                .push(media_key);
        }
    }
    for item in media {
        let relative_path = item
            .path
            .strip_prefix(configured_root)
            .or_else(|_| item.path.strip_prefix(canonical_root))
            .unwrap_or(&item.path)
            .to_path_buf();
        let canonical_path = canonical_root
            .join(&relative_path)
            .to_string_lossy()
            .into_owned();
        let legacy_path = relative_path.to_string_lossy().into_owned();
        let existing_key = identity_by_path.get(&canonical_path).cloned().or_else(|| {
            legacy_by_path.get(&legacy_path).and_then(|keys| {
                keys.iter()
                    .find(|key| {
                        !assigned_keys.contains(*key)
                            && !identity_by_path.values().any(|existing| existing == *key)
                    })
                    .cloned()
            })
        });
        let id_is_non_numeric = item.id.split_once('-').map_or_else(
            || item.id.parse::<u64>().is_err(),
            |(section, item_id)| section.parse::<u64>().is_err() || item_id.parse::<u64>().is_err(),
        );
        let current_key = (item.id.starts_with(&id_prefix) || id_is_non_numeric)
            .then(|| item.id.clone())
            .filter(|key| !reserved_keys.contains(key) && !assigned_keys.contains(key));
        let media_key = if let Some(key) = existing_key.or(current_key) {
            key
        } else {
            loop {
                if next_id > MAX_ITEM_ID {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                let key = format!("{id_prefix}{next_id}");
                next_id = next_id.saturating_add(1);
                if !reserved_keys.contains(&key) && !assigned_keys.contains(&key) {
                    break key;
                }
            }
        };
        assigned_keys.insert(media_key.clone());
        reserved_keys.insert(media_key.clone());
        item.id.clone_from(&media_key);
        transaction.execute(
            "INSERT INTO media_identity (library_section, canonical_path, media_key) VALUES (?1, ?2, ?3)
             ON CONFLICT(library_section, canonical_path) DO UPDATE SET media_key = excluded.media_key",
            (section, canonical_path, media_key),
        )?;
    }
    transaction.commit()
}

/// Give the web UI and Plex endpoints a chance to serve the cached inventory
/// before the first automatic scan starts touching media storage.
const AUTOMATIC_SCAN_STARTUP_GRACE: Duration = Duration::from_secs(30);

fn library_scan_interval(library: &ConfigLibrary) -> Duration {
    let value = u64::from(library.scan_interval_value.max(1));
    let seconds = match library.scan_interval_unit.as_str() {
        "seconds" => value,
        "hours" => value.saturating_mul(60 * 60),
        _ => value.saturating_mul(60),
    };
    Duration::from_secs(seconds)
}

pub(crate) fn metadata_scan_assets_only(newly_found: usize, match_unmatched: bool) -> bool {
    newly_found == 0 && !match_unmatched
}

pub(crate) async fn scan_library_inventory(
    state: &AppState,
    library_id: usize,
    match_unmatched: bool,
    force: bool,
    manual: bool,
) -> Result<usize, String> {
    let _scan_guard = state.inventory_scan_lock.lock().await;
    let library = state
        .config
        .lock()
        .map_err(|_| "Configuration is unavailable".to_owned())?
        .libraries
        .get(library_id)
        .cloned()
        .ok_or_else(|| "Library does not exist".to_owned())?;
    let _scan_activity = metrics::ActivityGuard::begin(
        &state.metrics,
        format!("scan:{library_id}"),
        "library scan",
        library.name.clone(),
        "Scanning configured folders",
    );
    if is_audiobook_library(&library.kind) {
        let count_library = library.clone();
        let count = tokio::task::spawn_blocking(move || {
            crate::audiobookshelf::catalog::count_audio_files(&count_library)
        })
        .await
        .unwrap_or_else(|error| {
            warn!(library = %library.name, %error, "audiobook file count failed");
            0
        });
        crate::audiobookshelf::catalog::invalidate_cache();
        let matched = if match_unmatched {
            crate::audiobookshelf::matching::match_library(state, library_id, &library, manual)
                .await
        } else {
            0
        };
        if matched > 0 {
            crate::audiobookshelf::catalog::invalidate_cache();
        }
        info!(library = %library.name, count, matched, "audiobook file scan completed");
        return Ok(count);
    }
    let mut roots = Vec::with_capacity(library.folders.len());
    for configured_root in &library.folders {
        let canonical_root = tokio::fs::canonicalize(configured_root)
            .await
            .map_err(|error| format!("Could not access configured library folder: {error}"))?;
        roots.push((configured_root.clone(), canonical_root));
    }
    let current = media_snapshot(state);
    let id_prefix = format!("{}-", library_id + 1);
    let mut existing_ids = HashMap::new();
    for media in current.iter() {
        for (root_index, (configured_root, canonical_root)) in roots.iter().enumerate() {
            let relative = media
                .path
                .strip_prefix(configured_root)
                .or_else(|_| media.path.strip_prefix(canonical_root));
            if let Ok(relative) = relative {
                existing_ids.insert((root_index, relative.to_path_buf()), media.id.clone());
                break;
            }
        }
    }
    let mut next_id = current
        .iter()
        .filter_map(|media| media.id.strip_prefix(&id_prefix)?.parse::<u64>().ok())
        .max()
        .unwrap_or_default()
        .saturating_add(1);
    let mut scanned_roots = Vec::new();
    let mut newly_found = Vec::new();
    for (root_index, (configured_root, canonical_root)) in roots.iter().enumerate() {
        let mut media = scan_media(canonical_root)
            .await
            .map_err(|error| format!("Could not scan library folder: {error}"))?;
        for item in &mut media {
            let relative = item
                .path
                .strip_prefix(canonical_root)
                .map_err(|error| format!("Scanned file escaped its library folder: {error}"))?
                .to_path_buf();
            if let Some(id) = existing_ids.get(&(root_index, relative.clone())) {
                item.id.clone_from(id);
            } else {
                let media_path = configured_root.join(&relative);
                info!(library = %library.name, path = %media_path.display(), media_kind = %library.kind, container = %item.container, "new media detected");
                newly_found.push(media_path);
                item.id = format!("{id_prefix}{next_id}");
                next_id = next_id.saturating_add(1);
            }
            item.path = configured_root.join(relative);
        }
        scanned_roots.push((configured_root.clone(), canonical_root.clone(), media));
    }
    {
        let connection = state
            .db
            .lock()
            .map_err(|_| "Media database is unavailable".to_owned())?;
        for (configured_root, canonical_root, media) in &mut scanned_roots {
            assign_media_ids(
                &connection,
                library_id + 1,
                canonical_root,
                configured_root,
                media,
            )
            .map_err(|error| format!("Could not assign stable media IDs: {error}"))?;
            persist_media_records(&connection, configured_root, media)
                .map_err(|error| format!("Could not save scanned media: {error}"))?;
        }
        mark_library_updated(&connection, &library.name, true)
            .map_err(|error| format!("Could not save library scan time: {error}"))?;
    }
    let scanned = scanned_roots
        .into_iter()
        .flat_map(|(_, _, media)| media)
        .collect::<Vec<_>>();
    let sidecar_state = state.clone();
    let sidecar_items = scanned.clone();
    if let Err(error) = tokio::task::spawn_blocking(move || {
        sync_marker_sidecars(&sidecar_state, &sidecar_items);
    })
    .await
    {
        warn!(library = %library.name, %error, "skip marker sidecar sync failed");
    }
    let scanned_count = scanned.len();
    let belongs_to_library = |media: &Media| {
        roots.iter().any(|(configured_root, canonical_root)| {
            media.path.starts_with(configured_root) || media.path.starts_with(canonical_root)
        })
    };
    let scanned_ids = scanned
        .iter()
        .map(|media| media.id.as_str())
        .collect::<HashSet<_>>();
    let previous_count = current
        .iter()
        .filter(|media| belongs_to_library(media))
        .count();
    let retained_media = current
        .iter()
        .filter(|media| {
            belongs_to_library(media)
                && !scanned_ids.contains(media.id.as_str())
                && media.path.is_file()
                && media_type(&media.path).is_some()
        })
        .cloned()
        .collect::<Vec<_>>();
    for media in &retained_media {
        warn!(
            library = %library.name,
            id = %media.id,
            path = %media.path.display(),
            "retaining existing media omitted by library scan"
        );
    }
    let removed_media = current
        .iter()
        .filter(|media| {
            belongs_to_library(media)
                && !scanned_ids.contains(media.id.as_str())
                && !media.path.exists()
        })
        .cloned()
        .collect::<Vec<_>>();
    info!(
        library = %library.name,
        previous_count,
        scanned_count,
        retained_count = retained_media.len(),
        removed_count = removed_media.len(),
        "library inventory scan completed"
    );
    let insertion_index = current
        .iter()
        .position(belongs_to_library)
        .unwrap_or(current.len());
    let mut updated = Vec::with_capacity(current.len() + scanned_count + retained_media.len());
    for (index, media) in current.iter().enumerate() {
        if index == insertion_index {
            updated.extend(scanned.iter().cloned());
            updated.extend(retained_media.iter().cloned());
        }
        if !belongs_to_library(media) {
            updated.push(media.clone());
        }
    }
    if insertion_index == current.len() {
        updated.extend(scanned);
        updated.extend(retained_media);
    }
    // Only warm inspection for files discovered by this scan. Existing items
    // are inspected lazily on demand, so a routine scan must not walk the
    // whole library again.
    let warm_media = updated
        .iter()
        .filter(|media| {
            belongs_to_library(media)
                && media_type(&media.path).is_some()
                && newly_found.contains(&media.path)
        })
        .cloned()
        .collect::<Vec<_>>();
    {
        let mut media = state
            .media
            .write()
            .map_err(|_| "Media inventory is unavailable".to_owned())?;
        *media = Arc::new(updated);
    }
    drop(_scan_guard);
    if !removed_media.is_empty() {
        let timeline_entries = removed_media
            .iter()
            .map(|media| {
                serde_json::json!({
                    "identifier": "com.plexapp.plugins.library",
                    "sectionID": (library_id + 1).to_string(),
                    "itemID": plex_rating_key(media).to_string(),
                    "type": 1,
                    "title": media.title,
                    "state": 9,
                    "updatedAt": SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                })
            })
            .collect::<Vec<_>>();
        let notification = serde_json::json!({
            "NotificationContainer": {
                "type": "timeline",
                "size": timeline_entries.len(),
                "TimelineEntry": timeline_entries
            }
        });
        let _ = state.plex_notifications.send(notification.to_string());
    }
    if !warm_media.is_empty() {
        let warm_state = state.clone();
        let warm_library_name = library.name.clone();
        tokio::spawn(async move {
            let mut inspected = 0usize;
            for media in &warm_media {
                if ensure_media_info(&warm_state, media).await.is_some() {
                    inspected += 1;
                }
            }
            debug!(
                library = %warm_library_name,
                items = warm_media.len(),
                inspected,
                "background media inspection warm-up completed"
            );
        });
    }
    if is_metadata_library(&library.kind) {
        let assets_only = metadata_scan_assets_only(newly_found.len(), match_unmatched);
        if !assets_only {
            info!(library = %library.name, count = newly_found.len(), "automatic metadata detection started");
        }
        let response = scan_library_metadata(
            State(state.clone()),
            Path(library_id),
            Query(ScanQuery {
                force: Some(force),
                path: None,
                refresh: None,
                assets_only: Some(assets_only),
            }),
        )
        .await;
        if !assets_only {
            info!(library = %library.name, status = response.status().as_u16(), "automatic metadata detection finished");
        }
    }
    if matches!(library.kind.as_str(), "tv" | "movie") {
        let detection_state = state.clone();
        tokio::spawn(async move {
            let mode = if manual { "manual" } else { "automatic" };
            if let Err(error) = detect_markers_for_library(
                detection_state,
                library_id + 1,
                force,
                markers::detection_threshold(None),
                manual,
            )
            .await
            {
                warn!(%error, mode, "marker detection failed");
            }
        });
    }
    Ok(scanned_count)
}

async fn library_scan_scheduler(state: AppState) {
    info!(
        delay_seconds = AUTOMATIC_SCAN_STARTUP_GRACE.as_secs(),
        "automatic library scans deferred after startup"
    );
    tokio::time::sleep(AUTOMATIC_SCAN_STARTUP_GRACE).await;
    let mut ticker = tokio::time::interval(Duration::from_secs(5));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_scans = HashMap::<usize, tokio::time::Instant>::new();
    let mut initial_scan = true;
    loop {
        ticker.tick().await;
        let libraries = match state.config.lock() {
            Ok(config) => config.libraries.clone(),
            Err(_) => continue,
        };
        let now = tokio::time::Instant::now();
        last_scans.retain(|index, _| *index < libraries.len());
        for (index, library) in libraries.iter().enumerate() {
            if !library.scan_enabled {
                last_scans.remove(&index);
                continue;
            }
            let last_scan = last_scans.entry(index).or_insert(now);
            if !initial_scan && now.duration_since(*last_scan) < library_scan_interval(library) {
                continue;
            }
            *last_scan = now;
            if let Err(error) = scan_library_inventory(
                &state,
                index,
                is_audiobook_library(&library.kind),
                false,
                false,
            )
            .await
            {
                warn!(library = %library.name, %error, "automatic library scan failed");
            }
        }
        initial_scan = false;
    }
}

fn load_config(path: &std::path::Path) -> Result<Config, Box<dyn std::error::Error>> {
    if !path.exists() {
        return Ok(Config {
            bind: default_bind(),
            libraries: Vec::new(),
            metadata_storage: default_metadata_storage(),
            tmdb_bearer_token: None,
            audio_language_primary: default_audio_language_primary(),
            audio_language_secondary: None,
            subtitle_language_primary: default_subtitle_language_primary(),
            subtitle_language_secondary: default_subtitle_language_secondary(),
            bilingual_subtitles_enabled: default_true(),
            skip_intro_mode: default_skip_mode(),
            skip_credits_mode: default_skip_mode(),
            marker_detection_enabled: default_true(),
            plex_account_preferences: BTreeMap::new(),
        });
    }
    let contents = std::fs::read_to_string(path)?;
    let mut config: Config = toml::from_str(&contents)?;
    let legacy: toml::Value = toml::from_str(&contents)?;
    if legacy.get("metadata_storage").is_none()
        && legacy
            .get("metadata_in_media_folder")
            .and_then(toml::Value::as_bool)
            == Some(true)
    {
        config.metadata_storage = "media".to_owned();
    } else {
        config.metadata_storage = match config.metadata_storage.as_str() {
            "database" | "config" => "config".to_owned(),
            "sidecar" | "media" => "media".to_owned(),
            _ => default_metadata_storage(),
        };
    }
    Ok(config)
}

pub(crate) fn save_config(
    path: &std::path::Path,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let serialized = toml::to_string_pretty(config)?;
    if std::fs::read_to_string(path).ok().as_deref() == Some(serialized.as_str()) {
        return Ok(());
    }
    std::fs::write(path, serialized)?;
    Ok(())
}

fn explicit_setting_key(name: &str) -> String {
    format!("setting:{name}")
}

fn explicit_setting_from_db(connection: &Connection, name: &str) -> Option<String> {
    let mut statement = connection
        .prepare("SELECT value FROM playback_preferences WHERE key = ?1")
        .ok()?;
    let mut rows = statement
        .query_map([explicit_setting_key(name)], |row| row.get::<_, String>(0))
        .ok()?;
    rows.next().and_then(Result::ok)
}

pub(crate) fn store_explicit_setting(state: &AppState, name: &str, value: &str) {
    let Ok(connection) = state.db.lock() else {
        return;
    };
    let result = connection.execute(
        "INSERT INTO playback_preferences (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![explicit_setting_key(name), value, now_timestamp()],
    );
    if result.is_err() {
        warn!(name, "Could not persist explicit setting");
    }
}

pub(crate) fn store_config_settings(state: &AppState, config: &Config) {
    store_explicit_setting(
        state,
        "audio_language_primary",
        &config.audio_language_primary,
    );
    store_explicit_setting(
        state,
        "audio_language_secondary",
        config
            .audio_language_secondary
            .as_deref()
            .unwrap_or_default(),
    );
    store_explicit_setting(
        state,
        "subtitle_language_primary",
        &config.subtitle_language_primary,
    );
    store_explicit_setting(
        state,
        "subtitle_language_secondary",
        &config.subtitle_language_secondary,
    );
    store_explicit_setting(
        state,
        "bilingual_subtitles_enabled",
        if config.bilingual_subtitles_enabled {
            "true"
        } else {
            "false"
        },
    );
    if let Ok(preferences) = serde_json::to_string(&config.plex_account_preferences) {
        store_explicit_setting(state, "plex_account_preferences", &preferences);
    }
}

pub(crate) fn apply_stored_settings(db: &Connection, config: &mut Config) {
    if let Some(value) = explicit_setting_from_db(db, "audio_language_primary") {
        config.audio_language_primary = value;
    }
    if let Some(value) = explicit_setting_from_db(db, "audio_language_secondary") {
        config.audio_language_secondary = (!value.is_empty()).then_some(value);
    }
    if let Some(value) = explicit_setting_from_db(db, "subtitle_language_primary") {
        config.subtitle_language_primary = value;
    }
    if let Some(value) = explicit_setting_from_db(db, "subtitle_language_secondary") {
        config.subtitle_language_secondary = value;
    }
    if let Some(value) = explicit_setting_from_db(db, "bilingual_subtitles_enabled") {
        config.bilingual_subtitles_enabled = value == "true";
    }
    if let Some(value) = explicit_setting_from_db(db, "plex_account_preferences") {
        if let Ok(preferences) = serde_json::from_str(&value) {
            config.plex_account_preferences = preferences;
        }
    }
}

pub(crate) fn sync_config_libraries(
    connection: &Connection,
    config: &Config,
) -> Result<(), rusqlite::Error> {
    for library in &config.libraries {
        let inserted = connection.execute(
            "INSERT OR IGNORE INTO library_groups (name, kind, added_at, updated_at) VALUES (?1, ?2, strftime('%s', 'now'), strftime('%s', 'now'))",
            (&library.name, &library.kind),
        )?;
        if inserted > 0 {
            update_library_catalog_timestamp(connection)?;
        }
        let id: i64 = connection.query_row(
            "SELECT id FROM library_groups WHERE name = ?1",
            [&library.name],
            |row| row.get(0),
        )?;
        let kind_changed = connection.execute(
            "UPDATE library_groups SET kind = ?1, updated_at = MAX(updated_at + 1, CAST(strftime('%s', 'now') AS INTEGER)) WHERE id = ?2 AND kind != ?1",
            (&library.kind, id),
        )?;
        if kind_changed > 0 {
            update_library_catalog_timestamp(connection)?;
        }
        for folder in &library.folders {
            let inserted = connection.execute(
                "INSERT OR IGNORE INTO library_folders (library_id, path, added_at) VALUES (?1, ?2, strftime('%s', 'now'))",
                (id, folder.to_string_lossy().as_ref()),
            )?;
            if inserted > 0 {
                connection.execute(
                    "UPDATE library_groups SET updated_at = MAX(updated_at + 1, CAST(strftime('%s', 'now') AS INTEGER)) WHERE id = ?1",
                    [id],
                )?;
                update_library_catalog_timestamp(connection)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn update_library_catalog_timestamp(
    connection: &Connection,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "UPDATE library_catalog_state SET updated_at = MAX(updated_at + 1, CAST(strftime('%s', 'now') AS INTEGER)) WHERE id = 1",
        [],
    )?;
    Ok(())
}

fn mark_library_updated(
    connection: &Connection,
    library_name: &str,
    scanned: bool,
) -> Result<(), rusqlite::Error> {
    let query = if scanned {
        "UPDATE library_groups SET scanned_at = MAX(scanned_at + 1, CAST(strftime('%s', 'now') AS INTEGER)), updated_at = MAX(updated_at + 1, CAST(strftime('%s', 'now') AS INTEGER)) WHERE name = ?1"
    } else {
        "UPDATE library_groups SET updated_at = MAX(updated_at + 1, CAST(strftime('%s', 'now') AS INTEGER)) WHERE name = ?1"
    };
    connection.execute(query, [library_name])?;
    update_library_catalog_timestamp(connection)
}

pub(crate) fn library_publication_state(
    state: &AppState,
    library_name: &str,
    fallback_section: usize,
) -> (i64, u64, u64) {
    state
        .db
        .lock()
        .ok()
        .and_then(|connection| {
            connection
                .query_row(
                    "SELECT id, scanned_at, updated_at FROM library_groups WHERE name = ?1",
                    [library_name],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, u64>(1)?,
                            row.get::<_, u64>(2)?,
                        ))
                    },
                )
                .ok()
        })
        .unwrap_or((fallback_section as i64, 0, 0))
}

pub(crate) fn library_catalog_updated_at(state: &AppState) -> u64 {
    state
        .db
        .lock()
        .ok()
        .and_then(|connection| {
            connection
                .query_row(
                    "SELECT updated_at FROM library_catalog_state WHERE id = 1",
                    [],
                    |row| row.get(0),
                )
                .ok()
        })
        .unwrap_or_default()
}

fn open_database(
    config_path: &std::path::Path,
) -> Result<(Connection, String), Box<dyn std::error::Error>> {
    let state_dir = config_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .to_path_buf();
    std::fs::create_dir_all(&state_dir)?;
    let connection = Connection::open(state_dir.join("state.sqlite"))?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS server_identity (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             machine_identifier TEXT NOT NULL UNIQUE,
             server_name TEXT NOT NULL,
             plex_account_token TEXT,
             claimed INTEGER NOT NULL DEFAULT 0,
             plex_last_error TEXT
         );
         CREATE TABLE IF NOT EXISTS tv_show_identity (
             show_id TEXT PRIMARY KEY,
             library_section INTEGER NOT NULL,
             relative_path TEXT NOT NULL,
             title TEXT NOT NULL,
             UNIQUE (library_section, relative_path)
         );
         CREATE TABLE IF NOT EXISTS plex_auth (
             id INTEGER PRIMARY KEY,
             code TEXT NOT NULL,
             created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS plex_device_keys (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             private_key BLOB NOT NULL,
             public_key BLOB NOT NULL,
             kid TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS users (
             id TEXT PRIMARY KEY,
             plex_user_id TEXT UNIQUE,
             name TEXT NOT NULL,
             username TEXT UNIQUE,
             password_hash TEXT,
             user_type TEXT NOT NULL DEFAULT 'guest',
             is_active INTEGER NOT NULL DEFAULT 1,
             created_at INTEGER NOT NULL DEFAULT 0,
             last_seen INTEGER NOT NULL DEFAULT 0,
             permissions TEXT NOT NULL DEFAULT '{}',
             libraries TEXT NOT NULL DEFAULT '[]'
         );
         CREATE TABLE IF NOT EXISTS web_admin (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             password_hash TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS web_sessions (
             token_hash TEXT PRIMARY KEY,
             expires_at INTEGER NOT NULL,
             user_id TEXT
         );
         CREATE TABLE IF NOT EXISTS media (
             media_key TEXT PRIMARY KEY,
             path TEXT NOT NULL,
             title TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS media_identity (
             library_section INTEGER NOT NULL,
             canonical_path TEXT NOT NULL,
             media_key TEXT NOT NULL UNIQUE,
             PRIMARY KEY (library_section, canonical_path)
         );
         CREATE TABLE IF NOT EXISTS watch_progress (
             user_id TEXT NOT NULL,
             media_key TEXT NOT NULL,
             position_ms INTEGER NOT NULL DEFAULT 0,
             duration_ms INTEGER,
             watched INTEGER NOT NULL DEFAULT 0,
             hidden_from_continue INTEGER NOT NULL DEFAULT 0,
             updated_at INTEGER NOT NULL,
             PRIMARY KEY (user_id, media_key)
         );
         CREATE TABLE IF NOT EXISTS stream_selections (
             media_path TEXT PRIMARY KEY,
             audio_stream_id INTEGER,
             subtitle_stream_id INTEGER,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS playback_preferences (
             key TEXT PRIMARY KEY,
             value TEXT NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS marker_sidecar_state (
             path TEXT PRIMARY KEY,
             modified_ns INTEGER NOT NULL,
             len INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS libraries (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             path TEXT NOT NULL UNIQUE,
             kind TEXT NOT NULL DEFAULT 'other',
             added_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS library_groups (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             name TEXT NOT NULL UNIQUE,
             kind TEXT NOT NULL DEFAULT 'other',
             added_at INTEGER NOT NULL,
             scanned_at INTEGER NOT NULL DEFAULT 0,
             updated_at INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS library_catalog_state (
             id INTEGER PRIMARY KEY CHECK (id = 1),
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS library_folders (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             library_id INTEGER NOT NULL REFERENCES library_groups(id) ON DELETE CASCADE,
             path TEXT NOT NULL UNIQUE,
             added_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS playlists (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             title TEXT NOT NULL,
             playlist_type TEXT NOT NULL,
             smart INTEGER NOT NULL DEFAULT 0,
             added_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS playlist_items (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             playlist_id INTEGER NOT NULL,
             media_key TEXT NOT NULL,
             position INTEGER NOT NULL,
             added_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS collections (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             title TEXT NOT NULL,
             section_id INTEGER NOT NULL,
             added_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS collection_items (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             collection_id INTEGER NOT NULL,
             media_key TEXT NOT NULL,
             position INTEGER NOT NULL,
             added_at INTEGER NOT NULL
         );
         CREATE UNIQUE INDEX IF NOT EXISTS collection_items_collection_media
             ON collection_items(collection_id, media_key);
         CREATE TABLE IF NOT EXISTS abs_collections (
             id TEXT PRIMARY KEY,
             library_id TEXT NOT NULL,
             name TEXT NOT NULL,
             description TEXT NOT NULL DEFAULT '',
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS abs_collection_items (
             collection_id TEXT NOT NULL,
             item_id TEXT NOT NULL,
             position INTEGER NOT NULL,
             added_at INTEGER NOT NULL,
             PRIMARY KEY (collection_id, item_id)
         );
         CREATE TABLE IF NOT EXISTS abs_playlists (
             id TEXT PRIMARY KEY,
             library_id TEXT NOT NULL,
             user_id TEXT NOT NULL,
             name TEXT NOT NULL,
             description TEXT NOT NULL DEFAULT '',
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS abs_playlist_items (
             playlist_id TEXT NOT NULL,
             item_id TEXT NOT NULL,
             position INTEGER NOT NULL,
             added_at INTEGER NOT NULL,
             PRIMARY KEY (playlist_id, item_id)
         );
         CREATE TABLE IF NOT EXISTS abs_playback_sessions (
             id TEXT PRIMARY KEY,
             user_id TEXT NOT NULL,
             library_id TEXT NOT NULL,
             item_id TEXT NOT NULL,
             duration REAL NOT NULL DEFAULT 0,
             current_time REAL NOT NULL DEFAULT 0,
             time_listened REAL NOT NULL DEFAULT 0,
             start_time REAL NOT NULL DEFAULT 0,
             started_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             closed_at INTEGER
         );
         CREATE TABLE IF NOT EXISTS abs_bookmarks (
             id TEXT PRIMARY KEY,
             user_id TEXT NOT NULL,
             item_id TEXT NOT NULL,
             time REAL NOT NULL,
             title TEXT NOT NULL,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             UNIQUE (user_id, item_id, time)
         );",
    )?;
    markers::ensure_marker_schema(&connection)?;

    let _ = connection.execute(
        "ALTER TABLE server_identity ADD COLUMN plex_last_error TEXT",
        [],
    );
    let _ = connection.execute(
        "ALTER TABLE libraries ADD COLUMN kind TEXT NOT NULL DEFAULT 'other'",
        [],
    );
    let _ = connection.execute(
        "ALTER TABLE watch_progress ADD COLUMN hidden_from_continue INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = connection.execute(
        "ALTER TABLE watch_progress ADD COLUMN finished_at INTEGER",
        [],
    );
    let _ = connection.execute(
        "ALTER TABLE abs_playback_sessions ADD COLUMN start_time REAL NOT NULL DEFAULT 0",
        [],
    );
    let _ = connection.execute(
        "ALTER TABLE library_groups ADD COLUMN scanned_at INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = connection.execute(
        "ALTER TABLE library_groups ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = connection.execute("ALTER TABLE users ADD COLUMN username TEXT", []);
    let _ = connection.execute("ALTER TABLE users ADD COLUMN password_hash TEXT", []);
    let _ = connection.execute(
        "ALTER TABLE users ADD COLUMN user_type TEXT NOT NULL DEFAULT 'guest'",
        [],
    );
    let _ = connection.execute(
        "ALTER TABLE users ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1",
        [],
    );
    let _ = connection.execute(
        "ALTER TABLE users ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = connection.execute(
        "ALTER TABLE users ADD COLUMN last_seen INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = connection.execute(
        "ALTER TABLE users ADD COLUMN permissions TEXT NOT NULL DEFAULT '{}'",
        [],
    );
    let _ = connection.execute(
        "ALTER TABLE users ADD COLUMN libraries TEXT NOT NULL DEFAULT '[]'",
        [],
    );
    let _ = connection.execute("ALTER TABLE web_sessions ADD COLUMN user_id TEXT", []);
    let _ = connection.execute(
        "INSERT OR IGNORE INTO users (id, name, username, password_hash, user_type)
         SELECT 'admin', 'Administrator', 'admin', password_hash, 'admin'
         FROM web_admin WHERE id = 1
           AND NOT EXISTS (SELECT 1 FROM users WHERE password_hash IS NOT NULL)",
        [],
    );
    // Web and Plex progress used a single shared identity before accounts were
    // wired into sessions. Keep the newest row under the administrator identity.
    let _ = connection.execute(
        "INSERT INTO watch_progress
            (user_id, media_key, position_ms, duration_ms, watched, hidden_from_continue, updated_at)
         SELECT 'admin', media_key, position_ms, duration_ms, watched, hidden_from_continue, updated_at
           FROM watch_progress WHERE user_id = 'mplex-user'
         ON CONFLICT(user_id, media_key) DO UPDATE SET
            position_ms = excluded.position_ms,
            duration_ms = excluded.duration_ms,
            watched = excluded.watched,
            hidden_from_continue = excluded.hidden_from_continue,
            updated_at = excluded.updated_at
         WHERE excluded.updated_at > watch_progress.updated_at",
        [],
    );
    let _ = connection.execute(
        "DELETE FROM watch_progress WHERE user_id = 'mplex-user'",
        [],
    );
    connection.execute(
        "UPDATE library_groups SET updated_at = added_at WHERE updated_at = 0",
        [],
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO library_catalog_state (id, updated_at) VALUES (1, strftime('%s', 'now'))",
        [],
    )?;
    let machine_identifier = connection
        .query_row(
            "SELECT machine_identifier FROM server_identity WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .or_else(|error| {
            if !matches!(error, rusqlite::Error::QueryReturnedNoRows) {
                return Err(error);
            }
            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(error))
            })?;
            let generated = format!("mplex-{:x}", timestamp.as_nanos());
            connection.execute(
                "INSERT INTO server_identity (id, machine_identifier, server_name) VALUES (1, ?1, 'harbinger')",
                [&generated],
            )?;
            Ok(generated)
        })?;
    let machine_identifier = if machine_identifier.len() == 40
        && machine_identifier
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        machine_identifier
    } else {
        let migrated = plex_machine_identifier(&machine_identifier);
        connection.execute(
            "UPDATE server_identity SET machine_identifier = ?1 WHERE id = 1",
            [&migrated],
        )?;
        migrated
    };
    Ok((connection, machine_identifier))
}

fn migrate_library_model(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute(
        "INSERT OR IGNORE INTO library_groups (name, kind, added_at) VALUES ('Default Library', 'other', strftime('%s', 'now'))",
        [],
    )?;
    let default_id: i64 = connection.query_row(
        "SELECT id FROM library_groups WHERE name = 'Default Library'",
        [],
        |row| row.get(0),
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO library_folders (library_id, path, added_at) SELECT ?1, path, added_at FROM libraries",
        [default_id],
    )?;
    Ok(())
}

pub(crate) fn is_metadata_library(kind: &str) -> bool {
    matches!(kind, "movie" | "tv")
}

pub(crate) fn is_audiobook_library(kind: &str) -> bool {
    kind == "audiobooks"
}

fn library_kind_label(kind: &str) -> &str {
    match kind {
        "movie" => "Movies",
        "tv" => "TV Shows",
        "audiobooks" => "Audiobooks",
        _ => "Other",
    }
}

fn library_kind_emoji(kind: &str) -> &'static str {
    match kind {
        "tv" => "📺",
        "audiobooks" => "🎧",
        _ => "🎬",
    }
}

fn library_kind_glyph(kind: &str) -> &'static str {
    match kind {
        "tv" => "▣",
        "audiobooks" => "♪",
        _ => "▤",
    }
}

#[derive(Deserialize)]
pub(crate) struct LibraryFolderForm {
    pub(crate) library_id: i64,
    pub(crate) path: String,
}

#[derive(Deserialize)]
pub(crate) struct RemoveLibraryFolderForm {
    pub(crate) library_id: usize,
    pub(crate) folder_index: usize,
    pub(crate) path: String,
}

#[derive(Deserialize)]
pub(crate) struct LibraryDisplayForm {
    pub(crate) library_id: usize,
    pub(crate) show_recently_added: Option<String>,
    pub(crate) show_continue_watching: Option<String>,
}

#[derive(Deserialize)]
struct RenameLibraryForm {
    name: String,
}

#[derive(Deserialize)]
pub(crate) struct LibrarySettingsForm {
    pub(crate) name: String,
    pub(crate) scan_enabled: Option<String>,
    pub(crate) scan_interval_value: u32,
    pub(crate) scan_interval_unit: String,
    pub(crate) show_recently_added: Option<String>,
    pub(crate) show_continue_watching: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct LibraryScanSettingsForm {
    pub(crate) library_id: usize,
    pub(crate) scan_enabled: Option<String>,
    pub(crate) scan_interval_value: u32,
    pub(crate) scan_interval_unit: String,
}

#[derive(Deserialize)]
pub(crate) struct PreferencesForm {
    pub(crate) metadata_storage: Option<String>,
    pub(crate) tmdb_bearer_token: Option<String>,
    pub(crate) copy_existing: Option<bool>,
}

#[derive(Deserialize)]
pub(crate) struct ServerNameForm {
    pub(crate) server_name: String,
}

#[derive(Deserialize, Default)]
pub(crate) struct SettingsQuery {
    pub(crate) folder: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) metadata_matched: Option<usize>,
    pub(crate) metadata_status: Option<String>,
    pub(crate) server_name_saved: Option<bool>,
    pub(crate) inventory_scan: Option<usize>,
    pub(crate) inventory_error: Option<bool>,
    pub(crate) force: Option<bool>,
    pub(crate) tab: Option<String>,
    pub(crate) library_renamed: Option<bool>,
    pub(crate) library_saved: Option<bool>,
    pub(crate) library_created: Option<bool>,
    pub(crate) user_status: Option<String>,
    pub(crate) user_error: Option<String>,
}

#[derive(Deserialize, Default)]
pub(crate) struct ScanQuery {
    pub(crate) force: Option<bool>,
    pub(crate) path: Option<String>,
    pub(crate) refresh: Option<bool>,
    pub(crate) assets_only: Option<bool>,
}

#[derive(Deserialize)]
pub(crate) struct TvMatchSearchQuery {
    pub(crate) library: usize,
    pub(crate) path: String,
    pub(crate) query: String,
}

#[derive(Deserialize)]
pub(crate) struct TvMatchSelection {
    pub(crate) library: usize,
    pub(crate) path: String,
    pub(crate) tmdb_id: i64,
}

#[derive(Deserialize)]
pub(crate) struct MediaScanForm {
    pub(crate) path: String,
    pub(crate) library: Option<usize>,
    pub(crate) title: String,
}

#[derive(Deserialize, Default)]
pub(crate) struct BrowseQuery {
    pub(crate) path: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ProgressQuery {
    pub(crate) path: String,
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct ProgressUpdate {
    pub(crate) path: String,
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
    pub(crate) position_ms: i64,
    pub(crate) duration_ms: Option<i64>,
    pub(crate) watched: bool,
}

#[derive(Deserialize)]
pub(crate) struct WebTranslationStopRequest {
    pub(crate) path: String,
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct WebSubtitleSelectionUpdate {
    pub(crate) path: String,
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
    pub(crate) stream_id: u64,
}

#[derive(Deserialize)]
pub(crate) struct WebAudioSelectionUpdate {
    pub(crate) path: String,
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
    pub(crate) stream_id: u64,
}

#[derive(Deserialize)]
pub(crate) struct WebSubtitleSelectionQuery {
    pub(crate) path: String,
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct WebTranslatedSubtitleRequest {
    pub(crate) path: String,
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
    #[serde(default)]
    pub(crate) stream_id: u64,
    #[serde(default)]
    pub(crate) source_language: Option<String>,
    #[serde(default)]
    pub(crate) source_cues: Vec<BrowserSubtitleCue>,
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) source_key: Option<String>,
    pub(crate) first_language: String,
    pub(crate) second_language: String,
    #[serde(default)]
    pub(crate) prepared: bool,
}

#[derive(Deserialize)]
pub(crate) struct WebTranslationProgressRequest {
    pub(crate) path: String,
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
    #[serde(default)]
    pub(crate) stream_id: u64,
    pub(crate) first_language: String,
    pub(crate) second_language: String,
    #[serde(default)]
    pub(crate) source_key: Option<String>,
    #[serde(default)]
    pub(crate) total_cues: Option<usize>,
}

#[derive(Serialize, Deserialize)]
struct SubtitleSourceMarker {
    fingerprint: String,
    source_key: Option<String>,
    #[serde(default)]
    total_cues: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct BrowserSubtitleCue {
    start_ms: f64,
    end_ms: f64,
    text: String,
}

fn browser_cues_to_srt(cues: &[BrowserSubtitleCue]) -> Result<String, String> {
    if cues.len() > 20_000 {
        return Err("Subtitle track contains too many cues".to_owned());
    }
    let mut output = String::new();
    for (index, cue) in cues.iter().enumerate() {
        if !cue.start_ms.is_finite()
            || !cue.end_ms.is_finite()
            || cue.start_ms < 0.0
            || cue.end_ms <= cue.start_ms
            || cue.text.is_empty()
        {
            return Err("Embedded subtitle cue data is invalid".to_owned());
        }
        if cue.text.len() > 16_384 {
            return Err("Embedded subtitle cue is too large".to_owned());
        }
        output.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            index + 1,
            srt_time(cue.start_ms.round() as u64),
            srt_time(cue.end_ms.round() as u64),
            cue.text
        ));
    }
    Ok(output)
}

fn subtitle_source_marker_path(path: &std::path::Path) -> PathBuf {
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{file_name}.source.json"))
}

fn subtitle_translation_fingerprint(
    media: &Media,
    contents: &str,
    source_language: &str,
    source_format: &str,
    first: &str,
    second: &str,
    _source_key: Option<&str>,
) -> Result<String, String> {
    let translation_input = match source_format.to_ascii_lowercase().as_str() {
        "srt" | "vtt" => contents.to_owned(),
        "ass" => ass_to_vtt(contents),
        _ => return Err("This subtitle format cannot be translated".to_owned()),
    };
    let cues = subtitles::parse_subtitles(&translation_input)?;
    let mut normalized_source = String::new();
    for cue in cues {
        normalized_source.push_str(&cue.start.replace(',', "."));
        normalized_source.push_str(" --> ");
        normalized_source.push_str(&cue.end.replace(',', "."));
        normalized_source.push('\n');
        normalized_source.push_str(cue.text.trim());
        normalized_source.push_str("\n\n");
    }
    let mut hash = Sha256::new();
    hash.update(&media.id);
    hash.update(normalized_source);
    hash.update(source_language.as_bytes());
    hash.update(first.as_bytes());
    hash.update(second.as_bytes());
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(crate) async fn subtitle_marker_total_cues(artifact_path: &std::path::Path) -> Option<usize> {
    let contents = tokio::fs::read_to_string(subtitle_source_marker_path(artifact_path))
        .await
        .ok()?;
    serde_json::from_str::<SubtitleSourceMarker>(&contents)
        .ok()
        .and_then(|marker| marker.total_cues)
}

async fn subtitle_source_marker_matches(
    artifact_path: &std::path::Path,
    fingerprint: &str,
    _source_key: Option<&str>,
) -> bool {
    let marker_path = subtitle_source_marker_path(artifact_path);
    let Ok(contents) = tokio::fs::read_to_string(marker_path).await else {
        return false;
    };
    let Ok(marker) = serde_json::from_str::<SubtitleSourceMarker>(&contents) else {
        return false;
    };
    marker.fingerprint == fingerprint
}

async fn subtitle_marker_source_matches(
    artifact_path: &std::path::Path,
    source_key: Option<&str>,
) -> bool {
    let marker_path = subtitle_source_marker_path(artifact_path);
    let Ok(contents) = tokio::fs::read_to_string(marker_path).await else {
        return false;
    };
    serde_json::from_str::<SubtitleSourceMarker>(&contents)
        .is_ok_and(|marker| marker.source_key.as_deref() == source_key)
}

async fn write_subtitle_source_marker(
    artifact_path: &std::path::Path,
    fingerprint: &str,
    source_key: Option<&str>,
    total_cues: Option<usize>,
) -> Result<(), String> {
    let marker_path = subtitle_source_marker_path(artifact_path);
    let total_cues = match total_cues {
        Some(total_cues) => Some(total_cues),
        None => tokio::fs::read_to_string(&marker_path)
            .await
            .ok()
            .and_then(|contents| serde_json::from_str::<SubtitleSourceMarker>(&contents).ok())
            .and_then(|marker| marker.total_cues),
    };
    let temporary_path = PathBuf::from(format!("{}.tmp", marker_path.to_string_lossy()));
    let contents = serde_json::to_vec(&SubtitleSourceMarker {
        fingerprint: fingerprint.to_owned(),
        source_key: source_key.map(str::to_owned),
        total_cues,
    })
    .map_err(|_| "Could not encode subtitle source metadata".to_owned())?;
    tokio::fs::write(&temporary_path, contents)
        .await
        .map_err(|_| "Could not write subtitle source metadata".to_owned())?;
    tokio::fs::rename(&temporary_path, &marker_path)
        .await
        .map_err(|_| "Could not save subtitle source metadata".to_owned())
}

async fn prepare_translation_checkpoint(
    checkpoint_path: &std::path::Path,
    translation_input: &str,
    fingerprint: &str,
    source_key: Option<&str>,
) -> Result<(), String> {
    let total_cues = subtitles::parse_subtitles(translation_input)
        .ok()
        .map(|cues| cues.len());
    if subtitle_source_marker_matches(checkpoint_path, fingerprint, source_key).await {
        write_subtitle_source_marker(checkpoint_path, fingerprint, source_key, total_cues).await?;
        return Ok(());
    }
    let has_checkpoint = tokio::fs::metadata(checkpoint_path).await.is_ok();
    let has_marker = tokio::fs::metadata(subtitle_source_marker_path(checkpoint_path))
        .await
        .is_ok();
    let has_generating_placeholder = tokio::fs::read_to_string(checkpoint_path)
        .await
        .ok()
        .filter(|contents| contents.contains(BILINGUAL_GENERATING_NOTICE))
        .is_some_and(|contents| {
            let without_notice = contents
                .replace("\r\n", "\n")
                .split("\n\n")
                .filter(|block| !block.contains(BILINGUAL_GENERATING_NOTICE))
                .collect::<Vec<_>>()
                .join("\n\n");
            subtitles::parse_subtitles(&without_notice).map_or(true, |cues| cues.is_empty())
        });
    let keep_legacy_checkpoint = has_checkpoint
        && ((!has_marker
            && subtitles::staged_vtt_matches_source(checkpoint_path, translation_input).await)
            || has_generating_placeholder);
    if has_checkpoint && !keep_legacy_checkpoint {
        tokio::fs::write(checkpoint_path, "WEBVTT\n\n")
            .await
            .map_err(|_| "Could not reset the subtitle checkpoint for this source".to_owned())?;
        info!("subtitle checkpoint belongs to a different source; restarting from the first cue");
    }
    write_subtitle_source_marker(checkpoint_path, fingerprint, source_key, total_cues).await
}

#[cfg(test)]
async fn translate_subtitle_cached(
    state: &AppState,
    media: &Media,
    contents: &str,
    source: &str,
    extension: &str,
    first: &str,
    second: &str,
) -> Result<String, String> {
    translate_subtitle_cached_cancellable(
        state, media, contents, source, extension, first, second, None, None,
    )
    .await
}

async fn translate_subtitle_cached_cancellable(
    state: &AppState,
    media: &Media,
    contents: &str,
    source: &str,
    extension: &str,
    first: &str,
    second: &str,
    cancellation: Option<Arc<std::sync::atomic::AtomicBool>>,
    source_key: Option<&str>,
) -> Result<String, String> {
    translate_subtitle_cached_with_progress(
        state,
        media,
        contents,
        source,
        extension,
        first,
        second,
        cancellation,
        None,
        source_key,
    )
    .await
}

/// Google's `client=gtx` endpoint returns HTTP 429 when a client advertises
/// `http/1.1` via ALPN. reqwest's rustls backend does that by default; the Ruby
/// translator sends no ALPN. Keep rustls (no OpenSSL dependency for Docker) but
/// send the same ClientHello shape Google accepts.
fn translation_http_client() -> Result<reqwest::Client, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().map(|trust| {
        rustls::pki_types::TrustAnchor {
            subject: trust.subject.clone(),
            subject_public_key_info: trust.subject_public_key_info.clone(),
            name_constraints: trust.name_constraints.clone(),
        }
    }));
    let provider = std::sync::Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| "Could not initialize Google Translate".to_owned())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols.clear();
    reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|_| "Could not initialize Google Translate".to_owned())
}

async fn translate_subtitle_cached_with_progress(
    state: &AppState,
    media: &Media,
    contents: &str,
    source: &str,
    extension: &str,
    first: &str,
    second: &str,
    cancellation: Option<Arc<std::sync::atomic::AtomicBool>>,
    progress: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    source_key: Option<&str>,
) -> Result<String, String> {
    let output_extension = match extension.to_ascii_lowercase().as_str() {
        "srt" | "vtt" | "ass" => extension.to_ascii_lowercase(),
        _ => return Err("This subtitle format cannot be translated".to_owned()),
    };
    let sidecar_path =
        configured_bilingual_subtitle_path(state, media, first, second, &output_extension);
    let media_sidecar_path = bilingual_subtitle_path(media, first, second, &output_extension);
    if media_sidecar_path != sidecar_path
        && tokio::fs::metadata(&media_sidecar_path).await.is_ok()
        && tokio::fs::metadata(&sidecar_path).await.is_err()
    {
        if let Ok(contents) = tokio::fs::read(&media_sidecar_path).await {
            if let Some(parent) = sidecar_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            let _ = tokio::fs::write(&sidecar_path, contents).await;
            let old_marker = subtitle_source_marker_path(&media_sidecar_path);
            if let Ok(marker) = tokio::fs::read(old_marker).await {
                let _ = tokio::fs::write(subtitle_source_marker_path(&sidecar_path), marker).await;
            }
        }
    }
    if tokio::fs::metadata(&sidecar_path).await.is_ok()
        && tokio::fs::metadata(subtitle_source_marker_path(&sidecar_path))
            .await
            .is_err()
    {
        if let Ok(contents) = tokio::fs::read_to_string(&sidecar_path).await {
            if let Ok(vtt) =
                subtitles::parse_subtitles(&subtitle_to_vtt(&contents, &output_extension))
            {
                if !vtt.is_empty() {
                    return Ok(subtitle_to_vtt(&contents, &output_extension));
                }
            }
        }
    }
    let translation_input = match extension {
        "srt" | "vtt" => contents.to_owned(),
        "ass" => ass_to_vtt(contents),
        _ => return Err("This subtitle format cannot be translated".to_owned()),
    };
    let sidecar_part_path = configured_bilingual_subtitle_path(state, media, first, second, "vtt");
    let sidecar_part_path = PathBuf::from(format!("{}.part", sidecar_part_path.to_string_lossy()));
    let fingerprint = subtitle_translation_fingerprint(
        media,
        &translation_input,
        source,
        "vtt",
        first,
        second,
        source_key,
    )?;
    let mut hash = Sha256::new();
    hash.update(&media.id);
    hash.update(contents);
    hash.update(source.as_bytes());
    hash.update(first.as_bytes());
    hash.update(second.as_bytes());
    let checkpoint_key = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let cache_lock = TRANSLATION_CACHE_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "Translation cache is unavailable".to_owned())?
        .entry(sidecar_path.to_string_lossy().into_owned())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    let _cache_guard = cache_lock.lock().await;
    if subtitle_source_marker_matches(&sidecar_path, &fingerprint, source_key).await {
        if let Ok(contents) = tokio::fs::read_to_string(&sidecar_path).await {
            write_subtitle_source_marker(&sidecar_path, &fingerprint, source_key, None).await?;
            cleanup_translation_checkpoint(&sidecar_part_path).await;
            let vtt = subtitle_to_vtt(&contents, &output_extension);
            send_vtt_progress(progress.as_ref(), &vtt)?;
            return Ok(vtt);
        }
    }
    let legacy_cache_path = state
        .subtitle_checkpoints_dir
        .join(format!("{checkpoint_key}.vtt"));
    if let Ok(cached) = tokio::fs::read_to_string(&legacy_cache_path).await {
        finalize_bilingual_translation(
            &sidecar_path,
            &sidecar_part_path,
            &cached,
            &output_extension,
        )
        .await?;
        write_subtitle_source_marker(&sidecar_path, &fingerprint, source_key, None).await?;
        send_vtt_progress(progress.as_ref(), &cached)?;
        return Ok(cached);
    }
    let client = translation_http_client()?;
    tokio::fs::create_dir_all(state.subtitle_checkpoints_dir.as_path())
        .await
        .map_err(|_| "Could not create subtitle checkpoint directory".to_owned())?;
    if tokio::fs::metadata(&sidecar_part_path).await.is_err() {
        for suffix in ["jsonl.part", "vtt.part"] {
            let legacy_checkpoint_path = state
                .subtitle_checkpoints_dir
                .join(format!("{checkpoint_key}.{suffix}"));
            if tokio::fs::metadata(&legacy_checkpoint_path).await.is_ok() {
                let _ = subtitles::migrate_legacy_checkpoint_to_vtt(
                    &translation_input,
                    &legacy_checkpoint_path,
                    &sidecar_part_path,
                )
                .await;
                break;
            }
        }
    }
    prepare_translation_checkpoint(
        &sidecar_part_path,
        &translation_input,
        &fingerprint,
        source_key,
    )
    .await?;
    ensure_generating_checkpoint_exists(&sidecar_part_path, &translation_input, false).await;
    let translated = if cancellation.is_some() || progress.is_some() {
        subtitles::translate_bilingual_resumable_cancellable(
            &client,
            &translation_input,
            source,
            first,
            second,
            &sidecar_part_path,
            cancellation.clone(),
            progress,
        )
        .await
    } else {
        subtitles::translate_bilingual_resumable(
            &client,
            &translation_input,
            source,
            first,
            second,
            &sidecar_part_path,
        )
        .await
    };
    let vtt = match translated {
        Ok(vtt) => vtt,
        Err(error) => {
            if error.starts_with("Google Translate rate limit (HTTP 429)") {
                record_incomplete_notice(&sidecar_part_path, &translation_input).await;
            }
            return Err(error);
        }
    };
    if cancellation
        .as_ref()
        .is_some_and(|token| token.load(std::sync::atomic::Ordering::Acquire))
    {
        return Err("Bilingual translation stopped with playback".to_owned());
    }
    finalize_bilingual_translation(&sidecar_path, &sidecar_part_path, &vtt, &output_extension)
        .await?;
    write_subtitle_source_marker(&sidecar_path, &fingerprint, source_key, None).await?;
    Ok(vtt)
}

fn send_vtt_progress(
    progress: Option<&tokio::sync::mpsc::UnboundedSender<String>>,
    vtt: &str,
) -> Result<(), String> {
    let Some(progress) = progress else {
        return Ok(());
    };
    for cue in subtitles::parse_subtitles(vtt)? {
        progress
            .send(format!(
                "{} --> {}\n{}\n\n",
                cue.start.replace(',', "."),
                cue.end.replace(',', "."),
                cue.text
            ))
            .map_err(|_| "Bilingual subtitle stream closed".to_owned())?;
    }
    Ok(())
}

async fn finalize_bilingual_translation(
    sidecar_path: &std::path::Path,
    checkpoint_path: &std::path::Path,
    vtt: &str,
    extension: &str,
) -> Result<(), String> {
    save_bilingual_sidecar(sidecar_path, vtt, extension).await?;
    cleanup_translation_checkpoint(checkpoint_path).await;
    Ok(())
}

async fn cleanup_translation_checkpoint(path: &std::path::Path) {
    for artifact in [path.to_path_buf(), subtitle_source_marker_path(path)] {
        if let Err(error) = tokio::fs::remove_file(&artifact).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                warn!(path = %artifact.display(), error = %error, "could not remove completed subtitle checkpoint metadata");
            }
        }
    }
}

async fn save_bilingual_sidecar(
    path: &std::path::Path,
    vtt: &str,
    extension: &str,
) -> Result<(), String> {
    let temporary = PathBuf::from(format!("{}.part", path.to_string_lossy()));
    let contents = match extension {
        "srt" => vtt_to_srt(vtt),
        "vtt" => vtt.to_owned(),
        "ass" => vtt_to_ass(vtt),
        _ => return Err("This subtitle format cannot be written".to_owned()),
    };
    tokio::fs::write(&temporary, contents)
        .await
        .map_err(|_| "Could not write bilingual subtitle sidecar".to_owned())?;
    tokio::fs::rename(&temporary, path)
        .await
        .map_err(|_| "Could not finalize bilingual subtitle sidecar".to_owned())
}

pub(crate) async fn bilingual_generation_source_stream_id(
    state: &AppState,
    media: &Media,
    translated_stream_id: u64,
) -> Option<u64> {
    if let Some(stream_id) = stored_plex_stream_selection(state, media)
        .and_then(|selection| selection.subtitle_stream_id)
        .filter(|stream_id| *stream_id != 0 && *stream_id != translated_stream_id)
    {
        return Some(stream_id);
    }
    let effective_primary = effective_plex_language_settings(state).3;
    let primary = if effective_primary == "off" {
        state
            .config
            .lock()
            .map(|config| config.subtitle_language_primary.clone())
            .unwrap_or(effective_primary)
    } else {
        effective_primary
    };
    let mut fallback = None;
    for path in external_subtitle_files(&media.path).await {
        if external_subtitle_pair(&media.path, &path).is_some() {
            continue;
        }
        let stream_id = plex_external_subtitle_id(media, &path);
        if external_subtitle_language(&media.path, &path).0 == primary {
            return Some(stream_id);
        }
        fallback.get_or_insert(stream_id);
    }
    if let Some(stream_id) = fallback {
        return Some(stream_id);
    }
    let info = ensure_media_info(state, media).await?;
    let mut fallback = None;
    for track in technical_tracks(&info) {
        if !is_translatable_embedded_track(&track) {
            continue;
        }
        let stream_id = technical_track_id(&track).saturating_add(1);
        if plex_track_language(&track).0 == primary {
            return Some(stream_id);
        }
        fallback.get_or_insert(stream_id);
    }
    fallback
}

fn vtt_time_millis(timestamp: &str) -> Option<u64> {
    let timestamp = timestamp.trim().replace(',', ".");
    let mut parts = timestamp.split(':');
    let first = parts.next()?.parse::<u64>().ok()?;
    let second = parts.next()?;
    let (hours, minutes, seconds) = match parts.next() {
        Some(seconds) => (first, second.parse::<u64>().ok()?, seconds),
        None => (0, first, second),
    };
    if parts.next().is_some() {
        return None;
    }
    let seconds = seconds.parse::<f64>().ok()?;
    Some((hours * 3_600 + minutes * 60) * 1_000 + (seconds * 1_000.0).round() as u64)
}

fn vtt_time_from_millis(total: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        total / 3_600_000,
        (total / 60_000) % 60,
        (total / 1_000) % 60,
        total % 1_000
    )
}

fn vtt_with_notice(vtt: &str, fallback_start: Option<&str>, notice: &str) -> Option<String> {
    let normalized = vtt.replace("\r\n", "\n").replace('\r', "\n");
    let blocks = normalized
        .trim()
        .split("\n\n")
        .filter(|block| {
            !block.contains(BILINGUAL_INCOMPLETE_NOTICE)
                && !block.contains(BILINGUAL_GENERATING_NOTICE)
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let cues = subtitles::parse_subtitles(&blocks).unwrap_or_default();
    let last_start = cues
        .last()
        .and_then(|cue| vtt_time_millis(&cue.start))
        .or_else(|| fallback_start.and_then(vtt_time_millis))?;
    let mut output = if blocks.is_empty() {
        "WEBVTT".to_owned()
    } else {
        blocks
    };
    output.push_str(&format!(
        "\n\n{} --> 09:00:00.000\n{}\n",
        vtt_time_from_millis(last_start),
        notice
    ));
    Some(output)
}

#[cfg(test)]
fn vtt_with_incomplete_notice(vtt: &str, fallback_start: Option<&str>) -> Option<String> {
    vtt_with_notice(vtt, fallback_start, BILINGUAL_INCOMPLETE_NOTICE)
}

fn vtt_with_generating_notice(vtt: &str, fallback_start: Option<&str>) -> Option<String> {
    vtt_with_notice(vtt, fallback_start, BILINGUAL_GENERATING_NOTICE)
}

async fn record_checkpoint_notice(
    checkpoint_path: &std::path::Path,
    translation_input: &str,
    notice: &str,
) {
    let Ok(vtt) = tokio::fs::read_to_string(checkpoint_path).await else {
        return;
    };
    let fallback_start = subtitles::parse_subtitles(translation_input)
        .ok()
        .and_then(|cues| cues.first().map(|cue| cue.start.clone()));
    if let Some(updated) = vtt_with_notice(&vtt, fallback_start.as_deref(), notice) {
        let _ = tokio::fs::write(checkpoint_path, updated).await;
    }
}

async fn record_incomplete_notice(checkpoint_path: &std::path::Path, translation_input: &str) {
    record_checkpoint_notice(
        checkpoint_path,
        translation_input,
        BILINGUAL_INCOMPLETE_NOTICE,
    )
    .await;
}

async fn ensure_generating_notice(
    checkpoint_path: &std::path::Path,
    _translation_input: &str,
    replace_failure: bool,
) {
    let vtt = tokio::fs::read_to_string(checkpoint_path)
        .await
        .unwrap_or_else(|_| "WEBVTT\n\n".to_owned());
    let without_notices = vtt
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .split("\n\n")
        .filter(|block| {
            !block.contains(BILINGUAL_INCOMPLETE_NOTICE)
                && !block.contains(BILINGUAL_GENERATING_NOTICE)
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let has_failure_notice = vtt.contains(BILINGUAL_INCOMPLETE_NOTICE);
    let has_generating_notice = vtt.contains(BILINGUAL_GENERATING_NOTICE);
    if has_failure_notice && !replace_failure {
        return;
    }
    if let Ok(cues) = subtitles::parse_subtitles(&without_notices) {
        if !cues.is_empty() {
            if has_generating_notice && !has_failure_notice {
                return;
            }
            if let Some(updated) = vtt_with_generating_notice(&vtt, None) {
                let _ = tokio::fs::write(checkpoint_path, updated).await;
            }
            return;
        }
    }
    if has_generating_notice {
        return;
    }
    if let Some(parent) = checkpoint_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(
        checkpoint_path,
        format!(
            "WEBVTT\n\n00:00:00.000 --> 09:00:00.000\n{}\n",
            BILINGUAL_GENERATING_NOTICE
        ),
    )
    .await;
}

async fn ensure_generating_checkpoint_exists(
    checkpoint_path: &std::path::Path,
    translation_input: &str,
    replace_failure: bool,
) {
    if tokio::fs::metadata(checkpoint_path).await.is_ok() {
        ensure_generating_notice(checkpoint_path, translation_input, replace_failure).await;
        return;
    }
    let vtt = format!(
        "WEBVTT\n\n{} --> 09:00:00.000\n{}\n",
        "00:00:00.000", BILINGUAL_GENERATING_NOTICE
    );
    if let Some(parent) = checkpoint_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(checkpoint_path, vtt).await;
}

fn bilingual_checkpoint_path(
    state: &AppState,
    media: &Media,
    first: &str,
    second: &str,
) -> PathBuf {
    let path = configured_bilingual_subtitle_path(state, media, first, second, "vtt");
    PathBuf::from(format!("{}.part", path.to_string_lossy()))
}

async fn staged_bilingual_vtt(
    state: &AppState,
    media: &Media,
    first: &str,
    second: &str,
) -> Option<String> {
    tokio::fs::read_to_string(bilingual_checkpoint_path(state, media, first, second))
        .await
        .ok()
}

pub(crate) async fn spawn_bilingual_generation(
    state: &AppState,
    media: &Media,
    first: &str,
    second: &str,
    contents: String,
    source_language: String,
    format: String,
    explicit: bool,
) -> bool {
    let job_key = bilingual_translation_job_key(media, first, second);
    let Some(cancellation) = start_translation_job(job_key.clone(), media.id.clone(), explicit)
    else {
        return false;
    };
    let checkpoint = bilingual_checkpoint_path(state, media, first, second);
    let translation_input = match format.as_str() {
        "srt" | "vtt" => contents.clone(),
        "ass" => ass_to_vtt(&contents),
        _ => contents.clone(),
    };
    ensure_generating_checkpoint_exists(&checkpoint, &translation_input, true).await;
    let state = state.clone();
    let media = media.clone();
    let first = first.to_owned();
    let second = second.to_owned();
    tokio::spawn(async move {
        let result = translate_subtitle_cached_cancellable(
            &state,
            &media,
            &contents,
            &source_language,
            &format,
            &first,
            &second,
            Some(cancellation),
            None,
        )
        .await;
        finish_translation_job(&job_key);
        match result {
            Ok(_) => info!(
                media = %media.title,
                source_language,
                first_language = %first,
                second_language = %second,
                "Plex bilingual subtitle generation completed"
            ),
            Err(error) => warn!(
                media = %media.title,
                source_language,
                first_language = %first,
                second_language = %second,
                error = %error,
                "Plex bilingual subtitle generation failed"
            ),
        }
    });
    true
}

pub(crate) async fn start_bilingual_generation_for_plex_selection(
    state: &AppState,
    media: &Media,
    subtitle_stream_id: Option<u64>,
) -> Option<u64> {
    let (enabled, first, second) = configured_bilingual_languages(state);
    let translated_stream_id = plex_translated_subtitle_id(media, &first, &second);
    if !enabled || subtitle_stream_id != Some(translated_stream_id) {
        return None;
    }
    if ["srt", "vtt", "ass"].into_iter().any(|extension| {
        configured_bilingual_subtitle_path(state, media, &first, &second, extension).is_file()
    }) {
        return None;
    }
    let source_stream_id =
        bilingual_generation_source_stream_id(state, media, translated_stream_id).await?;
    let Ok((contents, source_language, source_format)) =
        read_subtitle_source(state, media, source_stream_id).await
    else {
        return None;
    };
    let _ = spawn_bilingual_generation(
        state,
        media,
        &first,
        &second,
        contents,
        source_language,
        source_format,
        true,
    )
    .await;
    Some(source_stream_id)
}

pub(crate) async fn translated_subtitle_stream_response(
    state: &AppState,
    media: &Media,
    first: &str,
    second: &str,
    requested_format: &str,
) -> Response {
    for extension in ["srt", "vtt", "ass"] {
        let path = configured_bilingual_subtitle_path(state, media, first, second, extension);
        let Ok(contents) = tokio::fs::read_to_string(&path).await else {
            continue;
        };
        let Some((content_type, contents)) =
            external_subtitle_content(&contents, extension, requested_format)
        else {
            return StatusCode::NOT_FOUND.into_response();
        };
        return ([(header::CONTENT_TYPE, content_type)], contents).into_response();
    }
    let translated_stream_id = plex_translated_subtitle_id(media, first, second);
    let Some(source_stream_id) =
        bilingual_generation_source_stream_id(state, media, translated_stream_id).await
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok((contents, source_language, source_format)) =
        read_subtitle_source(state, media, source_stream_id).await
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let translation_input = match source_format.as_str() {
        "srt" | "vtt" => contents.clone(),
        "ass" => ass_to_vtt(&contents),
        _ => contents.clone(),
    };
    let checkpoint = bilingual_checkpoint_path(state, media, first, second);
    ensure_generating_checkpoint_exists(&checkpoint, &translation_input, true).await;
    let translated_stream_id = plex_translated_subtitle_id(media, first, second);
    if stored_plex_stream_selection(state, media).and_then(|selection| selection.subtitle_stream_id)
        == Some(translated_stream_id)
    {
        let _ = spawn_bilingual_generation(
            state,
            media,
            first,
            second,
            contents,
            source_language,
            source_format,
            false,
        )
        .await;
    }
    let staged = staged_bilingual_vtt(state, media, first, second)
        .await
        .unwrap_or_default();
    let Some((content_type, body)) = external_subtitle_content(&staged, "vtt", requested_format)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
async fn staged_subtitle_preview<F, Fut>(
    final_path: &std::path::Path,
    checkpoint_path: &std::path::Path,
    media_id: &str,
    job_key: String,
    explicit: bool,
    allow_final: bool,
    allow_checkpoint: bool,
    background_job: F,
) -> String
where
    F: FnOnce(Arc<std::sync::atomic::AtomicBool>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    if let Some(cancellation) =
        start_translation_job(job_key.clone(), media_id.to_owned(), explicit)
    {
        tokio::spawn(async move {
            background_job(cancellation).await;
            finish_translation_job(&job_key);
        });
    }

    if allow_final {
        if let Ok(contents) = tokio::fs::read_to_string(final_path).await {
            let extension = final_path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("srt");
            return subtitle_to_vtt(&contents, extension);
        }
    }
    let mut preview = if allow_checkpoint {
        tokio::fs::read_to_string(checkpoint_path)
            .await
            .unwrap_or_else(|_| "WEBVTT\n\n".to_owned())
    } else {
        "WEBVTT\n\n".to_owned()
    };
    for _ in 0..30 {
        if subtitles::parse_subtitles(&preview).is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        if let Ok(contents) = tokio::fs::read_to_string(&checkpoint_path).await {
            preview = contents;
        }
    }
    preview
}

#[derive(Deserialize)]
pub(crate) struct WebWatchActionUpdate {
    pub(crate) path: String,
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
    pub(crate) action: String,
    pub(crate) scope: Option<String>,
}

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn stream_selection_media_key(media: &Media) -> String {
    media.path.to_string_lossy().to_string()
}

pub(crate) fn stored_plex_stream_selection(
    state: &AppState,
    media: &Media,
) -> Option<PlexStreamSelection> {
    if let Some(selection) = state
        .plex_stream_selections
        .lock()
        .ok()
        .and_then(|selections| selections.get(&plex_part_id(media)).copied())
    {
        return Some(selection);
    }
    let connection = state.db.lock().ok()?;
    let mut statement = connection
        .prepare(
            "SELECT audio_stream_id, subtitle_stream_id FROM stream_selections WHERE media_path = ?1",
        )
        .ok()?;
    let mut rows = statement
        .query_map([stream_selection_media_key(media)], |row| {
            Ok(PlexStreamSelection {
                audio_stream_id: row.get(0)?,
                subtitle_stream_id: row.get(1)?,
            })
        })
        .ok()?;
    rows.next().and_then(Result::ok)
}

pub(crate) fn save_plex_stream_selection(
    state: &AppState,
    media: &Media,
    selection: PlexStreamSelection,
) {
    if let Ok(mut selections) = state.plex_stream_selections.lock() {
        selections.insert(plex_part_id(media), selection);
    }
    if let Ok(connection) = state.db.lock() {
        let _ = connection.execute(
            "INSERT INTO stream_selections (media_path, audio_stream_id, subtitle_stream_id, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(media_path) DO UPDATE SET
                 audio_stream_id = excluded.audio_stream_id,
                 subtitle_stream_id = excluded.subtitle_stream_id,
                 updated_at = excluded.updated_at",
            params![
                stream_selection_media_key(media),
                selection.audio_stream_id,
                selection.subtitle_stream_id,
                now_timestamp()
            ],
        );
    }
}

pub(crate) fn selected_web_subtitle_stream_id(
    state: &AppState,
    media: &Media,
    subtitles: &[SubtitleTrack],
) -> u64 {
    if let Some(stream_id) = stored_plex_stream_selection(state, media)
        .and_then(|selection| selection.subtitle_stream_id)
    {
        if stream_id == 0
            || subtitles
                .iter()
                .any(|track| track.stream_id == stream_id && !track.pending_bilingual)
        {
            return stream_id;
        }
    }
    let effective_primary = effective_plex_language_settings(state).3;
    let primary = if effective_primary == "off" {
        state
            .config
            .lock()
            .map(|config| config.subtitle_language_primary.clone())
            .unwrap_or(effective_primary)
    } else {
        effective_primary
    };
    if primary == "off" {
        return 0;
    }
    if let Some(track) = subtitles
        .iter()
        .find(|track| track.bilingual && !track.pending_bilingual)
    {
        return track.stream_id;
    }
    subtitles
        .iter()
        .find(|track| track.language.as_deref() == Some(primary.as_str()))
        .map(|track| track.stream_id)
        .unwrap_or_default()
}

fn find_plex_device_id(body: &str, machine_identifier: &str) -> Option<String> {
    body.split("<Device")
        .skip(1)
        .find(|device| {
            xml_attribute(device, "clientIdentifier").as_deref() == Some(machine_identifier)
        })
        .and_then(|device| xml_attribute(device, "id"))
}

pub(crate) fn xml_attribute(value: &str, name: &str) -> Option<String> {
    let marker = format!("{name}=\"");
    let start = value.find(&marker)? + marker.len();
    let end = value[start..].find('\"')? + start;
    Some(value[start..end].to_owned())
}

pub(crate) async fn claim_and_publish_plex_server(
    state: &AppState,
    account_token: &str,
) -> Result<(String, bool, Option<String>), String> {
    let configured_server_name = server_name(state);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| format!("could not initialize Plex server claim: {error}"))?;
    let claim_response = client
        .get("https://plex.tv/api/claim/token.json")
        .header("Accept", "application/json")
        .header("X-Plex-Product", "Plex Media Server")
        .header("X-Plex-Version", "1.43.4.10903-e5521bd8c")
        .header("X-Plex-Platform", "MacOSX")
        .header("X-Plex-Platform-Version", std::env::consts::ARCH)
        .header("X-Plex-Device-Name", &configured_server_name)
        .header("X-Plex-Device", "MacOSX")
        .header("X-Plex-Provides", "server")
        .header(
            "X-Plex-Client-Identifier",
            state.machine_identifier.as_ref(),
        )
        .header("X-Plex-Token", account_token)
        .send()
        .await
        .map_err(|error| format!("could not request a Plex claim token: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Plex rejected the claim-token request: {error}"))?;
    let claim = claim_response
        .json::<PlexClaimTokenResponse>()
        .await
        .map_err(|error| format!("Plex returned an invalid claim token: {error}"))?;
    if !claim.token.starts_with("claim-") {
        return Err("Plex returned an invalid claim token".to_owned());
    }

    let exchange_response = client
        .post("https://plex.tv/api/claim/exchange")
        .query(&[("token", claim.token.as_str())])
        .header("Accept", "application/xml")
        .header("X-Plex-Product", "Plex Media Server")
        .header("X-Plex-Version", "1.43.4.10903-e5521bd8c")
        .header("X-Plex-Platform", "MacOSX")
        .header("X-Plex-Platform-Version", std::env::consts::ARCH)
        .header("X-Plex-Device-Name", &configured_server_name)
        .header("X-Plex-Device", "MacOSX")
        .header("X-Plex-Provides", "server")
        .header(
            "X-Plex-Client-Identifier",
            state.machine_identifier.as_ref(),
        )
        .send()
        .await
        .map_err(|error| format!("could not exchange the Plex claim token: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Plex rejected the server claim: {error}"))?;
    let exchange_body = exchange_response
        .text()
        .await
        .map_err(|error| format!("could not read the Plex claim response: {error}"))?;
    let server_token = extract_xml_token(&exchange_body)
        .ok_or_else(|| "Plex claim response did not contain a server token".to_owned())?;

    let tls_error = state
        .plex_tls
        .ensure_certificate(
            &client,
            &state.machine_identifier,
            &configured_server_name,
            &server_token,
        )
        .await
        .err();
    if let Some(error) = &tls_error {
        warn!(error = %error, "Plex TLS certificate unavailable; publishing the HTTP connection fallback");
    }
    if let Err(error) =
        publish_plex_connections(&client, state, &configured_server_name, &server_token).await
    {
        return Ok((server_token, false, Some(error)));
    }

    Ok((server_token, true, tls_error))
}

fn set_plex_registration_state(
    connection: &rusqlite::Connection,
    claimed: bool,
    error: Option<&str>,
) -> rusqlite::Result<()> {
    connection.execute(
        "UPDATE server_identity SET claimed = ?1, plex_last_error = ?2 WHERE id = 1",
        (claimed, error),
    )?;
    Ok(())
}

async fn publish_plex_connections(
    client: &reqwest::Client,
    state: &AppState,
    configured_server_name: &str,
    server_token: &str,
) -> Result<(), String> {
    let registration = plex_server_registration_xml(state, configured_server_name)?;
    info!(server_name = %configured_server_name, "Registering Harbinger with Plex");
    let registration_response = client
        .post(plex_server_registration_url())
        .query(&[("auth_token", server_token)])
        .header("User-Agent", "PlexMediaServer/1.43.4.10903-e5521bd8c")
        .header("Accept", "*/*")
        .header("Accept-Encoding", "gzip")
        .header("Content-Type", "application/xml")
        .header("X-Plex-Product", "Plex Media Server")
        .header("X-Plex-Version", "1.43.4.10903-e5521bd8c")
        .header("X-Plex-Platform", "MacOSX")
        .header("X-Plex-Platform-Version", std::env::consts::ARCH)
        .header("X-Plex-Device-Name", configured_server_name)
        .header("X-Plex-Device", "MacOSX")
        .header("X-Plex-Model", std::env::consts::ARCH)
        .header(
            "X-Plex-Client-Identifier",
            state.machine_identifier.as_ref(),
        )
        .header("X-Plex-Provides", "server")
        .header("X-Plex-Device-Vendor", "Apple")
        .body(registration)
        .send()
        .await
        .map_err(|error| format!("could not register harbinger libraries with Plex: {error}"))?;
    // Plex Media Server's measured servers.xml registration can answer 422
    // after creating/updating the server record. Continue with the device
    // announce, which is the request that publishes client connection URIs.
    if !registration_response.status().is_success()
        && registration_response.status() != reqwest::StatusCode::UNPROCESSABLE_ENTITY
    {
        return Err(format!(
            "Plex rejected harbinger library registration: HTTP {}",
            registration_response.status()
        ));
    }
    info!(
        status = registration_response.status().as_u16(),
        server_name = %configured_server_name,
        "Plex server registration response"
    );
    publish_plex_device(client, state, server_token).await?;
    verify_plex_resource(client, state, server_token).await?;
    Ok(())
}

async fn verify_plex_resource(
    client: &reqwest::Client,
    state: &AppState,
    server_token: &str,
) -> Result<(), String> {
    const ATTEMPTS: usize = 6;
    let mut last_error = "Plex.tv did not publish a usable Harbinger server resource".to_owned();
    let mut last_resources = serde_json::Value::Null;
    for attempt in 0..ATTEMPTS {
        let resources = client
            .get("https://clients.plex.tv/api/v2/resources")
            .query(&[
                ("includeHttps", "1"),
                ("includeRelay", "1"),
                ("includeIPv6", "1"),
            ])
            .header("Accept", "application/json")
            .header("X-Plex-Product", "harbinger")
            .header(
                "X-Plex-Client-Identifier",
                state.machine_identifier.as_ref(),
            )
            .header("X-Plex-Token", server_token)
            .send()
            .await
            .map_err(|error| format!("could not verify harbinger in Plex resources: {error}"))?
            .error_for_status()
            .map_err(|error| format!("Plex resource verification failed: {error}"))?
            .json::<serde_json::Value>()
            .await
            .map_err(|error| format!("Plex returned invalid resource data: {error}"))?;
        info!(
            resources = %plex_account_resource_inventory(&resources),
            "Plex account resource inventory"
        );

        let has_server = plex_resources_contain(&resources, state.machine_identifier.as_ref());
        let has_local_https =
            plex_resources_contain_local_https(&resources, state.machine_identifier.as_ref());
        let has_local_connection =
            plex_resources_contain_local_connection(&resources, state.machine_identifier.as_ref());
        if has_server && has_local_connection {
            if state.plex_tls.hash().is_some() && !has_local_https {
                warn!(
                    resources = %plex_resource_connection_summary(
                        &resources,
                        state.machine_identifier.as_ref(),
                    ),
                    "Plex.tv has no local HTTPS connection for Harbinger; keeping its verified local HTTP connection"
                );
            }
            return Ok(());
        }
        last_error = "Plex.tv did not publish a usable local Harbinger connection".to_owned();
        last_resources =
            plex_resource_connection_summary(&resources, state.machine_identifier.as_ref());
        if attempt + 1 < ATTEMPTS {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    warn!(resources = %last_resources, error = %last_error, "Plex resource publication did not converge");
    Err(last_error)
}

pub(crate) async fn run_plex_presence(
    client: &reqwest::Client,
    state: &AppState,
    server_token: &str,
) {
    let account_id = loop {
        match plex_account_id(client, state, server_token).await {
            Ok(id) => break id,
            Err(error) => {
                warn!(error = %error, "Plex presence account lookup failed; retrying in 15 seconds");
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            }
        }
    };

    loop {
        let stream = client
            .get(plex_presence_url(&account_id, &state.machine_identifier))
            .query(&[("X-Plex-Token", server_token)])
            .header(
                "User-Agent",
                "PlexMediaServer-EventSourceClient/1.43.4.10903-e5521bd8c",
            )
            .header("Accept", "*/*")
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
            .send()
            .await;

        match stream {
            Ok(mut response) if response.status().is_success() => {
                info!("Plex presence stream connected");
                if let Err(error) = plex_connectivity_check(client, state, server_token).await {
                    warn!(error = %error, "Plex connectivity check failed");
                }
                loop {
                    match response.chunk().await {
                        Ok(Some(_event)) => {}
                        Ok(None) => {
                            warn!("Plex presence stream closed; reconnecting in 15 seconds");
                            break;
                        }
                        Err(error) => {
                            warn!(error = %error, "Plex presence stream failed; reconnecting in 15 seconds");
                            break;
                        }
                    }
                }
            }
            Ok(response) => {
                warn!(
                    status = response.status().as_u16(),
                    "Plex presence stream rejected; reconnecting in 15 seconds"
                );
            }
            Err(error) => {
                warn!(error = %error, "Plex presence stream could not connect; reconnecting in 15 seconds");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
    }
}

async fn publish_plex_device(
    client: &reqwest::Client,
    state: &AppState,
    server_token: &str,
) -> Result<(), String> {
    let urls = plex_connection_urls(state);
    if urls.is_empty() {
        return Err("could not determine a local connection address".to_owned());
    }
    info!(urls = ?urls, "publishing Plex connection URLs");
    let https_enabled = state.plex_tls.hash().is_some();
    let query = plex_device_announce_query(&urls, https_enabled, server_token);
    let version = "1.43.4.10903-e5521bd8c";
    client
        .put(plex_device_announce_url(&state.machine_identifier))
        .query(&query)
        .header("User-Agent", format!("PlexMediaServer/{version}"))
        .header("Accept", "*/*")
        .header("Accept-Encoding", "gzip")
        .header("Content-Length", "0")
        .header("X-Plex-Product", "Plex Media Server")
        .header("X-Plex-Version", version)
        .header("X-Plex-Platform", "MacOSX")
        .header("X-Plex-Platform-Version", std::env::consts::ARCH)
        .header("X-Plex-Device-Name", server_name(state))
        .header("X-Plex-Device", "MacOSX")
        .header("X-Plex-Model", std::env::consts::ARCH)
        .header("X-Plex-Device-Vendor", "Apple")
        .header("X-Plex-Provides", "server")
        .header(
            "X-Plex-Client-Identifier",
            state.machine_identifier.as_ref(),
        )
        .body(Vec::new())
        .send()
        .await
        .map_err(|error| format!("could not publish harbinger to Plex: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Plex rejected harbinger publication: {error}"))?;
    info!("Plex device announce accepted");
    Ok(())
}

fn extract_xml_token(body: &str) -> Option<String> {
    ["authentication-token", "authenticationToken", "authToken"]
        .iter()
        .find_map(|tag| {
            let start = body.find(&format!("<{tag}>"))? + tag.len() + 2;
            let end = body[start..].find(&format!("</{tag}>"))? + start;
            let value = body[start..end].trim();
            (!value.is_empty()).then(|| value.to_owned())
        })
}

#[derive(Deserialize)]
pub(crate) struct TranslationSettingsForm {
    #[serde(default)]
    pub(crate) audio_language_primary: Option<String>,
    #[serde(default)]
    pub(crate) audio_language_secondary: Option<String>,
    pub(crate) language_primary: String,
    pub(crate) language_secondary: String,
    #[serde(default)]
    pub(crate) bilingual_enabled: bool,
}

#[derive(Deserialize)]
pub(crate) struct SkipSettingsForm {
    pub(crate) skip_intro_mode: String,
    pub(crate) skip_credits_mode: String,
    #[serde(default)]
    pub(crate) marker_detection_disabled: bool,
}

fn copy_metadata_storage(
    state: &AppState,
    source: &str,
    destination: &str,
) -> Result<usize, std::io::Error> {
    let config = state
        .config
        .lock()
        .map_err(|_| std::io::Error::other("configuration lock is unavailable"))?
        .clone();
    let media_index = media_snapshot(state);
    let mut copied = 0;
    let mut copied_groups = HashSet::new();
    for (library_index, library) in config.libraries.iter().enumerate() {
        let section = library_index + 1;
        for media in media_index
            .iter()
            .filter(|media| media_in_library(media, library))
        {
            let Some(root) = library
                .folders
                .iter()
                .find(|folder| media.path.starts_with(folder.as_path()))
            else {
                continue;
            };
            let group_path = metadata_group_path(root, &media.path, &library.kind);
            if !copied_groups.insert((section, group_path.clone())) {
                continue;
            }
            let media_key = database_media_key(section, &media.id);
            let (from, to) = if source == "config" {
                (
                    config_metadata_sidecar_path(state, &media_key)
                        .parent()
                        .unwrap()
                        .to_path_buf(),
                    metadata_sidecar_info_path(&group_path)
                        .parent()
                        .unwrap()
                        .to_path_buf(),
                )
            } else {
                (
                    metadata_sidecar_info_path(&group_path)
                        .parent()
                        .unwrap()
                        .to_path_buf(),
                    config_metadata_sidecar_path(state, &media_key)
                        .parent()
                        .unwrap()
                        .to_path_buf(),
                )
            };
            if source != destination && copy_sidecar_tree(&from, &to)? {
                copied += 1;
            }
        }
    }
    Ok(copied)
}

fn copy_sidecar_tree(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<bool, std::io::Error> {
    let entries = match std::fs::read_dir(source) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let mut copied = false;
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copied |= copy_sidecar_tree(&entry.path(), &target)?;
        } else if file_type.is_file() && std::fs::metadata(&target).is_err() {
            std::fs::create_dir_all(destination)?;
            std::fs::copy(entry.path(), target)?;
            copied = true;
        }
    }
    Ok(copied)
}

pub(crate) fn server_name(state: &AppState) -> String {
    state
        .server_name
        .read()
        .map(|name| name.clone())
        .unwrap_or_else(|_| "harbinger".to_owned())
}

#[axum::debug_handler]
pub(crate) async fn scan_library_metadata(
    State(state): State<AppState>,
    Path(library_id): Path<usize>,
    Query(scan_query): Query<ScanQuery>,
) -> Response {
    let Ok(config) = state.config.lock().map(|config| config.clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let Some(library) = config.libraries.get(library_id) else {
        return settings_error("Library does not exist");
    };
    if !is_metadata_library(&library.kind) {
        info!(library = %library.name, kind = %library.kind, "TMDB metadata matching skipped for library kind");
        return Redirect::to("/settings?metadata_matched=0").into_response();
    }
    let client = reqwest::Client::new();
    let token = config
        .tmdb_bearer_token
        .as_deref()
        .filter(|token| !token.is_empty());
    let sidecar_mode = config.metadata_storage == "media";
    let kind = if library.kind == "tv" { "tv" } else { "movie" };
    let mut matched = 0;
    let indexed_ids = media_snapshot(&state)
        .iter()
        .map(|media| (media.path.clone(), media.id.clone()))
        .collect::<HashMap<_, _>>();
    let forced = scan_query.force.unwrap_or(false);
    let refreshing = scan_query.refresh.unwrap_or(false);
    let assets_only = scan_query.assets_only.unwrap_or(false);
    let target_path = match scan_query.path.as_deref() {
        Some(path) => match clean_relative_path(path) {
            Some(path) => Some(path),
            None => return settings_error("Media path is invalid"),
        },
        None => None,
    };
    if refreshing && target_path.is_none() {
        return settings_error("Refresh requires a media path");
    }
    info!(library = %library.name, forced, refreshing, "metadata scan started");
    for folder in &library.folders {
        let mut files = match scan_media(folder).await {
            Ok(files) => files,
            Err(error) => {
                warn!(library = %library.name, folder = %folder.display(), error = %error, "metadata scan could not read a library folder");
                continue;
            }
        };
        for media in &mut files {
            if let Some(id) = indexed_ids.get(&media.path) {
                media.id.clone_from(id);
            }
        }
        let mut groups = BTreeMap::<PathBuf, Vec<Media>>::new();
        for media in files {
            let group_path = metadata_group_path(folder, &media.path, kind);
            groups.entry(group_path).or_default().push(media);
        }
        for (group_path, group) in groups {
            if let Some(target_path) = &target_path {
                let group_relative = group_path.strip_prefix(folder).unwrap_or(&group_path);
                if group_relative != target_path {
                    continue;
                }
            }

            let group_title = metadata_group_title(&group_path, folder);
            for media in &group {
                if let Err(error) =
                    persist_media_technical_info(&state, library_id + 1, folder, kind, media).await
                {
                    warn!(
                        library = %library.name,
                        path = %media.path.display(),
                        %error,
                        "technical metadata detection failed"
                    );
                }
            }
            if refreshing {
                if let Err(error) = refresh_matched_metadata(
                    &state,
                    &client,
                    token,
                    folder,
                    &group_path,
                    &group_title,
                    &group,
                    library_id,
                    kind,
                    sidecar_mode,
                )
                .await
                {
                    return settings_error(&error);
                }
                matched += 1;
                continue;
            }
            let media_key = database_media_key(library_id + 1, &group[0].id);
            let selected = read_metadata_sidecar(&state, &media_key, &group_path);
            let stored_match = selected
                .as_ref()
                .is_some_and(|metadata| metadata.get("id").is_some());
            if assets_only && !stored_match {
                continue;
            }
            if !forced || stored_match {
                if let Some(mut metadata) = selected.filter(|metadata| metadata.get("id").is_some())
                {
                    if kind == "tv"
                        && (metadata.get("genres").is_none()
                            || metadata.get("content_ratings").is_none())
                    {
                        if let Some(token) = token {
                            if let Some(details) =
                                tmdb_tv_show_details(&client, token, &metadata).await
                            {
                                metadata = merge_tv_show_details(&metadata, &details);
                                if sidecar_mode {
                                    if store_metadata_sidecar(
                                        folder,
                                        &group_path,
                                        &group_title,
                                        &group[0],
                                        &metadata,
                                    )
                                    .await
                                    .is_err()
                                    {
                                        return settings_error(
                                            "Could not update TV show details in the media-folder sidecar",
                                        );
                                    }
                                } else {
                                    for media in &group {
                                        let metadata = tv_metadata_for_media(
                                            &metadata,
                                            folder,
                                            &media.path,
                                            kind,
                                        );
                                        let key = database_media_key(library_id + 1, &media.id);
                                        if !write_config_metadata(&state, &key, &metadata) {
                                            return StatusCode::INTERNAL_SERVER_ERROR
                                                .into_response();
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if sidecar_mode {
                        if (kind == "tv" || !metadata_sidecar_info_path(&group_path).is_file())
                            && store_metadata_sidecar(
                                folder,
                                &group_path,
                                &group_title,
                                &group[0],
                                &metadata,
                            )
                            .await
                            .is_err()
                        {
                            return settings_error(
                                "Could not migrate matched metadata to the JSON sidecar",
                            );
                        }
                    } else {
                        for media in &group {
                            let key = database_media_key(library_id + 1, &media.id);
                            let existing = read_config_metadata(&state, &key);
                            if existing
                                .as_ref()
                                .is_some_and(|metadata| metadata.get("id").is_some())
                            {
                                continue;
                            }
                            let metadata =
                                tv_metadata_for_media(&metadata, folder, &media.path, kind);
                            if !write_config_metadata(&state, &key, &metadata) {
                                warn!(library = %library.name, path = %media.path.display(), "existing match could not be applied to newly found media");
                                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                            }
                        }
                    }
                    if let Some(token) = token {
                        let media_key = database_media_key(library_id + 1, &group[0].id);
                        if let Err(error) = download_tmdb_artwork(
                            &client,
                            token,
                            folder,
                            &group_path,
                            &media_key,
                            &metadata,
                            &state,
                            false,
                        )
                        .await
                        {
                            warn!(library = %library.name, title = %group_title, error = %error, "TMDB artwork refresh failed");
                        }
                    }

                    matched += 1;
                    if kind == "tv" {
                        if let Some(token) = token {
                            if let Err(error) = ensure_tv_season_metadata(
                                &client,
                                token,
                                folder,
                                &group,
                                library_id + 1,
                                &metadata,
                                sidecar_mode,
                                &state,
                                false,
                            )
                            .await
                            {
                                warn!(library = %library.name, title = %group_title, error = %error, "TV season metadata scan failed");
                            }
                        }
                    }
                    continue;
                }
                let previous = read_metadata_sidecar(&state, &media_key, &group_path);
                if let Some(metadata) = previous.filter(|metadata| metadata.get("id").is_some()) {
                    if sidecar_mode {
                        let representative = &group[0];
                        if store_metadata_sidecar(
                            folder,
                            &group_path,
                            &group_title,
                            representative,
                            &metadata,
                        )
                        .await
                        .is_err()
                        {
                            return settings_error(
                                "Could not move matched metadata to the media-folder sidecar",
                            );
                        }
                        if let Some(token) = token {
                            let _ = download_tmdb_artwork(
                                &client,
                                token,
                                folder,
                                &group_path,
                                &media_key,
                                &metadata,
                                &state,
                                true,
                            )
                            .await;
                        }
                    } else {
                        for media in &group {
                            let metadata =
                                tv_metadata_for_media(&metadata, folder, &media.path, kind);
                            let key = database_media_key(library_id + 1, &media.id);
                            if !write_config_metadata(&state, &key, &metadata) {
                                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                            }
                        }
                    }
                    matched += 1;
                    if kind == "tv" {
                        if let Some(token) = token {
                            if let Err(error) = ensure_tv_season_metadata(
                                &client,
                                token,
                                folder,
                                &group,
                                library_id + 1,
                                &metadata,
                                sidecar_mode,
                                &state,
                                false,
                            )
                            .await
                            {
                                warn!(library = %library.name, title = %group_title, error = %error, "TV season metadata scan failed");
                            }
                        }
                    }
                    continue;
                }
            }
            let Some(token) = token else {
                warn!(library = %library.name, title = %group_title, "metadata matching skipped because no TMDB bearer token is configured");
                return settings_error(
                    "Add a TMDB bearer token before matching media without stored metadata",
                );
            };
            let title = metadata_search_title(&group_title);
            info!(library = %library.name, title = %group_title, media_kind = kind, "TMDB metadata search started");
            let response = client
                .get(format!("https://api.themoviedb.org/3/search/{}", kind))
                .bearer_auth(token)
                .query(&[
                    ("query", title.as_str()),
                    ("include_adult", "false"),
                    ("language", "en-US"),
                ])
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    warn!(library = %library.name, title = %group_title, error = %error, "TMDB metadata search request failed");
                    continue;
                }
            };
            let status = response.status();
            if !status.is_success() {
                warn!(library = %library.name, title = %group_title, status = status.as_u16(), "TMDB metadata search returned an error status");
                continue;
            }
            let payload = match response.json::<serde_json::Value>().await {
                Ok(payload) => payload,
                Err(error) => {
                    warn!(library = %library.name, title = %group_title, error = %error, "TMDB metadata response could not be parsed");
                    continue;
                }
            };
            let Some(results) = payload.get("results").and_then(serde_json::Value::as_array) else {
                info!(library = %library.name, title = %group_title, search = %title, "TMDB metadata search returned no results");
                continue;
            };
            let Some((result, match_score)) = best_metadata_result(results, &group_title, kind)
            else {
                info!(library = %library.name, title = %group_title, search = %title, result_count = results.len(), "TMDB results had no confident title match");
                continue;
            };
            let full_result = if kind == "tv" {
                if let Some(details) = tmdb_tv_show_details(&client, token, result).await {
                    merge_tv_show_details(result, &details)
                } else {
                    result.clone()
                }
            } else {
                result.clone()
            };
            let result = &full_result;
            info!(
                library = %library.name,
                title = %group_title,
                match_score,
                matched_title = ?result.get(if kind == "tv" { "name" } else { "title" }),
                tmdb_id = ?result.get("id"),
                poster_available = result.get("poster_path").and_then(serde_json::Value::as_str).is_some(),
                "TMDB metadata matched"
            );
            for media in &group {
                let media_metadata = tv_metadata_for_media(result, folder, &media.path, kind);
                if !sidecar_mode {
                    let media_key = database_media_key(library_id + 1, &media.id);
                    if !write_config_metadata(&state, &media_key, &media_metadata) {
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                }
            }
            if sidecar_mode {
                let representative = &group[0];
                if store_metadata_sidecar(
                    folder,
                    &group_path,
                    &group_title,
                    representative,
                    &tv_metadata_for_media(result, folder, &representative.path, kind),
                )
                .await
                .is_err()
                {
                    return settings_error(
                        "Could not write matched metadata to the media-folder sidecar",
                    );
                }
                let media_key = database_media_key(library_id + 1, &representative.id);
                if let Err(error) = download_tmdb_artwork(
                    &client,
                    token,
                    folder,
                    &group_path,
                    &media_key,
                    result,
                    &state,
                    true,
                )
                .await
                {
                    warn!(library = %library.name, title = %group_title, error = %error, "TMDB artwork download failed");
                }
            }
            if !sidecar_mode {
                let representative = &group[0];
                let media_key = database_media_key(library_id + 1, &representative.id);
                if let Err(error) = download_tmdb_artwork(
                    &client,
                    token,
                    folder,
                    &group_path,
                    &media_key,
                    result,
                    &state,
                    true,
                )
                .await
                {
                    warn!(library = %library.name, title = %group_title, error = %error, "TMDB artwork download failed");
                }
            }
            matched += 1;
            if kind == "tv" {
                if let Err(error) = ensure_tv_season_metadata(
                    &client,
                    token,
                    folder,
                    &group,
                    library_id + 1,
                    result,
                    sidecar_mode,
                    &state,
                    forced,
                )
                .await
                {
                    warn!(library = %library.name, title = %group_title, error = %error, "TV season metadata scan failed");
                }
            }
        }
    }
    info!(library = %library.name, matched, "metadata scan finished");
    let suffix = if forced { "&metadata_forced=true" } else { "" };
    Redirect::to(&format!("/settings?metadata_matched={matched}{suffix}")).into_response()
}

async fn refresh_matched_metadata(
    state: &AppState,
    client: &reqwest::Client,
    token: Option<&str>,
    folder: &PathBuf,
    group_path: &std::path::Path,
    group_title: &str,
    group: &[Media],
    library_id: usize,
    kind: &str,
    sidecar_mode: bool,
) -> Result<(), String> {
    let media_key = database_media_key(library_id + 1, &group[0].id);
    let metadata = read_metadata_sidecar(state, &media_key, group_path)
        .filter(|metadata| {
            metadata
                .get("id")
                .and_then(serde_json::Value::as_i64)
                .is_some()
        })
        .ok_or_else(|| "Refresh requires an existing metadata match".to_owned())?;
    let token =
        token.ok_or_else(|| "Add a TMDB bearer token before refreshing metadata".to_owned())?;
    let details = if kind == "tv" {
        tmdb_tv_show_details(client, token, &metadata)
            .await
            .ok_or_else(|| "Could not refresh the matched TV show from TMDB".to_owned())?
    } else {
        let id = metadata["id"].as_i64().unwrap_or_default();
        let response = client
            .get(format!("https://api.themoviedb.org/3/movie/{id}"))
            .bearer_auth(token)
            .query(&[("language", "en-US")])
            .send()
            .await
            .map_err(|_| "Could not reach TMDB to refresh the matched movie".to_owned())?;
        if !response.status().is_success() {
            return Err("TMDB could not refresh the matched movie".to_owned());
        }
        response
            .json::<serde_json::Value>()
            .await
            .map_err(|_| "TMDB returned invalid movie details".to_owned())?
    };
    if details.get("id").and_then(serde_json::Value::as_i64)
        != metadata.get("id").and_then(serde_json::Value::as_i64)
    {
        return Err("TMDB returned details for a different match".to_owned());
    }
    let refreshed = merge_tv_show_details(&metadata, &details);
    if sidecar_mode {
        store_metadata_sidecar(folder, group_path, group_title, &group[0], &refreshed)
            .await
            .map_err(|_| "Could not save refreshed metadata sidecar".to_owned())?;
    } else {
        for media in group {
            let item_metadata = tv_metadata_for_media(&refreshed, folder, &media.path, kind);
            let key = database_media_key(library_id + 1, &media.id);
            if !write_config_metadata(state, &key, &item_metadata) {
                return Err("Could not save refreshed metadata".to_owned());
            }
        }
    }
    if let Err(error) = download_tmdb_artwork(
        client, token, folder, group_path, &media_key, &refreshed, state, true,
    )
    .await
    {
        warn!(title = %group_title, error = %error, "TMDB artwork refresh failed");
    }
    if kind == "tv" {
        if let Err(error) = ensure_tv_season_metadata(
            client,
            token,
            folder,
            group,
            library_id + 1,
            &refreshed,
            sidecar_mode,
            state,
            true,
        )
        .await
        {
            warn!(title = %group_title, error = %error, "TV season metadata refresh failed");
        }
    }
    Ok(())
}

fn tv_match_target(
    state: &AppState,
    library_index: usize,
    relative_path: &str,
) -> Result<(ConfigLibrary, PathBuf, PathBuf, String, Vec<Media>), String> {
    let library = state
        .config
        .lock()
        .map_err(|_| "Configuration is unavailable".to_owned())?
        .libraries
        .get(library_index)
        .cloned()
        .ok_or_else(|| "Library does not exist".to_owned())?;
    if !matches!(library.kind.as_str(), "tv" | "movie") {
        return Err("Fix Match is only available for movie and TV libraries".to_owned());
    }
    let relative = clean_relative_path(relative_path).ok_or_else(|| {
        if library.kind == "tv" {
            "Invalid TV show path".to_owned()
        } else {
            "Invalid media path".to_owned()
        }
    })?;
    if library.kind == "tv" {
        if relative.components().count() != 1 {
            return Err("Invalid TV show path".to_owned());
        }
        let (root, show_directory) = library
            .folders
            .iter()
            .find_map(|root| {
                let show_directory = root.join(&relative);
                show_directory
                    .is_dir()
                    .then(|| (root.clone(), show_directory))
            })
            .ok_or_else(|| "TV show folder does not exist".to_owned())?;
        let episodes = media_snapshot(state)
            .iter()
            .filter(|media| media.path.starts_with(&show_directory))
            .cloned()
            .collect::<Vec<_>>();
        if episodes.is_empty() {
            return Err("No indexed media found for this TV show".to_owned());
        }
        let title = show_directory
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("TV Show")
            .to_owned();
        return Ok((library, root, show_directory, title, episodes));
    }
    for root in library.folders.clone() {
        let media = media_snapshot(state)
            .iter()
            .filter(|media| {
                metadata_group_path(&root, &media.path, "movie")
                    .strip_prefix(&root)
                    .is_ok_and(|path| path == relative)
            })
            .cloned()
            .collect::<Vec<_>>();
        if let Some(first) = media.first() {
            let group_path = metadata_group_path(&root, &first.path, "movie");
            let title = metadata_group_title(&group_path, &root);
            return Ok((library, root, group_path, title, media));
        }
    }
    Err("No indexed media found for this movie".to_owned())
}

pub(crate) fn metadata_group_path(root: &PathBuf, media: &std::path::Path, kind: &str) -> PathBuf {
    if kind == "tv" {
        if let Ok(relative) = media.strip_prefix(root) {
            if let Some(Component::Normal(show)) = relative.components().next() {
                return root.join(show);
            }
        }
    }
    let parent = media.parent().unwrap_or(root);
    if parent == root {
        media.to_path_buf()
    } else {
        parent.to_path_buf()
    }
}

fn tv_episode_show_directory(root: &PathBuf, media_path: &std::path::Path) -> Option<PathBuf> {
    tv_episode_info(root, media_path)?;
    let relative = media_path.strip_prefix(root).ok()?;
    let mut components = relative.components();
    let first = components.next()?;
    let first_name = first.as_os_str().to_str()?;
    if media_path.parent() == Some(root.as_path())
        || parse_tv_season_directory(first_name).is_some()
    {
        Some(root.clone())
    } else {
        Some(root.join(first.as_os_str()))
    }
}

fn metadata_sidecar_info_path(group_path: &std::path::Path) -> PathBuf {
    let directory = if group_path.is_file() {
        group_path.parent().unwrap_or(group_path)
    } else {
        group_path
    };
    directory.join(".hbgr/data.json")
}

fn parse_sidecar_info(contents: &str) -> Option<serde_json::Value> {
    serde_json::from_str(contents).ok()
}

async fn read_sidecar_info(directory: &std::path::Path) -> Option<serde_json::Value> {
    tokio::fs::read_to_string(directory.join(".hbgr/data.json"))
        .await
        .ok()
        .and_then(|contents| parse_sidecar_info(&contents))
}

fn parse_sidecar_metadata(contents: &str) -> Option<serde_json::Value> {
    parse_sidecar_info(contents)?
        .get("metadata_json")?
        .as_object()
        .map(|metadata| serde_json::Value::Object(metadata.clone()))
}

pub(crate) fn read_sidecar_metadata(group_path: &std::path::Path) -> Option<serde_json::Value> {
    std::fs::read_to_string(metadata_sidecar_info_path(group_path))
        .ok()
        .and_then(|contents| parse_sidecar_metadata(&contents))
}

pub(crate) fn database_media_key(section_id: usize, media_id: &str) -> String {
    let prefix = format!("{section_id}-");
    if media_id.starts_with(&prefix) {
        media_id.to_owned()
    } else {
        format!("{prefix}{media_id}")
    }
}

pub(crate) fn config_metadata_sidecar_path(state: &AppState, media_key: &str) -> PathBuf {
    state
        .subtitle_checkpoints_dir
        .parent()
        .unwrap_or(state.subtitle_checkpoints_dir.as_path())
        .join(".hbgr")
        .join(media_key)
        .join("data.json")
}

pub(crate) fn read_config_metadata(state: &AppState, media_key: &str) -> Option<serde_json::Value> {
    let contents = std::fs::read_to_string(config_metadata_sidecar_path(state, media_key)).ok()?;
    parse_sidecar_metadata(&contents)
}

pub(crate) fn read_metadata_sidecar(
    state: &AppState,
    media_key: &str,
    group_path: &std::path::Path,
) -> Option<serde_json::Value> {
    let config_mode = state
        .config
        .lock()
        .is_ok_and(|config| config.metadata_storage == "config");
    if config_mode {
        read_config_metadata(state, media_key).or_else(|| read_sidecar_metadata(group_path))
    } else {
        read_sidecar_metadata(group_path).or_else(|| read_config_metadata(state, media_key))
    }
}

fn read_info_file(path: &std::path::Path) -> Option<serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| parse_sidecar_info(&contents))
}

fn read_config_info(state: &AppState, media_key: &str) -> Option<serde_json::Value> {
    read_info_file(&config_metadata_sidecar_path(state, media_key))
}

fn sidecar_media_info(state: &AppState, media: &Media) -> Option<serde_json::Value> {
    let (section, library) = media_library(state, media)?;
    let media_key = database_media_key(section, &media.id);
    let root = library
        .folders
        .iter()
        .find(|folder| media.path.starts_with(folder))?;
    let group_path = metadata_group_path(root, &media.path, &library.kind);
    let config_info = read_config_info(state, &media_key);
    let group_info = read_info_file(&metadata_sidecar_info_path(&group_path));
    let episode_info =
        tv_episode_sidecar_metadata_path(root, media).and_then(|path| read_info_file(&path));
    let media_info = media
        .path
        .parent()
        .and_then(|directory| read_info_file(&directory.join(".hbgr/data.json")));
    let config_mode = state
        .config
        .lock()
        .is_ok_and(|config| config.metadata_storage == "config");
    let candidates = if config_mode {
        vec![config_info, episode_info, group_info, media_info]
    } else {
        vec![episode_info, media_info, group_info, config_info]
    };
    candidates
        .into_iter()
        .flatten()
        .find_map(|info| info.get("technical").cloned())
        .filter(|technical| !technical.is_null())
}

fn write_config_metadata(state: &AppState, media_key: &str, metadata: &serde_json::Value) -> bool {
    let path = config_metadata_sidecar_path(state, media_key);
    let Some(directory) = path.parent() else {
        return false;
    };
    if std::fs::create_dir_all(directory).is_err() {
        return false;
    }
    let title = json_string(metadata, "name");
    let title = if title.is_empty() {
        json_string(metadata, "title")
    } else {
        title
    };
    let technical = read_info_file(&path)
        .and_then(|info| info.get("technical").cloned())
        .filter(|technical| !technical.is_null());
    let info = MediaInfo {
        title,
        media_path: None,
        description: json_string(metadata, "overview"),
        release_date: json_string(metadata, "release_date"),
        first_air_date: json_string(metadata, "first_air_date"),
        number_of_seasons: json_i64(metadata, "number_of_seasons"),
        number_of_episodes: json_i64(metadata, "number_of_episodes"),
        season: None,
        episode: None,
        episode_end: None,
        episode_title: None,
        metadata_json: metadata.clone(),
        technical,
        show_id: metadata.get("id").map(|id| id.to_string()),
    };
    serde_json::to_vec_pretty(&info)
        .ok()
        .is_some_and(|contents| std::fs::write(path, contents).is_ok())
}

async fn persist_media_technical_info(
    state: &AppState,
    section: usize,
    root: &PathBuf,
    kind: &str,
    media: &Media,
) -> Result<bool, String> {
    if cached_media_info(state, media).await.is_some() {
        return Ok(false);
    }
    let technical = inspect_media(&media.path)
        .await
        .ok_or_else(|| format!("Could not inspect {}", media.path.display()))?;
    let config_mode = state
        .config
        .lock()
        .is_ok_and(|config| config.metadata_storage == "config");
    let path = if config_mode {
        config_metadata_sidecar_path(state, &database_media_key(section, &media.id))
    } else if kind == "tv" {
        tv_episode_sidecar_metadata_path(root, media).ok_or_else(|| {
            format!(
                "Could not build episode technical path for {}",
                media.path.display()
            )
        })?
    } else {
        metadata_sidecar_info_path(&metadata_group_path(root, &media.path, kind))
    };
    let Some(directory) = path.parent() else {
        return Err(format!(
            "Could not build technical metadata directory for {}",
            media.path.display()
        ));
    };
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("Could not create technical metadata directory: {error}"))?;
    let mut info = read_info_file(&path).unwrap_or_else(|| serde_json::json!({}));
    if !info.is_object() {
        info = serde_json::json!({});
    }
    info.as_object_mut()
        .expect("technical metadata is an object")
        .insert("technical".to_owned(), technical.clone());
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&info).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("Could not save technical metadata: {error}"))?;
    store_media_info(state, media, technical);
    Ok(true)
}

fn migrate_legacy_database_metadata(state: &AppState) {
    if !state
        .config
        .lock()
        .is_ok_and(|config| config.metadata_storage == "config")
    {
        return;
    }
    let current_keys_by_path = {
        let Ok(config) = state.config.lock() else {
            return;
        };
        let media_index = media_snapshot(state);
        let mut keys = HashMap::new();
        for (library_index, library) in config.libraries.iter().enumerate() {
            let section = library_index + 1;
            for media in media_index
                .iter()
                .filter(|media| media_in_library(media, library))
            {
                let media_key = database_media_key(section, &media.id);
                keys.insert(
                    (section, media.path.to_string_lossy().into_owned()),
                    media_key.clone(),
                );
                for folder in &library.folders {
                    if let Ok(relative) = media.path.strip_prefix(folder) {
                        keys.insert(
                            (section, relative.to_string_lossy().into_owned()),
                            media_key.clone(),
                        );
                    }
                }
            }
        }
        keys
    };
    let records = {
        let Ok(connection) = state.db.lock() else {
            return;
        };
        let Ok(mut statement) = connection.prepare(
            "SELECT media_key, path, metadata_json FROM media WHERE metadata_json IS NOT NULL",
        ) else {
            return;
        };
        let Ok(rows) = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        }) else {
            return;
        };
        rows.filter_map(Result::ok).collect::<Vec<_>>()
    };
    for (legacy_key, path, serialized) in records {
        let section = legacy_key
            .split_once('-')
            .and_then(|(section, _)| section.parse::<usize>().ok());
        let media_key = section
            .and_then(|section| current_keys_by_path.get(&(section, path)).cloned())
            .unwrap_or(legacy_key);
        if config_metadata_sidecar_path(state, &media_key).is_file() {
            continue;
        }
        let Ok(metadata) = serde_json::from_str(&serialized) else {
            continue;
        };
        if !write_config_metadata(state, &media_key, &metadata) {
            warn!(%media_key, "legacy metadata could not be migrated to the config sidecar");
        }
    }
}

fn metadata_group_title(group: &std::path::Path, root: &PathBuf) -> String {
    if group.is_file() {
        return group
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Video")
            .to_owned();
    }
    group
        .file_name()
        .and_then(|value| value.to_str())
        .or_else(|| root.file_name().and_then(|value| value.to_str()))
        .unwrap_or("Media")
        .to_owned()
}

fn metadata_search_title(title: &str) -> String {
    let mut words = Vec::new();
    let title = title
        .replace('.', " ")
        .replace('_', " ")
        .replace('-', " ")
        .replace('(', " ")
        .replace(')', " ")
        .replace('[', " ")
        .replace(']', " ");
    for word in title.split_whitespace() {
        let lower = word.to_ascii_lowercase();
        let is_year = word.len() == 4
            && word
                .parse::<u16>()
                .is_ok_and(|year| (1900..=2099).contains(&year));
        let is_release_tag = matches!(
            lower.as_str(),
            "480p"
                | "720p"
                | "1080p"
                | "2160p"
                | "4k"
                | "8k"
                | "bluray"
                | "brrip"
                | "dvdrip"
                | "hdrip"
                | "hdtv"
                | "webrip"
                | "webdl"
                | "web-dl"
                | "x264"
                | "x265"
                | "h264"
                | "h265"
                | "hevc"
                | "remux"
        );
        if is_release_tag || (is_year && !words.is_empty()) {
            break;
        }
        words.push(word);
    }
    words.join(" ")
}

fn normalized_metadata_title(title: &str) -> String {
    metadata_search_title(title)
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn metadata_title_year(title: &str) -> Option<&str> {
    title
        .split(|character: char| !character.is_ascii_digit())
        .find(|part| {
            part.len() == 4
                && part
                    .parse::<u16>()
                    .is_ok_and(|year| (1900..=2099).contains(&year))
        })
}

fn metadata_result_score(title: &str, result: &serde_json::Value, kind: &str) -> i32 {
    let candidate = if kind == "tv" {
        result.get("name").or_else(|| result.get("original_name"))
    } else {
        result.get("title").or_else(|| result.get("original_title"))
    }
    .and_then(serde_json::Value::as_str)
    .unwrap_or_default();
    let expected = normalized_metadata_title(title);
    let actual = normalized_metadata_title(candidate);
    if expected.is_empty() || actual.is_empty() {
        return 0;
    }
    let mut score = if expected == actual {
        100
    } else if expected.contains(&actual) || actual.contains(&expected) {
        70
    } else {
        let expected_words = metadata_search_title(title)
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        let actual_words = metadata_search_title(candidate)
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        let overlap = expected_words
            .iter()
            .filter(|word| actual_words.contains(word))
            .count();
        (60 * overlap / expected_words.len().max(1)) as i32
    };
    let expected_year = metadata_title_year(title);
    let candidate_year = result
        .get(if kind == "tv" {
            "first_air_date"
        } else {
            "release_date"
        })
        .and_then(serde_json::Value::as_str)
        .and_then(metadata_title_year);
    if let Some(year) = expected_year {
        score += if candidate_year == Some(year) {
            15
        } else {
            -10
        };
    }
    score
}

fn best_metadata_result<'a>(
    results: &'a [serde_json::Value],
    title: &str,
    kind: &str,
) -> Option<(&'a serde_json::Value, i32)> {
    results
        .iter()
        .map(|result| (result, metadata_result_score(title, result, kind)))
        .reduce(|best, candidate| {
            if candidate.1 > best.1 {
                candidate
            } else {
                best
            }
        })
        .filter(|(_, score)| *score >= 40)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct TvEpisodeInfo {
    pub(crate) season: u32,
    pub(crate) episode: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) episode_end: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) episode_title: Option<String>,
}

async fn tmdb_tv_show_details(
    client: &reqwest::Client,
    token: &str,
    metadata: &serde_json::Value,
) -> Option<serde_json::Value> {
    let id = metadata.get("id")?.as_u64()?;
    let response = client
        .get(format!("https://api.themoviedb.org/3/tv/{id}"))
        .bearer_auth(token)
        .query(&[
            ("language", "en-US"),
            ("append_to_response", "content_ratings"),
        ])
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json().await.ok()
}

fn merge_tv_show_details(
    metadata: &serde_json::Value,
    details: &serde_json::Value,
) -> serde_json::Value {
    let mut metadata = metadata.clone();
    if let (Some(target), Some(details)) = (metadata.as_object_mut(), details.as_object()) {
        target.extend(details.clone());
    }
    metadata
}

fn tv_metadata_for_media(
    metadata: &serde_json::Value,
    root: &PathBuf,
    media: &std::path::Path,
    kind: &str,
) -> serde_json::Value {
    if kind != "tv" {
        return metadata.clone();
    }
    let mut enriched = metadata.clone();
    let Some(object) = enriched.as_object_mut() else {
        return enriched;
    };
    for key in [
        "mplex_season",
        "mplex_episode",
        "mplex_episode_end",
        "mplex_episode_title",
    ] {
        object.remove(key);
    }
    let Some(episode) = tv_episode_info(root, media) else {
        return enriched;
    };
    object.insert(
        "mplex_season".to_owned(),
        serde_json::Value::from(episode.season),
    );
    object.insert(
        "mplex_episode".to_owned(),
        serde_json::Value::from(episode.episode),
    );
    if let Some(end) = episode.episode_end {
        object.insert("mplex_episode_end".to_owned(), serde_json::Value::from(end));
    }
    if let Some(title) = episode.episode_title {
        object.insert(
            "mplex_episode_title".to_owned(),
            serde_json::Value::from(title),
        );
    }
    enriched
}

pub(crate) fn tv_episode_info(root: &PathBuf, media: &std::path::Path) -> Option<TvEpisodeInfo> {
    let stem = media.file_stem()?.to_str()?;
    let (season_from_name, episode, marker_end) = tv_episode_marker(stem)?;
    let season = media
        .strip_prefix(root)
        .ok()?
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .find_map(parse_tv_season_directory)
        .or(season_from_name)
        .unwrap_or(1);
    Some(TvEpisodeInfo {
        season,
        episode: episode.0,
        episode_end: episode.1,
        episode_title: clean_episode_title(&stem[marker_end..]),
    })
}

fn media_has_season_directory(root: &PathBuf, media: &std::path::Path) -> bool {
    let Ok(relative) = media.strip_prefix(root) else {
        return false;
    };
    let components = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    components
        .iter()
        .take(components.len().saturating_sub(1))
        .any(|name| parse_tv_season_directory(name).is_some())
}

fn tv_season_number(root: &PathBuf, media_path: &std::path::Path) -> Option<u32> {
    tv_episode_info(root, media_path)
        .map(|episode| episode.season)
        .or_else(|| {
            media_path
                .strip_prefix(root)
                .ok()?
                .components()
                .filter_map(|component| component.as_os_str().to_str())
                .find_map(parse_tv_season_directory)
        })
        .or(Some(1))
}

fn tv_season_directory(
    show_directory: &std::path::Path,
    season: u32,
    episode_path: &std::path::Path,
) -> PathBuf {
    let physical_directory = episode_path.parent();
    if physical_directory
        .and_then(std::path::Path::file_name)
        .and_then(|name| name.to_str())
        .and_then(parse_tv_season_directory)
        == Some(season)
    {
        physical_directory.unwrap_or(show_directory).to_path_buf()
    } else {
        show_directory.join(format!("Season {season:02}"))
    }
}

pub(crate) fn tv_show_title(root: &PathBuf, media: &std::path::Path) -> String {
    let relative = media.strip_prefix(root).unwrap_or(media);
    let components = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components.len() > 1 {
        if parse_tv_season_directory(components[0]).is_some() {
            return root
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("TV Shows")
                .to_owned();
        }
        return components[0].to_owned();
    }
    let stem = media
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Episode");
    if let Some((_, _, marker_start)) = tv_episode_marker(stem) {
        let prefix = stem[..marker_start]
            .trim_end_matches([' ', '.', '_', '-'])
            .replace(['.', '_'], " ");
        if !prefix.trim().is_empty() {
            return prefix.trim().to_owned();
        }
    }
    root.file_name()
        .and_then(|value| value.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(stem)
        .to_owned()
}

fn parse_tv_season_directory(name: &str) -> Option<u32> {
    let normalized_name = name.trim().replace(['.', '_', '-'], " ");
    let normalized = normalized_name.split_whitespace().collect::<Vec<_>>();
    if normalized.len() == 1 && normalized[0].eq_ignore_ascii_case("specials") {
        return Some(0);
    }
    if normalized.len() == 2 && normalized[0].eq_ignore_ascii_case("season") {
        return normalized[1].parse().ok();
    }
    None
}

fn parse_tv_episode_marker(stem: &str) -> Option<(Option<u32>, (u32, Option<u32>), usize)> {
    let bytes = stem.as_bytes();
    for start in 0..bytes.len() {
        if !bytes[start].eq_ignore_ascii_case(&b's')
            || (start > 0 && bytes[start - 1].is_ascii_alphanumeric())
        {
            continue;
        }
        let Some((season, after_season)) = parse_digits(bytes, start + 1) else {
            continue;
        };
        if bytes.get(after_season)?.to_ascii_lowercase() != b'e' {
            continue;
        }
        let Some((episode, after_episode)) = parse_digits(bytes, after_season + 1) else {
            continue;
        };
        let (episode_end, marker_end) = if bytes.get(after_episode) == Some(&b'-')
            && bytes
                .get(after_episode + 1)
                .is_some_and(|byte| byte.eq_ignore_ascii_case(&b'e'))
        {
            if let Some((end, marker_end)) = parse_digits(bytes, after_episode + 2) {
                (Some(end), marker_end)
            } else {
                (None, after_episode)
            }
        } else {
            (None, after_episode)
        };
        return Some((Some(season), (episode, episode_end), marker_end));
    }
    for start in 0..bytes.len() {
        if start + 7 > bytes.len() || !bytes[start..start + 7].eq_ignore_ascii_case(b"episode") {
            continue;
        }
        let mut number_start = start + 7;
        while bytes
            .get(number_start)
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'.' | b'-' | b'#'))
        {
            number_start += 1;
        }
        if let Some((episode, marker_end)) = parse_digits(bytes, number_start) {
            return Some((None, (episode, None), marker_end));
        }
    }
    None
}

/// Single episode-recognition entry point for every UI, metadata, Plex, and marker surface.
///
/// Callers must use this instead of the individual parsers so a filename accepted in one place is
/// accepted everywhere.
pub(crate) fn tv_episode_marker(stem: &str) -> Option<(Option<u32>, (u32, Option<u32>), usize)> {
    parse_tv_episode_marker(stem).or_else(|| trailing_episode_marker(stem))
}

fn trailing_episode_marker(stem: &str) -> Option<(Option<u32>, (u32, Option<u32>), usize)> {
    let bytes = stem.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let start = bytes[..end]
        .iter()
        .rposition(|byte| !byte.is_ascii_digit())
        .map_or(0, |index| index + 1);
    if start == end || end - start > 3 {
        return None;
    }
    if start > 0 {
        let separator = bytes[start - 1];
        if separator.is_ascii_alphanumeric() || matches!(separator, b'(' | b')' | b']') {
            return None;
        }
    }
    let episode = std::str::from_utf8(&bytes[start..end])
        .ok()?
        .parse::<u32>()
        .ok()
        .filter(|episode| *episode > 0)?;
    Some((None, (episode, None), start))
}

fn parse_digits(bytes: &[u8], start: usize) -> Option<(u32, usize)> {
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() && end - start < 3 {
        end += 1;
    }
    if end == start {
        return None;
    }
    Some((
        std::str::from_utf8(&bytes[start..end]).ok()?.parse().ok()?,
        end,
    ))
}

fn clean_episode_title(value: &str) -> Option<String> {
    let title = value
        .trim_start_matches(|character: char| {
            character == '-' || character == '_' || character == '.' || character.is_whitespace()
        })
        .trim();
    (!title.is_empty()).then(|| title.to_owned())
}

async fn store_metadata_sidecar(
    folder: &PathBuf,
    group_path: &std::path::Path,
    title: &str,
    media: &Media,
    metadata: &serde_json::Value,
) -> Result<(), std::io::Error> {
    let media_directory = if group_path.is_file() {
        group_path.parent().unwrap_or(folder)
    } else {
        group_path
    };
    let directory = media_directory.join(".hbgr");
    tokio::fs::create_dir_all(&directory).await?;
    let tv_show =
        metadata.get("first_air_date").is_some() || metadata.get("number_of_seasons").is_some();
    let path = media
        .path
        .strip_prefix(folder)
        .unwrap_or(&media.path)
        .to_string_lossy();
    let episode = (!tv_show)
        .then(|| tv_episode_info(folder, &media.path))
        .flatten();
    let show_id = read_sidecar_info(media_directory)
        .await
        .and_then(|value| value.get("show_id")?.as_str().map(str::to_owned));
    let technical = if tv_show {
        None
    } else {
        match read_info_file(&directory.join("data.json"))
            .and_then(|info| info.get("technical").cloned())
        {
            Some(technical) => Some(technical),
            None => inspect_media(&media.path).await,
        }
    };
    let info = MediaInfo {
        title: title.to_owned(),
        media_path: (!tv_show).then(|| path.into_owned()),
        description: json_string(metadata, "overview"),
        release_date: json_string(metadata, "release_date"),
        first_air_date: json_string(metadata, "first_air_date"),
        number_of_seasons: json_i64(metadata, "number_of_seasons"),
        number_of_episodes: json_i64(metadata, "number_of_episodes"),
        season: episode.as_ref().map(|value| value.season),
        episode: episode.as_ref().map(|value| value.episode),
        episode_end: episode.as_ref().and_then(|value| value.episode_end),
        episode_title: episode.and_then(|value| value.episode_title),
        metadata_json: metadata.clone(),
        technical,
        show_id,
    };
    let contents = serde_json::to_string_pretty(&info).map_err(std::io::Error::other)?;
    tokio::fs::write(directory.join("data.json"), contents).await
}

#[derive(Serialize)]
struct MediaInfo {
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    media_path: Option<String>,
    description: String,
    release_date: String,
    first_air_date: String,
    number_of_seasons: Option<i64>,
    number_of_episodes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    season: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    episode: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    episode_end: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    episode_title: Option<String>,
    metadata_json: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    technical: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    show_id: Option<String>,
}

#[derive(Serialize)]
struct SeasonInfo {
    title: String,
    description: String,
    season: u32,
    metadata_json: serde_json::Value,
}

pub(crate) async fn inspect_media(path: &std::path::Path) -> Option<serde_json::Value> {
    let started = Instant::now();
    let inspect_path = path.to_owned();
    let result = tokio::task::spawn_blocking(move || {
        let json = vuio_media_info::inspect(&inspect_path)
            .ok()?
            .to_json()
            .ok()?;
        let mut value: serde_json::Value = serde_json::from_str(&json).ok()?;
        fill_missing_matroska_languages(&inspect_path, &mut value);
        Some(value)
    })
    .await
    .ok()
    .flatten();
    let elapsed_ms = started.elapsed().as_millis();
    if elapsed_ms > 250 {
        info!(
            path = %path.display(),
            elapsed_ms,
            inspected = result.is_some(),
            "media inspection completed"
        );
    }
    result
}

/// vuio sometimes reports Matroska text tracks without their language tag even
/// though the track has one. Fill only missing/unknown languages from the
/// container header so the subtitle picker and translation source agree with
/// ffprobe without running an external process.
fn fill_missing_matroska_languages(path: &std::path::Path, info: &mut serde_json::Value) {
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let Ok(container) = MatroskaFile::open(file) else {
        return;
    };
    let languages = container
        .tracks()
        .iter()
        .filter_map(|track| {
            let language = track.language_bcp47().or_else(|| track.language())?;
            let (code, _) = plex_language(language);
            (code != "und").then(|| (track.track_number().get(), code.to_owned()))
        })
        .collect::<HashMap<_, _>>();
    if languages.is_empty() {
        return;
    }
    fill_missing_track_languages(info, &languages);
}

fn fill_missing_track_languages(info: &mut serde_json::Value, languages: &HashMap<u64, String>) {
    let Some(tracks) = info
        .get_mut("media")
        .and_then(|media| media.get_mut("track"))
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for track in tracks {
        if track
            .get("Language")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|language| plex_language(language).0 != "und")
        {
            continue;
        }
        let id = track
            .get("ID")
            .or_else(|| track.get("StreamOrder"))
            .and_then(serde_json::Value::as_str)
            .and_then(|id| id.parse::<u64>().ok());
        if let Some(language) = id.and_then(|id| languages.get(&id)) {
            track["Language"] = serde_json::json!(language);
        }
    }
}

#[cfg(test)]
mod subtitle_language_tests {
    use super::fill_missing_track_languages;
    use std::collections::HashMap;

    #[test]
    fn fills_missing_matroska_languages_without_overwriting_known_ones() {
        let mut info = serde_json::json!({
            "media": {
                "track": [
                    {"ID": "3", "@type": "Text", "Format": "UTF-8"},
                    {"ID": "4", "@type": "Text", "Language": "chi"}
                ]
            }
        });
        let languages = HashMap::from([(3, "en".to_owned()), (4, "zh".to_owned())]);
        fill_missing_track_languages(&mut info, &languages);
        assert_eq!(info["media"]["track"][0]["Language"], "en");
        assert_eq!(info["media"]["track"][1]["Language"], "chi");
    }
}

pub(crate) fn technical_tracks(info: &serde_json::Value) -> &[serde_json::Value] {
    info.get("media")
        .and_then(|media| media.get("track"))
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

pub(crate) fn technical_text<'a>(track: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    track.get(key).and_then(serde_json::Value::as_str)
}

fn technical_bitrate_kbps(track: &serde_json::Value) -> Option<u64> {
    technical_text(track, "BitRate")
        .or_else(|| technical_text(track, "BitRate_Nominal"))
        .and_then(|value| value.parse::<u64>().ok())
        .map(|bits_per_second| bits_per_second / 1000)
}

fn average_bitrate_kbps(size_bytes: u64, duration_ms: i64) -> Option<u64> {
    let duration_ms = u128::try_from(duration_ms).ok()?;
    if duration_ms == 0 {
        return None;
    }
    let bits = u128::from(size_bytes).checked_mul(8)?;
    u64::try_from((bits + duration_ms / 2) / duration_ms).ok()
}

pub(crate) fn technical_track_id(track: &serde_json::Value) -> u64 {
    technical_text(track, "StreamOrder")
        .or_else(|| technical_text(track, "ID"))
        .and_then(|id| id.parse::<u64>().ok())
        .unwrap_or_default()
}

fn technical_duration_ms(info: &serde_json::Value) -> Option<i64> {
    technical_tracks(info)
        .iter()
        .find(|track| technical_text(track, "@type") == Some("General"))
        .and_then(|track| technical_text(track, "Duration"))
        .and_then(|seconds| seconds.parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .map(|seconds| (seconds * 1000.0).round() as i64)
}

pub(crate) fn find_media_part(state: &AppState, id: &str) -> Option<Media> {
    let id = id.parse::<u64>().ok()?;
    media_snapshot(state)
        .iter()
        .find(|media| plex_part_id(media) == id)
        .cloned()
}

pub(crate) fn iso_639_2_bibliographic_alias(code: &str) -> Option<&'static str> {
    match code {
        "alb" => Some("sqi"),
        "arm" => Some("hye"),
        "baq" => Some("eus"),
        "bur" => Some("mya"),
        "chi" => Some("zho"),
        "cze" => Some("ces"),
        "dut" => Some("nld"),
        "fre" => Some("fra"),
        "geo" => Some("kat"),
        "ger" => Some("deu"),
        "gre" => Some("ell"),
        "ice" => Some("isl"),
        "mac" => Some("mkd"),
        "mao" => Some("mri"),
        "may" => Some("msa"),
        "per" => Some("fas"),
        "rum" => Some("ron"),
        "slo" => Some("slk"),
        "tib" => Some("bod"),
        "wel" => Some("cym"),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct PlexBilingualStream {
    extension: &'static str,
    pending: bool,
    generating: bool,
}

pub(crate) async fn add_plex_technical_metadata(
    state: &AppState,
    item: &mut serde_json::Value,
    media: &Media,
) {
    let info = media_info_for(state, media)
        .await
        .unwrap_or_else(|| serde_json::json!({"media": {"track": []}}));
    let (
        _bilingual_enabled,
        audio_language_primary,
        audio_language_secondary,
        subtitle_language_primary,
        _subtitle_language_secondary,
    ) = effective_plex_language_settings(state);
    let subtitle_default_language = if subtitle_language_primary == "off" {
        state
            .config
            .lock()
            .map(|config| config.subtitle_language_primary.clone())
            .unwrap_or_else(|_| subtitle_language_primary.clone())
    } else {
        subtitle_language_primary
    };
    let (enabled, language_primary, language_secondary) = configured_bilingual_languages(state);
    let external_subtitles = if enabled {
        if state
            .config
            .lock()
            .is_ok_and(|config| config.metadata_storage == "media")
        {
            let _ = ensure_merged_bilingual_sidecar(media, &language_primary, &language_secondary)
                .await;
        }
        external_subtitle_files(&media.path).await
    } else {
        external_subtitle_files(&media.path)
            .await
            .into_iter()
            .filter(|path| external_subtitle_pair(&media.path, path).is_none())
            .collect()
    };
    let has_requested_bilingual_file = external_subtitles.iter().any(|path| {
        external_subtitle_pair(&media.path, path).is_some_and(|(first, second)| {
            first == language_primary && second == language_secondary
        })
    });
    let saved_translation_extension = ["srt", "vtt", "ass"].into_iter().find(|extension| {
        configured_bilingual_subtitle_path(
            state,
            media,
            &language_primary,
            &language_secondary,
            extension,
        )
        .is_file()
    });
    let has_checkpoint =
        bilingual_checkpoint_path(state, media, &language_primary, &language_secondary).is_file();
    let selected_stream_id = stored_plex_stream_selection(state, media)
        .and_then(|selection| selection.subtitle_stream_id);
    let translated_stream_id =
        plex_translated_subtitle_id(media, &language_primary, &language_secondary);
    let selected_checkpoint = selected_stream_id == Some(translated_stream_id);
    let translation_stream =
        (enabled && language_primary != language_secondary && !has_requested_bilingual_file)
            .then_some(PlexBilingualStream {
                extension: saved_translation_extension.unwrap_or("srt"),
                pending: saved_translation_extension.is_none()
                    && (!has_checkpoint || !selected_checkpoint),
                generating: saved_translation_extension.is_none()
                    && has_checkpoint
                    && selected_checkpoint,
            });
    let selection = stored_plex_stream_selection(state, media);
    apply_plex_technical_metadata(
        item,
        media,
        &info,
        &external_subtitles,
        selection,
        &audio_language_primary,
        audio_language_secondary.as_deref(),
        &language_primary,
        &language_secondary,
        &subtitle_default_language,
        translation_stream,
    );
}

fn plex_stream_is_subtitle(stream: &serde_json::Value) -> bool {
    stream.get("streamType").and_then(serde_json::Value::as_u64) == Some(3)
}

fn plex_stream_is_pending_bilingual(stream: &serde_json::Value) -> bool {
    stream.get("pending").and_then(serde_json::Value::as_bool) == Some(true)
}

fn plex_default_subtitle_stream_id(streams: &[serde_json::Value], language: &str) -> Option<u64> {
    if language == "off" {
        return None;
    }
    let subtitles = streams
        .iter()
        .filter(|stream| {
            plex_stream_is_subtitle(stream) && !plex_stream_is_pending_bilingual(stream)
        })
        .collect::<Vec<_>>();
    let preferred_code = plex_language(language).0;
    let preferred = if preferred_code == "und" {
        None
    } else {
        subtitles.iter().find(|stream| {
            stream
                .get("languageTag")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|tag| plex_language(tag).0 == preferred_code)
        })
    };
    subtitles
        .iter()
        .find(|stream| stream.get("bilingual").and_then(serde_json::Value::as_bool) == Some(true))
        .or_else(|| preferred)
        .and_then(|stream| stream.get("id").and_then(serde_json::Value::as_u64))
}

fn apply_plex_technical_metadata(
    item: &mut serde_json::Value,
    media: &Media,
    info: &serde_json::Value,
    external_subtitles: &[PathBuf],
    selection: Option<PlexStreamSelection>,
    audio_language_primary: &str,
    audio_language_secondary: Option<&str>,
    language_primary: &str,
    language_secondary: &str,
    subtitle_default_language: &str,
    translation_stream: Option<PlexBilingualStream>,
) {
    let tracks = technical_tracks(info);
    let duration = technical_duration_ms(&info)
        .or_else(|| item.get("duration").and_then(serde_json::Value::as_i64))
        .unwrap_or_default();
    let selected_stream_id = |stream_type: &str| {
        tracks
            .iter()
            .filter(|track| technical_text(track, "@type") == Some(stream_type))
            .find(|track| technical_text(track, "Default") == Some("Yes"))
            .or_else(|| {
                tracks
                    .iter()
                    .find(|track| technical_text(track, "@type") == Some(stream_type))
            })
            .map(|track| technical_track_id(track))
    };
    let selected_video = selected_stream_id("Video");
    let preferred_audio = [Some(audio_language_primary), audio_language_secondary]
        .into_iter()
        .flatten()
        .find_map(|language| {
            tracks
                .iter()
                .find(|track| {
                    technical_text(track, "@type") == Some("Audio")
                        && plex_track_language(track).0 == language
                })
                .map(technical_track_id)
        });
    let selected_audio = preferred_audio.or_else(|| selected_stream_id("Audio"));
    let audio_track_count = tracks
        .iter()
        .filter(|track| technical_text(track, "@type") == Some("Audio"))
        .count();
    if let Some(metadata) = item.as_object_mut() {
        metadata.insert("duration".to_owned(), serde_json::json!(duration));
    }
    let Some(media_json) = item
        .get_mut("Media")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|media| media.first_mut())
    else {
        return;
    };
    media_json["duration"] = serde_json::json!(duration);
    if let Some(bitrate) = tracks
        .iter()
        .find(|track| technical_text(track, "@type") == Some("General"))
        .and_then(|track| technical_bitrate_kbps(track))
        .or_else(|| average_bitrate_kbps(media.size, duration))
    {
        media_json["bitrate"] = serde_json::json!(bitrate);
    }
    let mut plex_streams = Vec::new();
    for track in tracks {
        let stream_type = match technical_text(track, "@type") {
            Some("Video") => 1,
            Some("Audio") => 2,
            Some("Text") => 3,
            _ => continue,
        };
        let track_id = technical_track_id(track);
        let codec_value = technical_text(track, "Format")
            .or_else(|| technical_text(track, "CodecID"))
            .unwrap_or("unknown");
        let codec = plex_codec(codec_value);
        let (mut language_code, mut language_name) = plex_track_language(track);
        let fallback_audio_language =
            (stream_type == 2 && audio_track_count == 1 && language_code == "und")
                .then_some(plex_language(PLEX_DEFAULT_AUDIO_LANGUAGE).1);
        if fallback_audio_language.is_some() {
            (language_code, language_name) = plex_language(PLEX_DEFAULT_AUDIO_LANGUAGE);
        }
        let display_title = fallback_audio_language
            .map(|language| plex_track_label_with_fallback(track, stream_type, Some(language)))
            .unwrap_or_else(|| plex_track_label(track, stream_type));
        let (stream_display_title, stream_extended_display_title) = if stream_type == 3 {
            let detail = display_title
                .split_once(" (")
                .and_then(|(_, detail)| detail.strip_suffix(')'))
                .unwrap_or(codec_value);
            (
                if language_name == "Unknown" {
                    display_title.as_str()
                } else {
                    language_name
                },
                detail,
            )
        } else {
            (display_title.as_str(), display_title.as_str())
        };
        let mut plex_stream = serde_json::json!({
            "id": track_id + 1,
            "streamType": stream_type,
            "type": stream_type,
            "index": track_id,
            "codec": codec,
            "language": if language_code == "und" { "None" } else { language_name },
            "languageCode": plex_language_code_3(language_code),
            "languageTag": language_code,
            "displayTitle": stream_display_title,
            "extendedDisplayTitle": stream_extended_display_title,
            "title": technical_text(track, "Title")
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    if stream_type == 3 {
                        stream_extended_display_title.to_owned()
                    } else {
                        display_title.clone()
                    }
                }),
            "default": technical_text(track, "Default") == Some("Yes"),
            "selected": match stream_type {
                1 => selected_video == Some(track_id),
                2 => selection
                    .and_then(|selection| selection.audio_stream_id)
                    .map(|selected| selected == track_id + 1)
                    .unwrap_or(selected_audio == Some(track_id)),
                3 => false,
                _ => false,
            },
            "forced": technical_text(track, "Forced") == Some("Yes")
        });
        if stream_type == 1 {
            let width = technical_text(track, "Width")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or_default();
            let height = technical_text(track, "Height")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or_default();
            if height > 0 {
                media_json["videoResolution"] = serde_json::json!(plex_video_resolution(height));
            }
            media_json["width"] = serde_json::json!(width);
            media_json["height"] = serde_json::json!(height);
            media_json["videoCodec"] = serde_json::json!(codec);
            plex_stream["width"] = serde_json::json!(width);
            plex_stream["height"] = serde_json::json!(height);
            if let Some(profile) = technical_text(track, "Format_Profile") {
                let profile = plex_video_profile(profile);
                media_json["videoProfile"] = serde_json::json!(profile);
                plex_stream["profile"] = serde_json::json!(profile);
            }
            if let Some(level) = technical_text(track, "Format_Level").and_then(plex_video_level) {
                media_json["videoLevel"] = serde_json::json!(level);
                plex_stream["level"] = serde_json::json!(level);
            }
            if let Some(bitrate) = technical_bitrate_kbps(track) {
                plex_stream["bitrate"] = serde_json::json!(bitrate);
            }
            if let Some(aspect_ratio) = technical_text(track, "DisplayAspectRatio")
                .and_then(|value| value.parse::<f64>().ok())
            {
                media_json["aspectRatio"] = serde_json::json!(aspect_ratio);
            } else if height > 0 {
                media_json["aspectRatio"] = serde_json::json!(width as f64 / height as f64);
            }
            if let Some(frame_rate) = technical_text(track, "FrameRate") {
                media_json["videoFrameRate"] = serde_json::json!(frame_rate);
                plex_stream["frameRate"] = serde_json::json!(frame_rate);
            }
        } else if stream_type == 2 {
            media_json["audioCodec"] = serde_json::json!(codec);
            if let Some(layout) = technical_text(track, "ChannelLayout") {
                media_json["audioChannelLayout"] = serde_json::json!(layout);
            }
            if let Some(channels) =
                technical_text(track, "Channels").and_then(|value| value.parse::<u64>().ok())
            {
                media_json["audioChannels"] = serde_json::json!(channels);
                plex_stream["channels"] = serde_json::json!(channels);
            }
            if let Some(rate) =
                technical_text(track, "SamplingRate").and_then(|value| value.parse::<u64>().ok())
            {
                plex_stream["samplingRate"] = serde_json::json!(rate);
            }
        }
        plex_streams.push(plex_stream);
    }
    for subtitle_path in external_subtitles {
        let id = plex_external_subtitle_id(media, &subtitle_path);
        let (display_title, extended_display_title) =
            external_subtitle_titles(&media.path, &subtitle_path);
        let bilingual =
            external_subtitle_pair(&media.path, &subtitle_path).is_some_and(|(first, second)| {
                first == language_primary && second == language_secondary
            });
        plex_streams.push(serde_json::json!({
            "id": id,
            "streamType": 3,
            "type": 3,
            "index": -1,
            "codec": subtitle_path.extension().and_then(|value| value.to_str()).unwrap_or("srt").to_ascii_lowercase(),
            "language": external_subtitle_language(&media.path, &subtitle_path).1,
            "languageCode": plex_language_code_3(external_subtitle_language(&media.path, &subtitle_path).0),
            "languageTag": external_subtitle_language(&media.path, &subtitle_path).0,
            "displayTitle": display_title,
            "extendedDisplayTitle": extended_display_title,
            "title": extended_display_title,
            "external": true,
            // Plex clients such as Plezy append the requested subtitle codec
            // extension to Stream.key when fetching the sidecar.
            "key": format!("/library/streams/{id}"),
            "default": false,
            "selected": false,
            "forced": false,
            "bilingual": bilingual
        }));
    }
    if let Some(translation) = translation_stream {
        let id = plex_translated_subtitle_id(media, language_primary, language_secondary);
        let label = format!(
            "{} + {}",
            plex_language(language_primary).1,
            plex_language(language_secondary).1
        );
        let language = plex_language(language_primary);
        let display_title = if translation.pending {
            format!("{label} (generate)")
        } else if translation.generating {
            format!("{label} (generating…)")
        } else {
            label.clone()
        };
        let extra_info = if translation.pending {
            format!(
                "{} External • Generate",
                translation.extension.to_ascii_uppercase()
            )
        } else if translation.generating {
            format!(
                "{} External • Generating",
                translation.extension.to_ascii_uppercase()
            )
        } else {
            format!(
                "Bilingual {} External",
                translation.extension.to_ascii_uppercase()
            )
        };
        plex_streams.push(serde_json::json!({
            "id": id, "streamType": 3, "type": 3, "index": -1, "codec": translation.extension,
            "language": language.1,
            "languageCode": plex_language_code_3(language.0), "languageTag": language.0,
            "displayTitle": display_title, "extendedDisplayTitle": extra_info, "title": extra_info,
            "external": true, "key": format!("/library/streams/{id}"), "default": false,
            "selected": false, "pending": translation.pending, "bilingual": true,
            "generating": translation.generating,
            "forced": false
        }));
    }
    let saved_subtitle_stream_id = selection.and_then(|selection| selection.subtitle_stream_id);
    let selected_subtitle_stream_id = match saved_subtitle_stream_id {
        Some(0) => None,
        Some(stream_id)
            if plex_streams.iter().any(|stream| {
                plex_stream_is_subtitle(stream)
                    && stream.get("id").and_then(serde_json::Value::as_u64) == Some(stream_id)
                    && !plex_stream_is_pending_bilingual(stream)
            }) =>
        {
            Some(stream_id)
        }
        _ => plex_default_subtitle_stream_id(&plex_streams, subtitle_default_language),
    };
    for stream in &mut plex_streams {
        if plex_stream_is_subtitle(stream) {
            stream["selected"] = serde_json::json!(
                stream.get("id").and_then(serde_json::Value::as_u64) == selected_subtitle_stream_id
            );
        }
    }
    media_json["id"] = serde_json::json!(plex_media_id(media));
    let part_technical_fields = ["bitrate", "videoProfile", "videoLevel"]
        .iter()
        .filter_map(|field| media_json.get(*field).cloned().map(|value| (*field, value)))
        .collect::<Vec<_>>();
    let part = media_json
        .get_mut("Part")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|parts| parts.first_mut());
    if let Some(part) = part {
        let part_id = plex_part_id(media);
        part["id"] = serde_json::json!(part_id);
        part["key"] = serde_json::json!(format!(
            "/library/parts/{part_id}/{}/file.{}",
            media_modified_at(media),
            media.container
        ));
        part["duration"] = serde_json::json!(duration);
        part["Stream"] = serde_json::json!(plex_streams);
        for (field, value) in part_technical_fields {
            part[field] = value;
        }
    }
}

fn json_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_owned()
}

fn json_i64(value: &serde_json::Value, key: &str) -> Option<i64> {
    value.get(key).and_then(|value| value.as_i64())
}

async fn download_tmdb_artwork(
    client: &reqwest::Client,
    token: &str,
    folder: &PathBuf,
    group_path: &std::path::Path,
    media_key: &str,
    metadata: &serde_json::Value,
    state: &AppState,
    replace_existing: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config_mode = state
        .config
        .lock()
        .map_err(|_| "Configuration is unavailable")?
        .metadata_storage
        == "config";
    let media_directory = if group_path.is_file() {
        group_path.parent().unwrap_or(folder)
    } else {
        group_path
    };
    let artwork = if config_mode {
        config_metadata_sidecar_path(state, media_key)
            .parent()
            .unwrap_or(state.subtitle_checkpoints_dir.as_path())
            .to_path_buf()
    } else {
        media_directory.join(".hbgr")
    };
    tokio::fs::create_dir_all(&artwork).await?;
    for (kind, field) in [("cover", "poster_path"), ("banner", "backdrop_path")] {
        let Some(path) = metadata.get(field).and_then(|value| value.as_str()) else {
            info!(artwork = kind, "TMDB match has no artwork path");
            continue;
        };
        let local = artwork.join(format!("{kind}.jpg"));
        if !replace_existing && tokio::fs::metadata(&local).await.is_ok() {
            continue;
        }

        info!(
            artwork = kind,
            image_path = path,
            "TMDB artwork request started"
        );
        let response = match client
            .get(format!("https://image.tmdb.org/t/p/w780{}", path))
            .bearer_auth(token)
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                warn!(artwork = kind, error = %error, "TMDB artwork request failed");
                return Err(Box::new(error));
            }
        };
        let status = response.status();
        if !status.is_success() {
            warn!(
                artwork = kind,
                status = status.as_u16(),
                "TMDB artwork request returned an error status"
            );
            continue;
        }
        let bytes = response.bytes().await?;
        tokio::fs::write(&local, &bytes).await?;
        info!(
            artwork = kind,
            bytes = bytes.len(),
            "TMDB artwork downloaded"
        );
    }
    Ok(())
}

async fn download_tmdb_season_artwork(
    client: &reqwest::Client,
    token: &str,
    season_directory: &std::path::Path,
    show_metadata: &serde_json::Value,
    season: u32,
    replace_existing: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !replace_existing
        && read_sidecar_info(season_directory)
            .await
            .and_then(|info| info.get("metadata_json").cloned())
            .is_some_and(|metadata| metadata.is_object())
    {
        return Ok(());
    }

    let Some(show_id) = show_metadata.get("id").and_then(|value| value.as_i64()) else {
        return Ok(());
    };
    let response = client
        .get(format!(
            "https://api.themoviedb.org/3/tv/{show_id}/season/{season}"
        ))
        .bearer_auth(token)
        .query(&[("language", "en-US")])
        .send()
        .await?;
    if !response.status().is_success() {
        return Ok(());
    }
    let metadata = response.json::<serde_json::Value>().await?;
    let artwork = season_directory.join(".hbgr");
    tokio::fs::create_dir_all(&artwork).await?;
    let info = SeasonInfo {
        title: format!("Season {season:02}"),
        description: json_string(&metadata, "overview"),
        season,
        metadata_json: metadata.clone(),
    };
    tokio::fs::write(
        artwork.join("data.json"),
        serde_json::to_string_pretty(&info)?,
    )
    .await?;
    let Some(path) = metadata.get("poster_path").and_then(|value| value.as_str()) else {
        return Ok(());
    };
    let image = client
        .get(format!("https://image.tmdb.org/t/p/w780{path}"))
        .bearer_auth(token)
        .send()
        .await?;
    if !image.status().is_success() {
        return Ok(());
    }
    let local = artwork.join("cover.jpg");
    tokio::fs::write(&local, image.bytes().await?).await?;
    Ok(())
}

async fn download_tmdb_season_metadata(
    client: &reqwest::Client,
    token: &str,
    folder: &PathBuf,
    show_metadata: &serde_json::Value,
    season: u32,
    episodes: &[&Media],
    library_key: usize,
    state: &AppState,
    replace_existing: bool,
) -> Result<(), String> {
    let Some(show_id) = show_metadata.get("id").and_then(serde_json::Value::as_i64) else {
        return Ok(());
    };
    if !replace_existing
        && episodes.iter().all(|media| {
            read_config_metadata(state, &database_media_key(library_key, &media.id))
                .and_then(|metadata| metadata.get("mplex_season_metadata_fetched").cloned())
                .and_then(|value| value.as_bool())
                == Some(true)
        })
    {
        return Ok(());
    }
    let response = client
        .get(format!(
            "https://api.themoviedb.org/3/tv/{show_id}/season/{season}"
        ))
        .bearer_auth(token)
        .query(&[("language", "en-US")])
        .send()
        .await
        .map_err(|error| format!("Could not fetch season {season}: {error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "TMDB returned {} for season {season}",
            response.status()
        ));
    }
    let season_metadata = response
        .json::<serde_json::Value>()
        .await
        .map_err(|error| format!("TMDB returned invalid details for season {season}: {error}"))?;
    for media in episodes {
        let key = database_media_key(library_key, &media.id);
        let metadata = read_config_metadata(state, &key)
            .unwrap_or_else(|| tv_metadata_for_media(show_metadata, folder, &media.path, "tv"));
        let metadata = tv_metadata_with_season_details(
            &metadata,
            &season_metadata,
            tv_episode_info(folder, &media.path).map(|episode| episode.episode),
        );
        if !write_config_metadata(state, &key, &metadata) {
            return Err(format!(
                "Could not save season {season} details for {}",
                media.path.display()
            ));
        }
    }
    Ok(())
}

fn tv_metadata_with_episode_details(
    metadata: &serde_json::Value,
    episode_metadata: &serde_json::Value,
) -> serde_json::Value {
    let mut metadata = metadata.clone();
    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            "mplex_episode_metadata".to_owned(),
            episode_metadata.clone(),
        );
        object.insert(
            "mplex_episode_details_fetched".to_owned(),
            serde_json::Value::Bool(true),
        );
    }
    metadata
}

fn tmdb_season_episode_count(show_metadata: &serde_json::Value, season: u32) -> Option<u32> {
    let seasons = show_metadata
        .get("seasons")
        .or_else(|| {
            show_metadata
                .get("metadata_json")
                .and_then(|metadata| metadata.get("seasons"))
        })
        .and_then(serde_json::Value::as_array)?;
    seasons
        .iter()
        .find(|entry| {
            entry
                .get("season_number")
                .and_then(serde_json::Value::as_u64)
                == Some(season as u64)
        })
        .and_then(|entry| entry.get("episode_count"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|count| u32::try_from(count).ok())
}

fn tmdb_episode_details_not_found(metadata: &serde_json::Value) -> bool {
    metadata
        .get("mplex_episode_details_not_found")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

fn tmdb_episode_request_needed(
    show_metadata: &serde_json::Value,
    episode: &TvEpisodeInfo,
    stored_metadata: Option<&serde_json::Value>,
    replace_existing: bool,
) -> bool {
    if tmdb_season_episode_count(show_metadata, episode.season)
        .is_some_and(|count| episode.episode > count)
    {
        return false;
    }
    if !replace_existing
        && stored_metadata.is_some_and(|metadata| tmdb_episode_details_not_found(metadata))
    {
        return false;
    }
    true
}

fn tmdb_episode_not_found_metadata() -> serde_json::Value {
    serde_json::json!({ "mplex_episode_details_not_found": true })
}

fn write_tv_episode_sidecar_metadata(
    root: &std::path::Path,
    media: &Media,
    metadata: &serde_json::Value,
) -> Result<(), String> {
    let metadata_path = tv_episode_sidecar_metadata_path(root, media).ok_or_else(|| {
        format!(
            "Could not build episode metadata path for {}",
            media.path.display()
        )
    })?;
    let Some(directory) = metadata_path.parent() else {
        return Err(format!(
            "Could not build episode metadata directory for {}",
            media.path.display()
        ));
    };
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("Could not create episode metadata directory: {error}"))?;
    let technical = read_info_file(&metadata_path)
        .and_then(|info| info.get("technical").cloned())
        .filter(|technical| !technical.is_null());
    let mut value = metadata.clone();
    if let (Some(object), Some(technical)) = (value.as_object_mut(), technical) {
        object.insert("technical".to_owned(), technical);
    }
    std::fs::write(
        &metadata_path,
        serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("Could not save episode metadata: {error}"))
}

fn tv_episode_cache_stem(episode: &TvEpisodeInfo) -> String {
    format!("s{:02}e{:02}", episode.season, episode.episode)
}

fn tv_episode_sidecar_metadata_path(root: &std::path::Path, media: &Media) -> Option<PathBuf> {
    let episode = tv_episode_info(&root.to_path_buf(), &media.path)?;
    Some(
        media
            .path
            .parent()?
            .join(".hbgr/episodes")
            .join(format!("{}.json", tv_episode_cache_stem(&episode))),
    )
}

fn tv_episode_artwork_path(
    state: &AppState,
    library_key: usize,
    root: &std::path::Path,
    media: &Media,
    sidecar_mode: bool,
) -> Option<PathBuf> {
    let episode = tv_episode_info(&root.to_path_buf(), &media.path)?;
    if sidecar_mode {
        Some(
            media
                .path
                .parent()?
                .join(".hbgr/episodes")
                .join(format!("{}.jpg", tv_episode_cache_stem(&episode))),
        )
    } else {
        let key = database_media_key(library_key, &media.id);
        Some(
            config_metadata_sidecar_path(state, &key)
                .parent()?
                .join("episode.jpg"),
        )
    }
}

pub(crate) fn read_tv_episode_sidecar_metadata(
    root: &std::path::Path,
    media: &Media,
) -> Option<serde_json::Value> {
    let path = tv_episode_sidecar_metadata_path(root, media)?;
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

async fn download_tmdb_episode_metadata(
    client: &reqwest::Client,
    token: &str,
    show_metadata: &serde_json::Value,
    root: &std::path::Path,
    episodes: &[&Media],
    library_key: usize,
    state: &AppState,
    sidecar_mode: bool,
    replace_existing: bool,
) -> Result<(), String> {
    let Some(show_id) = show_metadata.get("id").and_then(serde_json::Value::as_i64) else {
        return Ok(());
    };
    for media in episodes {
        let Some(episode) = tv_episode_info(&root.to_path_buf(), &media.path) else {
            continue;
        };
        let media_key = database_media_key(library_key, &media.id);
        let stored_metadata = if sidecar_mode {
            read_tv_episode_sidecar_metadata(root, media)
        } else {
            read_config_metadata(state, &media_key)
        };
        let cached_episode_metadata = if sidecar_mode {
            stored_metadata.clone()
        } else {
            stored_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("mplex_episode_metadata").cloned())
        };
        let details_cached = if sidecar_mode {
            cached_episode_metadata.is_some()
        } else {
            stored_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("mplex_episode_details_fetched").cloned())
                .and_then(|value| value.as_bool())
                == Some(true)
        };
        if !tmdb_episode_request_needed(
            show_metadata,
            &episode,
            stored_metadata.as_ref(),
            replace_existing,
        ) {
            debug!(
                show_id,
                season = episode.season,
                episode = episode.episode,
                "TMDB episode details request skipped"
            );
            continue;
        }
        let artwork_cached = tv_episode_artwork_path(state, library_key, root, media, sidecar_mode)
            .is_some_and(|path| path.is_file());
        let no_episode_still = cached_episode_metadata.as_ref().is_some_and(|metadata| {
            metadata
                .get("still_path")
                .and_then(serde_json::Value::as_str)
                .is_none_or(str::is_empty)
        });
        if details_cached && !replace_existing && (no_episode_still || artwork_cached) {
            continue;
        }
        let response = match client
            .get(format!(
                "https://api.themoviedb.org/3/tv/{show_id}/season/{}/episode/{}",
                episode.season, episode.episode
            ))
            .bearer_auth(token)
            .query(&[("language", "en-US")])
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                let status = response.status();
                warn!(
                    show_id,
                    season = episode.season,
                    episode = episode.episode,
                    status = status.as_u16(),
                    "TMDB episode details request returned an error status"
                );
                if status == reqwest::StatusCode::NOT_FOUND {
                    let cached = if sidecar_mode {
                        write_tv_episode_sidecar_metadata(
                            root,
                            media,
                            &tmdb_episode_not_found_metadata(),
                        )
                    } else {
                        let mut metadata = stored_metadata.clone().unwrap_or_else(|| {
                            tv_metadata_for_media(
                                show_metadata,
                                &root.to_path_buf(),
                                &media.path,
                                "tv",
                            )
                        });
                        if let Some(object) = metadata.as_object_mut() {
                            object.insert(
                                "mplex_episode_details_not_found".to_owned(),
                                serde_json::Value::Bool(true),
                            );
                        }
                        if write_config_metadata(state, &media_key, &metadata) {
                            Ok(())
                        } else {
                            Err(format!(
                                "Could not save episode metadata for {}",
                                media.path.display()
                            ))
                        }
                    };
                    if let Err(error) = cached {
                        warn!(show_id, season = episode.season, episode = episode.episode, error = %error, "Could not cache TMDB episode 404");
                    }
                }
                continue;
            }
            Err(error) => {
                warn!(show_id, season = episode.season, episode = episode.episode, error = %error, "TMDB episode details request failed");
                continue;
            }
        };
        let episode_metadata = match response.json::<serde_json::Value>().await {
            Ok(metadata) => metadata,
            Err(error) => {
                warn!(show_id, season = episode.season, episode = episode.episode, error = %error, "TMDB episode details response was invalid");
                continue;
            }
        };
        let matches_requested_episode = episode_metadata
            .get("season_number")
            .and_then(serde_json::Value::as_u64)
            .is_none_or(|season| season == episode.season as u64)
            && episode_metadata
                .get("episode_number")
                .and_then(serde_json::Value::as_u64)
                .is_none_or(|number| number == episode.episode as u64);
        if !matches_requested_episode {
            warn!(
                show_id,
                season = episode.season,
                episode = episode.episode,
                "TMDB returned details for a different episode"
            );
            continue;
        }
        if sidecar_mode {
            write_tv_episode_sidecar_metadata(root, media, &episode_metadata)?;
        } else {
            let metadata = read_config_metadata(state, &media_key).unwrap_or_else(|| {
                tv_metadata_for_media(show_metadata, &root.to_path_buf(), &media.path, "tv")
            });
            let metadata = tv_metadata_with_episode_details(&metadata, &episode_metadata);
            if !write_config_metadata(state, &media_key, &metadata) {
                return Err(format!(
                    "Could not save episode metadata for {}",
                    media.path.display()
                ));
            }
        }
        let Some(still_path) = episode_metadata
            .get("still_path")
            .and_then(serde_json::Value::as_str)
            .filter(|path| !path.is_empty())
        else {
            continue;
        };
        let Some(artwork_path) =
            tv_episode_artwork_path(state, library_key, root, media, sidecar_mode)
        else {
            continue;
        };
        if !replace_existing && artwork_path.is_file() {
            continue;
        }
        let image = match client
            .get(format!("https://image.tmdb.org/t/p/w780{still_path}"))
            .bearer_auth(token)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                warn!(
                    show_id,
                    season = episode.season,
                    episode = episode.episode,
                    status = response.status().as_u16(),
                    "TMDB episode still request returned an error status"
                );
                continue;
            }
            Err(error) => {
                warn!(show_id, season = episode.season, episode = episode.episode, error = %error, "TMDB episode still request failed");
                continue;
            }
        };
        let bytes = image.bytes().await.map_err(|error| error.to_string())?;
        if let Some(directory) = artwork_path.parent() {
            tokio::fs::create_dir_all(directory)
                .await
                .map_err(|error| format!("Could not create episode artwork directory: {error}"))?;
        }
        tokio::fs::write(artwork_path, bytes)
            .await
            .map_err(|error| format!("Could not save episode still: {error}"))?;
    }
    Ok(())
}

fn tv_metadata_with_season_details(
    metadata: &serde_json::Value,
    season_metadata: &serde_json::Value,
    episode_number: Option<u32>,
) -> serde_json::Value {
    let mut metadata = metadata.clone();
    let Some(object) = metadata.as_object_mut() else {
        return metadata;
    };
    object.insert(
        "mplex_season_description".to_owned(),
        serde_json::Value::String(json_string(season_metadata, "overview")),
    );
    object.insert(
        "mplex_season_metadata_fetched".to_owned(),
        serde_json::Value::Bool(true),
    );
    object.remove("mplex_episode_metadata");
    object.remove("mplex_episode_details_fetched");
    if let Some(episode_number) = episode_number {
        if let Some(details) = season_metadata
            .get("episodes")
            .and_then(serde_json::Value::as_array)
            .and_then(|items| {
                items.iter().find(|item| {
                    item.get("episode_number")
                        .and_then(serde_json::Value::as_u64)
                        == Some(episode_number as u64)
                })
            })
        {
            object.insert("mplex_episode_metadata".to_owned(), details.clone());
        }
    }
    metadata
}

async fn ensure_tv_season_metadata(
    client: &reqwest::Client,
    token: &str,
    folder: &PathBuf,
    group: &[Media],
    library_key: usize,
    show_metadata: &serde_json::Value,
    sidecar_mode: bool,
    state: &AppState,
    replace_existing: bool,
) -> Result<(), String> {
    let mut seasons = BTreeMap::<u32, Vec<&Media>>::new();
    for media in group {
        if let Some(season) = tv_season_number(folder, &media.path) {
            seasons.entry(season).or_default().push(media);
        }
    }
    for (season, episodes) in seasons {
        let Some(episode_path) = episodes.first().map(|media| media.path.as_path()) else {
            continue;
        };
        let Some(show_name) = episode_path
            .strip_prefix(folder)
            .ok()
            .and_then(|path| path.components().next())
        else {
            continue;
        };
        let show_directory = folder.join(show_name.as_os_str());
        let season_directory = tv_season_directory(&show_directory, season, episode_path);
        let result = if sidecar_mode {
            download_tmdb_season_artwork(
                client,
                token,
                &season_directory,
                show_metadata,
                season,
                replace_existing,
            )
            .await
            .map_err(|error| error.to_string())
        } else {
            download_tmdb_season_metadata(
                client,
                token,
                folder,
                show_metadata,
                season,
                &episodes,
                library_key,
                state,
                replace_existing,
            )
            .await
        };
        result?;

        download_tmdb_episode_metadata(
            client,
            token,
            show_metadata,
            folder,
            &episodes,
            library_key,
            state,
            sidecar_mode,
            replace_existing,
        )
        .await?;
    }
    Ok(())
}

#[derive(Deserialize, Default)]
pub(crate) struct WebPath {
    pub(crate) search: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) library: Option<usize>,
    pub(crate) folder: Option<usize>,
    pub(crate) kind: Option<String>,
    #[serde(alias = "v")]
    pub(crate) version: Option<i64>,
    pub(crate) offset: Option<usize>,
    pub(crate) stream: Option<usize>,
    pub(crate) play: Option<String>,
    pub(crate) episode: Option<String>,
    pub(crate) season: Option<u32>,
    pub(crate) bilingual: Option<bool>,
}

#[derive(Deserialize, Default)]
pub(crate) struct WebSearchQuery {
    pub(crate) query: Option<String>,
}

pub(crate) fn next_tv_episode_paths(
    libraries: &[ConfigLibrary],
    media_items: &[Media],
    watched_paths: &std::collections::HashSet<String>,
    hidden_paths: &std::collections::HashSet<String>,
    last_activity: &std::collections::HashMap<String, i64>,
) -> Vec<(String, i64)> {
    let mut next_paths = Vec::new();
    for library in libraries
        .iter()
        .filter(|library| library.kind == "tv" && library.show_continue_watching)
    {
        for root in &library.folders {
            let mut shows =
                std::collections::BTreeMap::<String, Vec<(String, (u32, u32, u32))>>::new();
            for media in media_items {
                let Ok(relative) = media.path.strip_prefix(root) else {
                    continue;
                };
                let Some(show_folder) = relative.components().next() else {
                    continue;
                };
                let Some(episode) = tv_episode_info(root, &media.path) else {
                    continue;
                };
                let path = relative.to_string_lossy().into_owned();
                let show = show_folder.as_os_str().to_string_lossy().into_owned();
                let last_episode = episode.episode_end.unwrap_or(episode.episode);
                shows
                    .entry(show)
                    .or_default()
                    .push((path, (episode.season, episode.episode, last_episode)));
            }
            for mut episodes in shows.into_values() {
                episodes.sort_by_key(|(_, (season, episode, _))| (*season, *episode));
                let Some(show_activity) = episodes
                    .iter()
                    .filter_map(|(path, _)| last_activity.get(path))
                    .copied()
                    .max()
                else {
                    continue;
                };
                if let Some((path, _)) = episodes
                    .iter()
                    .filter(|(path, _)| {
                        !watched_paths.contains(path) && !hidden_paths.contains(path)
                    })
                    .min_by_key(|(_, (season, episode, _))| (*season, *episode))
                {
                    next_paths.push((path.clone(), show_activity));
                }
            }
        }
    }
    next_paths
}

pub(crate) struct AudioTrack {
    pub(crate) label: String,
    pub(crate) language: String,
    pub(crate) stream_id: u64,
    pub(crate) index: usize,
}

pub(crate) struct SubtitleTrack {
    pub(crate) label: String,
    pub(crate) url: String,
    pub(crate) stream_id: u64,
    pub(crate) language: Option<String>,
    pub(crate) bilingual: bool,
    pub(crate) pending_bilingual: bool,
    pub(crate) generating: bool,
}

fn store_media_info(state: &AppState, media: &Media, info: serde_json::Value) {
    if let Ok(mut cache) = state.media_info.lock() {
        cache.insert(media.id.clone(), info);
    }
    if let Ok(mut pending) = state.media_info_pending.lock() {
        pending.remove(&media.id);
    }
}

pub(crate) async fn cached_media_info(
    state: &AppState,
    media: &Media,
) -> Option<serde_json::Value> {
    if let Some(info) = state
        .media_info
        .lock()
        .ok()
        .and_then(|cache| cache.get(&media.id).cloned())
    {
        return Some(info);
    }
    let read_state = state.clone();
    let read_media = media.clone();
    let info = tokio::task::spawn_blocking(move || {
        sidecar_media_info(&read_state, &read_media).map(|mut info| {
            fill_missing_matroska_languages(&read_media.path, &mut info);
            info
        })
    })
    .await
    .ok()
    .flatten()?;
    store_media_info(state, media, info.clone());
    Some(info)
}

async fn inspect_and_store_media_info(
    state: &AppState,
    media: &Media,
) -> Option<serde_json::Value> {
    let info = inspect_media(&media.path).await?;
    store_media_info(state, media, info.clone());
    Some(info)
}

pub(crate) fn schedule_media_info_inspection(state: &AppState, media: &Media) {
    {
        let Ok(mut pending) = state.media_info_pending.lock() else {
            return;
        };
        if !pending.insert(media.id.clone()) {
            return;
        }
    }
    let state = state.clone();
    let media = media.clone();
    tokio::spawn(async move {
        let _inspection = state.media_info_inspection_lock.lock().await;
        if cached_media_info(&state, &media).await.is_some() {
            if let Ok(mut pending) = state.media_info_pending.lock() {
                pending.remove(&media.id);
            }
            return;
        }
        inspect_and_store_media_info(&state, &media).await;
    });
}

pub(crate) async fn ensure_media_info(
    state: &AppState,
    media: &Media,
) -> Option<serde_json::Value> {
    if let Some(info) = cached_media_info(state, media).await {
        return Some(info);
    }
    let _inspection = state.media_info_inspection_lock.lock().await;
    if let Some(info) = cached_media_info(state, media).await {
        return Some(info);
    }
    inspect_and_store_media_info(state, media).await
}

pub(crate) async fn media_info_for(state: &AppState, media: &Media) -> Option<serde_json::Value> {
    if let Some(info) = cached_media_info(state, media).await {
        return Some(info);
    }
    schedule_media_info_inspection(state, media);
    None
}

pub(crate) async fn external_subtitle_files(video: &std::path::Path) -> Vec<PathBuf> {
    let Some(stem) = video.file_stem().and_then(|value| value.to_str()) else {
        return Vec::new();
    };
    let Ok(mut entries) =
        tokio::fs::read_dir(video.parent().unwrap_or(std::path::Path::new("."))).await
    else {
        return Vec::new();
    };
    let mut tracks = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let Some(candidate_stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let candidate_stem_lower = candidate_stem.to_ascii_lowercase();
        let stem_lower = stem.to_ascii_lowercase();
        if matches!(extension.as_str(), "srt" | "ass" | "vtt")
            && entry.file_type().await.is_ok_and(|kind| kind.is_file())
            && (candidate_stem_lower == stem_lower
                || candidate_stem_lower.starts_with(&format!("{stem_lower}.")))
        {
            tracks.push(path);
        }
    }
    tracks.sort_by_key(|path| path.file_name().unwrap_or_default().to_ascii_lowercase());
    tracks
}

async fn embedded_subtitle_source(
    state: &AppState,
    media: &Media,
    stream_id: u64,
) -> Result<(String, String), String> {
    if !supports_embedded_subtitle_extraction(media) {
        return Err(
            "Embedded subtitle extraction supports Matroska text tracks and MP4 mov_text tracks"
                .to_owned(),
        );
    }
    let info = ensure_media_info(state, media)
        .await
        .ok_or_else(|| "Embedded subtitle metadata is unavailable".to_owned())?;
    let track = technical_tracks(&info)
        .into_iter()
        .find(|track| {
            technical_text(track, "@type") == Some("Text")
                && technical_track_id(track).saturating_add(1) == stream_id
        })
        .ok_or_else(|| "The selected subtitle stream was not found".to_owned())?;
    let codec = technical_text(track, "Format")
        .or_else(|| technical_text(track, "CodecID"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !is_translatable_text_codec(&codec) {
        return Err(
            "This embedded subtitle is image based and cannot be translated as text".to_owned(),
        );
    }
    let stream_order =
        technical_text(track, "StreamOrder").and_then(|value| value.parse::<u64>().ok());
    let container_track_id =
        technical_text(track, "ID").and_then(|value| value.parse::<u64>().ok());
    let track_name = technical_text(track, "Title").map(str::to_owned);
    let track_language = technical_text(track, "Language")
        .or_else(|| technical_text(track, "Language_String"))
        .map(str::to_owned);
    let track_codec = technical_text(track, "CodecID")
        .or_else(|| technical_text(track, "Format"))
        .unwrap_or_default()
        .to_owned();
    let path = media.path.clone();
    let mp4_text = is_mov_text_codec(&track_codec);
    let srt = tokio::task::spawn_blocking(move || {
        if mp4_text {
            extract_mp4_mov_text_subtitle(&path, container_track_id)
        } else {
            extract_matroska_subtitle(
                &path,
                container_track_id,
                stream_order,
                track_name.as_deref(),
                track_language.as_deref(),
                &track_codec,
            )
        }
    })
    .await
    .map_err(|_| "Embedded subtitle extraction failed".to_owned())??;
    let language = plex_track_language(track).0.to_owned();
    Ok((srt, language))
}

async fn read_subtitle_source(
    state: &AppState,
    media: &Media,
    stream_id: u64,
) -> Result<(String, String, String), String> {
    for path in external_subtitle_files(&media.path).await {
        if plex_external_subtitle_id(media, &path) != stream_id {
            continue;
        }
        if external_subtitle_pair(&media.path, &path).is_some() {
            return Err("The selected subtitle is already bilingual".to_owned());
        }
        let contents = tokio::fs::read_to_string(&path)
            .await
            .map_err(|_| "Subtitle source could not be read".to_owned())?;
        let language = external_subtitle_language(&media.path, &path).0.to_owned();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        return Ok((contents, language, extension));
    }
    let (contents, language) = embedded_subtitle_source(state, media, stream_id).await?;
    Ok((contents, language, "srt".to_owned()))
}

fn extract_matroska_subtitle(
    video: &std::path::Path,
    container_track_id: Option<u64>,
    stream_order: Option<u64>,
    track_name: Option<&str>,
    track_language: Option<&str>,
    source_codec: &str,
) -> Result<String, String> {
    let file =
        std::fs::File::open(video).map_err(|_| "Media file could not be opened".to_owned())?;
    let mut container = MatroskaFile::open(file)
        .map_err(|_| "Embedded subtitles could not be read from this container".to_owned())?;
    let subtitle_tracks = container
        .tracks()
        .iter()
        .filter(|track| track.track_type() == TrackType::Subtitle)
        .collect::<Vec<_>>();
    let codec_matches = |track: &&matroska_demuxer::TrackEntry| {
        let matroska_codec = track.codec_id().to_ascii_lowercase();
        let source_codec = source_codec.to_ascii_lowercase();
        match matroska_codec.as_str() {
            "s_text/utf8" => ["utf-8", "utf8", "subrip", "s_text/utf8"]
                .iter()
                .any(|codec| source_codec.contains(codec)),
            "s_text/ass" => ["ass", "s_text/ass"]
                .iter()
                .any(|codec| source_codec.contains(codec)),
            "s_text/ssa" => ["ssa", "s_text/ssa"]
                .iter()
                .any(|codec| source_codec.contains(codec)),
            _ => false,
        }
    };
    let track_by_id = container_track_id.and_then(|id| {
        subtitle_tracks
            .iter()
            .copied()
            .find(|track| track.track_number().get() == id && codec_matches(track))
    });
    let track_by_metadata = subtitle_tracks.iter().copied().find(|track| {
        if !codec_matches(track) {
            return false;
        }
        let name_matches = track_name
            .zip(track.name())
            .is_some_and(|(source, container)| source.eq_ignore_ascii_case(container));
        let language_matches = track_language
            .zip(track.language_bcp47().or_else(|| track.language()))
            .is_some_and(|(source, container)| {
                let source_code = plex_language(source).0;
                let container_code = plex_language(container).0;
                (source_code != "und" && source_code == container_code)
                    || source.eq_ignore_ascii_case(container)
            });
        name_matches || language_matches
    });
    let track_by_order = stream_order
        .and_then(|order| container.tracks().get(order as usize))
        .filter(|track| track.track_type() == TrackType::Subtitle && codec_matches(track));
    let track = track_by_id
        .or(track_by_metadata)
        .or(track_by_order)
        .or_else(|| (subtitle_tracks.len() == 1).then(|| subtitle_tracks[0]))
        .ok_or_else(|| {
            "Could not match this subtitle to a text track in the Matroska container".to_owned()
        })?;
    let track_number = track.track_number().get();
    let codec = track.codec_id().to_ascii_uppercase();
    if !matches!(codec.as_str(), "S_TEXT/UTF8" | "S_TEXT/ASS" | "S_TEXT/SSA") {
        return Err(
            "This embedded subtitle is image based and cannot be translated as text".to_owned(),
        );
    }
    let scale = container.info().timestamp_scale().get();
    let default_duration_ms = track
        .default_duration()
        .map(|duration| duration.get() / 1_000_000)
        .unwrap_or(4_000)
        .max(1);
    let mut frame = MatroskaFrame::default();
    let mut cues = Vec::new();
    while container
        .next_frame(&mut frame)
        .map_err(|_| "Embedded subtitle frames could not be demuxed".to_owned())?
    {
        if frame.track != track_number {
            continue;
        }
        let start_ms = frame.timestamp.saturating_mul(scale) / 1_000_000;
        let duration_ms = frame
            .duration
            .map(|duration| duration.saturating_mul(scale) / 1_000_000)
            .filter(|duration| *duration > 0)
            .unwrap_or(default_duration_ms);
        let end_ms = start_ms.saturating_add(duration_ms);
        let raw_text = String::from_utf8_lossy(&frame.data)
            .trim_matches('\0')
            .trim()
            .to_owned();
        let text = if matches!(codec.as_str(), "S_TEXT/ASS" | "S_TEXT/SSA") {
            let payload = raw_text.splitn(9, ',').nth(8).unwrap_or(&raw_text);
            strip_ass_tags(payload)
                .replace("\\N", "\n")
                .replace("\\n", "\n")
        } else {
            raw_text
        };
        if text.trim().is_empty() {
            continue;
        }
        cues.push((start_ms, end_ms, text));
    }
    if cues.is_empty() {
        return Err("The selected embedded subtitle track has no readable text cues".to_owned());
    }
    let mut output = String::new();
    for (index, (start, end, text)) in cues.iter().enumerate() {
        output.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            index + 1,
            srt_time(*start),
            srt_time(*end),
            text
        ));
    }
    Ok(output)
}

fn extract_mp4_mov_text_subtitle(
    video: &std::path::Path,
    container_track_id: Option<u64>,
) -> Result<String, String> {
    let file =
        std::fs::File::open(video).map_err(|_| "Media file could not be opened".to_owned())?;
    let size = file
        .metadata()
        .map_err(|_| "Media file size could not be read".to_owned())?
        .len();
    let mut reader = mp4::Mp4Reader::read_header(file, size)
        .map_err(|_| "Embedded MP4 subtitles could not be read".to_owned())?;
    let mut tracks = reader
        .tracks()
        .iter()
        .filter(|(_, track)| {
            track.track_type().ok() == Some(mp4::TrackType::Subtitle)
                && track
                    .box_type()
                    .is_ok_and(|box_type| box_type.value == *b"tx3g")
        })
        .map(|(track_id, _)| *track_id)
        .collect::<Vec<_>>();
    tracks.sort_unstable();
    let track_id = container_track_id
        .and_then(|id| u32::try_from(id).ok())
        .filter(|id| tracks.contains(id))
        .or_else(|| (tracks.len() == 1).then(|| tracks[0]))
        .ok_or_else(|| "Could not match this subtitle to a mov_text track in the MP4".to_owned())?;
    let timescale = reader
        .tracks()
        .get(&track_id)
        .map(|track| track.timescale())
        .filter(|timescale| *timescale > 0)
        .ok_or_else(|| "The MP4 subtitle track has an invalid time scale".to_owned())?;
    let sample_count = reader
        .sample_count(track_id)
        .map_err(|_| "The MP4 subtitle sample table could not be read".to_owned())?;
    if sample_count > 20_000 {
        return Err("Subtitle track contains too many cues".to_owned());
    }
    let mut cues = Vec::new();
    for sample_id in 1..=sample_count {
        let Some(sample) = reader
            .read_sample(track_id, sample_id)
            .map_err(|_| "The MP4 subtitle sample could not be read".to_owned())?
        else {
            continue;
        };
        let Some(text) = mov_text_sample_text(&sample.bytes)? else {
            continue;
        };
        let start_ms = sample.start_time.saturating_mul(1_000) / u64::from(timescale);
        let duration_ms = u64::from(sample.duration).saturating_mul(1_000) / u64::from(timescale);
        cues.push((start_ms, start_ms.saturating_add(duration_ms.max(1)), text));
    }
    if cues.is_empty() {
        return Err("The selected embedded subtitle track has no readable text cues".to_owned());
    }
    let mut output = String::new();
    for (index, (start, end, text)) in cues.iter().enumerate() {
        output.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            index + 1,
            srt_time(*start),
            srt_time(*end),
            text
        ));
    }
    Ok(output)
}

fn mov_text_sample_text(sample: &[u8]) -> Result<Option<String>, String> {
    let length = sample
        .get(..2)
        .map(|header| u16::from_be_bytes([header[0], header[1]]) as usize)
        .ok_or_else(|| "The MP4 subtitle sample is truncated".to_owned())?;
    let text = sample
        .get(2..2 + length)
        .ok_or_else(|| "The MP4 subtitle sample has an invalid text length".to_owned())?;
    if text.is_empty() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(text).into_owned()))
}

fn is_mov_text_codec(format: &str) -> bool {
    let format = format.to_ascii_lowercase();
    ["mov_text", "tx3g", "timed text"]
        .iter()
        .any(|codec| format.contains(codec))
}

fn is_translatable_text_codec(format: &str) -> bool {
    let format = format.to_ascii_lowercase();
    is_mov_text_codec(&format)
        || ["utf-8", "utf8", "ass", "ssa", "subrip", "webvtt"]
            .iter()
            .any(|codec| format.contains(codec))
}

#[cfg(test)]
mod mov_text_tests {
    use super::{is_translatable_text_codec, mov_text_sample_text};

    #[test]
    fn reads_mov_text_sample_and_ignores_trailing_style_data() {
        assert_eq!(
            mov_text_sample_text(&[0, 5, b'H', b'e', b'l', b'l', b'o', 0, 1, 0, 0]),
            Ok(Some("Hello".to_owned()))
        );
    }

    #[test]
    fn handles_empty_and_malformed_mov_text_samples() {
        assert_eq!(mov_text_sample_text(&[0, 0]), Ok(None));
        assert!(mov_text_sample_text(&[0, 4, b'x']).is_err());
        assert!(mov_text_sample_text(&[0]).is_err());
    }

    #[test]
    fn recognizes_mov_text_as_a_translatable_subtitle_format() {
        assert!(is_translatable_text_codec("mov_text"));
        assert!(is_translatable_text_codec("Timed Text"));
        assert!(is_translatable_text_codec("tx3g"));
        assert!(!is_translatable_text_codec("HDMV PGS"));
    }
}

fn srt_time(milliseconds: u64) -> String {
    let hours = milliseconds / 3_600_000;
    let minutes = (milliseconds / 60_000) % 60;
    let seconds = (milliseconds / 1_000) % 60;
    let millis = milliseconds % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

fn external_subtitle_label(video: &std::path::Path, subtitle: &std::path::Path) -> String {
    let (language, detail) = external_subtitle_titles(video, subtitle);
    format!("{language} ({detail})")
}

fn external_subtitle_titles(
    video: &std::path::Path,
    subtitle: &std::path::Path,
) -> (String, String) {
    let stem = video
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let candidate = subtitle
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let format = subtitle
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_uppercase();
    if let Some((first, second)) = external_subtitle_pair(video, subtitle) {
        let merged = subtitle
            .file_stem()
            .and_then(|value| value.to_str())
            .is_some_and(|candidate| {
                candidate
                    .to_ascii_lowercase()
                    .split(['.', '_', ' '])
                    .skip(1)
                    .any(|part| part == "merge")
            });
        return (
            format!("{} + {}", plex_language(&first).1, plex_language(&second).1),
            if merged {
                format!("Bilingual {format} External · Merged")
            } else {
                format!("Bilingual {format} External")
            },
        );
    }
    let (_, language) = plex_language(&external_subtitle_language_token(stem, candidate));
    if language == "Unknown" {
        (format, "External".to_owned())
    } else {
        (language.to_owned(), format!("{format} External"))
    }
}

fn external_subtitle_language(
    video: &std::path::Path,
    subtitle: &std::path::Path,
) -> (&'static str, &'static str) {
    if let Some((first, _)) = external_subtitle_pair(video, subtitle) {
        return plex_language(&first);
    }
    let stem = video
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let candidate = subtitle
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    plex_language(&external_subtitle_language_token(stem, candidate))
}

pub(crate) fn external_subtitle_pair(
    video: &std::path::Path,
    subtitle: &std::path::Path,
) -> Option<(String, String)> {
    let stem = video.file_stem()?.to_str()?;
    let candidate = subtitle.file_stem()?.to_str()?;
    let suffix = candidate
        .get(stem.len()..)
        .filter(|_| {
            candidate
                .get(..stem.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(stem))
        })?
        .trim_start_matches(['.', '_']);
    let pair = suffix.split(['.', '_', ' ']).next()?;
    let mut languages = pair.split('-');
    let first = languages.next()?.to_ascii_lowercase();
    let second = languages.next()?.to_ascii_lowercase();
    if languages.next().is_some()
        || first.len() != 2
        || second.len() != 2
        || isolang::Language::from_639_1(&first).is_none()
        || isolang::Language::from_639_1(&second).is_none()
        || first == second
    {
        return None;
    }
    Some((first, second))
}

fn bilingual_subtitle_path(media: &Media, first: &str, second: &str, extension: &str) -> PathBuf {
    let stem = media
        .path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("subtitle");
    media
        .path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join(format!(
            "{stem}.{}-{}.{}",
            first.to_ascii_lowercase(),
            second.to_ascii_lowercase(),
            extension.to_ascii_lowercase()
        ))
}

pub(crate) fn configured_bilingual_subtitle_path(
    state: &AppState,
    media: &Media,
    first: &str,
    second: &str,
    extension: &str,
) -> PathBuf {
    let config_mode = state
        .config
        .lock()
        .is_ok_and(|config| config.metadata_storage == "config");
    if config_mode {
        return state.subtitle_checkpoints_dir.join(format!(
            "{}.{}-{}.{}",
            media.id,
            first.to_ascii_lowercase(),
            second.to_ascii_lowercase(),
            extension.to_ascii_lowercase()
        ));
    }
    bilingual_subtitle_path(media, first, second, extension)
}

fn merged_bilingual_subtitle_path(
    media: &Media,
    first: &str,
    second: &str,
    extension: &str,
) -> PathBuf {
    let path = bilingual_subtitle_path(media, first, second, extension);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("subtitle");
    path.with_file_name(format!("{stem}.merge.{extension}"))
}

async fn ensure_merged_bilingual_sidecar(
    media: &Media,
    first_language: &str,
    second_language: &str,
) -> Result<Option<PathBuf>, String> {
    let files = external_subtitle_files(&media.path).await;
    let source_for = |language: &str| {
        files.iter().find(|path| {
            external_subtitle_pair(&media.path, path).is_none()
                && external_subtitle_language(&media.path, path).0 == language
        })
    };
    let (Some(first_path), Some(second_path)) =
        (source_for(first_language), source_for(second_language))
    else {
        return Ok(None);
    };
    let first_extension = first_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("srt")
        .to_ascii_lowercase();
    let second_extension = second_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("srt")
        .to_ascii_lowercase();
    let output_extension = if first_extension == second_extension {
        first_extension.as_str()
    } else {
        "srt"
    };
    let output_path =
        merged_bilingual_subtitle_path(media, first_language, second_language, output_extension);
    if tokio::fs::metadata(&output_path).await.is_ok() {
        return Ok(Some(output_path));
    }
    let first_contents = tokio::fs::read_to_string(first_path)
        .await
        .map_err(|_| "Could not read the first subtitle track for merging".to_owned())?;
    let second_contents = tokio::fs::read_to_string(second_path)
        .await
        .map_err(|_| "Could not read the second subtitle track for merging".to_owned())?;
    let first_cues =
        subtitles::parse_subtitles(&subtitle_to_vtt(&first_contents, &first_extension))?;
    let second_cues =
        subtitles::parse_subtitles(&subtitle_to_vtt(&second_contents, &second_extension))?;
    let merged = merge_subtitle_cues(&first_cues, &second_cues)?;
    save_bilingual_sidecar(&output_path, &merged, output_extension).await?;
    Ok(Some(output_path))
}

fn merge_subtitle_cues(
    first: &[subtitles::Cue],
    second: &[subtitles::Cue],
) -> Result<String, String> {
    let first_times = first
        .iter()
        .map(|cue| Some((subtitle_time_ms(&cue.start)?, subtitle_time_ms(&cue.end)?)))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "Subtitle merge found an invalid timestamp".to_owned())?;
    let second_times = second
        .iter()
        .map(|cue| Some((subtitle_time_ms(&cue.start)?, subtitle_time_ms(&cue.end)?)))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "Subtitle merge found an invalid timestamp".to_owned())?;
    let mut used_second = vec![false; second.len()];
    let mut merged = Vec::new();
    for (index, cue) in first.iter().enumerate() {
        let (start, end) = first_times[index];
        let candidate = second
            .iter()
            .enumerate()
            .filter(|(index, _)| !used_second[*index])
            .filter_map(|(index, candidate)| {
                let (other_start, other_end) = second_times[index];
                let close = (start - other_start).abs() <= 1_500
                    && start <= other_end
                    && other_start <= end;
                close.then_some((
                    index,
                    candidate,
                    other_start,
                    other_end,
                    (start - other_start).abs(),
                ))
            })
            .min_by_key(|(_, _, _, _, distance)| *distance);
        let (start_text, end_text, text) =
            if let Some((index, other, other_start, other_end, _)) = candidate {
                used_second[index] = true;
                let merged_start = if start <= other_start {
                    &cue.start
                } else {
                    &other.start
                };
                let merged_end = if end >= other_end {
                    &cue.end
                } else {
                    &other.end
                };
                (
                    merged_start.clone(),
                    merged_end.clone(),
                    format!("{}\n{}", cue.text, other.text),
                )
            } else {
                (cue.start.clone(), cue.end.clone(), cue.text.clone())
            };
        merged.push(subtitles::Cue {
            start: start_text,
            end: end_text,
            text,
        });
    }
    for (index, cue) in second.iter().enumerate() {
        if !used_second[index] {
            merged.push(cue.clone());
        }
    }
    merged.sort_by_key(|cue| subtitle_time_ms(&cue.start).unwrap_or_default());
    let mut vtt = String::from("WEBVTT\n\n");
    for cue in merged {
        vtt.push_str(&format!(
            "{} --> {}\n{}\n\n",
            cue.start.replace(',', "."),
            cue.end.replace(',', "."),
            cue.text
        ));
    }
    Ok(vtt)
}

fn subtitle_time_ms(value: &str) -> Option<i64> {
    let value = value.split_whitespace().next()?;
    let parts = value.split(':').collect::<Vec<_>>();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (0i64, minutes.parse::<i64>().ok()?, *seconds),
        [hours, minutes, seconds] => (
            hours.parse::<i64>().ok()?,
            minutes.parse::<i64>().ok()?,
            *seconds,
        ),
        _ => return None,
    };
    let (seconds, millis) = seconds.replace(',', ".").split_once('.').map_or_else(
        || Some((seconds.parse::<i64>().ok()?, 0i64)),
        |(seconds, millis)| {
            Some((
                seconds.parse::<i64>().ok()?,
                millis
                    .chars()
                    .take(3)
                    .collect::<String>()
                    .parse::<i64>()
                    .ok()?
                    * 10i64.pow(3u32.saturating_sub(millis.len().min(3) as u32)),
            ))
        },
    )?;
    Some(((hours * 60 + minutes) * 60 + seconds) * 1000 + millis)
}

fn subtitle_to_vtt(contents: &str, extension: &str) -> String {
    match extension {
        "srt" => srt_to_vtt(contents),
        "ass" => ass_to_vtt(contents),
        _ => contents.to_owned(),
    }
}

fn vtt_to_ass(contents: &str) -> String {
    let mut output = String::from(
        "[Script Info]\nScriptType: v4.00+\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,0,0,1,2,0,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
    );
    for block in contents.replace("\r\n", "\n").split("\n\n") {
        let mut lines = block.lines();
        let first = lines.next().unwrap_or_default();
        if first.trim_start().starts_with("WEBVTT") || !first.contains(" --> ") {
            continue;
        }
        let mut timing = first.split(" --> ");
        let Some(start) = timing.next() else { continue };
        let Some(end) = timing.next() else { continue };
        let text = lines.collect::<Vec<_>>().join("\\N");
        if text.is_empty() {
            continue;
        }
        output.push_str(&format!(
            "Dialogue: 0,{},{},Default,,0,0,0,,{}\n",
            vtt_time_to_ass(start),
            vtt_time_to_ass(end),
            text
        ));
    }
    output
}

fn vtt_time_to_ass(value: &str) -> String {
    let value = value.split_whitespace().next().unwrap_or(value);
    let mut parts = value.split(':');
    let hours = parts
        .next()
        .unwrap_or("0")
        .parse::<u32>()
        .unwrap_or_default();
    let minutes = parts
        .next()
        .unwrap_or("0")
        .parse::<u32>()
        .unwrap_or_default();
    let seconds = parts
        .next()
        .unwrap_or("0")
        .parse::<f64>()
        .unwrap_or_default();
    format!("{}:{:02}:{:05.2}", hours, minutes, seconds)
}

fn external_subtitle_language_token<'a>(stem: &str, candidate: &'a str) -> String {
    let suffix = candidate
        .get(stem.len()..)
        .filter(|_| {
            candidate
                .get(..stem.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(stem))
        })
        .unwrap_or_default()
        .trim_start_matches(['.', '_']);
    suffix
        .split(['.', '_', ' '])
        .find(|token| {
            !token.is_empty()
                && !matches!(
                    token.to_ascii_lowercase().as_str(),
                    "forced" | "default" | "sdh" | "hi"
                )
        })
        .unwrap_or_default()
        .to_owned()
}

fn is_translatable_embedded_track(track: &serde_json::Value) -> bool {
    technical_text(track, "@type") == Some("Text")
        && technical_text(track, "Format")
            .or_else(|| technical_text(track, "CodecID"))
            .is_some_and(is_translatable_text_codec)
}

fn supports_embedded_subtitle_extraction(media: &Media) -> bool {
    matches!(
        media.container.to_ascii_lowercase().as_str(),
        "mkv" | "matroska" | "webm" | "mp4" | "m4v" | "mov"
    )
}

pub(crate) async fn web_audio_tracks(state: &AppState, media: &Media) -> Vec<AudioTrack> {
    let Some(info) = media_info_for(state, media).await else {
        return Vec::new();
    };
    technical_tracks(&info)
        .iter()
        .filter(|track| technical_text(track, "@type") == Some("Audio"))
        .enumerate()
        .filter_map(|(index, track)| {
            let stream_id = technical_track_id(track).saturating_add(1);
            (stream_id <= (1_u64 << 53) - 1).then(|| AudioTrack {
                label: plex_track_label(track, 2),
                language: plex_track_language(track).0.to_owned(),
                stream_id,
                index,
            })
        })
        .collect()
}

async fn subtitle_tracks(
    state: &AppState,
    media: &Media,
    relative: &std::path::Path,
    library: Option<usize>,
    folder: Option<usize>,
) -> Vec<SubtitleTrack> {
    let started = Instant::now();
    let mut tracks = Vec::new();
    let library_query = format!(
        "{}{}",
        library
            .map(|index| format!("library={index}&"))
            .unwrap_or_default(),
        folder
            .map(|index| format!("folder={index}&"))
            .unwrap_or_default()
    );
    let (bilingual_enabled, subtitle_primary, subtitle_secondary) =
        configured_bilingual_languages(state);
    let languages = bilingual_enabled.then_some((subtitle_primary, subtitle_secondary));
    let bilingual_enabled = languages.is_some();
    if let Some((first, second)) = languages.as_ref() {
        if state
            .config
            .lock()
            .is_ok_and(|config| config.metadata_storage == "media")
        {
            let _ = ensure_merged_bilingual_sidecar(media, first, second).await;
        }
    }
    for path in external_subtitle_files(&media.path).await {
        if !bilingual_enabled && external_subtitle_pair(&media.path, &path).is_some() {
            continue;
        }
        tracks.push(SubtitleTrack {
            label: format!("External: {}", external_subtitle_label(&media.path, &path)),
            url: format!(
                "/web/subtitle?{}path={}",
                library_query,
                url_encode(
                    &relative
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .join(path.file_name().unwrap_or_default())
                        .to_string_lossy()
                )
            ),
            stream_id: plex_external_subtitle_id(media, &path),
            language: Some(external_subtitle_language(&media.path, &path).0.to_owned()),
            bilingual: external_subtitle_pair(&media.path, &path).is_some(),
            pending_bilingual: false,
            generating: false,
        });
    }
    if let Some((first, second)) = languages.as_ref() {
        let stream_id = plex_translated_subtitle_id(media, first, second);
        if !tracks.iter().any(|track| track.stream_id == stream_id) {
            let saved = ["srt", "vtt", "ass"].into_iter().any(|extension| {
                configured_bilingual_subtitle_path(state, media, first, second, extension).is_file()
            });
            let checkpoint = bilingual_checkpoint_path(state, media, first, second).is_file();
            let selected_checkpoint = stored_plex_stream_selection(state, media)
                .and_then(|selection| selection.subtitle_stream_id)
                == Some(stream_id);
            let language_pair = format!("{} + {}", plex_language(first).1, plex_language(second).1);
            if saved {
                tracks.push(SubtitleTrack {
                    label: format!("Bilingual: {language_pair}"),
                    url: format!(
                        "/web/subtitle?{}path={}&bilingual=1",
                        library_query,
                        url_encode(&relative.to_string_lossy())
                    ),
                    stream_id,
                    language: Some(first.clone()),
                    bilingual: true,
                    pending_bilingual: false,
                    generating: false,
                });
            } else if checkpoint && selected_checkpoint {
                tracks.push(SubtitleTrack {
                    label: format!("{language_pair} (generating…)"),
                    url: format!(
                        "/web/subtitle?{}path={}&bilingual=1",
                        library_query,
                        url_encode(&relative.to_string_lossy())
                    ),
                    stream_id,
                    language: Some(first.clone()),
                    bilingual: true,
                    pending_bilingual: false,
                    generating: true,
                });
            } else {
                tracks.push(SubtitleTrack {
                    label: format!("{language_pair} (generate)"),
                    url: format!(
                        "/web/subtitle?{}path={}&bilingual=1",
                        library_query,
                        url_encode(&relative.to_string_lossy())
                    ),
                    stream_id,
                    language: None,
                    bilingual: false,
                    pending_bilingual: true,
                    generating: false,
                });
            }
        }
    }
    let info = media_info_for(state, media).await;
    if supports_embedded_subtitle_extraction(media) {
        if let Some(info) = info {
            for track in technical_tracks(&info)
                .into_iter()
                .filter(|track| is_translatable_embedded_track(track))
            {
                let stream_id = technical_track_id(track).saturating_add(1);
                if stream_id > (1_u64 << 53) - 1 {
                    continue;
                }
                let label = plex_track_label(track, 3);
                tracks.push(SubtitleTrack {
                    label: format!("Embedded: {label}"),
                    url: format!(
                        "/web/subtitle?{}path={}&stream={stream_id}",
                        library_query,
                        url_encode(&relative.to_string_lossy())
                    ),
                    stream_id,
                    language: Some(plex_track_language(track).0.to_owned()),
                    bilingual: false,
                    pending_bilingual: false,
                    generating: false,
                });
            }
        }
    }
    tracks.sort_by_key(|track| track.label.to_ascii_lowercase());
    let elapsed_ms = started.elapsed().as_millis();
    if elapsed_ms > 250 {
        info!(
            path = %media.path.display(),
            elapsed_ms,
            tracks = tracks.len(),
            "subtitle tracks built"
        );
    }
    tracks
}

fn matched_tv_show_metadata(
    state: &AppState,
    media_index: &[Media],
    show_directory: &std::path::Path,
) -> Option<serde_json::Value> {
    media_index
        .iter()
        .filter(|media| media.path.starts_with(show_directory))
        .find_map(|media| {
            let (library_id, library) = media_library(state, media)?;
            let root = library
                .folders
                .iter()
                .find(|folder| media.path.starts_with(folder))?;
            let media_key = database_media_key(library_id, &media.id);
            let group_path = metadata_group_path(root, &media.path, &library.kind);
            read_metadata_sidecar(state, &media_key, &group_path)
        })
}

fn srt_to_vtt(contents: &str) -> String {
    let mut output = String::from("WEBVTT\n\n");
    for block in contents.replace("\r\n", "\n").split("\n\n") {
        let mut lines = block.lines();
        let first = lines.next().unwrap_or_default();
        let timing = if first.contains(" --> ") {
            first
        } else {
            lines.next().unwrap_or_default()
        };
        if !timing.contains(" --> ") {
            continue;
        }
        output.push_str(&timing.replace(',', "."));
        output.push('\n');
        for line in lines {
            output.push_str(line);
            output.push('\n');
        }
        output.push('\n');
    }
    output
}

fn ass_to_vtt(contents: &str) -> String {
    let mut output = String::from("WEBVTT\n\n");
    for line in contents
        .lines()
        .filter(|line| line.starts_with("Dialogue:"))
    {
        let fields = line
            .strip_prefix("Dialogue:")
            .unwrap_or_default()
            .trim()
            .splitn(10, ',')
            .collect::<Vec<_>>();
        if fields.len() < 10 {
            continue;
        }
        let start = ass_timestamp(fields[1]);
        let end = ass_timestamp(fields[2]);
        let text = fields[9].replace("\\N", "\n").replace("\\n", "\n");
        let text = strip_ass_tags(&text);
        output.push_str(&format!("{} --> {}\n{}\n\n", start, end, text));
    }
    output
}

fn ass_timestamp(value: &str) -> String {
    let parts = value.trim().split(':').collect::<Vec<_>>();
    if parts.len() != 3 {
        return "00:00:00.000".to_owned();
    }
    let seconds = parts[2].parse::<f32>().unwrap_or_default();
    format!(
        "{:02}:{:02}:{:06.3}",
        parts[0].parse::<u32>().unwrap_or_default(),
        parts[1].parse::<u32>().unwrap_or_default(),
        seconds
    )
}

fn strip_ass_tags(value: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '{' => in_tag = true,
            '}' => in_tag = false,
            _ if !in_tag => output.push(character),
            _ => {}
        }
    }
    output
}

pub(crate) fn tv_episode_display_name(name: &str) -> String {
    let stem = std::path::Path::new(name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(name);
    let Some((_, (episode, episode_end), marker_end)) = tv_episode_marker(stem) else {
        return name.to_owned();
    };
    let title = clean_episode_title(&stem[marker_end..]);
    let episode_label = episode_end
        .map(|end| format!("Episodes {episode}–{end}"))
        .unwrap_or_else(|| format!("Episode {episode}"));
    title
        .map(|title| format!("{episode_label} · {title}"))
        .unwrap_or(episode_label)
}

fn media_cover_card(
    media_id: Option<&str>,
    href: &str,
    title: &str,
    subtitle: Option<&str>,
    details: Option<&str>,
    progress: f64,
    unwatched: Option<usize>,
    media_path: &std::path::Path,
    library: usize,
    tv_card: bool,
    allow_metadata_actions: bool,
    watch_scope: &str,
    show_remove_from_continue: bool,
) -> String {
    media_card_with_options(
        media_id,
        href,
        title,
        subtitle,
        details,
        progress,
        unwatched,
        media_path,
        library,
        tv_card,
        allow_metadata_actions,
        watch_scope,
        show_remove_from_continue,
        None,
        None,
    )
}

pub(crate) fn media_card_with_options(
    media_id: Option<&str>,
    href: &str,
    title: &str,
    subtitle: Option<&str>,
    details: Option<&str>,
    progress: f64,
    unwatched: Option<usize>,
    media_path: &std::path::Path,
    library: usize,
    tv_card: bool,
    allow_metadata_actions: bool,
    watch_scope: &str,
    show_remove_from_continue: bool,
    cover_url: Option<&str>,
    watch_key: Option<&str>,
) -> String {
    let subtitle = subtitle
        .map(|value| format!("<span class=\"media-card-subtitle\">{}</span>", html(value)))
        .unwrap_or_default();
    let details = details
        .map(|value| {
            format!(
                "<span class=\"media-card-details\" title=\"{}\">{}</span>",
                html(value),
                html(value)
            )
        })
        .unwrap_or_default();
    let episode_card = href.contains("&episode=");
    let cover = if let Some(url) = cover_url {
        format!(
            r#"<img src="{}" alt="Cover for {}" loading="lazy">"#,
            html(url),
            html(title)
        )
    } else if let Some(id) = media_id {
        format!(
            r#"<img src="{}" alt="Cover for {}" loading="lazy">"#,
            html(&if episode_card {
                format!("/web/episode-artwork/{}", url_encode(id))
            } else {
                media_cover_url(id)
            }),
            html(title)
        )
    } else {
        format!(
            r#"<div class="media-cover-placeholder" role="img" aria-label="Cover unavailable for {}">No cover available</div>"#,
            html(title)
        )
    };
    let path = html(&media_path.to_string_lossy());
    let folder_index = href
        .split('&')
        .find_map(|part| part.strip_prefix("folder="))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();
    let watch_key = watch_key.map(html).unwrap_or_else(|| {
        if watch_scope == "show" {
            media_path
                .components()
                .next()
                .map(|component| html(&component.as_os_str().to_string_lossy()))
                .unwrap_or_else(|| path.clone())
        } else {
            path.clone()
        }
    });
    let book_card = watch_scope == "audiobook";
    let card_class = if episode_card {
        "media-card media-card-tv media-card-episode"
    } else if tv_card {
        "media-card media-card-tv"
    } else if book_card {
        "media-card media-card-book"
    } else {
        "media-card"
    };
    let unwatched_badge = unwatched
        .map(|count| {
            let (label, aria) = if count == 0 {
                (
                    "✓".to_owned(),
                    if book_card {
                        "Read".to_owned()
                    } else {
                        "No unwatched episodes".to_owned()
                    },
                )
            } else {
                (
                    count.to_string(),
                    format!(
                        "{count} unwatched episode{}",
                        if count == 1 { "" } else { "s" }
                    ),
                )
            };
            format!(
                r#"<span class="media-card-unwatched" aria-label="{}" style="position:absolute;right:0;top:0;z-index:2;min-width:1.5rem;padding:.2rem .4rem;border-radius:0 0 0 .35rem;color:#fff;background:rgba(17,24,39,.8);font-size:.85rem;font-weight:700;line-height:1;text-align:center">{}</span>"#,
                html(&aria),
                html(&label)
            )
        })
        .unwrap_or_default();
    let card_style = "align-self:start;background:rgba(31,41,55,.3);padding:.7rem";
    let card_info_style = " style=\"box-sizing:border-box;height:4rem;padding-top:0;gap:0;grid-template-rows:1rem 1rem 1rem 1rem\"";
    let title_style = " style=\"height:auto;margin:0;white-space:nowrap;text-overflow:ellipsis\"";
    let remove_action = format!(
        r#"<button type="button" data-watch-action="remove_from_continue" data-path="{path}" data-library="{library}" data-watch-scope="{watch_scope}" data-watch-key="{watch_key}">Remove from Continue Watching</button>"#
    );
    let fix_match_action = if allow_metadata_actions {
        let action_path = if tv_card || watch_scope == "show" {
            media_path
                .components()
                .next()
                .map(|component| PathBuf::from(component.as_os_str()))
                .unwrap_or_default()
        } else {
            media_path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| media_path.to_path_buf())
        };
        let action_path = html(&action_path.to_string_lossy());
        format!(
            "<button type=\"button\" data-refresh-metadata data-library=\"{library}\" data-path=\"{action_path}\">Refresh metadata</button><button type=\"button\" data-fix-match data-library=\"{library}\" data-path=\"{action_path}\">Fix Match</button>"
        )
    } else {
        String::new()
    };
    let (watched_label, unwatched_label) = if book_card {
        ("Mark as read", "Mark as unread")
    } else {
        ("Mark as watched", "Mark as unwatched")
    };
    let set_cover_action = if book_card {
        r#"<button type="button" data-set-cover>Set cover image</button><button type="button" data-book-match>Match audiobook</button>"#
    } else {
        ""
    };
    let menu = format!(
        r#"<details class="media-card-menu"><summary aria-label="More options" title="More options">⋮</summary><div class="media-card-menu-items"><button type="button" data-watch-action="watched" data-path="{path}" data-library="{library}" data-watch-scope="{watch_scope}" data-watch-key="{watch_key}">{watched_label}</button><button type="button" data-watch-action="unwatched" data-path="{path}" data-library="{library}" data-watch-scope="{watch_scope}" data-watch-key="{watch_key}">{unwatched_label}</button>{set_cover_action}{remove_action}{fix_match_action}</div></details>"#
    );
    let card = format!(
        r#"<div class="{}" style="{}" data-media-path="{}" data-library="{}" data-folder="{}" data-watch-scope="{}" data-watch-key="{}"><div class="media-card-artwork"><a class="media-card-cover" data-spa-link href="{}">{}{}</a>{}</div><a class="media-card-info" data-spa-link href="{}"{}><span class="media-card-title" title="{}"{}>{}</span>{}{}{}</a></div>"#,
        card_class,
        card_style,
        path,
        library,
        folder_index,
        watch_scope,
        watch_key,
        html(href),
        cover,
        unwatched_badge,
        menu,
        html(href),
        card_info_style,
        html(title),
        title_style,
        html(title),
        subtitle,
        details,
        media_progress_markup(progress),
    );
    if show_remove_from_continue {
        card
    } else {
        card.replace(&remove_action, "")
    }
}

pub(crate) fn url_encode(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (*byte as char).to_string()
            }
            _ => format!("%{:02X}", byte),
        })
        .collect()
}

pub(crate) fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

async fn log_not_found(State(state): State<AppState>, req: Request<Body>) -> Response {
    if let Some(resource) = req
        .uri()
        .path()
        .strip_prefix("/video/:/transcode/universal/session/")
    {
        return plex_remux_resource(
            &state,
            resource,
            req.method().clone(),
            req.headers().clone(),
        )
        .await;
    }

    if req.uri().path() == "/video/:/transcode/universal/decision" {
        let query = req
            .uri()
            .query()
            .and_then(|query| serde_urlencoded::from_str::<BTreeMap<String, String>>(query).ok());
        let Some(query) = query else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        info!(
            protocol = ?query.get("protocol"),
            direct_play = ?query.get("directPlay"),
            direct_stream = ?query.get("directStream"),
            direct_stream_audio = ?query.get("directStreamAudio"),
            media_index = ?query.get("mediaIndex"),
            part_index = ?query.get("partIndex"),
            media_path = ?query.get("path"),
            plex_product = ?query.get("X-Plex-Product"),
            client_profile = ?query.get("X-Plex-Client-Profile-Name"),
            client_platform = ?query.get("X-Plex-Platform"),
            "Plex universal playback decision requested"
        );
        return plex_playback_decision(State(state), Query(query)).await;
    }
    if let Some(extension) = req
        .uri()
        .path()
        .strip_prefix("/video/:/transcode/universal/start.")
    {
        let query = req
            .uri()
            .query()
            .and_then(|query| serde_urlencoded::from_str::<BTreeMap<String, String>>(query).ok())
            .unwrap_or_default();
        info!(
            extension,
            protocol = ?query.get("protocol"),
            direct_play = ?query.get("directPlay"),
            direct_stream = ?query.get("directStream"),
            direct_stream_audio = ?query.get("directStreamAudio"),
            offset = ?query.get("offset"),
            media_path = ?query.get("path"),
            plex_product = ?query.get("X-Plex-Product"),
            client_profile = ?query.get("X-Plex-Client-Profile-Name"),
            client_platform = ?query.get("X-Plex-Platform"),
            profile_requests_hls_mkv = plex_profile_requests_hls_mkv(&query),
            "Plex universal stream start requested"
        );
        if extension.eq_ignore_ascii_case("mpd") || extension.eq_ignore_ascii_case("m3u8") {
            return plex_remux_start(&state, extension, &query).await;
        }
        let Some(media) = plex_playback_media_from_query(&state, &query) else {
            warn!(
                extension,
                reason = "media_not_found",
                "Plex passthrough stream rejected"
            );
            return StatusCode::NOT_FOUND.into_response();
        };
        return plex_protocol_response(
            stream_file(&media, req.method().clone(), req.headers().clone(), false).await,
        );
    }
    if req.uri().path() == "/photo/:/transcode" {
        let query = req
            .uri()
            .query()
            .and_then(|query| serde_urlencoded::from_str::<BTreeMap<String, String>>(query).ok());
        let Some(source) = query.and_then(|query| query.get("url").cloned()) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if let Some(user_id) = plex_user_avatar_source(&source) {
            return plex_user_avatar(&state, &user_id).await;
        }

        let Some((id, filename)) = plex_photo_source(&source) else {
            let source_path = Uri::try_from(source.as_str())
                .ok()
                .map(|uri| uri.path().to_owned())
                .unwrap_or_else(|| "<invalid-uri>".to_owned());
            warn!(
                status = StatusCode::NOT_FOUND.as_u16(),
                source_path, "unsupported Plex photo source"
            );
            let mut response = StatusCode::NOT_FOUND.into_response();
            response.extensions_mut().insert(SuppressHttpResponseLog);
            return response;
        };
        return plex_artwork(&state, &id, filename).await;
    }
    if matches!(
        req.uri().path(),
        "/:/websockets/notifications" | "/:/websocket/notifications"
    ) {
        let (mut parts, _) = req.into_parts();
        return match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
            Ok(upgrade) => plex_notification_response(upgrade, state.clone()),
            Err(rejection) => rejection.into_response(),
        };
    }
    if req.uri().path() == "/:/timeline" {
        let query = req
            .uri()
            .query()
            .and_then(|query| serde_urlencoded::from_str::<PlexTimelineQuery>(query).ok());
        let Some(query) = query else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let headers = req.headers().clone();
        let request_token = plex_request_token(&headers, req.uri()).map(str::to_owned);
        return plex_timeline(&state, query, headers, request_token);
    }
    if req.uri().path() == "/:/prefs" {
        return plex_prefs(&state);
    }
    if matches!(req.uri().path(), "/:/scrobble" | "/:/unscrobble") {
        let watched = req.uri().path() == "/:/scrobble";
        let query = req
            .uri()
            .query()
            .and_then(|query| serde_urlencoded::from_str::<PlexScrobbleQuery>(query).ok());
        let Some(query) = query else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let headers = req.headers().clone();
        let request_token = plex_request_token(&headers, req.uri()).map(str::to_owned);
        return plex_scrobble(&state, query, watched, headers, request_token);
    }
    warn!(method = %req.method(), path = %req.uri().path(), "unsupported route");
    StatusCode::NOT_FOUND.into_response()
}

pub(crate) fn prune_plex_remux_sessions(state: &AppState) {
    if let Ok(mut sessions) = state.plex_remux_sessions.lock() {
        let now = Instant::now();
        sessions.retain(|_, session| session.expires_at > now);
    }
}

pub(crate) fn spawn_plex_webhook_delivery(
    state: &AppState,
    event: &'static str,
    media_id: String,
    position: i64,
    duration: Option<i64>,
    headers: HeaderMap,
    request_token: Option<String>,
) {
    let state = state.clone();
    let player = plex_webhook_player(&headers);
    let delivery_lock = state.plex_webhook_delivery_lock.clone();

    tokio::spawn(async move {
        let _guard = delivery_lock.lock().await;
        deliver_plex_webhook(
            &state,
            event,
            &media_id,
            position,
            duration,
            player,
            request_token,
        )
        .await;
    });
}

async fn deliver_plex_webhook(
    state: &AppState,
    event: &'static str,
    media_id: &str,
    position: i64,
    duration: Option<i64>,
    player: serde_json::Value,
    request_token: Option<String>,
) {
    let Some(server_token) = plex_webhook_server_token(state) else {
        warn!(
            event,
            reason = "server_not_claimed",
            "Plex webhook delivery skipped"
        );
        return;
    };
    let Some((media, metadata)) = plex_webhook_metadata(state, media_id, position, duration).await
    else {
        warn!(
            event,
            reason = "media_not_found",
            "Plex webhook delivery skipped"
        );
        return;
    };
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    else {
        warn!(
            event,
            reason = "http_client_unavailable",
            "Plex webhook delivery skipped"
        );
        return;
    };
    let (profile, access_tokens, configuration) = tokio::join!(
        plex_webhook_account(&client, state, &server_token),
        plex_webhook_access_tokens(&client, state, &server_token),
        plex_webhook_destinations(&client, state, &server_token)
    );
    let Ok(configuration) = configuration else {
        warn!(
            event,
            reason = "destination_lookup_failed",
            "Plex webhook delivery skipped"
        );
        return;
    };
    let owner_id = configuration.owner_id;
    let mut account = access_tokens
        .as_deref()
        .map(|body| {
            plex_webhook_account_from_access_tokens(
                body,
                request_token.as_deref(),
                &server_token,
                owner_id.as_deref(),
            )
        })
        .unwrap_or_else(|| serde_json::json!({}));
    for field in ["id", "title", "username", "thumb"] {
        if account.get(field).is_none() {
            if let Some(value) = profile.get(field) {
                account[field] = value.clone();
            }
        }
    }
    if account.get("id").is_none() {
        if let Some(id) = &owner_id {
            account["id"] = id
                .parse::<u64>()
                .map(serde_json::Value::from)
                .unwrap_or_else(|_| serde_json::json!(id));
        }
    }
    let account_id = account.get("id").and_then(|id| {
        id.as_str()
            .map(str::to_owned)
            .or_else(|| id.as_u64().map(|id| id.to_string()))
    });
    let is_owner = owner_id.as_deref() == account_id.as_deref();
    let destinations =
        plex_webhook_destinations_for_account(configuration.destinations, account_id.as_deref());
    if destinations.is_empty() {
        info!(
            event,
            "no Plex webhook destinations are configured for this account"
        );
        return;
    }
    let payload = serde_json::json!({
        "event": event,
        "user": true,
        "owner": is_owner,
        "Account": account,
        "Server": {
            "title": server_name(state),
            "uuid": state.machine_identifier.as_ref()
        },
        "Player": player,
        "Metadata": metadata
    });
    let thumbnail = if event == "media.play" {
        plex_frame_cover(state, &media).await
    } else {
        None
    };
    for (index, destination) in destinations.iter().enumerate() {
        let Ok(payload_part) =
            reqwest::multipart::Part::text(payload.to_string()).mime_str("application/json")
        else {
            continue;
        };
        let mut form = reqwest::multipart::Form::new().part("payload", payload_part);
        if let Some(thumbnail) = &thumbnail {
            if let Ok(part) = reqwest::multipart::Part::bytes(thumbnail.clone())
                .file_name("thumb.jpg")
                .mime_str("image/jpeg")
            {
                form = form.part("thumb", part);
            }
        }
        match client
            .post(destination.clone())
            .multipart(form)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                info!(event, destination_index = index, "Plex webhook delivered");
            }
            Ok(response) => {
                warn!(
                    event,
                    destination_index = index,
                    status = response.status().as_u16(),
                    "Plex webhook destination rejected event"
                );
            }
            Err(error) => {
                warn!(
                    event,
                    destination_index = index,
                    timed_out = error.is_timeout(),
                    "Plex webhook destination request failed"
                );
            }
        }
    }
}

#[derive(Clone)]
struct SuppressHttpResponseLog;

#[derive(Deserialize)]
pub(crate) struct AdminLoginForm {
    pub(crate) password: String,
    #[serde(default)]
    pub(crate) confirm_password: String,
}

fn issue_web_session(state: &AppState) -> Response {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    if state.db.lock().ok().is_none_or(|db| {
        db.execute(
            "INSERT INTO web_sessions (token_hash, expires_at, user_id) VALUES (?1, ?2, 'admin')",
            (&token_hash, now + 60 * 60 * 24 * 14),
        )
        .is_err()
    }) {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let mut response = Redirect::to("/").into_response();
    if let Ok(value) = HeaderValue::from_str(&format!(
        "harbinger_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=1209600"
    )) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

fn refresh_web_session_cookie(headers: &HeaderMap, response: &mut Response) {
    let Some(token) = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie| {
            cookie
                .split(';')
                .find_map(|part| part.trim().strip_prefix("harbinger_session="))
        })
    else {
        return;
    };
    if let Ok(value) = HeaderValue::from_str(&format!(
        "harbinger_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=1209600"
    )) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
}

pub(crate) fn media_modified_at(media: &Media) -> u64 {
    std::fs::metadata(&media.path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

pub(crate) fn query_bool(query: &BTreeMap<String, String>, key: &str) -> bool {
    matches!(
        query.get(key).map(String::as_str),
        Some("1") | Some("true") | Some("TRUE")
    )
}

pub(crate) fn query_i64(query: &BTreeMap<String, String>, key: &str) -> Option<i64> {
    query.get(key).and_then(|value| value.trim().parse().ok())
}

pub(crate) fn marker_include_value(value: Option<&str>) -> bool {
    matches!(value, Some("1") | Some("true") | Some("TRUE"))
}

pub(crate) fn marker_include_requested(uri: &Uri) -> bool {
    marker_include_value(plex_query_value(uri, "includeMarkers"))
}

pub(crate) fn attach_markers_to_item(
    state: &AppState,
    item: &mut serde_json::Value,
    media_key: &str,
    include_markers: bool,
) {
    if !include_markers {
        return;
    }
    let Ok(markers) = state
        .db
        .lock()
        .map_err(|_| ())
        .and_then(|db| markers::markers_for_media(&db, media_key).map_err(|_| ()))
    else {
        return;
    };
    if !markers.is_empty() {
        item["Marker"] = serde_json::json!(markers);
    }
}

pub(crate) fn marker_response(markers: Vec<markers::SkipMarker>) -> Response {
    plex_json(serde_json::json!({
        "MediaContainer": {
            "size": markers.len(),
            "Marker": markers,
        }
    }))
}

pub(crate) async fn media_duration_ms(media: &Media) -> Option<i64> {
    inspect_media(&media.path)
        .await
        .and_then(|info| technical_duration_ms(&info))
}

fn portable_relative_path(path: &std::path::Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn media_fingerprint(root: &std::path::Path, media: &Media) -> String {
    let relative = media.path.strip_prefix(root).unwrap_or(&media.path);
    let mut hasher = Sha256::new();
    hasher.update(portable_relative_path(relative).as_bytes());
    hasher.update(media.size.to_le_bytes());
    format!("{:x}", hasher.finalize())
}

fn group_fingerprint(root: &std::path::Path, episodes: &[Media]) -> String {
    let mut fingerprints = episodes
        .iter()
        .map(|episode| media_fingerprint(root, episode))
        .collect::<Vec<_>>();
    fingerprints.sort();
    let mut hasher = Sha256::new();
    for fingerprint in fingerprints {
        hasher.update(fingerprint.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn marker_sidecar_target(
    library: &ConfigLibrary,
    root: &std::path::Path,
    media: &Media,
) -> Option<(PathBuf, String)> {
    if !media.path.starts_with(root) {
        return None;
    }
    let group_path = metadata_group_path(&root.to_path_buf(), &media.path, &library.kind);
    let directory = if group_path.is_file() {
        group_path.parent().unwrap_or(group_path.as_path())
    } else {
        group_path.as_path()
    };
    let key = portable_relative_path(media.path.strip_prefix(directory).ok()?);
    (!key.is_empty()).then(|| (directory.join(".hbgr").join("skip_markers.json"), key))
}

fn config_hbgr_dir(state: &AppState) -> PathBuf {
    state
        .subtitle_checkpoints_dir
        .parent()
        .unwrap_or(state.subtitle_checkpoints_dir.as_path())
        .join(".hbgr")
}

fn config_marker_sidecar_path(state: &AppState) -> PathBuf {
    config_hbgr_dir(state).join("skip_markers.json")
}

fn config_marker_sidecar_key(
    library_key: usize,
    root: &std::path::Path,
    media: &Media,
) -> Option<String> {
    let relative = portable_relative_path(media.path.strip_prefix(root).ok()?);
    (!relative.is_empty()).then(|| format!("{library_key}/{relative}"))
}

struct MarkerSidecarLocations {
    path: PathBuf,
    key: String,
    fallback: Option<(PathBuf, String)>,
}

fn marker_sidecar_locations(
    state: &AppState,
    library_key: usize,
    library: &ConfigLibrary,
    root: &std::path::Path,
    media: &Media,
) -> Option<MarkerSidecarLocations> {
    let media_target = marker_sidecar_target(library, root, media);
    let config_target = || {
        Some((
            config_marker_sidecar_path(state),
            config_marker_sidecar_key(library_key, root, media)?,
        ))
    };
    if state
        .config
        .lock()
        .is_ok_and(|config| config.metadata_storage == "config")
    {
        let (path, key) = config_target()?;
        Some(MarkerSidecarLocations {
            path,
            key,
            fallback: media_target,
        })
    } else {
        let (path, key) = media_target?;
        Some(MarkerSidecarLocations {
            path,
            key,
            fallback: config_target(),
        })
    }
}

#[derive(Hash, Eq, PartialEq)]
struct SeasonFingerprintKey {
    library: usize,
    root: PathBuf,
    show_title: String,
    season: u32,
}

type SeasonFingerprintCache = HashMap<SeasonFingerprintKey, HashMap<String, (String, String)>>;

fn season_fingerprints(
    state: &AppState,
    library: &ConfigLibrary,
    root: &std::path::Path,
    show_title: &str,
    season: u32,
) -> HashMap<String, (String, String)> {
    let groups = season_episode_candidates_by_number(
        state,
        library,
        &root.to_path_buf(),
        show_title,
        season,
    );
    if groups.len() < 2 {
        return HashMap::new();
    }
    let episodes = groups
        .values()
        .filter_map(|files| files.first().cloned())
        .collect::<Vec<_>>();
    let group_fingerprint = group_fingerprint(root, &episodes);
    groups
        .values()
        .flatten()
        .map(|episode| {
            (
                episode.id.clone(),
                (media_fingerprint(root, episode), group_fingerprint.clone()),
            )
        })
        .collect()
}

fn stable_marker_fingerprints(
    state: &AppState,
    cache: &mut SeasonFingerprintCache,
    library_key: usize,
    library: &ConfigLibrary,
    root: &std::path::Path,
    media: &Media,
) -> Option<(String, String)> {
    if library.kind != "tv" {
        return None;
    }
    let info = tv_episode_info(&root.to_path_buf(), &media.path)?;
    let key = SeasonFingerprintKey {
        library: library_key,
        root: root.to_path_buf(),
        show_title: tv_show_title(&root.to_path_buf(), &media.path),
        season: info.season,
    };
    let fingerprints = if let Some(cached) = cache.get(&key) {
        cached
    } else {
        let computed = season_fingerprints(state, library, root, &key.show_title, key.season);
        cache.entry(key).or_insert(computed)
    };
    fingerprints.get(&media.id).cloned()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SidecarStamp {
    Missing,
    Present { modified_ns: i64, len: i64 },
}

fn sidecar_stamp_unchanged(
    current: SidecarStamp,
    stored: SidecarStamp,
    missing_unchanged: bool,
) -> bool {
    match (current, stored) {
        (SidecarStamp::Present { .. }, SidecarStamp::Present { .. }) => current == stored,
        (SidecarStamp::Missing, SidecarStamp::Missing) => missing_unchanged,
        _ => false,
    }
}

fn current_sidecar_stamp(path: &std::path::Path) -> SidecarStamp {
    let Ok(metadata) = std::fs::metadata(path) else {
        return SidecarStamp::Missing;
    };
    let Ok(modified) = metadata.modified() else {
        return SidecarStamp::Missing;
    };
    let modified_ns = modified
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or_default();
    SidecarStamp::Present {
        modified_ns,
        len: metadata.len() as i64,
    }
}

fn set_stored_sidecar_stamp(db: &Connection, path: &std::path::Path, stamp: SidecarStamp) {
    let path = path.to_string_lossy();
    let result = match stamp {
        SidecarStamp::Present { modified_ns, len } => db.execute(
            "INSERT INTO marker_sidecar_state (path, modified_ns, len, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET
                 modified_ns = excluded.modified_ns,
                 len = excluded.len,
                 updated_at = excluded.updated_at",
            params![path.as_ref(), modified_ns, len, now_timestamp()],
        ),
        SidecarStamp::Missing => db.execute(
            "DELETE FROM marker_sidecar_state WHERE path = ?1",
            [path.as_ref()],
        ),
    };
    if result.is_err() {
        warn!(path = %path, "could not record marker sidecar state");
    }
}

fn cached_current_stamp(
    cache: &mut HashMap<PathBuf, SidecarStamp>,
    path: &std::path::Path,
) -> SidecarStamp {
    *cache
        .entry(path.to_path_buf())
        .or_insert_with(|| current_sidecar_stamp(path))
}

fn ensure_marker_sidecar<'a>(
    cache: &'a mut HashMap<PathBuf, markers::MarkerSidecar>,
    loaded: &mut HashSet<PathBuf>,
    path: &std::path::Path,
) -> &'a mut markers::MarkerSidecar {
    let cache_key = path.to_path_buf();
    if !loaded.contains(&cache_key) {
        let sidecar = markers::read_marker_sidecar(path).unwrap_or_default();
        cache.insert(cache_key.clone(), sidecar);
        loaded.insert(cache_key.clone());
    }
    cache.get_mut(&cache_key).expect("sidecar was just loaded")
}

pub(crate) fn sync_marker_sidecars(state: &AppState, items: &[Media]) {
    // Load the stored stamps in one query and drop the database lock before
    // touching the filesystem. Holding the lock across per-item sidecar stats
    // blocks every request that needs the connection (including admin session
    // checks) for as long as the media storage takes to answer.
    let stored_stamps = match state.db.lock() {
        Ok(db) => db
            .prepare("SELECT path, modified_ns, len FROM marker_sidecar_state")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| {
                        Ok((
                            PathBuf::from(row.get::<_, String>(0)?),
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    })
                    .map(|rows| {
                        rows.filter_map(Result::ok)
                            .map(|(path, modified_ns, len)| {
                                (path, SidecarStamp::Present { modified_ns, len })
                            })
                            .collect::<HashMap<_, _>>()
                    })
            })
            .unwrap_or_default(),
        Err(_) => return,
    };
    let mut sidecars: HashMap<PathBuf, markers::MarkerSidecar> = HashMap::new();
    let mut loaded = HashSet::new();
    let mut season_fingerprints = SeasonFingerprintCache::new();
    let mut dirty = HashSet::new();
    let mut current_stamps = HashMap::<PathBuf, SidecarStamp>::new();
    let mut prepared = Vec::new();
    let mut imported_count = 0usize;
    let mut exported_count = 0usize;

    for media in items {
        let Some((library_key, library)) = media_library(state, media) else {
            continue;
        };
        let Some(root) = library
            .folders
            .iter()
            .find(|folder| media.path.starts_with(folder))
        else {
            continue;
        };
        let Some(locations) = marker_sidecar_locations(state, library_key, &library, root, media)
        else {
            continue;
        };
        let path = locations.path.clone();
        let key = locations.key.clone();
        let fallback = locations.fallback.clone();
        let primary_changed = !sidecar_stamp_unchanged(
            cached_current_stamp(&mut current_stamps, &path),
            stored_stamps
                .get(&path)
                .copied()
                .unwrap_or(SidecarStamp::Missing),
            false,
        );
        let fallback_changed = fallback.as_ref().is_some_and(|(fallback_path, _)| {
            !sidecar_stamp_unchanged(
                cached_current_stamp(&mut current_stamps, fallback_path),
                stored_stamps
                    .get(fallback_path)
                    .copied()
                    .unwrap_or(SidecarStamp::Missing),
                true,
            )
        });
        if !primary_changed && !fallback_changed {
            continue;
        }
        ensure_marker_sidecar(&mut sidecars, &mut loaded, &path);
        if let Some((fallback_path, _)) = &fallback {
            ensure_marker_sidecar(&mut sidecars, &mut loaded, fallback_path);
        }
        let root_path = root.clone();
        prepared.push((media, library_key, library, root_path, path, key, fallback));
    }

    let Ok(db) = state.db.lock() else {
        return;
    };

    for (media, library_key, library, root, path, key, fallback) in &prepared {
        let imported = sidecars
            .get(path)
            .and_then(|sidecar| sidecar.items.get(key).cloned())
            .or_else(|| {
                fallback.as_ref().and_then(|(fallback_path, fallback_key)| {
                    sidecars
                        .get(fallback_path)
                        .and_then(|sidecar| sidecar.items.get(fallback_key).cloned())
                })
            });
        match imported {
            Some(imported) => {
                debug!(
                    path = %path.display(),
                    key,
                    markers = imported.markers.len(),
                    "importing skip markers from sidecar"
                );
                if let Err(error) =
                    markers::replace_markers_from_sidecar(&db, &media.id, &imported.markers)
                {
                    warn!(
                        path = %path.display(),
                        %error,
                        "could not import markers from sidecar"
                    );
                    continue;
                }
                imported_count += 1;
                let stable = stable_marker_fingerprints(
                    state,
                    &mut season_fingerprints,
                    *library_key,
                    library,
                    root,
                    media,
                );
                let mut updated = imported;
                let mut changed = false;
                if let Some((media_fingerprint, group_fingerprint)) = &stable {
                    for kind in [markers::MarkerKind::Intro, markers::MarkerKind::Credits] {
                        let detected = updated.fingerprints.contains_key(kind.as_str())
                            || markers::marker_fingerprints(&db, &media.id, kind)
                                .ok()
                                .flatten()
                                .is_some();
                        if !detected {
                            continue;
                        }
                        let fingerprint = markers::MarkerFingerprint {
                            media: media_fingerprint.clone(),
                            group: group_fingerprint.clone(),
                        };
                        if updated.fingerprints.get(kind.as_str()) != Some(&fingerprint) {
                            updated
                                .fingerprints
                                .insert(kind.as_str().to_owned(), fingerprint);
                            changed = true;
                        }
                        if let Err(error) = markers::update_detected_marker_fingerprints(
                            &db,
                            &media.id,
                            kind,
                            media_fingerprint,
                            group_fingerprint,
                        ) {
                            warn!(
                                path = %path.display(),
                                %error,
                                "could not update detected marker fingerprints"
                            );
                        }
                    }
                }
                if changed {
                    ensure_marker_sidecar(&mut sidecars, &mut loaded, path)
                        .items
                        .insert(key.clone(), updated);
                    dirty.insert(path.clone());
                }
            }
            None => {
                let Ok(database_markers) = markers::database_markers_for_sidecar(&db, &media.id)
                else {
                    continue;
                };
                if database_markers.is_empty() {
                    continue;
                }
                debug!(
                    path = %path.display(),
                    key,
                    markers = database_markers.len(),
                    "exporting database skip markers to sidecar"
                );
                exported_count += 1;
                let stable = stable_marker_fingerprints(
                    state,
                    &mut season_fingerprints,
                    *library_key,
                    library,
                    root,
                    media,
                );
                let mut fingerprints =
                    markers::marker_fingerprint_map(&db, &media.id).unwrap_or_default();
                if let Some((media_fingerprint, group_fingerprint)) = &stable {
                    for kind in [markers::MarkerKind::Intro, markers::MarkerKind::Credits] {
                        if markers::marker_fingerprints(&db, &media.id, kind)
                            .ok()
                            .flatten()
                            .is_none()
                        {
                            continue;
                        }
                        fingerprints.insert(
                            kind.as_str().to_owned(),
                            markers::MarkerFingerprint {
                                media: media_fingerprint.clone(),
                                group: group_fingerprint.clone(),
                            },
                        );
                        if let Err(error) = markers::update_detected_marker_fingerprints(
                            &db,
                            &media.id,
                            kind,
                            media_fingerprint,
                            group_fingerprint,
                        ) {
                            warn!(
                                path = %path.display(),
                                %error,
                                "could not update detected marker fingerprints"
                            );
                        }
                    }
                }
                ensure_marker_sidecar(&mut sidecars, &mut loaded, path)
                    .items
                    .insert(
                        key.clone(),
                        markers::MarkerSidecarEntry {
                            markers: database_markers,
                            fingerprints,
                        },
                    );
                dirty.insert(path.clone());
            }
        }
    }

    drop(db);

    let mut failed_writes = HashSet::new();
    for path in &dirty {
        if let Some(sidecar) = sidecars.get(path) {
            if let Err(error) = markers::write_marker_sidecar_snapshot(path, sidecar) {
                warn!(
                    path = %path.display(),
                    %error,
                    "could not save marker sidecar"
                );
                failed_writes.insert(path.clone());
            }
        }
    }
    let mut updated_stamps = Vec::new();
    for (path, stamp) in current_stamps {
        if failed_writes.contains(&path) {
            continue;
        }
        let stamp = if dirty.contains(&path) {
            current_sidecar_stamp(&path)
        } else {
            stamp
        };
        if stored_stamps
            .get(&path)
            .copied()
            .unwrap_or(SidecarStamp::Missing)
            == stamp
        {
            continue;
        }
        updated_stamps.push((path, stamp));
    }
    if !updated_stamps.is_empty() {
        let Ok(db) = state.db.lock() else {
            return;
        };
        for (path, stamp) in updated_stamps {
            set_stored_sidecar_stamp(&db, &path, stamp);
        }
    }
    info!(
        items = items.len(),
        changed_items = prepared.len(),
        imported = imported_count,
        exported = exported_count,
        sidecars = loaded.len(),
        "skip marker sidecar sync completed"
    );
}

pub(crate) fn export_marker_sidecar(
    state: &AppState,
    media: &Media,
    db: &Connection,
    fingerprint: Option<(markers::MarkerKind, &str, &str)>,
) -> Result<(), String> {
    let (library_key, library) = media_library(state, media)
        .ok_or_else(|| "media item is not in a configured library".to_owned())?;
    let root = library
        .folders
        .iter()
        .find(|folder| media.path.starts_with(folder))
        .ok_or_else(|| "media library folder was not found".to_owned())?;
    let locations = marker_sidecar_locations(state, library_key, &library, root, media)
        .ok_or_else(|| "marker sidecar location was not found".to_owned())?;
    let database_markers = markers::database_markers_for_sidecar(db, &media.id)
        .map_err(|error| format!("could not read markers from database: {error}"))?;
    markers::update_marker_sidecar_entry(
        &locations.path,
        &locations.key,
        Some(&database_markers),
        fingerprint,
    )
    .map_err(|error| format!("could not write marker sidecar: {error}"))
}

fn season_episode_candidates_by_number(
    state: &AppState,
    library: &ConfigLibrary,
    root: &PathBuf,
    show_title: &str,
    season: u32,
) -> BTreeMap<u32, Vec<Media>> {
    let mut episodes = BTreeMap::<u32, Vec<Media>>::new();
    for candidate in media_snapshot(state).iter() {
        let Some(candidate_root) = library
            .folders
            .iter()
            .find(|folder| candidate.path.starts_with(folder))
        else {
            continue;
        };
        if candidate_root != root || !media_in_library(candidate, library) {
            continue;
        }
        let Some(info) = tv_episode_info(candidate_root, &candidate.path) else {
            continue;
        };
        if info.season != season || tv_show_title(candidate_root, &candidate.path) != show_title {
            continue;
        }
        let in_season_directory = media_has_season_directory(candidate_root, &candidate.path);
        let files = episodes.entry(info.episode).or_default();
        let preferred_in_season_directory = files
            .first()
            .is_some_and(|existing| media_has_season_directory(candidate_root, &existing.path));
        if in_season_directory && !preferred_in_season_directory {
            files.insert(0, (*candidate).clone());
        } else {
            files.push((*candidate).clone());
        }
    }
    episodes
}

fn media_tmdb_id(state: &AppState, media: &Media) -> Option<i64> {
    let (section, library) = media_library(state, media)?;
    let root = library
        .folders
        .iter()
        .find(|folder| media.path.starts_with(folder))?;
    let media_key = database_media_key(section, &media.id);
    let group_path = metadata_group_path(root, &media.path, &library.kind);
    read_metadata_sidecar(state, &media_key, &group_path)?
        .get("id")
        .and_then(serde_json::Value::as_i64)
        .filter(|id| *id > 0)
}

fn markers_present(state: &AppState, media: &Media, kind: markers::MarkerKind) -> bool {
    if let Ok(db) = state.db.lock() {
        if markers::markers_for_media(&db, &media.id)
            .is_ok_and(|markers| markers.iter().any(|marker| marker.kind == kind))
        {
            return true;
        }
    }
    let Some((_, library)) = media_library(state, media) else {
        return false;
    };
    let Some(root) = library
        .folders
        .iter()
        .find(|folder| media.path.starts_with(folder))
    else {
        return false;
    };
    marker_sidecar_target(&library, root, media).is_some_and(|(path, key)| {
        markers::read_marker_sidecar(&path).is_some_and(|sidecar| {
            sidecar
                .items
                .get(&key)
                .is_some_and(|entry| entry.markers.iter().any(|marker| marker.kind == kind))
        })
    })
}

fn lookup_state_current(
    state: &AppState,
    media: &Media,
    kind: markers::MarkerKind,
    media_fingerprint: &str,
    group_fingerprint: &str,
) -> bool {
    let Ok(db) = state.db.lock() else {
        return false;
    };
    markers::marker_lookup_is_current(&db, &media.id, kind, media_fingerprint, group_fingerprint)
        .unwrap_or(false)
}

fn introdb_lookup_needed(
    state: &AppState,
    files: &[Media],
    fingerprints: &[String],
    group_fingerprint: &str,
    kind: markers::MarkerKind,
    force: bool,
) -> bool {
    if force {
        return true;
    }
    !files.iter().zip(fingerprints).all(|(file, fingerprint)| {
        // Existing markers, whatever their source, are authoritative for the
        // fallback: if the episode or movie already has this marker type there
        // is nothing to fetch. The local lookup state additionally remembers
        // negative results so a scan does not repeat a request that had no
        // data. Requiring only the lookup state would also repeat requests
        // solely because a fresh host has an empty SQLite database.
        markers_present(state, file, kind)
            || lookup_state_current(state, file, kind, fingerprint, group_fingerprint)
    })
}

fn movie_introdb_lookup_needed(
    state: &AppState,
    media: &Media,
    root: &std::path::Path,
    force: bool,
) -> bool {
    let fingerprint = media_fingerprint(root, media);
    let fingerprints = std::slice::from_ref(&fingerprint);
    let group = group_fingerprint(root, std::slice::from_ref(media));
    let stale = [markers::MarkerKind::Intro, markers::MarkerKind::Credits]
        .into_iter()
        .any(|kind| {
            introdb_lookup_needed(
                state,
                std::slice::from_ref(media),
                fingerprints,
                &group,
                kind,
                force,
            )
        });
    // Unmatched media has no TMDB identity to query. Skipping it here keeps it
    // from consuming the per-pass lookup budget.
    stale && media_tmdb_id(state, media).is_some()
}

fn episode_introdb_lookup_needed(
    state: &AppState,
    root: &std::path::Path,
    files: &[Media],
    force: bool,
) -> bool {
    let Some(representative) = files.first() else {
        return false;
    };
    let fingerprints = files
        .iter()
        .map(|file| media_fingerprint(root, file))
        .collect::<Vec<_>>();
    let group = group_fingerprint(root, std::slice::from_ref(representative));
    let stale = [markers::MarkerKind::Intro, markers::MarkerKind::Credits]
        .into_iter()
        .any(|kind| introdb_lookup_needed(state, files, &fingerprints, &group, kind, force));
    stale && media_tmdb_id(state, representative).is_some()
}

async fn store_introdb_segments(
    state: &AppState,
    files: &[Media],
    fingerprints: &[String],
    group_fingerprint: &str,
    segments: introdb::Segments,
    kinds: &[markers::MarkerKind],
    duration_ms: i64,
) -> Result<usize, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "marker database is unavailable".to_owned())?;
    let mut updated = 0usize;
    for &kind in kinds {
        let range = match kind {
            markers::MarkerKind::Intro => segments.intro,
            markers::MarkerKind::Credits => segments.credits,
        };
        for (file, fingerprint) in files.iter().zip(fingerprints) {
            if let Some(range) = range {
                let detected = markers::DetectedRange {
                    start_ms: range.start_ms,
                    end_ms: range.end_ms,
                };
                markers::replace_detected_markers(
                    &db,
                    &file.id,
                    kind,
                    Some(&detected),
                    fingerprint,
                    group_fingerprint,
                    duration_ms,
                )
                .map_err(|error| format!("could not persist detected marker: {error}"))?;
                updated += 1;
                export_marker_sidecar(
                    state,
                    file,
                    &db,
                    Some((kind, fingerprint, group_fingerprint)),
                )?;
            }
            // Record the lookup even when TheIntroDB had nothing, so the next
            // scan does not repeat the request for the same file and group.
            markers::record_marker_lookup(
                &db,
                &file.id,
                kind,
                fingerprint,
                group_fingerprint,
                range.is_some(),
            )
            .map_err(|error| format!("could not record marker lookup: {error}"))?;
        }
    }
    Ok(updated)
}

fn store_missing_introdb_lookup(
    state: &AppState,
    files: &[Media],
    fingerprints: &[String],
    group_fingerprint: &str,
    kinds: &[markers::MarkerKind],
) -> Result<(), String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "marker database is unavailable".to_owned())?;
    for &kind in kinds {
        for (file, fingerprint) in files.iter().zip(fingerprints) {
            markers::record_marker_lookup(
                &db,
                &file.id,
                kind,
                fingerprint,
                group_fingerprint,
                false,
            )
            .map_err(|error| format!("could not record marker lookup: {error}"))?;
        }
    }
    Ok(())
}

async fn media_duration_from_cache(state: &AppState, media: &Media) -> Option<i64> {
    ensure_media_info(state, media)
        .await
        .as_ref()
        .and_then(technical_duration_ms)
}

/// Single-episode seasons cannot be correlated with another episode, so use
/// the public TheIntroDB timestamps instead. Both intro and credits come from
/// one request.
async fn detect_introdb_markers_for_episode(
    state: &AppState,
    root: &std::path::Path,
    files: &[Media],
    season: u32,
    episode: u32,
    kinds: &[markers::MarkerKind],
    force: bool,
) -> Result<usize, String> {
    let Some(representative) = files.first() else {
        return Ok(0);
    };
    let fingerprints = files
        .iter()
        .map(|file| media_fingerprint(root, file))
        .collect::<Vec<_>>();
    let group_fingerprint = group_fingerprint(root, std::slice::from_ref(representative));
    let needed = kinds
        .iter()
        .copied()
        .filter(|kind| {
            introdb_lookup_needed(
                state,
                files,
                &fingerprints,
                &group_fingerprint,
                *kind,
                force,
            )
        })
        .collect::<Vec<_>>();
    if needed.is_empty() {
        return Ok(0);
    }
    let Some(tmdb_id) = media_tmdb_id(state, representative) else {
        debug!(
            path = %representative.path.display(),
            "TheIntroDB fallback skipped because the episode has no TMDB show match"
        );
        return Ok(0);
    };
    let duration = media_duration_from_cache(state, representative).await;
    let started = Instant::now();
    match introdb::lookup(tmdb_id, Some((season, episode)), duration).await {
        Ok(segments) => {
            let updated = store_introdb_segments(
                state,
                files,
                &fingerprints,
                &group_fingerprint,
                segments,
                &needed,
                duration.unwrap_or(i64::MAX),
            )
            .await?;
            info!(
                show_episode = ?(season, episode),
                tmdb_id,
                force,
                rows_updated = updated,
                intro = segments.intro.is_some(),
                credits = segments.credits.is_some(),
                elapsed_ms = started.elapsed().as_millis(),
                "TheIntroDB single-episode marker lookup finished"
            );
            Ok(updated)
        }
        Err(introdb::LookupError::Missing) => {
            store_missing_introdb_lookup(state, files, &fingerprints, &group_fingerprint, &needed)?;
            debug!(
                show_episode = ?(season, episode),
                tmdb_id,
                elapsed_ms = started.elapsed().as_millis(),
                "TheIntroDB has no timestamps for this episode"
            );
            Ok(0)
        }
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) async fn detect_markers_for_movie(
    state: AppState,
    media: Media,
    kind: markers::MarkerKind,
    force: bool,
) -> Result<usize, String> {
    detect_markers_for_movie_kinds(state, media, &[kind], force).await
}

async fn detect_markers_for_movie_kinds(
    state: AppState,
    media: Media,
    kinds: &[markers::MarkerKind],
    force: bool,
) -> Result<usize, String> {
    let kind = kinds.first().copied().unwrap_or(markers::MarkerKind::Intro);
    let (_, library) = media_library(&state, &media)
        .ok_or_else(|| "media item is not in a configured library".to_owned())?;
    if library.kind != "movie" {
        return Err("movie marker detection requires a movie library".to_owned());
    }
    let root = library
        .folders
        .iter()
        .find(|folder| media.path.starts_with(folder))
        .cloned()
        .ok_or_else(|| "movie library folder was not found".to_owned())?;
    let Some(tmdb_id) = media_tmdb_id(&state, &media) else {
        debug!(
            path = %media.path.display(),
            "TheIntroDB fallback skipped because the movie has no TMDB match"
        );
        return Ok(0);
    };
    let fingerprint = media_fingerprint(&root, &media);
    let group_fingerprint = group_fingerprint(&root, std::slice::from_ref(&media));
    let files = std::slice::from_ref(&media);
    let fingerprints = [fingerprint];
    let needed = kinds
        .iter()
        .copied()
        .filter(|kind| {
            introdb_lookup_needed(
                &state,
                files,
                &fingerprints,
                &group_fingerprint,
                *kind,
                force,
            )
        })
        .collect::<Vec<_>>();
    if needed.is_empty() {
        return Ok(0);
    }
    let duration = media_duration_from_cache(&state, &media).await;
    let started = Instant::now();
    match introdb::lookup(tmdb_id, None, duration).await {
        Ok(segments) => {
            let updated = store_introdb_segments(
                &state,
                files,
                &fingerprints,
                &group_fingerprint,
                segments,
                &needed,
                duration.unwrap_or(i64::MAX),
            )
            .await?;
            info!(
                title = %media.title,
                marker_type = kind.as_str(),
                tmdb_id,
                force,
                rows_updated = updated,
                intro = segments.intro.is_some(),
                credits = segments.credits.is_some(),
                elapsed_ms = started.elapsed().as_millis(),
                "TheIntroDB movie marker lookup finished"
            );
            Ok(updated)
        }
        Err(introdb::LookupError::Missing) => {
            store_missing_introdb_lookup(
                &state,
                files,
                &fingerprints,
                &group_fingerprint,
                &needed,
            )?;
            debug!(
                title = %media.title,
                tmdb_id,
                elapsed_ms = started.elapsed().as_millis(),
                "TheIntroDB has no timestamps for this movie"
            );
            Ok(0)
        }
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) async fn detect_markers_for_media(
    state: AppState,
    media: Media,
    kind: markers::MarkerKind,
    force: bool,
    threshold: f32,
) -> Result<usize, String> {
    let library_kind = media_library(&state, &media)
        .map(|(_, library)| library.kind)
        .ok_or_else(|| "media item is not in a configured library".to_owned())?;
    if library_kind == "movie" {
        detect_markers_for_movie(state, media, kind, force).await
    } else {
        detect_markers_for_episode(state, media, kind, force, threshold).await
    }
}

pub(crate) async fn detect_markers_for_episode(
    state: AppState,
    media: Media,
    kind: markers::MarkerKind,
    force: bool,
    threshold: f32,
) -> Result<usize, String> {
    detect_markers_for_episode_kinds(state, media, &[kind], force, threshold).await
}

/// `kinds` is a single type for the audio path. A single-episode season
/// resolves every requested type from one TheIntroDB lookup, so the automatic
/// scan passes both.
async fn detect_markers_for_episode_kinds(
    state: AppState,
    media: Media,
    kinds: &[markers::MarkerKind],
    force: bool,
    threshold: f32,
) -> Result<usize, String> {
    let kind = kinds.first().copied().unwrap_or(markers::MarkerKind::Intro);
    let detection_started = Instant::now();
    let (_, library) = media_library(&state, &media)
        .ok_or_else(|| "media item is not in a configured library".to_owned())?;
    if library.kind != "tv" {
        return Err("marker detection is only available for TV episodes".to_owned());
    }
    let root = library
        .folders
        .iter()
        .find(|folder| media.path.starts_with(folder))
        .cloned()
        .ok_or_else(|| "episode library folder was not found".to_owned())?;
    let episode_info = tv_episode_info(&root, &media.path)
        .ok_or_else(|| "media item is not a recognizable TV episode".to_owned())?;
    let show_title = tv_show_title(&root, &media.path);
    let episode_groups = season_episode_candidates_by_number(
        &state,
        &library,
        &root,
        &show_title,
        episode_info.season,
    );
    if episode_groups.len() < 2 {
        let Some(files) = episode_groups.values().next() else {
            return Ok(0);
        };
        return detect_introdb_markers_for_episode(
            &state,
            &root,
            files,
            episode_info.season,
            episode_info.episode,
            kinds,
            force,
        )
        .await;
    }
    let episodes = episode_groups
        .values()
        .filter_map(|files| files.first().cloned())
        .collect::<Vec<_>>();
    let all_episode_files = episode_groups.values().flatten().collect::<Vec<_>>();

    let group_fingerprint = group_fingerprint(&root, &episodes);
    info!(
        marker_type = kind.as_str(),
        show = %show_title,
        season = episode_info.season,
        force,
        threshold,
        episodes = episodes.len(),
        all_files = all_episode_files.len(),
        "marker detection started"
    );
    if !force {
        let Ok(db) = state.db.lock() else {
            return Err("marker database is unavailable".to_owned());
        };
        let mut files_checked = 0usize;
        let mut sidecar_fingerprints = 0usize;
        let mut database_fingerprints = 0usize;
        let fresh = all_episode_files.iter().all(|episode| {
            files_checked += 1;
            let expected_media = media_fingerprint(&root, episode);
            let sidecar = marker_sidecar_target(&library, &root, episode);
            let from_sidecar = sidecar
                .as_ref()
                .and_then(|(path, key)| markers::marker_fingerprint_from_sidecar(path, key, kind));
            let from_database = markers::marker_fingerprints(&db, &episode.id, kind)
                .ok()
                .flatten();
            if from_sidecar.is_some() {
                sidecar_fingerprints += 1;
            }
            if from_database.is_some() {
                database_fingerprints += 1;
            }
            [from_sidecar, from_database]
                .into_iter()
                .flatten()
                .any(|(media, group)| media == expected_media && group == group_fingerprint)
        });
        if fresh {
            info!(
                marker_type = kind.as_str(),
                show = %show_title,
                season = episode_info.season,
                files_checked,
                sidecar_fingerprints,
                database_fingerprints,
                "marker detection skipped because every episode has a current fingerprint"
            );
            return Ok(0);
        }
        info!(
            marker_type = kind.as_str(),
            show = %show_title,
            season = episode_info.season,
            files_checked,
            sidecar_fingerprints,
            database_fingerprints,
            "marker detection fingerprint check found stale or missing markers"
        );
    }

    let mut decoded = Vec::new();
    for episode in &episodes {
        let Some(episode_number) = tv_episode_info(&root, &episode.path).map(|info| info.episode)
        else {
            warn!(path = %episode.path.display(), "skipping episode without a number in marker detection");
            continue;
        };
        let duration = media_duration_ms(episode).await;
        if kind == markers::MarkerKind::Credits && duration.is_none() {
            warn!(path = %episode.path.display(), "skipping credits detection: duration is unknown");
            continue;
        }
        match markers::decode_audio_window(&episode.path, kind, duration).await {
            Ok(audio) => decoded.push((
                episode.clone(),
                episode_number,
                duration.unwrap_or(i64::MAX),
                audio,
            )),
            Err(error) => warn!(
                path = %episode.path.display(),
                %error,
                "skipping episode in marker detection"
            ),
        }
    }
    info!(
        marker_type = kind.as_str(),
        episodes = episodes.len(),
        decoded = decoded.len(),
        "marker detection audio decoding finished"
    );
    if decoded.len() < 2 {
        return Err("fewer than two episodes had decodable audio".to_owned());
    }

    let episode_audio = decoded
        .iter()
        .map(|(_, _, _, audio)| audio.clone())
        .collect::<Vec<_>>();
    let ranges =
        markers::run_low_priority(move || markers::detect_shared_ranges(&episode_audio, threshold))
            .await?;
    let mut updated = 0usize;
    let Ok(db) = state.db.lock() else {
        return Err("marker database is unavailable".to_owned());
    };
    for (index, (episode, episode_number, duration, _)) in decoded.iter().enumerate() {
        let files = episode_groups
            .get(episode_number)
            .map(Vec::as_slice)
            .unwrap_or(std::slice::from_ref(episode));
        for file in files {
            let file_fingerprint = media_fingerprint(&root, file);
            match markers::replace_detected_markers(
                &db,
                &file.id,
                kind,
                ranges[index].as_ref(),
                &file_fingerprint,
                &group_fingerprint,
                *duration,
            ) {
                Ok(()) => {
                    updated += 1;
                    if let Err(error) = export_marker_sidecar(
                        &state,
                        file,
                        &db,
                        Some((kind, &file_fingerprint, &group_fingerprint)),
                    ) {
                        return Err(error);
                    }
                }
                Err(error) => warn!(
                    path = %file.path.display(),
                    %error,
                    "could not persist detected marker"
                ),
            }
        }
    }
    info!(
        marker_type = kind.as_str(),
        show = %show_title,
        season = episode_info.season,
        rows_updated = updated,
        elapsed_ms = detection_started.elapsed().as_millis(),
        "marker detection finished"
    );
    Ok(updated)
}

pub(crate) fn marker_detection_allowed(enabled: bool, manual: bool) -> bool {
    enabled || manual
}

pub(crate) async fn detect_markers_for_library(
    state: AppState,
    library_id: usize,
    force: bool,
    threshold: f32,
    manual: bool,
) -> Result<usize, String> {
    let (marker_detection_enabled, library) = {
        let config = state
            .config
            .lock()
            .map_err(|_| "server configuration is unavailable".to_owned())?;
        (
            config.marker_detection_enabled,
            config.libraries.get(library_id - 1).cloned(),
        )
    };
    let library = library.ok_or_else(|| "library was not found".to_owned())?;
    if !marker_detection_allowed(marker_detection_enabled, manual) {
        info!(
            library_id,
            "automatic marker detection is disabled in settings"
        );
        return Ok(0);
    }
    // A first scan of a large library must not turn into thousands of
    // TheIntroDB calls. Each pass resolves a bounded batch; recorded lookups
    // make later passes continue with the remaining items.
    const INTRODB_LOOKUPS_PER_PASS: usize = 25;
    let mut introdb_lookups_remaining = INTRODB_LOOKUPS_PER_PASS;
    if library.kind == "movie" {
        let mut updated = 0usize;
        for media in media_snapshot(&state)
            .iter()
            .filter(|media| media_in_library(media, &library))
            .filter(|media| {
                library
                    .folders
                    .iter()
                    .any(|folder| media.path.starts_with(folder))
            })
        {
            let Some(root) = library
                .folders
                .iter()
                .find(|folder| media.path.starts_with(folder))
            else {
                continue;
            };
            if !movie_introdb_lookup_needed(&state, media, root, force) {
                continue;
            }
            if introdb_lookups_remaining == 0 {
                debug!(
                    library_id,
                    "TheIntroDB lookup budget for this pass is exhausted; remaining items resume on the next scan"
                );
                break;
            }
            introdb_lookups_remaining -= 1;
            let key = format!("{}|movie|{}", library_id, media.id);
            let Some(_detection_activity) = begin_movie_marker_detection(
                &state,
                key.clone(),
                library.name.clone(),
                &media.title,
            ) else {
                continue;
            };
            match detect_markers_for_movie_kinds(
                state.clone(),
                (*media).clone(),
                &[markers::MarkerKind::Intro, markers::MarkerKind::Credits],
                force,
            )
            .await
            {
                Ok(count) => updated += count,
                Err(error) => {
                    warn!(
                        library_id,
                        title = %media.title,
                        %error,
                        "automatic TheIntroDB marker lookup failed"
                    );
                    end_marker_detection_group(&key);
                    // A transport or rate-limit failure would repeat for every
                    // remaining movie, so stop this pass instead of hammering.
                    break;
                }
            }
            end_marker_detection_group(&key);
        }
        return Ok(updated);
    }
    if library.kind != "tv" {
        return Ok(0);
    }

    let mut groups = BTreeMap::<(String, u32), (Media, HashSet<u32>, Vec<Media>)>::new();
    for media in media_snapshot(&state).iter() {
        let Some(root) = library
            .folders
            .iter()
            .find(|folder| media.path.starts_with(folder))
        else {
            continue;
        };
        let Some(info) = tv_episode_info(root, &media.path) else {
            continue;
        };
        let group = groups
            .entry((tv_show_title(root, &media.path), info.season))
            .or_insert_with(|| ((*media).clone(), HashSet::new(), Vec::new()));
        group.1.insert(info.episode);
        group.2.push((*media).clone());
    }

    let mut updated = 0usize;
    for ((show, season), (media, episodes, files)) in groups {
        if episodes.len() < 2 {
            // A single episode cannot be correlated with another episode, so
            // use TheIntroDB instead. One lookup resolves both intro and
            // credits.
            let Some(root) = library
                .folders
                .iter()
                .find(|folder| media.path.starts_with(folder))
            else {
                continue;
            };
            if !episode_introdb_lookup_needed(&state, root, &files, force) {
                continue;
            }
            if introdb_lookups_remaining == 0 {
                debug!(
                    library_id,
                    "TheIntroDB lookup budget for this pass is exhausted; remaining items resume on the next scan"
                );
                break;
            }
            introdb_lookups_remaining -= 1;
            let key = format!("{}|{}|{}|intro", library_id, show, season);
            let Some(_detection_activity) = begin_marker_detection_group(
                &state,
                key.clone(),
                library.name.clone(),
                &show,
                season,
                markers::MarkerKind::Intro,
            ) else {
                continue;
            };
            match detect_markers_for_episode_kinds(
                state.clone(),
                media.clone(),
                &[markers::MarkerKind::Intro, markers::MarkerKind::Credits],
                force,
                threshold,
            )
            .await
            {
                Ok(count) => updated += count,
                Err(error) => warn!(
                    library_id,
                    %show,
                    season,
                    %error,
                    "automatic TheIntroDB marker lookup failed"
                ),
            }
            end_marker_detection_group(&key);
            continue;
        }
        for kind in [markers::MarkerKind::Intro, markers::MarkerKind::Credits] {
            let key = format!("{}|{}|{}|{}", library_id, show, season, kind.as_str());
            let Some(_detection_activity) = begin_marker_detection_group(
                &state,
                key.clone(),
                library.name.clone(),
                &show,
                season,
                kind,
            ) else {
                continue;
            };
            match detect_markers_for_episode(state.clone(), media.clone(), kind, force, threshold)
                .await
            {
                Ok(count) => updated += count,
                Err(error) => warn!(
                    library_id,
                    %show,
                    season,
                    marker_type = kind.as_str(),
                    %error,
                    "automatic marker detection failed"
                ),
            }
            end_marker_detection_group(&key);
        }
    }
    Ok(updated)
}

pub(crate) fn find_plex_tv_location<'a>(
    state: &AppState,
    media_items: &'a [Media],
    rating_key: &str,
) -> Option<(usize, ConfigLibrary, PlexTvShowGroup<'a>, Option<u32>)> {
    let libraries = state.config.lock().ok()?.libraries.clone();
    for (index, library) in libraries.into_iter().enumerate() {
        if library.kind != "tv" {
            continue;
        }
        let section = index + 1;
        let shows = plex_tv_show_groups(
            section,
            &library.folders,
            media_items
                .iter()
                .filter(|media| media_in_library(media, &library)),
        );
        for show in shows {
            if show.rating_key == rating_key {
                return Some((section, library, show, None));
            }
            for season in plex_tv_season_groups(&show, &library.folders)
                .keys()
                .copied()
            {
                if plex_tv_season_key(&show.rating_key, season) == rating_key {
                    return Some((section, library, show, Some(season)));
                }
            }
        }
    }
    None
}

pub(crate) fn external_subtitle_content(
    contents: &str,
    source_format: &str,
    requested_format: &str,
) -> Option<(&'static str, String)> {
    match requested_format {
        "srt" => Some((
            "text/srt; charset=utf-8",
            match source_format {
                "srt" => contents.to_owned(),
                "vtt" => vtt_to_srt(contents),
                "ass" => vtt_to_srt(&ass_to_vtt(contents)),
                _ => return None,
            },
        )),
        "vtt" | "webvtt" => Some((
            "text/vtt; charset=utf-8",
            match source_format {
                "srt" => srt_to_vtt(contents),
                "ass" => ass_to_vtt(contents),
                "vtt" => contents.to_owned(),
                _ => return None,
            },
        )),
        "ass" | "ssa" if source_format == "ass" => {
            Some(("text/x-ssa; charset=utf-8", contents.to_owned()))
        }
        _ => None,
    }
}

fn vtt_to_srt(contents: &str) -> String {
    let mut output = String::new();
    let mut cue = 0;
    for block in contents.replace("\r\n", "\n").split("\n\n") {
        let mut lines = block.lines();
        let first = lines.next().unwrap_or_default();
        if first.trim_start().starts_with("WEBVTT") {
            continue;
        }
        let (identifier, timing) = if first.contains(" --> ") {
            (None, first)
        } else {
            let second = lines.next().unwrap_or_default();
            (Some(first), second)
        };
        if !timing.contains(" --> ") {
            continue;
        }
        cue += 1;
        output.push_str(
            identifier
                .map(str::to_owned)
                .unwrap_or_else(|| cue.to_string())
                .as_str(),
        );
        output.push('\n');
        output.push_str(&timing.replace('.', ","));
        output.push('\n');
        for line in lines {
            output.push_str(line);
            output.push('\n');
        }
        output.push('\n');
    }
    output
}

pub(crate) fn media_in_library(media: &Media, library: &ConfigLibrary) -> bool {
    library
        .folders
        .iter()
        .any(|folder| media.path.starts_with(folder))
}

pub(crate) fn media_library(state: &AppState, media: &Media) -> Option<(usize, ConfigLibrary)> {
    state
        .config
        .lock()
        .ok()?
        .libraries
        .iter()
        .enumerate()
        .find(|(_, library)| media_in_library(media, library))
        .map(|(index, library)| (index + 1, library.clone()))
}

pub(crate) fn title_year(title: &str) -> Option<u32> {
    title
        .split(|character: char| !character.is_ascii_digit())
        .find_map(|part| {
            (part.len() == 4)
                .then(|| part.parse::<u32>().ok())
                .flatten()
                .filter(|year| (1888..=2100).contains(year))
        })
}

pub(crate) fn tmdb_genre_name(media_type: &str, id: i64) -> Option<&'static str> {
    let name = match id {
        12 => "Adventure",
        14 => "Fantasy",
        16 => "Animation",
        18 => "Drama",
        27 => "Horror",
        28 => "Action",
        35 => "Comedy",
        36 => "History",
        37 => "Western",
        53 => "Thriller",
        80 => "Crime",
        99 => "Documentary",
        878 => "Science Fiction",
        9648 => "Mystery",
        10402 => "Music",
        10749 => "Romance",
        10751 => "Family",
        10752 => "War",
        10759 if media_type == "show" => "Action & Adventure",
        10762 if media_type == "show" => "Kids",
        10763 if media_type == "show" => "News",
        10764 if media_type == "show" => "Reality",
        10765 if media_type == "show" => "Sci-Fi & Fantasy",
        10766 if media_type == "show" => "Soap",
        10767 if media_type == "show" => "Talk",
        10768 if media_type == "show" => "War & Politics",
        10770 => "TV Movie",
        _ => return None,
    };
    Some(name)
}

pub(crate) async fn stream_media(
    State(s): State<AppState>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    stream_for_id(s, id, method, headers).await
}

pub(crate) async fn stream_file(
    media: &Media,
    method: Method,
    headers: HeaderMap,
    download: bool,
) -> Response {
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let (start, end) = match parse_range(range, media.size) {
        Ok(value) => value,
        Err(()) => {
            return (
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(header::CONTENT_RANGE, format!("bytes */{}", media.size))],
                Body::empty(),
            )
                .into_response()
        }
    };
    let length = end - start + 1;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&media.mime).unwrap(),
    );
    response_headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).unwrap(),
    );
    response_headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if download {
        let filename = media
            .path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| std::borrow::Cow::Borrowed("media"));
        let fallback = filename
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, ' ' | '.' | '-' | '_') {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let encoded = filename
            .as_bytes()
            .iter()
            .map(|byte| {
                if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'.' | b'_') {
                    char::from(*byte).to_string()
                } else {
                    format!("%{byte:02X}")
                }
            })
            .collect::<String>();
        let value = format!("attachment; filename=\"{fallback}\"; filename*=UTF-8''{encoded}");
        response_headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&value).unwrap(),
        );
    }
    if range.is_some() {
        response_headers.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {}-{}/{}", start, end, media.size)).unwrap(),
        );
    }
    if method == Method::HEAD {
        return (
            if range.is_some() {
                StatusCode::PARTIAL_CONTENT
            } else {
                StatusCode::OK
            },
            response_headers,
            Body::empty(),
        )
            .into_response();
    }
    let mut file = match File::open(&media.path).await {
        Ok(file) => file,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    if file.seek(SeekFrom::Start(start)).await.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let stream = ReaderStream::new(file.take(length));
    (
        if range.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        },
        response_headers,
        Body::from_stream(stream),
    )
        .into_response()
}

fn parse_range(value: Option<&str>, size: u64) -> Result<(u64, u64), ()> {
    if size == 0 {
        return Err(());
    }
    let Some(value) = value else {
        return Ok((0, size - 1));
    };
    let value = value.strip_prefix("bytes=").ok_or(())?;
    if value.contains(',') {
        return Err(());
    }
    let (left, right) = value.split_once('-').ok_or(())?;
    let (start, end) = if left.is_empty() {
        let suffix: u64 = right.parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        (size.saturating_sub(suffix), size - 1)
    } else {
        let start: u64 = left.parse().map_err(|_| ())?;
        if start >= size {
            return Err(());
        }
        let end = if right.is_empty() {
            size - 1
        } else {
            right.parse::<u64>().map_err(|_| ())?.min(size - 1)
        };
        if end < start {
            return Err(());
        }
        (start, end)
    };
    Ok((start, end))
}

fn html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn add_build_info_footer(mut document: String) -> String {
    let footer = format!(
        r#"<aside class="harbinger-build-info" aria-label="Harbinger version and commit">Harbinger v{} · commit {}</aside><style>.harbinger-build-info{{position:fixed;left:.75rem;bottom:.65rem;z-index:30;padding:.35rem .55rem;border:1px solid rgba(148,163,184,.18);border-radius:.45rem;color:#9ca3af;background:rgba(11,16,32,.88);backdrop-filter:blur(8px);font:500 .68rem/1.2 ui-monospace,SFMono-Regular,Menlo,monospace;pointer-events:none;white-space:nowrap}}html{{scrollbar-gutter:stable}}.media-card-cover,.media-card-cover img{{display:block;box-sizing:border-box;width:100%;aspect-ratio:2/3}}.media-card-cover img{{object-fit:cover;border-radius:.35rem;background:#111827}}.media-card-book .media-card-cover,.media-card-book .media-card-cover img{{aspect-ratio:1/1}}.media-cover-placeholder{{display:grid;box-sizing:border-box;width:100%;height:100%;place-items:center;border-radius:.35rem;color:#bfdbfe;background:linear-gradient(135deg,#1e3a8a,#312e81 55%,#111827);font-size:.9rem;padding:.7rem;text-align:center;overflow-wrap:anywhere}}@media(max-width:700px){{.harbinger-build-info{{display:none}}}}</style>"#,
        env!("CARGO_PKG_VERSION"),
        env!("HARBINGER_GIT_COMMIT"),
    );
    if let Some(position) = document.rfind("</body>") {
        document.insert_str(position, &footer);
    } else if let Some(position) = document.rfind("</html>") {
        document.insert_str(position, &footer);
    }
    document
}

#[cfg(test)]
#[path = "../../tests/unit/plex_helpers.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/unit/plex_endpoints.rs"]
mod plex_endpoint_tests;
