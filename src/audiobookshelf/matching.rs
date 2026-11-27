use super::{
    catalog::{Catalog, Item},
    providers::{self, MatchedBook},
    scanner,
};
use crate::harbinger::{is_audiobook_library, AppState, ConfigLibrary};
use std::{
    fs,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const REQUEST_SPACING: Duration = Duration::from_millis(250);
const RETRY_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

pub(crate) async fn match_library(
    state: &AppState,
    library_index: usize,
    library: &ConfigLibrary,
    force: bool,
) -> usize {
    if !is_audiobook_library(&library.kind) {
        return 0;
    }
    super::catalog::invalidate_cache();
    let catalog = Catalog::load_async(state).await;
    let Some(catalog_library) = catalog
        .libraries
        .iter()
        .find(|candidate| candidate.source_index == library_index)
    else {
        return 0;
    };
    let pending = catalog_library
        .items
        .iter()
        .filter(|item| !item.metadata_present)
        .cloned()
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return 0;
    }
    let Ok(client) = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(format!("harbinger/{}", env!("CARGO_PKG_VERSION")))
        .build()
    else {
        tracing::warn!(library = %library.name, "Audible matching client could not be built");
        return 0;
    };
    let mut matched = 0;
    for item in pending {
        let sidecar = super::catalog::item_sidecar_dir(state, library_index, &item);
        if !force && recently_attempted(&sidecar) {
            continue;
        }
        if let Some(book) = find_match(&client, &item).await {
            if persist_match(&client, state, library_index, &item, &book, false).await {
                matched += 1;
                tracing::info!(
                    library = %library.name,
                    title = %item.title,
                    matched_title = %book.title,
                    asin = ?book.asin,
                    duration_minutes = ?book.duration_minutes,
                    "Audiobook metadata matched"
                );
            }
        }
        record_attempt(&sidecar);
        tokio::time::sleep(REQUEST_SPACING).await;
    }
    if matched > 0 {
        super::catalog::invalidate_cache();
    }
    matched
}

async fn find_match(client: &reqwest::Client, item: &Item) -> Option<MatchedBook> {
    let title = item
        .metadata
        .title
        .as_deref()
        .unwrap_or(item.title.as_str());
    let author = item.metadata.authors.join(", ");
    let asin = (!item.metadata.asin.trim().is_empty()).then_some(item.metadata.asin.as_str());
    let duration_minutes = (item.duration > 0.0).then_some(item.duration / 60.0);
    providers::automatic_match(client, title, &author, asin, duration_minutes).await
}

pub(crate) async fn persist_match(
    client: &reqwest::Client,
    state: &AppState,
    library_index: usize,
    item: &Item,
    book: &MatchedBook,
    overwrite_cover: bool,
) -> bool {
    let sidecar = super::catalog::item_sidecar_dir(state, library_index, item);
    if fs::create_dir_all(&sidecar).is_err() {
        tracing::warn!(path = %sidecar.display(), "Audiobook metadata folder could not be created");
        return false;
    }
    let metadata = match serde_json::to_vec_pretty(&book.metadata_json()) {
        Ok(metadata) => metadata,
        Err(error) => {
            tracing::warn!(title = %item.title, error = %error, "Audiobook metadata could not be serialized");
            return false;
        }
    };
    if let Err(error) = fs::write(sidecar.join("metadata.json"), metadata) {
        tracing::warn!(path = %sidecar.display(), error = %error, "Audiobook metadata could not be saved");
        return false;
    }
    if overwrite_cover || scanner::stored_cover(&sidecar).is_none() {
        if let Some(url) = book.cover_url.as_deref() {
            if let Some(bytes) = download_image(client, url).await {
                if let Some(extension) = scanner::image_extension(&bytes) {
                    super::catalog::write_cover_file(&sidecar, &bytes, extension);
                }
            }
        }
    }
    if let Some(cover) = scanner::stored_cover(&sidecar) {
        super::catalog::store_cover_reference(&sidecar, &cover);
    }
    true
}

fn recently_attempted(sidecar: &std::path::Path) -> bool {
    fs::metadata(sidecar.join("match.json"))
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|elapsed| elapsed < RETRY_AFTER)
}

fn record_attempt(sidecar: &std::path::Path) {
    if fs::create_dir_all(sidecar).is_err() {
        return;
    }
    let attempted_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let _ = fs::write(
        sidecar.join("match.json"),
        serde_json::json!({ "attemptedAt": attempted_at }).to_string(),
    );
}

async fn download_image(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        tracing::debug!(url, status = %response.status(), "Audiobook cover download failed");
        return None;
    }
    let bytes = response.bytes().await.ok()?.to_vec();
    if bytes.is_empty() {
        tracing::debug!(url, "Audiobook cover download returned no data");
        return None;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_match_attempts_are_throttled_for_a_day() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sidecar = std::env::temp_dir().join(format!(
            "mplex-matching-throttle-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&sidecar).unwrap();
        assert!(!recently_attempted(&sidecar));
        record_attempt(&sidecar);
        assert!(recently_attempted(&sidecar));
        fs::remove_dir_all(sidecar).unwrap();
    }
}
