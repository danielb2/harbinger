//! Client for TheIntroDB's public `/media` endpoint.
//!
//! TheIntroDB publishes community-verified intro and credits timestamps for
//! movies and TV episodes. It is used as the fallback when audio correlation
//! cannot run: a season with fewer than two decodable episodes, or a movie.
//! The v4 API documents `GET /media` with `tmdb_id` (or `tvdb_id`/`imdb_id`)
//! plus `season`/`episode`, and returns millisecond `intro`/`credits` arrays
//! where a null start means "beginning of media" and a null end means "end of
//! media": https://theintrodb.github.io/theintrodb-npm/functions.html

use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://api.theintrodb.org/v4";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Segment {
    pub(crate) start_ms: i64,
    pub(crate) end_ms: i64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Segments {
    pub(crate) intro: Option<Segment>,
    pub(crate) credits: Option<Segment>,
}

#[derive(Clone, Debug)]
pub(crate) enum LookupError {
    /// TheIntroDB has no submissions for this media.
    Missing,
    RateLimited,
    Request(String),
}

impl std::fmt::Display for LookupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(formatter, "TheIntroDB has no timestamps for this media"),
            Self::RateLimited => write!(formatter, "TheIntroDB rate limit reached"),
            Self::Request(message) => write!(formatter, "TheIntroDB request failed: {message}"),
        }
    }
}

#[derive(serde::Deserialize)]
struct RawResponse {
    #[serde(default)]
    intro: Option<Vec<RawSegment>>,
    #[serde(default)]
    credits: Option<Vec<RawSegment>>,
}

#[derive(serde::Deserialize)]
struct RawSegment {
    #[serde(default)]
    start_ms: Option<i64>,
    #[serde(default)]
    end_ms: Option<i64>,
}

fn base_url() -> String {
    std::env::var("HARBINGER_THEINTRODB_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned())
}

fn http_client() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .user_agent(concat!("Harbinger/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        })
        .clone()
}

pub(crate) fn media_query(
    base: &str,
    tmdb_id: i64,
    season_episode: Option<(u32, u32)>,
    duration_ms: Option<i64>,
) -> String {
    let mut query = form_urlencoded::Serializer::new(String::new());
    query.append_pair("tmdb_id", &tmdb_id.to_string());
    if let Some((season, episode)) = season_episode {
        query.append_pair("season", &season.to_string());
        query.append_pair("episode", &episode.to_string());
    }
    if let Some(duration_ms) = duration_ms.filter(|duration| *duration > 0) {
        query.append_pair("duration_ms", &duration_ms.to_string());
    }
    format!("{base}/media?{}", query.finish())
}

/// Merge every submission for one segment type into the range that covers all
/// of them. The API can return several entries for the same type (for example
/// mid-credits and post-credits), and Plex clients expect one marker per type.
fn merged_segment(segments: &[RawSegment], duration_ms: Option<i64>) -> Option<Segment> {
    let mut start_ms = i64::MAX;
    let mut end_ms = i64::MIN;
    let mut found = false;
    for segment in segments {
        let Some(end_ms_candidate) = segment.end_ms.or(duration_ms) else {
            continue;
        };
        let start_ms_candidate = segment.start_ms.unwrap_or(0).max(0);
        if end_ms_candidate <= start_ms_candidate {
            continue;
        }
        start_ms = start_ms.min(start_ms_candidate);
        end_ms = end_ms.max(end_ms_candidate);
        found = true;
    }
    found.then_some(Segment { start_ms, end_ms })
}

fn parse_response(body: &[u8], duration_ms: Option<i64>) -> Result<Segments, LookupError> {
    let raw: RawResponse = serde_json::from_slice(body)
        .map_err(|error| LookupError::Request(format!("invalid response body: {error}")))?;
    Ok(Segments {
        intro: merged_segment(&raw.intro.unwrap_or_default(), duration_ms),
        credits: merged_segment(&raw.credits.unwrap_or_default(), duration_ms),
    })
}

pub(crate) async fn lookup(
    tmdb_id: i64,
    season_episode: Option<(u32, u32)>,
    duration_ms: Option<i64>,
) -> Result<Segments, LookupError> {
    if tmdb_id <= 0 {
        return Err(LookupError::Missing);
    }
    let url = media_query(&base_url(), tmdb_id, season_episode, duration_ms);
    fetch_segments(&url, duration_ms).await
}

async fn fetch_segments(url: &str, duration_ms: Option<i64>) -> Result<Segments, LookupError> {
    let response = http_client()
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| LookupError::Request(error.to_string()))?;
    let status = response.status();
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(LookupError::RateLimited);
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(LookupError::Missing);
    }
    if !status.is_success() {
        // The API reports "no data for this media" as 4xx; anything else is a
        // transport/server problem that should be retried, not cached.
        if status.is_client_error() {
            return Err(LookupError::Missing);
        }
        return Err(LookupError::Request(format!("status {status}")));
    }
    let body = response
        .bytes()
        .await
        .map_err(|error| LookupError::Request(error.to_string()))?;
    parse_response(&body, duration_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_query_includes_episode_and_duration() {
        assert_eq!(
            media_query(
                "https://api.example/v4",
                1399,
                Some((1, 2)),
                Some(3_600_000)
            ),
            "https://api.example/v4/media?tmdb_id=1399&season=1&episode=2&duration_ms=3600000"
        );
    }

    #[test]
    fn media_query_omits_movie_episode_fields() {
        assert_eq!(
            media_query("https://api.example/v4", 603, None, None),
            "https://api.example/v4/media?tmdb_id=603"
        );
    }

    #[test]
    fn response_null_start_and_end_are_resolved() {
        let segments = parse_response(
            br#"{
                "tvdb_id": 169,
                "type": "movie",
                "intro": [{"start_ms": null, "end_ms": 90000}],
                "credits": [{"start_ms": 7000000, "end_ms": null}]
            }"#,
            Some(7_200_000),
        )
        .unwrap();
        assert_eq!(
            segments.intro,
            Some(Segment {
                start_ms: 0,
                end_ms: 90_000
            })
        );
        assert_eq!(
            segments.credits,
            Some(Segment {
                start_ms: 7_000_000,
                end_ms: 7_200_000
            })
        );
    }

    #[test]
    fn response_multiple_segments_merge_into_one_range() {
        let segments = parse_response(
            br#"{
                "tvdb_id": 71663,
                "type": "tv",
                "season": 1,
                "episode": 1,
                "credits": [
                    {"start_ms": 5801777, "end_ms": 6371111},
                    {"start_ms": 6408000, "end_ms": null}
                ]
            }"#,
            Some(7_200_000),
        )
        .unwrap();
        assert_eq!(
            segments.credits,
            Some(Segment {
                start_ms: 5_801_777,
                end_ms: 7_200_000
            })
        );
        assert_eq!(segments.intro, None);
    }

    #[test]
    fn response_without_segments_yields_empty_result() {
        let segments = parse_response(br#"{"tvdb_id": 71663, "type": "tv"}"#, None).unwrap();
        assert_eq!(segments, Segments::default());
        let empty =
            parse_response(br#"{"tvdb_id": 71663, "type": "tv", "intro": null}"#, None).unwrap();
        assert_eq!(empty, Segments::default());
    }

    #[test]
    fn response_unknown_end_without_duration_is_skipped() {
        let segments = parse_response(
            br#"{"tvdb_id": 1, "type": "movie", "credits": [{"start_ms": 1000, "end_ms": null}]}"#,
            None,
        )
        .unwrap();
        assert_eq!(segments.credits, None);
    }

    async fn serve_status(status: &str, body: &str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_owned();
        let body = body.to_owned();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request).await;
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn http_fetch_parses_timestamps_from_the_api() {
        let base = serve_status(
            "200 OK",
            r#"{"tvdb_id":169,"type":"movie","intro":[{"start_ms":null,"end_ms":40000}],"credits":[{"start_ms":7000000,"end_ms":null}]}"#,
        )
        .await;
        let segments = fetch_segments(&base, Some(7_200_000)).await.unwrap();
        assert_eq!(
            segments.intro,
            Some(Segment {
                start_ms: 0,
                end_ms: 40_000
            })
        );
        assert_eq!(
            segments.credits,
            Some(Segment {
                start_ms: 7_000_000,
                end_ms: 7_200_000
            })
        );
    }

    #[tokio::test]
    async fn http_fetch_classifies_missing_and_rate_limited_responses() {
        let missing = serve_status("404 Not Found", r#"{"error":"not found"}"#).await;
        assert!(matches!(
            fetch_segments(&missing, None).await,
            Err(LookupError::Missing)
        ));
        let limited = serve_status("429 Too Many Requests", "{}").await;
        assert!(matches!(
            fetch_segments(&limited, None).await,
            Err(LookupError::RateLimited)
        ));
        let failing = serve_status("500 Internal Server Error", "{}").await;
        assert!(matches!(
            fetch_segments(&failing, None).await,
            Err(LookupError::Request(_))
        ));
    }
}
