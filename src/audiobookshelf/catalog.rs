use super::scanner::{self, BookFiles, ParsedBookPath};
use crate::harbinger::{database_media_key, is_audiobook_library, is_hidden_path, AppState};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(crate) const SERVER_VERSION: &str = "2.35.0";
const CATALOG_TTL: Duration = Duration::from_secs(60);

#[derive(Clone, Debug)]
pub(crate) struct AudioFile {
    pub(crate) index: usize,
    pub(crate) path: PathBuf,
    pub(crate) rel_path: String,
    pub(crate) title: String,
    pub(crate) mime: String,
    pub(crate) size: u64,
    pub(crate) modified_ms: i64,
    pub(crate) duration: f64,
    pub(crate) track_number: Option<u32>,
    pub(crate) disc_number: Option<u32>,
}

#[derive(Clone, Debug)]
pub(crate) struct BookChapter {
    pub(crate) id: usize,
    pub(crate) title: String,
    pub(crate) start: f64,
    pub(crate) end: f64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct BookMetadata {
    pub(crate) title: Option<String>,
    pub(crate) subtitle: String,
    pub(crate) authors: Vec<String>,
    pub(crate) narrators: Vec<String>,
    pub(crate) series: Vec<String>,
    pub(crate) genres: Vec<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) published_year: String,
    pub(crate) published_date: String,
    pub(crate) publisher: String,
    pub(crate) description: String,
    pub(crate) isbn: String,
    pub(crate) asin: String,
    pub(crate) language: String,
    pub(crate) explicit: bool,
    pub(crate) abridged: bool,
    pub(crate) chapters: Vec<BookChapter>,
}

#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub(crate) id: String,
    pub(crate) library_id: String,
    pub(crate) folder_id: String,
    pub(crate) path: PathBuf,
    pub(crate) rel_path: String,
    pub(crate) is_file: bool,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) description: String,
    pub(crate) cover_path: Option<PathBuf>,
    pub(crate) audio_files: Vec<AudioFile>,
    pub(crate) duration: f64,
    pub(crate) size: u64,
    pub(crate) added_ms: i64,
    pub(crate) updated_ms: i64,
    pub(crate) metadata: BookMetadata,
}

impl Item {
    pub(crate) fn chapters(&self) -> Vec<BookChapter> {
        if !self.metadata.chapters.is_empty() {
            return self.metadata.chapters.clone();
        }
        let mut start = 0.0;
        self.audio_files
            .iter()
            .enumerate()
            .map(|(id, file)| {
                let chapter = BookChapter {
                    id,
                    title: file.title.clone(),
                    start,
                    end: start + file.duration,
                };
                start += file.duration;
                chapter
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
struct CoverSettings {
    config_mode: bool,
    config_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct Folder {
    pub(crate) id: String,
    pub(crate) full_path: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Library {
    pub(crate) id: String,
    pub(crate) source_index: usize,
    pub(crate) name: String,
    pub(crate) folders: Vec<Folder>,
    pub(crate) items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub(crate) struct Catalog {
    pub(crate) libraries: Vec<Library>,
}

#[derive(Clone)]
struct CacheEntry {
    key: String,
    created: Instant,
    catalog: Catalog,
}

static CATALOG_CACHE: OnceLock<Mutex<Option<CacheEntry>>> = OnceLock::new();

impl Catalog {
    pub(crate) fn load(state: &AppState) -> Catalog {
        let config = state
            .config
            .lock()
            .map(|config| (*config).clone())
            .unwrap_or_else(|poison| (*poison.into_inner()).clone());
        let key = config
            .libraries
            .iter()
            .filter(|library| is_audiobook_library(&library.kind))
            .map(|library| {
                let folders = library
                    .folders
                    .iter()
                    .map(|folder| canonical_or_absolute(folder).to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{}|{}|{}", library.name, library.kind, folders)
            })
            .collect::<Vec<_>>()
            .join(";");
        let cache = CATALOG_CACHE.get_or_init(Default::default);
        if let Ok(guard) = cache.lock() {
            if let Some(entry) = guard.as_ref() {
                if entry.key == key && entry.created.elapsed() < CATALOG_TTL {
                    return entry.catalog.clone();
                }
            }
        }
        let cover_settings = CoverSettings {
            config_mode: config.metadata_storage == "config",
            config_dir: state
                .subtitle_checkpoints_dir
                .parent()
                .unwrap_or(state.subtitle_checkpoints_dir.as_path())
                .to_path_buf(),
        };
        let catalog = build_catalog(&config.libraries, &cover_settings);
        if let Ok(mut guard) = cache.lock() {
            *guard = Some(CacheEntry {
                key,
                created: Instant::now(),
                catalog: catalog.clone(),
            });
        }
        catalog
    }

    pub(crate) fn default_library_id(&self) -> String {
        self.libraries
            .first()
            .map(|library| library.id.clone())
            .unwrap_or_else(|| "lib_harbinger".to_owned())
    }

    pub(crate) fn find_library(&self, id: &str) -> Option<&Library> {
        self.libraries.iter().find(|library| library.id == id)
    }

    pub(crate) fn find_item(&self, id: &str) -> Option<&Item> {
        self.libraries
            .iter()
            .flat_map(|library| library.items.iter())
            .find(|item| item.id == id)
    }

    pub(crate) fn item_library(&self, item: &Item) -> Option<&Library> {
        self.libraries
            .iter()
            .find(|library| library.id == item.library_id)
    }

    #[cfg(test)]
    pub(crate) fn clear_cache() {
        invalidate_cache();
    }
}

pub(crate) fn invalidate_cache() {
    if let Ok(mut guard) = CATALOG_CACHE.get_or_init(Default::default).lock() {
        *guard = None;
    }
}

pub(crate) fn count_audio_files(library: &crate::harbinger::ConfigLibrary) -> usize {
    let mut files = BTreeSet::new();
    for root in &library.folders {
        let root = canonical_or_absolute(root);
        let mut directories = Vec::new();
        collect_directories(&root, &mut directories);
        directories.insert(0, root);
        for directory in directories {
            let Ok(entries) = fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && audio_mime(&path).is_some() {
                    files.insert(path);
                }
            }
        }
    }
    files.len()
}

fn build_catalog(
    config_libraries: &[crate::harbinger::ConfigLibrary],
    cover_settings: &CoverSettings,
) -> Catalog {
    let libraries = config_libraries
        .iter()
        .enumerate()
        .filter(|(_, configured)| is_audiobook_library(&configured.kind))
        .map(|(index, configured)| {
            let roots = configured
                .folders
                .iter()
                .map(|folder| canonical_or_absolute(folder))
                .collect::<Vec<_>>();
            let id = stable_id("lib", &format!("{}|{}", index, configured.name));
            let folders = roots
                .iter()
                .enumerate()
                .map(|(folder_index, root)| Folder {
                    id: stable_id("fol", &format!("{id}|{folder_index}|{}", root.display())),
                    full_path: root.to_string_lossy().into_owned(),
                })
                .collect::<Vec<_>>();
            let folder_id = folders
                .first()
                .map(|folder| folder.id.clone())
                .unwrap_or_else(|| stable_id("fol", &id));
            let mut items = Vec::new();
            for (folder_index, root) in roots.iter().enumerate() {
                for book in scanner::scan_library(root) {
                    if let Some(item) = build_item(
                        book,
                        &id,
                        folders
                            .get(folder_index)
                            .map(|folder| folder.id.as_str())
                            .unwrap_or(&folder_id),
                        index,
                        cover_settings,
                    ) {
                        items.push(item);
                    }
                }
            }
            let mut seen_paths = BTreeSet::new();
            items.retain(|item| seen_paths.insert(item.path.clone()));
            items.sort_by(|left, right| {
                left.title
                    .cmp(&right.title)
                    .then_with(|| left.id.cmp(&right.id))
            });
            Library {
                id,
                source_index: index,
                name: configured.name.clone(),
                folders,
                items,
            }
        })
        .collect();
    Catalog { libraries }
}

fn build_item(
    book: BookFiles,
    library_id: &str,
    folder_id: &str,
    source_index: usize,
    cover_settings: &CoverSettings,
) -> Option<Item> {
    if book.audio_files.is_empty() {
        return None;
    }
    let id = stable_id("li", &format!("{}|{}", library_id, book.path.display()));
    let parsed = if book.is_file {
        ParsedBookPath {
            title: Some(scanner::file_stem(&book.path)),
            ..Default::default()
        }
    } else {
        scanner::parse_book_path(&book.rel_path, false)
    };
    let mut audio_files = book
        .audio_files
        .iter()
        .map(|file| {
            let mime = audio_mime(&file.path)?;
            let metadata = fs::metadata(&file.path).ok()?;
            let (track_number, disc_number) = scanner::filename_track_numbers(&parsed, file);
            Some(AudioFile {
                index: 0,
                path: file.path.clone(),
                rel_path: file.rel_path.clone(),
                title: scanner::file_stem(&file.path),
                mime: mime.to_owned(),
                size: metadata.len(),
                modified_ms: timestamp_ms(metadata.modified().ok()),
                duration: sidecar_duration(&file.path).unwrap_or(0.0),
                track_number,
                disc_number,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    sort_audio_files(&mut audio_files);

    let mut metadata = if book.is_file {
        BookMetadata::default()
    } else {
        read_book_metadata(&book.path.join("metadata.json"))
    };
    let fallback_title = if book.is_file {
        scanner::file_stem(&book.path)
    } else {
        book.path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Audiobook")
            .to_owned()
    };
    let folder_title = parsed.title.clone().unwrap_or(fallback_title);
    if metadata
        .title
        .as_deref()
        .is_some_and(|title| title.trim().is_empty())
    {
        metadata.title = None;
    }
    if metadata.authors.is_empty() {
        metadata.authors = parsed.authors.clone();
    }
    if metadata.narrators.is_empty() && !book.is_file {
        metadata.narrators = read_reader_txt(&book.path);
    }
    if metadata.narrators.is_empty() {
        metadata.narrators = parsed.narrators.clone();
    }
    if metadata.series.is_empty() {
        if let Some(series) = parsed.series.as_deref() {
            metadata.series = vec![scanner::series_label(series, parsed.sequence.as_deref())];
        }
    }
    if metadata.published_year.is_empty() {
        metadata.published_year = parsed.published_year.clone().unwrap_or_default();
    }
    if metadata.asin.is_empty() {
        metadata.asin = parsed.asin.clone().unwrap_or_default();
    }
    if metadata.description.is_empty() && !book.is_file {
        metadata.description = read_desc_txt(&book.path);
    }
    let title = metadata.title.clone().unwrap_or(folder_title);
    let author = if metadata.authors.is_empty() {
        "Unknown".to_owned()
    } else {
        metadata.authors.join(", ")
    };
    let cover_path = resolve_cover(&book, &id, source_index, cover_settings);
    let duration = audio_files.iter().map(|file| file.duration).sum();
    let size = audio_files.iter().map(|file| file.size).sum();
    let added_ms = audio_files
        .iter()
        .map(|file| file.modified_ms)
        .min()
        .unwrap_or_default();
    let updated_ms = audio_files
        .iter()
        .map(|file| file.modified_ms)
        .max()
        .unwrap_or_default();
    Some(Item {
        id,
        library_id: library_id.to_owned(),
        folder_id: folder_id.to_owned(),
        path: book.path,
        rel_path: book.rel_path,
        is_file: book.is_file,
        title,
        author,
        description: metadata.description.clone(),
        cover_path,
        audio_files,
        duration,
        size,
        added_ms,
        updated_ms,
        metadata,
    })
}

fn resolve_cover(
    book: &BookFiles,
    item_id: &str,
    source_index: usize,
    settings: &CoverSettings,
) -> Option<PathBuf> {
    let store = cover_store_dir(settings, source_index, item_id, &book.path, book.is_file);
    if let Some(cover) = scanner::stored_cover(&store) {
        return Some(cover);
    }
    let fallback_store = cover_store_dir(
        &CoverSettings {
            config_mode: !settings.config_mode,
            config_dir: settings.config_dir.clone(),
        },
        source_index,
        item_id,
        &book.path,
        book.is_file,
    );
    if let Some(cover) = scanner::stored_cover(&fallback_store) {
        return Some(cover);
    }
    if let Some(file) = book.audio_files.first() {
        if let Some((bytes, extension)) = scanner::embedded_cover(&file.path) {
            if let Some(cover) = write_stored_cover(&store, &bytes, extension) {
                return Some(cover);
            }
        }
    }
    let directory = if book.is_file {
        book.path.parent()?
    } else {
        book.path.as_path()
    };
    if let Some(cover) = scanner::cover_image(directory, "cover") {
        return Some(cover);
    }
    let filename_stem = if book.is_file {
        scanner::file_stem(&book.path)
    } else {
        book.path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned()
    };
    if let Some(cover) = scanner::cover_image(directory, &filename_stem) {
        return Some(cover);
    }
    if !book.is_file {
        if let Some(title) = scanner::parse_book_path(&book.rel_path, false).title {
            if title != filename_stem {
                if let Some(cover) = scanner::cover_image(directory, &title) {
                    return Some(cover);
                }
            }
        }
    }
    scanner::first_image(directory)
}

fn cover_store_dir(
    settings: &CoverSettings,
    source_index: usize,
    item_id: &str,
    item_path: &Path,
    is_file: bool,
) -> PathBuf {
    if settings.config_mode {
        settings
            .config_dir
            .join(".hbgr")
            .join(database_media_key(source_index + 1, item_id))
    } else if is_file {
        item_path
            .parent()
            .unwrap_or(item_path)
            .join(".hbgr")
            .join(item_id)
    } else {
        item_path.join(".hbgr")
    }
}

pub(crate) fn item_cover_store_dir(state: &AppState, source_index: usize, item: &Item) -> PathBuf {
    let config_mode = state
        .config
        .lock()
        .is_ok_and(|config| config.metadata_storage == "config");
    cover_store_dir(
        &CoverSettings {
            config_mode,
            config_dir: state
                .subtitle_checkpoints_dir
                .parent()
                .unwrap_or(state.subtitle_checkpoints_dir.as_path())
                .to_path_buf(),
        },
        source_index,
        &item.id,
        &item.path,
        item.is_file,
    )
}

pub(crate) fn write_cover_file(directory: &Path, bytes: &[u8], extension: &str) -> Option<PathBuf> {
    fs::create_dir_all(directory).ok()?;
    for existing in ["jpg", "jpeg", "png", "webp"] {
        let path = directory.join(format!("cover.{existing}"));
        if path.is_file() {
            fs::remove_file(path).ok();
        }
    }
    let path = directory.join(format!("cover.{extension}"));
    fs::write(&path, bytes).ok()?;
    Some(path)
}

fn write_stored_cover(directory: &Path, bytes: &[u8], extension: &str) -> Option<PathBuf> {
    write_cover_file(directory, bytes, extension)
}

fn sort_audio_files(audio_files: &mut [AudioFile]) {
    let all_discs =
        !audio_files.is_empty() && audio_files.iter().all(|file| file.disc_number.is_some());
    let disc_order = if all_discs {
        let mut discs = audio_files
            .iter()
            .filter_map(|file| file.disc_number)
            .collect::<Vec<_>>();
        discs.sort_unstable();
        discs.windows(2).all(|window| window[1] - window[0] <= 1)
    } else {
        false
    };
    if disc_order {
        audio_files.sort_by(|left, right| {
            left.disc_number
                .unwrap_or_default()
                .cmp(&right.disc_number.unwrap_or_default())
                .then_with(|| {
                    left.track_number
                        .unwrap_or_default()
                        .cmp(&right.track_number.unwrap_or_default())
                })
                .then_with(|| left.rel_path.cmp(&right.rel_path))
        });
    } else {
        audio_files.sort_by(|left, right| {
            left.track_number
                .unwrap_or_default()
                .cmp(&right.track_number.unwrap_or_default())
                .then_with(|| left.rel_path.cmp(&right.rel_path))
        });
    }
    for (index, file) in audio_files.iter_mut().enumerate() {
        file.index = index + 1;
    }
}

fn read_desc_txt(directory: &Path) -> String {
    fs::read_to_string(directory.join("desc.txt"))
        .map(|description| description.trim().to_owned())
        .unwrap_or_default()
}

fn read_reader_txt(directory: &Path) -> Vec<String> {
    let Ok(raw) = fs::read_to_string(directory.join("reader.txt")) else {
        return Vec::new();
    };
    raw.lines()
        .next()
        .map(scanner::parse_names)
        .unwrap_or_default()
}

fn read_book_metadata(path: &Path) -> BookMetadata {
    let Ok(raw) = fs::read_to_string(path) else {
        return BookMetadata::default();
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return BookMetadata::default();
    };
    let mut merged = parsed.clone();
    if let Some(legacy) = parsed.get("metadata").and_then(Value::as_object) {
        if let Some(target) = merged.as_object_mut() {
            for (key, value) in legacy {
                target.insert(key.clone(), value.clone());
            }
        }
    }
    let string = |key: &str| -> String {
        match merged.get(key) {
            Some(Value::String(value)) => value.clone(),
            Some(Value::Number(value)) => value.to_string(),
            _ => String::new(),
        }
    };
    let string_array = |key: &str| -> Vec<String> {
        merged
            .get(key)
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let boolean = |key: &str| -> bool {
        merged
            .get(key)
            .and_then(|value| match value {
                Value::Bool(value) => Some(*value),
                Value::String(value) => match value.to_ascii_lowercase().as_str() {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or(false)
    };
    let chapters = merged
        .get("chapters")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .enumerate()
                .filter_map(|(index, value)| {
                    let title = value.get("title")?.as_str()?.to_owned();
                    let start = number_value(value.get("start"))?;
                    let end = number_value(value.get("end"))?;
                    let id = number_value(value.get("id"))
                        .map(|id| id.round().max(0.0) as usize)
                        .unwrap_or(index);
                    Some(BookChapter {
                        id,
                        title,
                        start,
                        end,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    BookMetadata {
        title: {
            let title = string("title");
            (!title.trim().is_empty()).then_some(title)
        },
        subtitle: string("subtitle"),
        authors: string_array("authors"),
        narrators: string_array("narrators"),
        series: string_array("series"),
        genres: string_array("genres"),
        tags: string_array("tags"),
        published_year: string("publishedYear"),
        published_date: string("publishedDate"),
        publisher: string("publisher"),
        description: string("description"),
        isbn: string("isbn"),
        asin: string("asin"),
        language: string("language"),
        explicit: boolean("explicit"),
        abridged: boolean("abridged"),
        chapters,
    }
}

fn number_value(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    }
}

fn collect_directories(root: &Path, result: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() || is_hidden_path(&entry.path()) {
            continue;
        }
        if file_type.is_dir() {
            result.push(entry.path());
            collect_directories(&entry.path(), result);
        }
    }
}

fn canonical_or_absolute(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        }
    })
}

pub(crate) fn audio_mime(path: &Path) -> Option<&'static str> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())?
        .to_ascii_lowercase();
    match extension.as_str() {
        "mp3" => Some("audio/mpeg"),
        "m4a" | "m4b" | "mp4" | "aac" => Some("audio/mp4"),
        "flac" => Some("audio/flac"),
        "ogg" | "oga" => Some("audio/ogg"),
        "opus" => Some("audio/opus"),
        "wav" => Some("audio/wav"),
        "wma" => Some("audio/x-ms-wma"),
        "aif" | "aiff" => Some("audio/aiff"),
        _ => None,
    }
}

pub(crate) fn image_content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "image/jpeg",
    }
}

fn sidecar_duration(path: &Path) -> Option<f64> {
    let sidecar = path.parent()?.join(".hbgr/data.json");
    let raw = fs::read_to_string(sidecar).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let tracks = value["technical"]["media"]["track"].as_array()?;
    tracks.iter().find_map(|track| {
        let duration = track["Duration"].as_str()?;
        parse_duration(duration)
    })
}

fn parse_duration(value: &str) -> Option<f64> {
    if let Ok(milliseconds) = value.parse::<f64>() {
        return Some(milliseconds / 1000.0);
    }
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut seconds = 0.0;
    for part in parts {
        seconds = seconds * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(seconds)
}

fn timestamp_ms(time: Option<SystemTime>) -> i64 {
    time.and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

fn stable_id(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let encoded = format!("{digest:x}");
    format!("{prefix}_{}", &encoded[..24])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_metadata_json_falls_back_to_defaults() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "mplex-invalid-book-metadata-{}-{nonce}.json",
            std::process::id()
        ));
        fs::write(&path, "{ not valid json").unwrap();
        let metadata = read_book_metadata(&path);
        fs::remove_file(&path).unwrap();

        assert!(metadata.title.is_none());
        assert!(metadata.authors.is_empty());
        assert!(metadata.narrators.is_empty());
        assert!(metadata.chapters.is_empty());
        assert!(metadata.description.is_empty());
    }

    const EMBEDDED_COVER_MP3: &str = "SUQzAwAAAAACLFRTU0UAAAAPAAAATGF2ZjYzLjE2LjEwMABBUElDAAAA/wAAAGltYWdlL2pwZWcAA0FsYnVtIGNvdmVyAP/Y/+AAEEpGSUYAAQIAAAEAAQAA//4AEExhdmM2My4yNC4xMDAA/9sAQwAIBAQEBAQFBQUFBQUGBgYGBgYGBgYGBgYGBwcHCAgIBwcHBgYHBwgICAgJCQkICAgICQkKCgoMDAsLDg4OEREU/8QATQABAQAAAAAAAAAAAAAAAAAAAAYBAQEBAAAAAAAAAAAAAAAAAAAGBxABAAAAAAAAAAAAAAAAAAAAABEBAAAAAAAAAAAAAAAAAAAAAP/AABEIACAAIAMBIgACEQADEQD/2gAMAwEAAhEDEQA/AIsBJt/AAAAAAf/ZAAAAAAAAAAAAAP/jOMQAAAAAAAAAAABJbmZvAAAADwAAAAQAAAH4AJKSkpKSkpKSkpKSkpKSkpKSkpKSkpKSkra2tra2tra2tra2tra2tra2tra2tra2trbb29vb29vb29vb29vb29vb29vb29vb29vb/////////////////////////////////wAAAABMYXZjNjMuMjQAAAAAAAAAAAAAAAAC86AAAAAAAAAB+KYmbGoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAP/jGMQACviOxblDAAKqBSi20B3d3dz+IiIn7u7v/9cDAwNzUEDiIPg+/BB13+GOA36Qxy7+c6fd0iaHOFnEN/xlRCUcoQt/g//jGMQPEJlCkAGUUABvB+oHuQAZcRJwD4mA5kdYHeph74BoAkAqKwiguv/yEeiqRD4fGm//+QoDQlCQNf+JToNV5Xt8lMtmpf/jGMQHDnkKoAGZQAA///T29xuWx59Yr//8+/5w9MEX0IAzIJBwOv/2DgyEoBITg6LWT//CVCejWSxVn/OlZ1ZMQU1FNC4wqv/jGMQIAAADSAHAAACqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqg==";

    fn catalog_temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mplex-book-catalog-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn cover_settings(root: &Path) -> CoverSettings {
        CoverSettings {
            config_mode: false,
            config_dir: root.join("config"),
        }
    }

    fn ffmpeg_available() -> bool {
        let ffmpeg = std::env::var("HARBINGER_FFMPEG").unwrap_or_else(|_| "ffmpeg".to_owned());
        std::process::Command::new(ffmpeg)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn embedded_cover_precedes_folder_images() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};

        if !ffmpeg_available() {
            eprintln!("skipping embedded cover test because ffmpeg is unavailable");
            return;
        }
        let root = catalog_temp_dir("embedded-cover");
        let book_dir = root.join("Author").join("Book");
        fs::create_dir_all(&book_dir).unwrap();
        fs::write(
            book_dir.join("Chapter 01.mp3"),
            STANDARD.decode(EMBEDDED_COVER_MP3).unwrap(),
        )
        .unwrap();
        fs::write(book_dir.join("cover.jpg"), b"folder-cover").unwrap();

        let books = scanner::scan_library(&root);
        assert_eq!(books.len(), 1);
        let cover = resolve_cover(&books[0], "li_embedded", 0, &cover_settings(&root))
            .expect("embedded cover is extracted");
        assert!(cover.starts_with(book_dir.join(".hbgr")));
        let bytes = fs::read(&cover).unwrap();
        assert_eq!(
            bytes,
            scanner::embedded_cover(&book_dir.join("Chapter 01.mp3"))
                .unwrap()
                .0
        );
        assert_ne!(bytes, b"folder-cover");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cover_images_fall_back_from_cover_stem_to_filename_stem_and_any_image() {
        let root = catalog_temp_dir("cover-order");
        let book_dir = root.join("Author").join("Named Book");
        fs::create_dir_all(&book_dir).unwrap();
        fs::write(book_dir.join("Chapter 01.mp3"), b"not real audio").unwrap();
        fs::write(book_dir.join("zzz.jpg"), b"any-image").unwrap();
        fs::write(book_dir.join("Named Book.jpg"), b"filename-image").unwrap();

        let books = scanner::scan_library(&root);
        let settings = cover_settings(&root);
        let cover = resolve_cover(&books[0], "li_named", 0, &settings).unwrap();
        assert_eq!(cover, book_dir.join("Named Book.jpg"));

        fs::write(book_dir.join("cover.png"), b"cover-image").unwrap();
        let cover = resolve_cover(&books[0], "li_named", 0, &settings).unwrap();
        assert_eq!(cover, book_dir.join("cover.png"));

        fs::remove_file(book_dir.join("cover.png")).unwrap();
        fs::remove_file(book_dir.join("Named Book.jpg")).unwrap();
        let cover = resolve_cover(&books[0], "li_named", 0, &settings).unwrap();
        assert_eq!(cover, book_dir.join("zzz.jpg"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stored_cover_overrides_automatic_cover_sources() {
        let root = catalog_temp_dir("stored-cover");
        let book_dir = root.join("Author").join("Book");
        fs::create_dir_all(&book_dir).unwrap();
        fs::write(book_dir.join("Chapter 01.mp3"), b"not real audio").unwrap();
        fs::write(book_dir.join("cover.jpg"), b"folder-cover").unwrap();
        let store = book_dir.join(".hbgr");
        write_cover_file(&store, b"custom-cover", "png").unwrap();

        let books = scanner::scan_library(&root);
        let cover = resolve_cover(&books[0], "li_stored", 0, &cover_settings(&root)).unwrap();
        assert_eq!(cover, store.join("cover.png"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn metadata_json_accepts_legacy_nested_metadata() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "mplex-nested-book-metadata-{}-{nonce}.json",
            std::process::id()
        ));
        fs::write(
            &path,
            serde_json::json!({
                "metadata": {
                    "title": "Nested Title",
                    "authors": ["Nested Author"],
                    "publishedYear": 2022,
                    "chapters": [{"id": 7, "title": "Opening", "start": 0.0, "end": 10.0}]
                }
            })
            .to_string(),
        )
        .unwrap();
        let metadata = read_book_metadata(&path);
        fs::remove_file(&path).unwrap();

        assert_eq!(metadata.title.as_deref(), Some("Nested Title"));
        assert_eq!(
            metadata.authors.first().map(String::as_str),
            Some("Nested Author")
        );
        assert_eq!(metadata.published_year, "2022");
        assert_eq!(metadata.chapters[0].id, 7);
    }
}
