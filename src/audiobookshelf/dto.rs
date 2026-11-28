use super::catalog::{AudioFile, BookChapter, Item, Library, SERVER_VERSION};
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
    pub(crate) start_time: f64,
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
    bookmarks: Vec<Value>,
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
        "bookmarks": bookmarks,
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
    let mut author_counts = std::collections::BTreeMap::<String, usize>::new();
    for item in &library.items {
        if item.metadata.authors.is_empty() {
            *author_counts.entry(item.author.clone()).or_default() += 1;
        } else {
            for author in &item.metadata.authors {
                *author_counts.entry(author.clone()).or_default() += 1;
            }
        }
    }
    let mut series_counts = std::collections::BTreeMap::<String, usize>::new();
    for item in &library.items {
        let mut seen = std::collections::BTreeSet::new();
        for label in &item.metadata.series {
            let (name, _) = super::scanner::split_series_label(label);
            if seen.insert(name.to_owned()) {
                *series_counts.entry(name.to_owned()).or_default() += 1;
            }
        }
    }
    json!({
        "library": library_json(library),
        "filterdata": {
            "authors": author_counts.iter().map(|(name, count)| author_json(name, &library.id, *count)).collect::<Vec<_>>(),
            "genres": unique_strings(
                library.items.iter().flat_map(|item| item.metadata.genres.iter().cloned())
            ),
            "tags": unique_strings(
                library.items.iter().flat_map(|item| item.metadata.tags.iter().cloned())
            ),
            "series": series_counts.iter().map(|(name, count)| series_filter_json(name, &library.id, *count)).collect::<Vec<_>>(),
            "narrators": unique_strings(
                library.items.iter().flat_map(|item| item.metadata.narrators.iter().cloned())
            ),
            "languages": unique_strings(library.items.iter().map(|item| {
                if item.metadata.language.is_empty() {
                    "en".to_owned()
                } else {
                    item.metadata.language.clone()
                }
            })),
            "publishers": unique_strings(library.items.iter().filter_map(|item| {
                (!item.metadata.publisher.is_empty()).then(|| item.metadata.publisher.clone())
            })),
            "publishedDecades": unique_strings(library.items.iter().filter_map(|item| {
                let date = if item.metadata.published_year.is_empty() {
                    &item.metadata.published_date
                } else {
                    &item.metadata.published_year
                };
                published_decade(date)
            })),
            "bookCount": library.items.len(),
            "authorCount": author_counts.len(),
            "seriesCount": series_counts.len(),
            "podcastCount": 0,
            "numIssues": 0,
            "loadedAt": now_ms()
        },
        "issues": 0,
        "numUserPlaylists": 0
    })
}

fn unique_strings<I, S>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut seen = std::collections::BTreeSet::new();
    values
        .into_iter()
        .map(|value| value.as_ref().to_owned())
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn published_decade(year: &str) -> Option<String> {
    let year = year.trim().get(0..4)?.parse::<u32>().ok()?;
    Some(format!("{}s", year / 10 * 10))
}

fn named_id(prefix: &str, name: &str) -> String {
    let mut slug = String::new();
    for character in name.to_ascii_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    format!(
        "{prefix}-{}",
        if slug.is_empty() { "unknown" } else { slug }
    )
}

pub(crate) fn author_json(name: &str, library_id: &str, num_books: usize) -> Value {
    json!({
        "id": author_id(name),
        "asin": null,
        "name": name,
        "description": null,
        "imagePath": null,
        "libraryId": library_id,
        "addedAt": 0,
        "updatedAt": 0,
        "numBooks": num_books
    })
}

pub(crate) fn author_id(name: &str) -> String {
    named_id("author", name)
}

pub(crate) fn series_id(name: &str) -> String {
    named_id("series", name)
}

fn series_json(value: &str, library_id: &str, num_books: usize) -> Value {
    let (name, sequence) = super::scanner::split_series_label(value);
    json!({
        "id": named_id("series", name),
        "name": name,
        "libraryId": library_id,
        "books": [],
        "numBooks": num_books,
        "sequence": sequence
    })
}

fn series_filter_json(name: &str, library_id: &str, num_books: usize) -> Value {
    json!({
        "id": series_id(name),
        "name": name,
        "libraryId": library_id,
        "numBooks": num_books
    })
}

fn chapter_json(chapter: &BookChapter) -> Value {
    json!({
        "id": chapter.id,
        "start": chapter.start,
        "end": chapter.end,
        "title": chapter.title
    })
}

pub(crate) fn item_json(item: &Item, minified: bool) -> Value {
    let book = &item.metadata;
    let authors = if book.authors.is_empty() {
        vec![author_json(&item.author, &item.library_id, 0)]
    } else {
        book.authors
            .iter()
            .map(|name| author_json(name, &item.library_id, 0))
            .collect::<Vec<_>>()
    };
    let language = if book.language.is_empty() {
        "en"
    } else {
        &book.language
    };
    let metadata = if minified {
        json!({
            "title": item.title,
            "titleIgnorePrefix": item.title,
            "subtitle": book.subtitle,
            "authorName": item.author,
            "authorNameLF": item.author,
            "narratorName": book.narrators.join(", "),
            "seriesName": book.series.join(", "),
            "genres": book.genres,
            "publishedYear": book.published_year,
            "publishedDate": book.published_date,
            "publisher": book.publisher,
            "description": item.description,
            "isbn": book.isbn,
            "asin": book.asin,
            "language": language,
            "explicit": book.explicit,
            "abridged": book.abridged
        })
    } else {
        json!({
            "title": item.title,
            "titleIgnorePrefix": item.title,
            "subtitle": book.subtitle,
            "authors": authors,
            "narrators": book.narrators,
            "series": book.series.iter().map(|name| series_json(name, &item.library_id, 0)).collect::<Vec<_>>(),
            "genres": book.genres,
            "description": item.description,
            "publishedYear": book.published_year,
            "publishedDate": book.published_date,
            "publisher": book.publisher,
            "language": language,
            "explicit": book.explicit,
            "abridged": book.abridged,
            "isbn": book.isbn,
            "asin": book.asin
        })
    };
    let chapters = item.chapters();
    let mut media = json!({
        "id": format!("me_{}", item.id.trim_start_matches("li_")),
        "libraryItemId": item.id,
        "metadata": metadata,
        "coverPath": cover_path(item),
        "tags": book.tags,
        "numTracks": item.audio_files.len().max(1),
        "numAudioFiles": item.audio_files.len().max(1),
        "numChapters": chapters.len(),
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
        media["tracks"] = json!(audio_tracks_json(item));
        media["chapters"] = json!(chapters.iter().map(chapter_json).collect::<Vec<_>>());
    }
    let mut result = json!({
        "id": item.id,
        "ino": "0",
        "oldLibraryItemId": null,
        "libraryId": item.library_id,
        "folderId": item.folder_id,
        "path": item.path.to_string_lossy(),
        "relPath": item.rel_path,
        "isFile": item.is_file,
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
    let mut audio_tracks = audio_tracks_json(item);
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
        "chapters": detail["media"]["chapters"].clone(),
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
        "startTime": session.start_time,
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
        "trackNumFromFilename": file.track_number,
        "discNumFromFilename": file.disc_number,
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

fn audio_tracks_json(item: &Item) -> Vec<Value> {
    let mut start_offset = 0.0;
    item.audio_files
        .iter()
        .map(|file| {
            let track = audio_track_json(file, &item.id, start_offset);
            start_offset += file.duration.max(0.0);
            track
        })
        .collect()
}

fn audio_track_json(file: &AudioFile, item_id: &str, start_offset: f64) -> Value {
    json!({
        "index": file.index,
        "startOffset": start_offset,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audiobookshelf::catalog::BookMetadata;
    use std::path::PathBuf;

    fn audio_file(index: usize, duration: f64) -> AudioFile {
        AudioFile {
            index,
            path: PathBuf::from(format!("/tmp/{index}.mp3")),
            rel_path: format!("Book/{index}.mp3"),
            title: format!("Track {index}"),
            mime: "audio/mpeg".to_owned(),
            size: 0,
            modified_ms: 0,
            duration,
            track_number: None,
            disc_number: None,
        }
    }

    #[test]
    fn audio_track_offsets_are_cumulative() {
        let item = Item {
            id: "li_test".to_owned(),
            library_id: "lib_test".to_owned(),
            folder_id: "fol_test".to_owned(),
            path: PathBuf::from("/tmp/Book"),
            rel_path: "Book".to_owned(),
            is_file: false,
            title: "Book".to_owned(),
            author: "Author".to_owned(),
            description: String::new(),
            cover_path: None,
            cover_version: None,
            audio_files: vec![audio_file(1, 10.0), audio_file(2, 20.5)],
            duration: 30.5,
            size: 0,
            added_ms: 0,
            updated_ms: 0,
            metadata: BookMetadata::default(),
            metadata_present: true,
        };
        let tracks = audio_tracks_json(&item);
        assert_eq!(tracks[0]["startOffset"], 0.0);
        assert_eq!(tracks[0]["duration"], 10.0);
        assert_eq!(tracks[1]["startOffset"], 10.0);
        assert_eq!(tracks[1]["duration"], 20.5);
        assert_eq!(item_json(&item, false)["media"]["duration"], 30.5);
    }
}
