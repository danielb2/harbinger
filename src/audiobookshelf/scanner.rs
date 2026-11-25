//! Audiobookshelf-compatible book library scanning.
//!
//! This follows the documented book library directory structure:
//! `{Author}/{Series}/{Book}`, `{Author}/{Book}`, and book folders at the
//! library root. Media files in the library root are single-file books.
//! Disc directories (`Disc 1`, `CD 2`, `Disk 3`) belong to their parent book.
//!
//! Reference: <https://audiobookshelf.org/docs/documentation/libraries/book-library/directory-structure>

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const IGNORED_EXTENSIONS: &[&str] = &[
    ".part",
    ".tmp",
    ".crdownload",
    ".download",
    ".bak",
    ".old",
    ".temp",
    ".tempfile",
    ".tempfile~",
];

const AUDIO_EXTENSIONS: &[&str] = &[
    "m4b", "mp3", "m4a", "flac", "opus", "ogg", "oga", "mp4", "aac", "wma", "aiff", "aif", "wav",
    "webm", "webma", "mka", "awb", "caf", "mpg", "mpeg",
];

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

#[derive(Clone, Debug)]
pub(crate) struct ScannedFile {
    pub(crate) path: PathBuf,
    pub(crate) rel_path: String,
}

#[derive(Clone, Debug)]
pub(crate) struct BookFiles {
    pub(crate) path: PathBuf,
    pub(crate) rel_path: String,
    pub(crate) is_file: bool,
    pub(crate) audio_files: Vec<ScannedFile>,
    pub(crate) other_files: Vec<ScannedFile>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ParsedBookPath {
    pub(crate) title: Option<String>,
    pub(crate) subtitle: Option<String>,
    pub(crate) authors: Vec<String>,
    pub(crate) narrators: Vec<String>,
    pub(crate) series: Option<String>,
    pub(crate) sequence: Option<String>,
    pub(crate) published_year: Option<String>,
    pub(crate) asin: Option<String>,
}

/// Scan a library folder into Audiobookshelf book items.
pub(crate) fn scan_library(library_root: &Path) -> Vec<BookFiles> {
    let files = collect_files(library_root);
    let mut directories = BTreeSet::new();
    let mut direct_audio = BTreeSet::new();
    for file in &files {
        let Some((directory, _)) = file.rel_path.rsplit_once('/') else {
            continue;
        };
        let mut current = Some(directory);
        while let Some(path) = current {
            directories.insert(path.to_owned());
            current = path.rsplit_once('/').map(|(parent, _)| parent);
        }
        if is_audio_filename(&file.rel_path) {
            direct_audio.insert(directory.to_owned());
        }
    }

    let mut ordered_directories = directories.iter().collect::<Vec<_>>();
    ordered_directories.sort_by(|left, right| {
        left.matches('/')
            .count()
            .cmp(&right.matches('/').count())
            .then_with(|| left.cmp(right))
    });

    let mut roots: Vec<String> = Vec::new();
    for directory in ordered_directories {
        if roots.iter().any(|root| is_inside(directory, root)) {
            continue;
        }
        if direct_audio.contains(directory.as_str()) {
            roots.push(directory.to_string());
            continue;
        }
        let has_disc_directory = direct_audio.iter().any(|audio_directory| {
            audio_directory
                .rsplit_once('/')
                .is_some_and(|(parent, name)| parent == directory && is_disc_directory(name))
        });
        if has_disc_directory {
            roots.push(directory.to_string());
        }
    }

    let mut books = BTreeMap::<String, BookFiles>::new();
    for file in files {
        let audio = is_audio_filename(&file.rel_path);
        let Some((directory, _)) = file.rel_path.rsplit_once('/') else {
            if audio {
                let rel_path = file.rel_path.clone();
                books.entry(rel_path.clone()).or_insert_with(|| BookFiles {
                    path: file.path.clone(),
                    rel_path,
                    is_file: true,
                    audio_files: Vec::new(),
                    other_files: Vec::new(),
                });
                books
                    .get_mut(&file.rel_path)
                    .expect("single-file book was inserted")
                    .audio_files
                    .push(file);
            }
            continue;
        };
        let Some(book_root) = nearest_root(&roots, directory) else {
            continue;
        };
        let entry = books
            .entry(book_root.to_owned())
            .or_insert_with(|| BookFiles {
                path: library_root.join(book_root),
                rel_path: book_root.to_owned(),
                is_file: false,
                audio_files: Vec::new(),
                other_files: Vec::new(),
            });
        if audio {
            entry.audio_files.push(file);
        } else {
            entry.other_files.push(file);
        }
    }

    let mut books = books.into_values().collect::<Vec<_>>();
    books.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
    for book in &mut books {
        book.audio_files
            .sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
    }
    books
}

fn nearest_root<'a>(roots: &'a [String], directory: &str) -> Option<&'a str> {
    roots
        .iter()
        .filter(|root| directory == root.as_str() || is_inside(directory, root))
        .max_by_key(|root| root.len())
        .map(String::as_str)
}

fn is_inside(directory: &str, root: &str) -> bool {
    directory
        .strip_prefix(root)
        .is_some_and(|remainder| remainder.starts_with('/'))
}

fn collect_files(root: &Path) -> Vec<ScannedFile> {
    let mut files = Vec::new();
    collect_directory(root, root, &mut files);
    files.sort_by(|left, right| left.rel_path.cmp(&right.rel_path));
    files
}

fn collect_directory(base: &Path, directory: &Path, files: &mut Vec<ScannedFile>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if should_ignore(&name) {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_directory(base, &path, files);
        } else if file_type.is_file() {
            let Ok(relative) = path.strip_prefix(base) else {
                continue;
            };
            let rel_path = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            files.push(ScannedFile { path, rel_path });
        }
    }
}

fn should_ignore(name: &str) -> bool {
    if name.starts_with('.') || name.contains("@eaDir") {
        return true;
    }
    let lowercase = name.to_ascii_lowercase();
    IGNORED_EXTENSIONS
        .iter()
        .any(|extension| lowercase.ends_with(extension))
}

pub(crate) fn is_audio_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            AUDIO_EXTENSIONS
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(extension))
        })
}

fn is_audio_filename(relative: &str) -> bool {
    is_audio_path(Path::new(relative))
}

fn is_disc_directory(name: &str) -> bool {
    let lowercase = name.to_ascii_lowercase();
    let Some(rest) = lowercase
        .strip_prefix("disc")
        .or_else(|| lowercase.strip_prefix("disk"))
        .or_else(|| lowercase.strip_prefix("cd"))
    else {
        return false;
    };
    let number = rest.trim_start_matches(char::is_whitespace);
    !number.is_empty()
        && number.len() <= 3
        && number.chars().all(|character| character.is_ascii_digit())
}

/// Parse title, author, series, sequence, year, narrator, and ASIN from a book
/// directory path relative to the library root.
pub(crate) fn parse_book_path(rel_path: &str, parse_subtitle: bool) -> ParsedBookPath {
    let mut parts = rel_path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let Some(title_folder) = parts.pop() else {
        return ParsedBookPath::default();
    };
    let series_folder = if parts.len() > 1 { parts.pop() } else { None };
    let author_folder = if parts.is_empty() { None } else { parts.pop() };

    let (after_asin, asin) = get_asin(title_folder);
    let (after_narrators, narrators) = get_narrator(&after_asin);
    let (after_sequence, sequence) = if series_folder.is_some() {
        get_sequence(&after_narrators)
    } else {
        (after_narrators, None)
    };
    let (after_year, published_year) = get_published_year(&after_sequence);
    let (title, subtitle) = if parse_subtitle {
        get_subtitle(&after_year)
    } else {
        (after_year, None)
    };

    ParsedBookPath {
        title: (!title.trim().is_empty()).then(|| title.trim().to_owned()),
        subtitle: subtitle.filter(|value| !value.trim().is_empty()),
        authors: author_folder.map(parse_names).unwrap_or_default(),
        narrators: narrators
            .map(|value| parse_names(&value))
            .unwrap_or_default(),
        series: series_folder.map(str::to_owned),
        sequence,
        published_year,
        asin,
    }
}

/// Format a series and sequence the same way Audiobookshelf stores them in
/// `metadata.json` (`"Series #1"`).
pub(crate) fn series_label(name: &str, sequence: Option<&str>) -> String {
    match sequence.map(str::trim).filter(|value| !value.is_empty()) {
        Some(sequence) => format!("{name} #{sequence}"),
        None => name.to_owned(),
    }
}

pub(crate) fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Audio")
        .to_owned()
}

/// Choose the cover image for a book folder, preferring `cover.*` and then the
/// first supported image in the folder.
pub(crate) fn choose_cover(directory: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(directory).ok()?;
    let mut images = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_image_path(path))
        .collect::<Vec<_>>();
    images.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    let preferred = images.iter().find(|path| {
        path.file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("cover"))
    });
    preferred.cloned().or_else(|| images.into_iter().next())
}

fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            IMAGE_EXTENSIONS
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(extension))
        })
}

/// Track and disc numbers parsed from the audio filename and disc folder, as
/// documented in the Audiobookshelf track ordering rules.
pub(crate) fn filename_track_numbers(
    parsed: &ParsedBookPath,
    file: &ScannedFile,
) -> (Option<u32>, Option<u32>) {
    let mut stem = file_stem(&file.path);
    if let Some(title) = parsed.title.as_deref() {
        stem = stem.replace(title, "");
    }
    for author in &parsed.authors {
        stem = stem.replace(author, "");
    }
    if let Some(series) = parsed.series.as_deref() {
        stem = stem.replace(series, "");
    }
    if let Some(year) = parsed.published_year.as_deref() {
        stem = stem.replace(year, "");
    }

    let mut track_source = stem.clone();
    let mut disc = find_filename_disc(&stem).map(|(number, start, end)| {
        track_source.replace_range(start..end, "");
        number
    });
    if let Some(parent) = file.rel_path.rsplit_once('/').map(|(parent, _)| parent) {
        if let Some(name) = parent.rsplit('/').next() {
            if let Some(number) = disc_directory_number(name) {
                disc = Some(number);
            }
        }
    }
    let track = first_number(&track_source);
    (track, disc)
}

fn find_filename_disc(stem: &str) -> Option<(u32, usize, usize)> {
    let lowercase = stem.to_ascii_lowercase();
    let bytes = lowercase.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let label_length = if bytes[index..].starts_with(b"disc") {
            4
        } else if bytes[index..].starts_with(b"cd") {
            2
        } else {
            index += 1;
            continue;
        };
        let before_is_boundary = index == 0 || !bytes[index - 1].is_ascii_alphanumeric();
        if !before_is_boundary {
            index += 1;
            continue;
        }
        let mut cursor = index + label_length;
        if cursor < bytes.len() && bytes[cursor] == b' ' {
            cursor += 1;
        }
        let start = cursor;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() && cursor - start < 2 {
            cursor += 1;
        }
        if cursor > start && (cursor == bytes.len() || !bytes[cursor].is_ascii_alphanumeric()) {
            return lowercase[start..cursor]
                .parse()
                .ok()
                .map(|number| (number, index, cursor));
        }
        index += 1;
    }
    None
}

fn disc_directory_number(name: &str) -> Option<u32> {
    let lowercase = name.to_ascii_lowercase();
    let rest = lowercase
        .strip_prefix("disc")
        .or_else(|| lowercase.strip_prefix("disk"))
        .or_else(|| lowercase.strip_prefix("cd"))?;
    let number = rest.trim_start_matches(char::is_whitespace);
    if number.is_empty()
        || number.len() > 3
        || !number.chars().all(|character| character.is_ascii_digit())
    {
        return None;
    }
    number.parse().ok()
}

fn first_number(value: &str) -> Option<u32> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() && index - start < 4 {
            index += 1;
        }
        return value[start..index].parse().ok();
    }
    None
}

/// Parse an Audiobookshelf author or narrator string into display names.
/// Supports `First Last`, `Last, First`, and lists separated by `,`, `;`, `&`,
/// or ` and `.
pub(crate) fn parse_names(value: &str) -> Vec<String> {
    let value = value.trim();
    if value.is_empty() {
        return Vec::new();
    }
    let mut segments = split_name_segments(value);
    segments.retain(|segment| !segment.is_empty());
    if segments.is_empty() {
        return Vec::new();
    }
    if segments
        .first()
        .is_some_and(|segment| contains_cjk(segment))
    {
        return dedupe_names(segments);
    }
    if segments.len() == 1 {
        return dedupe_names(vec![segments.remove(0)]);
    }

    let comma_separated = value.contains(',');
    let first_is_last_name = comma_separated && is_last_name(&segments[0]);
    let mut names = Vec::new();
    if first_is_last_name {
        if segments.len() % 2 == 1 {
            segments.pop();
        }
        let mut iter = segments.into_iter();
        while let (Some(last), Some(first)) = (iter.next(), iter.next()) {
            names.push(format!("{first} {last}"));
        }
    } else {
        names = segments;
    }
    dedupe_names(names)
}

fn split_name_segments(value: &str) -> Vec<String> {
    let pieces = if value.contains('&') {
        value.split('&').collect::<Vec<_>>()
    } else if value.contains(" and ") {
        value.split(" and ").collect::<Vec<_>>()
    } else if value.contains(';') {
        value.split(';').collect::<Vec<_>>()
    } else {
        vec![value]
    };
    pieces
        .into_iter()
        .flat_map(|piece| piece.split(','))
        .map(|piece| piece.trim().to_owned())
        .filter(|piece| !piece.is_empty())
        .collect()
}

fn is_last_name(segment: &str) -> bool {
    !segment.contains(' ')
}

fn dedupe_names(names: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    names
        .into_iter()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty() && seen.insert(name.clone()))
        .collect()
}

fn contains_cjk(value: &str) -> bool {
    value.chars().any(
        |character| matches!(character as u32, 0x4e00..=0x9fff | 0x3040..=0x30ff | 0x31f0..=0x31ff),
    )
}

fn get_asin(folder: &str) -> (String, Option<String>) {
    let bytes = folder.as_bytes();
    let mut index = 0;
    while index + 12 <= bytes.len() {
        if bytes[index] == b'['
            && (index == 0 || bytes[index - 1] == b' ')
            && bytes[index + 11] == b']'
            && bytes[index + 1..index + 11]
                .iter()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
            && (index + 12 == bytes.len() || bytes[index + 12] == b' ')
        {
            let remove_start = if index > 0 { index - 1 } else { index };
            let rest = format!("{}{}", &folder[..remove_start], &folder[index + 12..]);
            return (
                rest.trim().to_owned(),
                Some(folder[index + 1..index + 11].to_owned()),
            );
        }
        index += 1;
    }
    (folder.trim().to_owned(), None)
}

fn get_narrator(folder: &str) -> (String, Option<String>) {
    if folder.ends_with('}') {
        if let Some(position) = folder.rfind(" {") {
            return (
                folder[..position].trim_end().to_owned(),
                Some(folder[position + 2..folder.len() - 1].to_owned()),
            );
        }
    }
    (folder.to_owned(), None)
}

fn get_sequence(folder: &str) -> (String, Option<String>) {
    let mut parts = folder.split(" - ").map(str::to_owned).collect::<Vec<_>>();
    for index in 0..parts.len() {
        let Some(sequence) = sequence_match(&parts[index]) else {
            continue;
        };
        parts[index] = sequence.1;
        if parts[index].is_empty() {
            parts.remove(index);
        }
        return (parts.join(" - "), Some(sequence.0));
    }
    (folder.to_owned(), None)
}

fn sequence_match(value: &str) -> Option<(String, String)> {
    let (label, rest) = if let Some(rest) = strip_prefix_ignore_case(value, "vol. ") {
        (true, rest)
    } else if let Some(rest) = strip_prefix_ignore_case(value, "vol ") {
        (true, rest)
    } else if let Some(rest) = strip_prefix_ignore_case(value, "volume ") {
        (true, rest)
    } else if let Some(rest) = strip_prefix_ignore_case(value, "book ") {
        (true, rest)
    } else {
        (false, value)
    };
    let bytes = rest.as_bytes();
    let digits_start = 0;
    let mut cursor = 0;
    while cursor < bytes.len() && bytes[cursor].is_ascii_digit() && cursor - digits_start < 3 {
        cursor += 1;
    }
    if cursor == digits_start {
        return None;
    }
    let mut sequence = rest[..cursor].to_owned();
    if cursor + 1 < bytes.len() && bytes[cursor] == b'.' && bytes[cursor + 1].is_ascii_digit() {
        let fraction_start = cursor + 1;
        let mut fraction_end = fraction_start;
        while fraction_end < bytes.len()
            && bytes[fraction_end].is_ascii_digit()
            && fraction_end - fraction_start < 2
        {
            fraction_end += 1;
        }
        sequence.push_str(&rest[cursor..fraction_end]);
        cursor = fraction_end;
    }
    let trailing_dot = cursor < bytes.len() && bytes[cursor] == b'.';
    if trailing_dot {
        cursor += 1;
    }
    let suffix = if cursor == bytes.len() {
        None
    } else if bytes[cursor] == b' ' {
        Some(rest[cursor + 1..].to_owned())
    } else {
        return None;
    };
    if suffix.as_ref().is_some_and(|suffix| !suffix.is_empty()) && !label && !trailing_dot {
        return None;
    }
    Some((normalize_sequence(&sequence), suffix.unwrap_or_default()))
}

fn normalize_sequence(value: &str) -> String {
    match value.parse::<f64>() {
        Ok(number) if number.fract() == 0.0 => format!("{}", number as i64),
        Ok(number) => number.to_string(),
        Err(_) => value.to_owned(),
    }
}

fn strip_prefix_ignore_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .then(|| &value[prefix.len()..])
}

fn get_published_year(folder: &str) -> (String, Option<String>) {
    let value = folder.trim_start_matches(' ');
    let (value, parenthesized) = match value.strip_prefix('(') {
        Some(rest) => (rest, true),
        None => (value, false),
    };
    if value.len() < 4 || !value.as_bytes()[..4].iter().all(u8::is_ascii_digit) {
        return (folder.to_owned(), None);
    }
    let year = value[..4].to_owned();
    let mut rest = &value[4..];
    if parenthesized {
        let Some(after_parenthesis) = rest.strip_prefix(')') else {
            return (folder.to_owned(), None);
        };
        rest = after_parenthesis;
    }
    let rest = rest.trim_start_matches(' ');
    let Some(rest) = rest.strip_prefix('-') else {
        return (folder.to_owned(), None);
    };
    let rest = rest.trim_start_matches(' ');
    if rest.is_empty() {
        return (folder.to_owned(), None);
    }
    (rest.to_owned(), Some(year))
}

fn get_subtitle(folder: &str) -> (String, Option<String>) {
    match folder.split_once(" - ") {
        Some((title, subtitle)) => (title.to_owned(), Some(subtitle.to_owned())),
        None => (folder.to_owned(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mplex-book-scanner-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_file(root: &Path, relative: &str, contents: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn parses_documented_author_and_series_paths() {
        let parsed = parse_book_path(
            "Terry Goodkind/Sword of Truth/Vol 1 - 1994 - Wizards First Rule {Sam Tsoutsouvas}",
            false,
        );
        assert_eq!(parsed.title.as_deref(), Some("Wizards First Rule"));
        assert_eq!(parsed.authors, vec!["Terry Goodkind"]);
        assert_eq!(parsed.narrators, vec!["Sam Tsoutsouvas"]);
        assert_eq!(parsed.series.as_deref(), Some("Sword of Truth"));
        assert_eq!(parsed.sequence.as_deref(), Some("1"));
        assert_eq!(parsed.published_year.as_deref(), Some("1994"));

        let parsed = parse_book_path(
            "Steven Levy/Hackers - Heroes of the Computer Revolution {Mike Chamberlain}",
            false,
        );
        assert_eq!(
            parsed.title.as_deref(),
            Some("Hackers - Heroes of the Computer Revolution")
        );
        assert_eq!(parsed.authors, vec!["Steven Levy"]);
        assert_eq!(parsed.narrators, vec!["Mike Chamberlain"]);
        assert_eq!(parsed.series, None);
    }

    #[test]
    fn parses_documented_book_folder_names() {
        let cases = [
            ("1945 - Animal Farm", "Animal Farm", Some("1945")),
            (
                "(1994) - Wizards First Rule",
                "Wizards First Rule",
                Some("1994"),
            ),
            ("Wizards First Rule", "Wizards First Rule", None),
            (
                "Wizards First Rule {Sam Tsoutsouvas}",
                "Wizards First Rule",
                None,
            ),
            (
                "1994 - Wizards First Rule - A Really Good Subtitle",
                "Wizards First Rule - A Really Good Subtitle",
                Some("1994"),
            ),
        ];
        for (folder, title, year) in cases {
            let parsed = parse_book_path(folder, false);
            assert_eq!(parsed.title.as_deref(), Some(title), "folder {folder}");
            assert_eq!(parsed.published_year.as_deref(), year, "folder {folder}");
            assert!(parsed.authors.is_empty(), "folder {folder}");
            assert!(parsed.series.is_none(), "folder {folder}");
        }
    }

    #[test]
    fn parses_documented_sequence_and_year_combinations() {
        let parsed = parse_book_path(
            "Terry Goodkind/Sword of Truth/1994 - Wizards First Rule - Volume 1",
            false,
        );
        assert_eq!(parsed.title.as_deref(), Some("Wizards First Rule"));
        assert_eq!(parsed.published_year.as_deref(), Some("1994"));
        assert_eq!(parsed.sequence.as_deref(), Some("1"));

        let parsed = parse_book_path(
            "Terry Goodkind/Sword of Truth/1994 - Wizards First Rule",
            false,
        );
        assert_eq!(parsed.title.as_deref(), Some("Wizards First Rule"));
        assert_eq!(parsed.published_year.as_deref(), Some("1994"));
        assert_eq!(parsed.sequence, None);

        let parsed = parse_book_path(
            "Terry Goodkind/Sword of Truth/1 - Wizards First Rule",
            false,
        );
        assert_eq!(parsed.title.as_deref(), Some("Wizards First Rule"));
        assert_eq!(parsed.sequence.as_deref(), Some("1"));

        let parsed = parse_book_path("Terry Goodkind/Sword of Truth/1. Wizards First Rule", false);
        assert_eq!(parsed.title.as_deref(), Some("Wizards First Rule"));
        assert_eq!(parsed.sequence.as_deref(), Some("1"));

        let parsed = parse_book_path(
            "Terry Goodkind/Sword of Truth/Vol. 1 - 1994 - Wizards First Rule - A Really Good Subtitle {Sam Tsoutsouvas}",
            false,
        );
        assert_eq!(
            parsed.title.as_deref(),
            Some("Wizards First Rule - A Really Good Subtitle")
        );
        assert_eq!(parsed.published_year.as_deref(), Some("1994"));
        assert_eq!(parsed.sequence.as_deref(), Some("1"));
        assert_eq!(parsed.narrators, vec!["Sam Tsoutsouvas"]);

        let parsed = parse_book_path("Author/Series/1. Wizards First Rule [B002V0QK4C]", false);
        assert_eq!(parsed.title.as_deref(), Some("Wizards First Rule"));
        assert_eq!(parsed.asin.as_deref(), Some("B002V0QK4C"));
        assert_eq!(parsed.sequence.as_deref(), Some("1"));

        let parsed = parse_book_path("Terry Goodkind/Series/101 Dalmations", false);
        assert_eq!(parsed.title.as_deref(), Some("101 Dalmations"));
        assert_eq!(parsed.sequence, None);

        let parsed = parse_book_path("Terry Goodkind/Series/101. Dalmations", false);
        assert_eq!(parsed.title.as_deref(), Some("Dalmations"));
        assert_eq!(parsed.sequence.as_deref(), Some("101"));

        let parsed = parse_book_path("Terry Goodkind/Series/0.5 - A Prequel", false);
        assert_eq!(parsed.title.as_deref(), Some("A Prequel"));
        assert_eq!(parsed.sequence.as_deref(), Some("0.5"));
    }

    #[test]
    fn parses_documented_author_name_forms() {
        let cases = [
            ("Ichiro Kishimi", vec!["Ichiro Kishimi"]),
            ("Kishimi, Ichiro", vec!["Ichiro Kishimi"]),
            (
                "Ichiro Kishimi, Fumitake Koga",
                vec!["Ichiro Kishimi", "Fumitake Koga"],
            ),
            (
                "Kishimi, Ichiro, Koga, Fumitake",
                vec!["Ichiro Kishimi", "Fumitake Koga"],
            ),
            (
                "Ichiro Kishimi & Fumitake Koga",
                vec!["Ichiro Kishimi", "Fumitake Koga"],
            ),
            (
                "Kishimi, Ichiro & Koga, Fumitake",
                vec!["Ichiro Kishimi", "Fumitake Koga"],
            ),
            (
                "Terry Goodkind, Ichiro Kishimi and Fumitake Koga",
                vec!["Terry Goodkind", "Ichiro Kishimi", "Fumitake Koga"],
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_names(input), expected, "input {input}");
        }
    }

    #[test]
    fn groups_root_files_author_books_and_series_books() {
        let root = temp_root("structure");
        write_file(&root, "Ender's Game.m4b", b"root");
        write_file(&root, "Human Action.m4b", b"root");
        write_file(
            &root,
            "Steven Levy/Hackers {Mike Chamberlain}/Audio.m4a",
            b"a",
        );
        write_file(&root, "Only Author/Only Author.m4a", b"a");
        write_file(&root, "1945 - Animal Farm/Audiobook.mp3", b"a");
        write_file(
            &root,
            "Terry Goodkind/Heart of Black Ice - Sister of Darkness/Audio.m4a",
            b"a",
        );
        write_file(
            &root,
            "Terry Goodkind/Sword of Truth/Vol 1 - 1994 - Wizards First Rule/Audio Track 1.mp3",
            b"a",
        );
        write_file(
            &root,
            "Terry Goodkind/Sword of Truth/Vol 1 - 1994 - Wizards First Rule/Audio Track 2.mp3",
            b"a",
        );
        write_file(
            &root,
            "Terry Goodkind/Sword of Truth/Vol 2 - 1995 - Stone of Tears/Audiobook.m4b",
            b"a",
        );

        let books = scan_library(&root);
        let titles = books
            .iter()
            .map(|book| book.rel_path.as_str())
            .collect::<Vec<_>>();
        assert!(titles.contains(&"Ender's Game.m4b"));
        assert!(titles.contains(&"Human Action.m4b"));
        assert!(titles.contains(&"Steven Levy/Hackers {Mike Chamberlain}"));
        assert!(titles.contains(&"Only Author"));
        assert!(titles.contains(&"1945 - Animal Farm"));
        assert!(titles.contains(&"Terry Goodkind/Heart of Black Ice - Sister of Darkness"));
        assert!(titles.contains(&"Terry Goodkind/Sword of Truth/Vol 1 - 1994 - Wizards First Rule"));
        assert!(titles.contains(&"Terry Goodkind/Sword of Truth/Vol 2 - 1995 - Stone of Tears"));
        assert!(!titles.contains(&"Steven Levy"));

        let wizards = books
            .iter()
            .find(|book| book.rel_path.contains("Wizards First Rule"))
            .unwrap();
        assert_eq!(wizards.audio_files.len(), 2);
        assert!(!wizards.is_file);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn folds_disc_directories_into_the_parent_book() {
        let root = temp_root("discs");
        write_file(&root, "Disc Book/Disc 1/01.mp3", b"a");
        write_file(&root, "Disc Book/Disc 2/01.mp3", b"a");
        write_file(&root, "Disc Book/CD 3/01.mp3", b"a");
        write_file(&root, "Disc Book/cover.jpg", b"cover");

        let books = scan_library(&root);
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].rel_path, "Disc Book");
        assert_eq!(books[0].audio_files.len(), 3);
        assert!(books[0]
            .other_files
            .iter()
            .any(|file| file.rel_path.ends_with("cover.jpg")));
        assert!(choose_cover(&books[0].path).is_some_and(|cover| cover.ends_with("cover.jpg")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_track_and_disc_numbers_from_names() {
        let parsed = parse_book_path("Author/Series/Vol 1 - Title", false);
        let file = ScannedFile {
            path: PathBuf::from("/library/Author/Series/Vol 1 - Title/Chapter 07.mp3"),
            rel_path: "Author/Series/Vol 1 - Title/Chapter 07.mp3".to_owned(),
        };
        assert_eq!(filename_track_numbers(&parsed, &file), (Some(7), None));

        let disc_file = ScannedFile {
            path: PathBuf::from("/library/Author/Series/Vol 1 - Title/Disc 2/01.mp3"),
            rel_path: "Author/Series/Vol 1 - Title/Disc 2/01.mp3".to_owned(),
        };
        assert_eq!(
            filename_track_numbers(&parsed, &disc_file),
            (Some(1), Some(2))
        );

        let named_disc = ScannedFile {
            path: PathBuf::from("/library/Author/Series/Vol 1 - Title/Book CD 03 - Track 4.mp3"),
            rel_path: "Author/Series/Vol 1 - Title/Book CD 03 - Track 4.mp3".to_owned(),
        };
        assert_eq!(
            filename_track_numbers(&parsed, &named_disc),
            (Some(4), Some(3))
        );

        let multibyte = ScannedFile {
            path: PathBuf::from("/library/Author/Series/Vol 1 - Title/Wizards – First Rule.mp3"),
            rel_path: "Author/Series/Vol 1 - Title/Wizards – First Rule.mp3".to_owned(),
        };
        assert_eq!(filename_track_numbers(&parsed, &multibyte), (None, None));
    }

    #[test]
    fn subtitle_parsing_is_opt_in() {
        let disabled = parse_book_path("Author/Series/1994 - Title - Subtitle", false);
        assert_eq!(disabled.title.as_deref(), Some("Title - Subtitle"));
        assert_eq!(disabled.subtitle, None);

        let enabled = parse_book_path("Author/Series/1994 - Title - Subtitle", true);
        assert_eq!(enabled.title.as_deref(), Some("Title"));
        assert_eq!(enabled.subtitle.as_deref(), Some("Subtitle"));
    }
}
