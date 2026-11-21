use crate::harbinger::{is_audiobook_library, is_hidden_path, AppState};
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
}

#[derive(Clone, Debug)]
pub(crate) struct Item {
    pub(crate) id: String,
    pub(crate) library_id: String,
    pub(crate) folder_id: String,
    pub(crate) path: PathBuf,
    pub(crate) rel_path: String,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) description: String,
    pub(crate) cover_path: Option<PathBuf>,
    pub(crate) audio_files: Vec<AudioFile>,
    pub(crate) duration: f64,
    pub(crate) size: u64,
    pub(crate) added_ms: i64,
    pub(crate) updated_ms: i64,
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
        let catalog = build_catalog(&config.libraries);
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

fn build_catalog(config_libraries: &[crate::harbinger::ConfigLibrary]) -> Catalog {
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
                let mut directories = Vec::new();
                collect_directories(root, &mut directories);
                directories.insert(0, root.clone());
                let mut seen = BTreeSet::new();
                for directory in directories {
                    if !seen.insert(directory.clone()) {
                        continue;
                    }
                    if let Some(item) = build_item(
                        &directory,
                        root,
                        &id,
                        folders
                            .get(folder_index)
                            .map(|folder| folder.id.as_str())
                            .unwrap_or(&folder_id),
                    ) {
                        items.push(item);
                    }
                }
            }
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
    directory: &Path,
    library_root: &Path,
    library_id: &str,
    folder_id: &str,
) -> Option<Item> {
    let mut audio_files = Vec::new();
    let mut cover_path = None;
    for entry in fs::read_dir(directory).ok()?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(mime) = audio_mime(&path) {
            let metadata = fs::metadata(&path).ok()?;
            let title = path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("Audio")
                .to_owned();
            let rel_path = path
                .strip_prefix(library_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            let index = audio_files.len() + 1;
            audio_files.push(AudioFile {
                index,
                path: path.clone(),
                rel_path,
                title,
                mime: mime.to_owned(),
                size: metadata.len(),
                modified_ms: timestamp_ms(metadata.modified().ok()),
                duration: sidecar_duration(&path).unwrap_or(0.0),
            });
        } else if cover_path.is_none() && is_cover(&path) {
            cover_path = Some(path);
        }
    }
    if audio_files.is_empty() {
        return None;
    }
    if cover_path.is_none() {
        cover_path = [".hbgr/cover.jpg", ".hbgr/cover.jpeg", ".hbgr/cover.png"]
            .iter()
            .map(|name| directory.join(name))
            .find(|path| path.is_file());
    }
    audio_files.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
    for (index, file) in audio_files.iter_mut().enumerate() {
        file.index = index + 1;
    }
    let directory_name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Audiobook")
        .to_owned();
    let title = if directory == library_root {
        audio_files[0].title.clone()
    } else {
        directory_name
    };
    let author = if directory == library_root {
        "Unknown".to_owned()
    } else {
        directory
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown")
            .to_owned()
    };
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
    let id = stable_id("li", &format!("{}|{}", library_id, directory.display()));
    Some(Item {
        id,
        library_id: library_id.to_owned(),
        folder_id: folder_id.to_owned(),
        path: directory.to_path_buf(),
        rel_path: directory
            .strip_prefix(library_root)
            .unwrap_or(directory)
            .to_string_lossy()
            .into_owned(),
        title,
        author,
        description: String::new(),
        cover_path,
        audio_files,
        duration,
        size,
        added_ms,
        updated_ms,
    })
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

fn is_cover(path: &Path) -> bool {
    matches!(
        path.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("cover.jpg")
            | Some("cover.jpeg")
            | Some("cover.png")
            | Some("folder.jpg")
            | Some("folder.png")
    )
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
