use super::providers::MatchedBook;
use serde::Deserialize;
use std::collections::HashSet;

const SEARCH_RESULT_LIMIT: usize = 10;
pub(crate) const DEFAULT_REGION: &str = "us";

#[derive(Clone, Debug, Deserialize, Default)]
struct Person {
    name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct Genre {
    #[serde(rename = "type")]
    kind: Option<String>,
    name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct SeriesInfo {
    name: Option<String>,
    position: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct CatalogProduct {
    asin: Option<String>,
    title: Option<String>,
    authors: Option<Vec<Person>>,
    narrators: Option<Vec<Person>>,
    runtime_length_min: Option<u64>,
    release_date: Option<String>,
    publisher_name: Option<String>,
    format_type: Option<String>,
    is_adult_product: Option<bool>,
    language: Option<String>,
    product_images: Option<std::collections::BTreeMap<String, String>>,
}

#[derive(Clone, Debug, Deserialize, Default)]
struct CatalogResponse {
    #[serde(default)]
    products: Vec<CatalogProduct>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct BookDetails {
    asin: Option<String>,
    title: Option<String>,
    subtitle: Option<String>,
    authors: Option<Vec<Person>>,
    narrators: Option<Vec<Person>>,
    publisher_name: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    release_date: Option<String>,
    image: Option<String>,
    genres: Option<Vec<Genre>>,
    series_primary: Option<SeriesInfo>,
    series_secondary: Option<SeriesInfo>,
    language: Option<String>,
    runtime_length_min: Option<u64>,
    format_type: Option<String>,
    isbn: Option<String>,
    is_adult: Option<bool>,
}

pub(crate) async fn find_book(
    client: &reqwest::Client,
    title: &str,
    author: &str,
    asin: Option<&str>,
    region: &str,
    duration_minutes: Option<f64>,
) -> Option<MatchedBook> {
    if let Some(asin) = asin.filter(|asin| is_valid_asin(asin)) {
        return book_by_asin(client, asin, region).await;
    }
    if is_valid_asin(title) {
        return book_by_asin(client, title, region).await;
    }
    let products = search(client, title, author, region).await;
    if products.is_empty() {
        return None;
    }
    let best = products.into_iter().max_by(|left, right| {
        score(left, title, author, duration_minutes).total_cmp(&score(
            right,
            title,
            author,
            duration_minutes,
        ))
    })?;
    let asin = best.asin.as_deref()?;
    match book_by_asin(client, asin, region).await {
        Some(book) => Some(book),
        None => Some(book_from_product(best)),
    }
}

pub(crate) async fn search_books(
    client: &reqwest::Client,
    title: &str,
    author: &str,
    region: &str,
) -> Vec<MatchedBook> {
    search(client, title, author, region)
        .await
        .into_iter()
        .filter_map(|product| {
            let asin = product.asin?;
            Some(MatchedBook {
                provider: "audible".to_owned(),
                provider_id: asin.clone(),
                asin: Some(asin),
                title: product.title.unwrap_or_default(),
                authors: names(product.authors),
                narrators: names(product.narrators),
                published_year: product
                    .release_date
                    .as_deref()
                    .and_then(|date| date.get(..4))
                    .map(str::to_owned),
                duration_minutes: product.runtime_length_min,
                cover_url: product
                    .product_images
                    .and_then(|images| images.get("500").cloned()),
                ..Default::default()
            })
        })
        .collect()
}

async fn search(
    client: &reqwest::Client,
    title: &str,
    author: &str,
    region: &str,
) -> Vec<CatalogProduct> {
    let mut request = client
        .get(format!(
            "https://api.audible{}/1.0/catalog/products",
            region_tld(region)
        ))
        .query(&[
            ("num_results", SEARCH_RESULT_LIMIT.to_string()),
            ("products_sort_by", "Relevance".to_owned()),
            ("title", title.to_owned()),
            (
                "response_groups",
                "product_desc,product_attrs,media,contributors".to_owned(),
            ),
        ]);
    if !author.trim().is_empty() {
        request = request.query(&[("author", author.to_owned())]);
    }
    let Ok(response) = request.send().await else {
        tracing::warn!(
            title,
            author,
            region,
            "Audible catalog search request failed"
        );
        return Vec::new();
    };
    if !response.status().is_success() {
        tracing::warn!(
            title,
            author,
            region,
            status = %response.status(),
            "Audible catalog search returned an error status"
        );
        return Vec::new();
    }
    response
        .json::<CatalogResponse>()
        .await
        .map(|response| response.products)
        .unwrap_or_default()
}

pub(crate) async fn book_by_asin(
    client: &reqwest::Client,
    asin: &str,
    region: &str,
) -> Option<MatchedBook> {
    let asin = asin.trim().to_ascii_uppercase();
    let response = client
        .get(format!("https://api.audnex.us/books/{asin}"))
        .query(&[("region", region)])
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        tracing::debug!(
            asin,
            region,
            status = %response.status(),
            "Audnexus book lookup returned an error status"
        );
        return None;
    }
    let details = response.json::<BookDetails>().await.ok()?;
    if details.asin.is_none() && details.title.is_none() {
        return None;
    }
    Some(book_from_details(details))
}

fn book_from_details(details: BookDetails) -> MatchedBook {
    let mut genres = Vec::new();
    let mut tags = Vec::new();
    let mut seen_genres = HashSet::new();
    let mut seen_tags = HashSet::new();
    for genre in details.genres.unwrap_or_default() {
        let Some(name) = genre.name.filter(|name| !name.trim().is_empty()) else {
            continue;
        };
        match genre.kind.as_deref() {
            Some("genre") if seen_genres.insert(name.clone()) => genres.push(name),
            Some("tag") if seen_tags.insert(name.clone()) => tags.push(name),
            _ => {}
        }
    }
    let mut series = Vec::new();
    for info in [details.series_primary, details.series_secondary]
        .into_iter()
        .flatten()
    {
        let Some(name) = info.name.filter(|name| !name.trim().is_empty()) else {
            continue;
        };
        let sequence = info.position.and_then(|position| clean_sequence(&position));
        series.push(match sequence {
            Some(sequence) => format!("{name} #{sequence}"),
            None => name,
        });
    }
    let published_date = details.release_date;
    let asin = details.asin;
    MatchedBook {
        provider: "audible".to_owned(),
        provider_id: asin.clone().unwrap_or_default(),
        title: details.title.unwrap_or_default(),
        subtitle: details.subtitle,
        authors: names(details.authors),
        narrators: names(details.narrators),
        series,
        genres,
        tags,
        published_year: published_date
            .as_deref()
            .and_then(|date| date.get(..4))
            .map(str::to_owned),
        published_date,
        publisher: details.publisher_name,
        description: details.summary.or(details.description),
        isbn: details.isbn,
        asin,
        language: details.language.map(clean_language),
        explicit: details.is_adult.unwrap_or(false),
        abridged: details.format_type.as_deref() == Some("abridged"),
        cover_url: details.image,
        duration_minutes: details.runtime_length_min,
    }
}

fn book_from_product(product: CatalogProduct) -> MatchedBook {
    let published_date = product.release_date;
    let asin = product.asin;
    MatchedBook {
        provider: "audible".to_owned(),
        provider_id: asin.clone().unwrap_or_default(),
        title: product.title.unwrap_or_default(),
        authors: names(product.authors),
        narrators: names(product.narrators),
        published_year: published_date
            .as_deref()
            .and_then(|date| date.get(..4))
            .map(str::to_owned),
        published_date,
        publisher: product.publisher_name,
        asin,
        language: product.language.map(clean_language),
        explicit: product.is_adult_product.unwrap_or(false),
        abridged: product.format_type.as_deref() == Some("abridged"),
        cover_url: product
            .product_images
            .and_then(|images| images.get("500").cloned()),
        duration_minutes: product.runtime_length_min,
        ..Default::default()
    }
}

fn names(people: Option<Vec<Person>>) -> Vec<String> {
    let mut seen = HashSet::new();
    people
        .unwrap_or_default()
        .into_iter()
        .filter_map(|person| person.name)
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty() && seen.insert(name.clone()))
        .collect()
}

fn score(
    product: &CatalogProduct,
    title: &str,
    author: &str,
    duration_minutes: Option<f64>,
) -> f64 {
    let title_score = similarity(
        &normalize_text(title),
        &normalize_text(product.title.as_deref().unwrap_or_default()),
    );
    let product_author = names(product.authors.clone()).join(", ");
    let author_score = if author.trim().is_empty() {
        1.0
    } else {
        similarity(&normalize_text(author), &normalize_text(&product_author))
    };
    let duration_score = match (duration_minutes, product.runtime_length_min) {
        (Some(actual), Some(candidate)) if actual > 0.0 => {
            let difference = (actual - candidate as f64).abs();
            (1.0 - difference / 10.0).clamp(0.0, 1.0)
        }
        _ => 0.1,
    };
    0.7 * duration_score + 0.2 * title_score + 0.1 * author_score
}

fn normalize_text(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || character.is_whitespace() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn similarity(left: &str, right: &str) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    let longest = left_chars.len().max(right_chars.len());
    if longest == 0 {
        return 1.0;
    }
    1.0 - levenshtein(&left_chars, &right_chars) as f64 / longest as f64
}

fn levenshtein(left: &[char], right: &[char]) -> usize {
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];
    for (left_index, left_character) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_character) in right.iter().enumerate() {
            let substitution =
                previous[right_index] + usize::from(left_character != right_character);
            current[right_index + 1] = (current[right_index] + 1)
                .min(previous[right_index + 1] + 1)
                .min(substitution);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

fn clean_sequence(value: &str) -> Option<String> {
    let start = value
        .char_indices()
        .find(|(_, character)| character.is_ascii_digit() || *character == '.')
        .map(|(index, _)| index)?;
    let sequence = value[start..]
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect::<String>();
    if sequence.is_empty() || sequence == "." {
        return None;
    }
    Some(sequence.trim_end_matches('.').to_owned())
}

fn clean_language(value: String) -> String {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => value,
    }
}

fn is_valid_asin(value: &str) -> bool {
    let value = value.trim();
    value.len() == 10
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

fn region_tld(region: &str) -> &'static str {
    match region {
        "ca" => ".ca",
        "uk" => ".co.uk",
        "au" => ".com.au",
        "fr" => ".fr",
        "de" => ".de",
        "jp" => ".co.jp",
        "it" => ".it",
        "in" => ".in",
        "es" => ".es",
        _ => ".com",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_audible_series_sequences() {
        assert_eq!(clean_sequence("Book 1").as_deref(), Some("1"));
        assert_eq!(
            clean_sequence("2, Dramatized Adaptation").as_deref(),
            Some("2")
        );
        assert_eq!(clean_sequence("1.5").as_deref(), Some("1.5"));
        assert_eq!(clean_sequence("Special Edition"), None);
    }

    #[test]
    fn details_map_to_abs_metadata() {
        let details: BookDetails = serde_json::from_value(serde_json::json!({
            "asin": "B002V0QK4C",
            "title": "Wizards First Rule",
            "subtitle": "Sword of Truth, Book 1",
            "authors": [{"name": "Terry Goodkind"}],
            "narrators": [{"name": "Sam Tsoutsouvas"}],
            "publisherName": "Brilliance Audio",
            "summary": "A summary",
            "releaseDate": "1994-10-01T00:00:00.000Z",
            "image": "https://example.com/cover.jpg",
            "genres": [
                {"type": "genre", "name": "Fantasy"},
                {"type": "tag", "name": "Epic"}
            ],
            "seriesPrimary": {"name": "Sword of Truth", "position": "Book 1"},
            "language": "english",
            "runtimeLengthMin": 1860,
            "formatType": "unabridged",
            "isAdult": false
        }))
        .unwrap();
        let book = book_from_details(details);
        assert_eq!(book.title, "Wizards First Rule");
        assert_eq!(book.authors, vec!["Terry Goodkind"]);
        assert_eq!(book.series, vec!["Sword of Truth #1"]);
        assert_eq!(book.genres, vec!["Fantasy"]);
        assert_eq!(book.tags, vec!["Epic"]);
        assert_eq!(book.published_year.as_deref(), Some("1994"));
        assert_eq!(book.language.as_deref(), Some("English"));
        assert_eq!(book.duration_minutes, Some(1860));
        let metadata = book.metadata_json();
        assert_eq!(metadata["title"], "Wizards First Rule");
        assert_eq!(metadata["series"][0], "Sword of Truth #1");
        assert_eq!(metadata["publishedYear"], "1994");
        assert_eq!(metadata["authors"][0], "Terry Goodkind");
    }

    #[test]
    fn scores_closest_title_author_and_duration() {
        let close = CatalogProduct {
            title: Some("Wizards First Rule".to_owned()),
            authors: Some(vec![Person {
                name: Some("Terry Goodkind".to_owned()),
            }]),
            runtime_length_min: Some(1860),
            ..Default::default()
        };
        let far = CatalogProduct {
            title: Some("A Different Book".to_owned()),
            authors: Some(vec![Person {
                name: Some("Someone Else".to_owned()),
            }]),
            runtime_length_min: Some(120),
            ..Default::default()
        };
        assert!(
            score(&close, "Wizards First Rule", "Terry Goodkind", Some(1859.0))
                > score(&far, "Wizards First Rule", "Terry Goodkind", Some(1859.0))
        );
    }
}
