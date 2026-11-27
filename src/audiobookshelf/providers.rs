use super::audible;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct MatchedBook {
    pub(crate) provider: String,
    pub(crate) provider_id: String,
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    pub(crate) authors: Vec<String>,
    pub(crate) narrators: Vec<String>,
    pub(crate) series: Vec<String>,
    pub(crate) genres: Vec<String>,
    pub(crate) tags: Vec<String>,
    pub(crate) published_year: Option<String>,
    pub(crate) published_date: Option<String>,
    pub(crate) publisher: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) isbn: Option<String>,
    pub(crate) asin: Option<String>,
    pub(crate) language: Option<String>,
    pub(crate) explicit: bool,
    pub(crate) abridged: bool,
    pub(crate) cover_url: Option<String>,
    pub(crate) duration_minutes: Option<u64>,
}

impl MatchedBook {
    pub(crate) fn metadata_json(&self) -> serde_json::Value {
        serde_json::json!({
            "title": self.title,
            "subtitle": self.subtitle,
            "authors": self.authors,
            "narrators": self.narrators,
            "series": self.series,
            "genres": self.genres,
            "tags": self.tags,
            "publishedYear": self.published_year,
            "publishedDate": self.published_date,
            "publisher": self.publisher,
            "description": self.description,
            "isbn": self.isbn,
            "asin": self.asin,
            "language": self.language,
            "explicit": self.explicit,
            "abridged": self.abridged,
            "matchProvider": (!self.provider.trim().is_empty()).then_some(self.provider.as_str()),
            "matchProviderId": (!self.provider_id.trim().is_empty()).then_some(self.provider_id.as_str())
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Provider {
    Audible,
    ITunes,
    GoogleBooks,
    OpenLibrary,
    FantLab,
}

impl Provider {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Audible => "audible",
            Self::ITunes => "itunes",
            Self::GoogleBooks => "google",
            Self::OpenLibrary => "openlibrary",
            Self::FantLab => "fantlab",
        }
    }

    pub(crate) fn from_key(value: &str) -> Option<Self> {
        match value {
            "audible" => Some(Self::Audible),
            "itunes" => Some(Self::ITunes),
            "google" => Some(Self::GoogleBooks),
            "openlibrary" => Some(Self::OpenLibrary),
            "fantlab" => Some(Self::FantLab),
            _ => None,
        }
    }

    pub(crate) fn display_name(self) -> &'static str {
        match self {
            Self::Audible => "Audible",
            Self::ITunes => "iTunes",
            Self::GoogleBooks => "Google Books",
            Self::OpenLibrary => "Open Library",
            Self::FantLab => "FantLab",
        }
    }
}

pub(crate) async fn search(
    provider: Provider,
    client: &reqwest::Client,
    title: &str,
    author: &str,
) -> Vec<MatchedBook> {
    for (title, author) in query_terms(title, author) {
        let results = search_once(provider, client, &title, &author).await;
        if !results.is_empty() {
            return results;
        }
    }
    Vec::new()
}

async fn search_once(
    provider: Provider,
    client: &reqwest::Client,
    title: &str,
    author: &str,
) -> Vec<MatchedBook> {
    match provider {
        Provider::Audible => {
            audible::search_books(client, title, author, audible::DEFAULT_REGION).await
        }
        Provider::ITunes => itunes_search(client, title, author).await,
        Provider::GoogleBooks => google_books_search(client, title, author).await,
        Provider::OpenLibrary => open_library_search(client, title, author).await,
        Provider::FantLab => fantlab_search(client, title, author).await,
    }
}

pub(crate) fn query_terms(title: &str, author: &str) -> Vec<(String, String)> {
    let title = title.trim();
    let author = author.trim();
    let mut terms = vec![(title.to_owned(), author.to_owned())];
    if author.is_empty() {
        if let Some((candidate_author, candidate_title)) = title.split_once(" - ") {
            let candidate_author = candidate_author.trim();
            let candidate_title = candidate_title.trim();
            if plausible_author(candidate_author) && !candidate_title.is_empty() {
                terms.push((candidate_title.to_owned(), candidate_author.to_owned()));
            }
        }
    }
    terms
}

fn plausible_author(value: &str) -> bool {
    let words = value.split_whitespace().count();
    (2..=4).contains(&words) && !value.chars().any(|character| character.is_ascii_digit())
}

pub(crate) async fn lookup(
    provider: Provider,
    client: &reqwest::Client,
    id: &str,
) -> Option<MatchedBook> {
    match provider {
        Provider::Audible => audible::book_by_asin(client, id, audible::DEFAULT_REGION).await,
        Provider::ITunes => itunes_lookup(client, id).await,
        Provider::GoogleBooks => google_books_lookup(client, id).await,
        Provider::OpenLibrary => open_library_lookup(client, id).await,
        Provider::FantLab => fantlab_lookup(client, id).await,
    }
}

pub(crate) async fn automatic_match(
    client: &reqwest::Client,
    title: &str,
    author: &str,
    asin: Option<&str>,
    duration_minutes: Option<f64>,
) -> Option<MatchedBook> {
    for (title, author) in query_terms(title, author) {
        if let Some(book) = audible::find_book(
            client,
            &title,
            &author,
            asin,
            audible::DEFAULT_REGION,
            duration_minutes,
        )
        .await
        {
            return Some(add_cover_fallback(client, book).await);
        }
    }
    for provider in [
        Provider::ITunes,
        Provider::GoogleBooks,
        Provider::OpenLibrary,
        Provider::FantLab,
    ] {
        let Some(candidate) = search(provider, client, title, author)
            .await
            .into_iter()
            .next()
        else {
            continue;
        };
        let book = lookup(provider, client, &candidate.provider_id)
            .await
            .filter(|book| !book.title.trim().is_empty())
            .unwrap_or(candidate);
        return Some(add_cover_fallback(client, book).await);
    }
    None
}

async fn add_cover_fallback(client: &reqwest::Client, mut book: MatchedBook) -> MatchedBook {
    if book.cover_url.is_none() {
        book.cover_url = audiobook_covers(client, &book.title).await;
    }
    book
}

async fn audiobook_covers(client: &reqwest::Client, title: &str) -> Option<String> {
    let response = client
        .get("https://api.audiobookcovers.com/cover/bytext/")
        .query(&[("q", title)])
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response
        .json::<Vec<AudiobookCover>>()
        .await
        .ok()?
        .into_iter()
        .next()
        .and_then(|cover| cover.versions.png.original)
}

#[derive(Deserialize)]
struct AudiobookCover {
    versions: AudiobookCoverVersions,
}

#[derive(Deserialize)]
struct AudiobookCoverVersions {
    png: AudiobookCoverImage,
}

#[derive(Deserialize)]
struct AudiobookCoverImage {
    original: Option<String>,
}

// ---- iTunes ----

#[derive(Deserialize)]
struct ITunesResponse {
    #[serde(default)]
    results: Vec<ITunesResult>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ITunesResult {
    collection_id: Option<u64>,
    collection_name: Option<String>,
    artist_name: Option<String>,
    description: Option<String>,
    release_date: Option<String>,
    primary_genre_name: Option<String>,
    artwork_url_600: Option<String>,
    artwork_url_100: Option<String>,
    artwork_url_30: Option<String>,
}

async fn itunes_search(client: &reqwest::Client, title: &str, author: &str) -> Vec<MatchedBook> {
    let term = if author.trim().is_empty() {
        title.to_owned()
    } else {
        format!("{title} {author}")
    };
    let response = match client
        .get("https://itunes.apple.com/search")
        .query(&[
            ("term", term),
            ("entity", "audiobook".to_owned()),
            ("media", "audiobook".to_owned()),
            ("limit", "10".to_owned()),
        ])
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Vec::new(),
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    response
        .json::<ITunesResponse>()
        .await
        .map(|response| {
            response
                .results
                .into_iter()
                .filter_map(itunes_book)
                .collect()
        })
        .unwrap_or_default()
}

async fn itunes_lookup(client: &reqwest::Client, id: &str) -> Option<MatchedBook> {
    let response = client
        .get("https://itunes.apple.com/lookup")
        .query(&[("id", id), ("entity", "audiobook")])
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response
        .json::<ITunesResponse>()
        .await
        .ok()?
        .results
        .into_iter()
        .find_map(itunes_book)
}

fn itunes_book(item: ITunesResult) -> Option<MatchedBook> {
    let provider_id = item.collection_id?.to_string();
    let title = item.collection_name?;
    let author = item.artist_name.unwrap_or_default();
    let cover_url = item
        .artwork_url_600
        .or(item.artwork_url_100)
        .or(item.artwork_url_30);
    Some(MatchedBook {
        provider: Provider::ITunes.key().to_owned(),
        provider_id,
        title,
        authors: split_author_names(&author),
        genres: item.primary_genre_name.into_iter().collect(),
        description: item.description,
        published_year: item
            .release_date
            .as_deref()
            .and_then(|date| date.get(..4))
            .map(str::to_owned),
        published_date: item.release_date,
        cover_url,
        ..Default::default()
    })
}

fn split_author_names(value: &str) -> Vec<String> {
    value
        .split([',', '&'])
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

// ---- Google Books ----

#[derive(Deserialize)]
struct GoogleBooksResponse {
    #[serde(default)]
    items: Vec<GoogleVolume>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleVolume {
    id: String,
    #[serde(default)]
    volume_info: GoogleVolumeInfo,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GoogleVolumeInfo {
    title: Option<String>,
    subtitle: Option<String>,
    authors: Option<Vec<String>>,
    publisher: Option<String>,
    published_date: Option<String>,
    description: Option<String>,
    industry_identifiers: Option<Vec<GoogleIdentifier>>,
    categories: Option<Vec<String>>,
    image_links: Option<GoogleImageLinks>,
}

#[derive(Clone, Debug, Deserialize)]
struct GoogleIdentifier {
    #[serde(rename = "type")]
    kind: String,
    identifier: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleImageLinks {
    thumbnail: Option<String>,
    small_thumbnail: Option<String>,
    medium: Option<String>,
    large: Option<String>,
    extra_large: Option<String>,
}

async fn google_books_search(
    client: &reqwest::Client,
    title: &str,
    author: &str,
) -> Vec<MatchedBook> {
    let mut query = format!("intitle:{title}");
    if !author.trim().is_empty() {
        query.push_str(&format!(" inauthor:{author}"));
    }
    let response = match client
        .get("https://www.googleapis.com/books/v1/volumes")
        .query(&[("q", query), ("maxResults", "10".to_owned())])
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Vec::new(),
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    response
        .json::<GoogleBooksResponse>()
        .await
        .map(|response| response.items.into_iter().filter_map(google_book).collect())
        .unwrap_or_default()
}

async fn google_books_lookup(client: &reqwest::Client, id: &str) -> Option<MatchedBook> {
    let response = client
        .get(format!("https://www.googleapis.com/books/v1/volumes/{id}"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let volume = response.json::<GoogleVolume>().await.ok()?;
    google_book(volume)
}

fn google_book(volume: GoogleVolume) -> Option<MatchedBook> {
    let info = volume.volume_info;
    let title = info.title?;
    let cover_url = info.image_links.and_then(|links| {
        [
            links.extra_large,
            links.large,
            links.medium,
            links.thumbnail,
            links.small_thumbnail,
        ]
        .into_iter()
        .flatten()
        .next()
        .map(|url| url.replace("http://", "https://"))
    });
    Some(MatchedBook {
        provider: Provider::GoogleBooks.key().to_owned(),
        provider_id: volume.id,
        title,
        subtitle: info.subtitle,
        authors: info.authors.unwrap_or_default(),
        genres: info.categories.unwrap_or_default(),
        publisher: info.publisher,
        published_date: info.published_date.clone(),
        published_year: info
            .published_date
            .as_deref()
            .and_then(|date| date.get(..4))
            .map(str::to_owned),
        description: info.description,
        isbn: info.industry_identifiers.and_then(|identifiers| {
            identifiers
                .iter()
                .find(|identifier| identifier.kind == "ISBN_13")
                .or_else(|| {
                    identifiers
                        .iter()
                        .find(|identifier| identifier.kind == "ISBN_10")
                })
                .map(|identifier| identifier.identifier.clone())
        }),
        cover_url,
        ..Default::default()
    })
}

// ---- OpenLibrary ----

#[derive(Deserialize)]
struct OpenLibrarySearchResponse {
    #[serde(default)]
    docs: Vec<OpenLibraryDoc>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct OpenLibraryDoc {
    key: Option<String>,
    title: Option<String>,
    author_name: Option<Vec<String>>,
    first_publish_year: Option<u32>,
    cover_edition_key: Option<String>,
    cover_i: Option<u64>,
    isbn: Option<Vec<String>>,
    publisher: Option<Vec<String>>,
}

async fn open_library_search(
    client: &reqwest::Client,
    title: &str,
    author: &str,
) -> Vec<MatchedBook> {
    let mut query = vec![("title", title.to_owned()), ("limit", "10".to_owned())];
    if !author.trim().is_empty() {
        query.push(("author", author.to_owned()));
    }
    let response = match client
        .get("https://openlibrary.org/search.json")
        .query(&query)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Vec::new(),
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    response
        .json::<OpenLibrarySearchResponse>()
        .await
        .map(|response| {
            response
                .docs
                .into_iter()
                .filter_map(open_library_book)
                .collect()
        })
        .unwrap_or_default()
}

async fn open_library_lookup(client: &reqwest::Client, id: &str) -> Option<MatchedBook> {
    let key = if id.starts_with('/') {
        id.to_owned()
    } else {
        format!("/works/{id}")
    };
    let response = client
        .get(format!("https://openlibrary.org{key}.json"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value = response.json::<serde_json::Value>().await.ok()?;
    let title = value.get("title")?.as_str()?.to_owned();
    let description = match value.get("description") {
        Some(serde_json::Value::String(description)) => Some(description.clone()),
        Some(serde_json::Value::Object(description)) => description
            .get("value")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        _ => None,
    };
    let cover_url = value
        .get("covers")
        .and_then(serde_json::Value::as_array)
        .and_then(|covers| covers.iter().find_map(serde_json::Value::as_u64))
        .map(|cover| format!("https://covers.openlibrary.org/b/id/{cover}-L.jpg"));
    let published_year = value
        .get("first_publish_date")
        .and_then(serde_json::Value::as_str)
        .and_then(|date| date.get(..4))
        .map(str::to_owned);
    Some(MatchedBook {
        provider: Provider::OpenLibrary.key().to_owned(),
        provider_id: key,
        title,
        description,
        published_year,
        cover_url,
        ..Default::default()
    })
}

fn open_library_book(doc: OpenLibraryDoc) -> Option<MatchedBook> {
    let key = doc.key?;
    let title = doc.title?;
    let cover_url = doc
        .cover_i
        .map(|cover| format!("https://covers.openlibrary.org/b/id/{cover}-L.jpg"))
        .or_else(|| {
            doc.cover_edition_key
                .map(|cover| format!("https://covers.openlibrary.org/b/OLID/{cover}-L.jpg"))
        });
    Some(MatchedBook {
        provider: Provider::OpenLibrary.key().to_owned(),
        provider_id: key,
        title,
        authors: doc.author_name.unwrap_or_default(),
        genres: Vec::new(),
        published_year: doc.first_publish_year.map(|year| year.to_string()),
        isbn: doc.isbn.and_then(|isbn| isbn.into_iter().next()),
        publisher: doc
            .publisher
            .and_then(|publishers| publishers.into_iter().next()),
        cover_url,
        ..Default::default()
    })
}

// ---- FantLab ----

#[derive(Clone, Debug, Deserialize, Default)]
struct FantLabSearchItem {
    work_id: Option<u64>,
    work_type_id: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct FantLabWork {
    work_id: Option<u64>,
    work_name: Option<String>,
    work_name_alts: Option<Vec<String>>,
    work_year: Option<u32>,
    work_description: Option<String>,
    image: Option<String>,
    authors: Option<Vec<FantLabAuthor>>,
    classificatory: Option<FantLabClassificatory>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct FantLabAuthor {
    name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct FantLabClassificatory {
    genre_group: Option<Vec<FantLabGenreGroup>>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct FantLabGenreGroup {
    genre_group_id: Option<u64>,
    genre: Option<Vec<FantLabGenre>>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct FantLabGenre {
    label: Option<String>,
    genre: Option<Vec<FantLabGenre>>,
}

async fn fantlab_search(client: &reqwest::Client, title: &str, author: &str) -> Vec<MatchedBook> {
    let query = if author.trim().is_empty() {
        title.to_owned()
    } else {
        format!("{title} {author}")
    };
    let response = match client
        .get("https://api.fantlab.ru/search-works")
        .query(&[
            ("q", query),
            ("page", "1".to_owned()),
            ("onlymatches", "1".to_owned()),
        ])
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return Vec::new(),
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    let items = match response.json::<Vec<FantLabSearchItem>>().await {
        Ok(items) => items,
        Err(_) => return Vec::new(),
    };
    let mut books = Vec::new();
    for item in items.into_iter().take(10) {
        let Some(work_id) = item.work_id else {
            continue;
        };
        if matches!(
            item.work_type_id,
            Some(7 | 11 | 12 | 22 | 23 | 24 | 25 | 26 | 46 | 47 | 49 | 51 | 52 | 55 | 56 | 57)
        ) {
            continue;
        }
        if let Some(book) = fantlab_work(client, work_id).await {
            books.push(book);
        }
    }
    books
}

async fn fantlab_lookup(client: &reqwest::Client, id: &str) -> Option<MatchedBook> {
    let work_id = id.parse::<u64>().ok()?;
    fantlab_work(client, work_id).await
}

async fn fantlab_work(client: &reqwest::Client, work_id: u64) -> Option<MatchedBook> {
    let response = client
        .get(format!("https://api.fantlab.ru/work/{work_id}/extended"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let work = response.json::<FantLabWork>().await.ok()?;
    fantlab_book(work)
}

fn fantlab_book(work: FantLabWork) -> Option<MatchedBook> {
    let title = work.work_name?;
    let authors = work
        .authors
        .unwrap_or_default()
        .into_iter()
        .filter_map(|author| author.name)
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    Some(MatchedBook {
        provider: Provider::FantLab.key().to_owned(),
        provider_id: work.work_id?.to_string(),
        title,
        subtitle: work
            .work_name_alts
            .and_then(|names| names.into_iter().next()),
        authors,
        genres: fantlab_genres(work.classificatory),
        published_year: work.work_year.map(|year| year.to_string()),
        description: work.work_description,
        cover_url: work.image.map(|image| format!("https://fantlab.ru{image}")),
        ..Default::default()
    })
}

fn fantlab_genres(classificatory: Option<FantLabClassificatory>) -> Vec<String> {
    let Some(mut group) = classificatory
        .and_then(|classificatory| classificatory.genre_group)
        .and_then(|groups| {
            groups
                .into_iter()
                .find(|group| group.genre_group_id == Some(1))
        })
    else {
        return Vec::new();
    };
    let Some(root) = group.genre.as_mut().and_then(|genres| genres.first_mut()) else {
        return Vec::new();
    };
    let mut genres = Vec::new();
    if let Some(label) = root.label.take() {
        genres.push(label);
    }
    fn push_subgenres(genre: &FantLabGenre, genres: &mut Vec<String>) {
        for subgenre in genre.genre.iter().flatten() {
            if let Some(label) = subgenre.label.as_deref() {
                genres.push(label.to_owned());
            }
        }
    }
    push_subgenres(root, &mut genres);
    genres
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_keys_round_trip() {
        for provider in [
            Provider::Audible,
            Provider::ITunes,
            Provider::GoogleBooks,
            Provider::OpenLibrary,
            Provider::FantLab,
        ] {
            assert_eq!(Provider::from_key(provider.key()), Some(provider));
        }
    }

    #[test]
    fn query_terms_split_an_author_from_a_root_folder_title() {
        let terms = query_terms("Noam Chomsky - Necessary Illusions", "");
        assert_eq!(
            terms,
            vec![
                (
                    "Noam Chomsky - Necessary Illusions".to_owned(),
                    String::new()
                ),
                ("Necessary Illusions".to_owned(), "Noam Chomsky".to_owned())
            ]
        );
        assert_eq!(
            query_terms("Hackers - Heroes of the Computer Revolution", "").len(),
            1
        );
        assert_eq!(query_terms("1945 - Animal Farm", "").len(), 1);
    }

    #[test]
    fn google_volume_maps_to_book() {
        let volume: GoogleVolume = serde_json::from_value(serde_json::json!({
            "id": "abc",
            "volumeInfo": {
                "title": "Wizards First Rule",
                "authors": ["Terry Goodkind"],
                "publisher": "Tor",
                "publishedDate": "1994-10-01",
                "description": "A summary",
                "categories": ["Fantasy"],
                "industryIdentifiers": [
                    {"type": "ISBN_10", "identifier": "0000000000"},
                    {"type": "ISBN_13", "identifier": "9780000000000"}
                ],
                "imageLinks": {"thumbnail": "http://example.com/cover.jpg"}
            }
        }))
        .unwrap();
        let book = google_book(volume).unwrap();
        assert_eq!(book.provider, "google");
        assert_eq!(book.provider_id, "abc");
        assert_eq!(book.authors, vec!["Terry Goodkind"]);
        assert_eq!(book.published_year.as_deref(), Some("1994"));
        assert_eq!(book.isbn.as_deref(), Some("9780000000000"));
        assert_eq!(
            book.cover_url.as_deref(),
            Some("https://example.com/cover.jpg")
        );
    }

    #[test]
    fn itunes_result_maps_to_book() {
        let item: ITunesResult = serde_json::from_value(serde_json::json!({
            "collectionId": 1234,
            "collectionName": "Wizards First Rule",
            "artistName": "Terry Goodkind",
            "description": "A summary",
            "releaseDate": "1994-10-01T07:00:00Z",
            "primaryGenreName": "Fantasy",
            "artworkUrl600": "https://example.com/cover.jpg"
        }))
        .unwrap();
        let book = itunes_book(item).unwrap();
        assert_eq!(book.provider, "itunes");
        assert_eq!(book.provider_id, "1234");
        assert_eq!(book.authors, vec!["Terry Goodkind"]);
        assert_eq!(book.published_year.as_deref(), Some("1994"));
    }
}
