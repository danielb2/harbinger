use super::catalog::{AudioFile, Item, Library, SERVER_VERSION};
use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub(crate) struct UserIdentity {
    pub(crate) id: String,
    pub(crate) username: String,
    pub(crate) user_type: String,
    pub(crate) created_at: i64,
    pub(crate) last_seen: i64,
}

impl UserIdentity {
    fn audiobookshelf_type(&self) -> &str {
        match self.user_type.as_str() {
            "admin" => "root",
            other => other,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PlaybackSession {
    pub(crate) id: String,
    pub(crate) user_id: String,
    pub(crate) library_id: String,
    pub(crate) library_item_id: String,
    pub(crate) current_time: f64,
    pub(crate) time_listened: f64,
    pub(crate) duration: f64,
    pub(crate) started_at: i64,
    pub(crate) updated_at: i64,
}

pub(crate) fn server_settings_json() -> Value {
    json!({
        "id": "server-settings",
        "scannerFindCovers": false,
        "scannerCoverProvider": "audible",
        "scannerParseSubtitle": false,
        "scannerPreferMatchedMetadata": false,
        "scannerDisableWatcher": true,
        "storeCoverWithItem": false,
        "storeMetadataWithItem": false,
        "metadataFileFormat": "json",
        "rateLimitLoginRequests": 10,
        "rateLimitLoginWindow": 600000,
        "backupSchedule": "30 1 * * *",
        "backupsToKeep": 2,
        "maxBackupSize": 1,
        "loggerDailyLogsToKeep": 7,
        "loggerScannerLogsToKeep": 2,
        "homeBookshelfView": 1,
        "bookshelfView": 1,
        "sortingIgnorePrefix": false,
        "sortingPrefixes": ["the", "a"],
        "chromecastEnabled": false,
        "dateFormat": "MM/dd/yyyy",
        "language": "en-us",
        "logLevel": 2,
        "version": SERVER_VERSION
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn user_json(
    identity: &UserIdentity,
    token: &str,
    default_library_id: &str,
    media_progress: Vec<Value>,
    access_token: Option<&str>,
    refresh_token: Option<&str>,
) -> Value {
    let last_seen = if identity.last_seen > 0 {
        identity.last_seen * 1000
    } else {
        now_ms()
    };
    let mut user = json!({
        "id": identity.id,
        "username": identity.username,
        "email": "",
        "type": identity.audiobookshelf_type(),
        "token": token,
        "mediaProgress": media_progress,
        "seriesHideFromContinueListening": [],
        "bookmarks": [],
        "isActive": true,
        "isLocked": false,
        "lastSeen": last_seen,
        "createdAt": identity.created_at * 1000,
        "defaultLibraryId": default_library_id,
        "librariesAccessible": [],
        "itemTagsAccessible": [],
        "itemTagsSelected": [],
        "isOldToken": false,
        "permissions": {
            "download": true,
            "update": true,
            "delete": true,
            "upload": true,
            "accessAllLibraries": true,
            "accessAllTags": true,
            "accessExplicitContent": true
        }
    });
    if let Some(access_token) = access_token {
        user["accessToken"] = json!(access_token);
        user["refreshToken"] = match refresh_token {
            Some(refresh_token) => json!(refresh_token),
            None => Value::Null,
        };
        user["tokenType"] = json!("Bearer");
        user["expiresIn"] = json!(3_600);
    }
    user
}

pub(crate) fn login_envelope(user: Value, default_library_id: &str) -> Value {
    json!({
        "user": user,
        "userDefaultLibraryId": default_library_id,
        "serverSettings": server_settings_json(),
        "ereaderDevices": [],
        "Source": "docker"
    })
}

pub(crate) fn library_json(library: &Library) -> Value {
    json!({
        "id": library.id,
        "name": library.name,
        "folders": library.folders.iter().map(|folder| json!({
            "id": folder.id,
            "fullPath": folder.full_path,
            "libraryId": library.id,
            "addedAt": 0
        })).collect::<Vec<_>>(),
        "displayOrder": 1,
        "icon": "audiobookshelf",
        "mediaType": "book",
        "provider": "audible",
        "settings": library_settings_json(),
        "createdAt": 0,
        "lastUpdate": 0,
        "lastScan": null,
        "lastScanVersion": SERVER_VERSION
    })
}

pub(crate) fn library_detail_json(library: &Library) -> Value {
    json!({
        "library": library_json(library),
        "filterdata": {
            "authors": [],
            "genres": [],
            "tags": [],
            "series": [],
            "narrators": [],
            "languages": [],
            "publishers": [],
            "publishedDecades": []
        },
        "issues": 0,
        "numUserPlaylists": 0
    })
}

pub(crate) fn item_json(item: &Item, minified: bool) -> Value {
    let metadata = if minified {
        json!({
            "title": item.title,
            "titleIgnorePrefix": item.title,
            "subtitle": "",
            "authorName": item.author,
            "authorNameLF": item.author,
            "narratorName": "",
            "seriesName": "",
            "genres": [],
            "publishedYear": "",
            "publishedDate": "",
            "publisher": "",
            "description": item.description,
            "isbn": "",
            "asin": "",
            "language": "en",
            "explicit": false,
            "abridged": false
        })
    } else {
        json!({
            "title": item.title,
            "titleIgnorePrefix": item.title,
            "subtitle": "",
            "authors": [{"id": "author-unknown", "name": item.author}],
            "narrators": [],
            "series": [],
            "genres": [],
            "description": item.description,
            "publishedYear": "",
            "publishedDate": "",
            "publisher": "",
            "language": "en",
            "explicit": false,
            "abridged": false,
            "isbn": "",
            "asin": ""
        })
    };
    let mut media = json!({
        "id": format!("me_{}", item.id.trim_start_matches("li_")),
        "libraryItemId": item.id,
        "metadata": metadata,
        "coverPath": cover_path(item),
        "tags": [],
        "numTracks": item.audio_files.len().max(1),
        "numAudioFiles": item.audio_files.len().max(1),
        "numChapters": 0,
        "duration": item.duration,
        "size": item.size,
        "ebookFormat": null
    });
    if !minified {
        media["audioFiles"] = json!(item
            .audio_files
            .iter()
            .map(|file| audio_file_json(file))
            .collect::<Vec<_>>());
        media["tracks"] = json!(item
            .audio_files
            .iter()
            .map(|file| audio_track_json(file, &item.id))
            .collect::<Vec<_>>());
        media["chapters"] = json!([]);
    }
    let mut result = json!({
        "id": item.id,
        "ino": "0",
        "oldLibraryItemId": null,
        "libraryId": item.library_id,
        "folderId": item.folder_id,
        "path": item.path.to_string_lossy(),
        "relPath": item.rel_path,
        "isFile": false,
        "mtimeMs": item.updated_ms,
        "ctimeMs": item.added_ms,
        "birthtimeMs": item.added_ms,
        "addedAt": item.added_ms,
        "updatedAt": item.updated_ms,
        "isMissing": false,
        "isInvalid": false,
        "mediaType": "book",
        "media": media
    });
    if minified {
        result["numFiles"] = json!(item.audio_files.len().max(1));
        result["size"] = json!(0);
    } else {
        result["lastScan"] = json!(item.updated_ms);
        result["scanVersion"] = json!(SERVER_VERSION);
        result["libraryFiles"] = json!(library_files_json(item));
        result["size"] = json!(item.size);
    }
    result
}

pub(crate) fn media_progress_json(
    user_id: &str,
    item: &Item,
    current_time: f64,
    duration: f64,
    is_finished: bool,
    hidden: bool,
    updated_at_ms: i64,
) -> Value {
    let progress = if duration > 0.0 {
        (current_time / duration).clamp(0.0, 1.0)
    } else if is_finished {
        1.0
    } else {
        0.0
    };
    json!({
        "id": item.id,
        "userId": user_id,
        "libraryItemId": item.id,
        "episodeId": null,
        "mediaItemId": item.id,
        "mediaItemType": "book",
        "duration": duration,
        "progress": progress,
        "currentTime": current_time,
        "isFinished": is_finished,
        "hideFromContinueListening": hidden,
        "ebookLocation": null,
        "ebookProgress": 0,
        "lastUpdate": updated_at_ms,
        "startedAt": item.added_ms,
        "finishedAt": if is_finished { json!(updated_at_ms) } else { Value::Null }
    })
}

pub(crate) fn playback_session_json(session: &PlaybackSession, item: &Item) -> Value {
    let now = chrono::Local::now();
    let mut audio_tracks = item
        .audio_files
        .iter()
        .map(|file| audio_track_json(file, &item.id))
        .collect::<Vec<_>>();
    for track in &mut audio_tracks {
        let index = track["index"].as_u64().unwrap_or(1);
        track["contentUrl"] = json!(format!("/public/session/{}/track/{index}", session.id));
    }
    let detail = item_json(item, false);
    let metadata = detail["media"]["metadata"].clone();
    json!({
        "id": session.id,
        "userId": session.user_id,
        "libraryId": session.library_id,
        "libraryItemId": session.library_item_id,
        "episodeId": null,
        "mediaType": "book",
        "mediaMetadata": metadata,
        "chapters": [],
        "displayTitle": item.title,
        "displayAuthor": item.author,
        "coverPath": cover_path(item),
        "duration": session.duration,
        "playMethod": 0,
        "mediaPlayer": "html5",
        "deviceInfo": {
            "ipAddress": "",
            "browserName": "Harbinger",
            "browserVersion": SERVER_VERSION,
            "osName": "Harbinger",
            "osVersion": SERVER_VERSION,
            "serverVersion": SERVER_VERSION
        },
        "date": now.format("%Y-%m-%d").to_string(),
        "dayOfWeek": now.format("%A").to_string(),
        "timeListening": session.time_listened,
        "startTime": session.current_time,
        "currentTime": session.current_time,
        "startedAt": session.started_at,
        "updatedAt": session.updated_at,
        "audioTracks": audio_tracks,
        "videoTrack": null,
        "libraryItem": detail
    })
}

fn library_settings_json() -> Value {
    json!({
        "coverAspectRatio": 1,
        "disableWatcher": true,
        "skipMatchingMediaWithAsin": false,
        "skipMatchingMediaWithIsbn": false,
        "autoScanCronExpression": null,
        "audiobooksOnly": true,
        "hideSingleBookSeries": false,
        "onlyShowLaterBooksInContinueSeries": false,
        "metadataPrecedence": [],
        "epubsAllowScriptedContent": false,
        "markAsFinishedPercentComplete": null,
        "markAsFinishedTimeRemaining": 10
    })
}

fn audio_file_json(file: &AudioFile) -> Value {
    let extension = extension(file);
    json!({
        "index": file.index,
        "ino": file.index.to_string(),
        "metadata": file_metadata(file),
        "addedAt": file.modified_ms,
        "updatedAt": file.modified_ms,
        "trackNumFromMeta": null,
        "discNumFromMeta": null,
        "trackNumFromFilename": null,
        "discNumFromFilename": null,
        "manuallyVerified": false,
        "exclude": false,
        "error": null,
        "format": format_name(&extension),
        "duration": file.duration,
        "bitRate": 0,
        "language": null,
        "codec": codec_name(&extension),
        "timeBase": null,
        "channels": 0,
        "channelLayout": null,
        "chapters": [],
        "metaTags": {},
        "mimeType": file.mime
    })
}

fn audio_track_json(file: &AudioFile, item_id: &str) -> Value {
    json!({
        "index": file.index,
        "startOffset": 0,
        "duration": file.duration,
        "title": file.title,
        "contentUrl": format!("/api/items/{item_id}/file/{}", file.index),
        "mimeType": file.mime,
        "metadata": file_metadata(file)
    })
}

fn library_files_json(item: &Item) -> Vec<Value> {
    let mut files = item
        .audio_files
        .iter()
        .map(|file| {
            json!({
                "ino": file.index.to_string(),
                "metadata": file_metadata(file),
                "addedAt": file.modified_ms,
                "updatedAt": file.modified_ms,
                "fileType": "audio"
            })
        })
        .collect::<Vec<_>>();
    if let Some(cover) = &item.cover_path {
        let metadata = fs_metadata(cover, item);
        files.push(json!({
            "ino": "cover",
            "metadata": metadata,
            "addedAt": item.added_ms,
            "updatedAt": item.updated_ms,
            "fileType": "image"
        }));
    }
    files
}

fn file_metadata(file: &AudioFile) -> Value {
    let filename = file
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    json!({
        "filename": filename,
        "ext": format!(".{}", extension(file)),
        "path": file.path.to_string_lossy(),
        "relPath": file.rel_path,
        "size": file.size,
        "mtimeMs": file.modified_ms,
        "ctimeMs": file.modified_ms,
        "birthtimeMs": file.modified_ms
    })
}

fn fs_metadata(path: &std::path::Path, item: &Item) -> Value {
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_owned();
    json!({
        "filename": filename,
        "ext": format!(".{extension}"),
        "path": path.to_string_lossy(),
        "relPath": format!("{}/{}", item.rel_path, filename).trim_start_matches('/').to_owned(),
        "size": std::fs::metadata(path).map(|metadata| metadata.len()).unwrap_or(0),
        "mtimeMs": item.updated_ms,
        "ctimeMs": item.added_ms,
        "birthtimeMs": item.added_ms
    })
}

fn extension(file: &AudioFile) -> String {
    file.path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn format_name(extension: &str) -> &'static str {
    match extension {
        "mp3" => "MP2/3 (MPEG audio layer 2/3)",
        "m4a" | "m4b" | "mp4" | "aac" => "MPEG-4 Audio",
        "flac" => "FLAC",
        "ogg" | "oga" | "opus" => "Ogg Vorbis",
        "wav" => "PCM WAV",
        "wma" => "Windows Media Audio",
        "aif" | "aiff" => "AIFF",
        _ => "Unknown",
    }
}

fn codec_name(extension: &str) -> &'static str {
    match extension {
        "mp3" => "mp3",
        "m4a" | "m4b" | "mp4" | "aac" => "aac",
        "flac" => "flac",
        "ogg" | "oga" => "vorbis",
        "opus" => "opus",
        "wav" => "pcm_s16le",
        "wma" => "wmav2",
        "aif" | "aiff" => "pcm_s16be",
        _ => "unknown",
    }
}

fn cover_path(item: &Item) -> String {
    format!("/api/items/{}/cover", item.id)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}
