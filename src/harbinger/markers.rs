use std::{
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

const SAMPLE_RATE: u32 = 2_000;
const FRAME_SAMPLES: usize = (SAMPLE_RATE / 2) as usize;
const FRAME_MS: i64 = 500;
const SEARCH_WINDOW: Duration = Duration::from_secs(600);
const BAND_COUNT: usize = 16;
const MIN_USEFUL_RMS: f32 = 0.008;
const MIN_RUN_FRAMES: usize = 24;
const MAX_RUN_FRAMES: usize = 360;
const MIN_USEFUL_FRAME_RATIO: f32 = 0.8;
const DEFAULT_THRESHOLD: f32 = 0.6;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum MarkerKind {
    Intro,
    Credits,
}

impl MarkerKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Intro => "intro",
            Self::Credits => "credits",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SkipMarker {
    pub(crate) id: i64,
    #[serde(rename = "type")]
    pub(crate) kind: MarkerKind,
    #[serde(rename = "startTimeOffset")]
    pub(crate) start_ms: i64,
    #[serde(rename = "endTimeOffset")]
    pub(crate) end_ms: i64,
    #[serde(rename = "final", skip_serializing_if = "is_false")]
    pub(crate) final_marker: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl MarkerKind {
    /// Plex documents this request field only as an integer. Values 1/2 match
    /// the marker type IDs used by Plex-compatible implementations; the string
    /// forms are accepted for robustness.
    pub(crate) fn from_marker_input(value: &str) -> Option<Self> {
        match value.trim() {
            "1" | "intro" => Some(Self::Intro),
            "2" | "credits" | "credit" => Some(Self::Credits),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MarkerSource {
    Detected,
    User,
}

impl MarkerSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::User => "user",
        }
    }
}

pub(crate) fn ensure_marker_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS media_markers (
             marker_id INTEGER PRIMARY KEY AUTOINCREMENT,
             media_key TEXT NOT NULL,
             marker_type TEXT NOT NULL CHECK(marker_type IN ('intro', 'credits')),
             start_ms INTEGER NOT NULL,
             end_ms INTEGER NOT NULL,
             final_marker INTEGER NOT NULL DEFAULT 0,
             source TEXT NOT NULL,
             media_fingerprint TEXT NOT NULL DEFAULT '',
             group_fingerprint TEXT NOT NULL DEFAULT '',
             created_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS media_markers_lookup
             ON media_markers(media_key, marker_type, start_ms);",
    )
}

pub(crate) fn markers_for_media(
    connection: &Connection,
    media_key: &str,
) -> Result<Vec<SkipMarker>, rusqlite::Error> {
    let mut statement = connection.prepare(
        "SELECT marker_id, marker_type, start_ms, end_ms, final_marker
           FROM media_markers
          WHERE media_key = ?1
          ORDER BY start_ms, marker_id",
    )?;
    let rows = statement.query_map([media_key], |row| {
        Ok(SkipMarker {
            id: row.get(0)?,
            kind: parse_marker_kind(&row.get::<_, String>(1)?),
            start_ms: row.get(2)?,
            end_ms: row.get(3)?,
            final_marker: row.get::<_, i64>(4)? != 0,
        })
    })?;
    rows.collect()
}

fn parse_marker_kind(value: &str) -> MarkerKind {
    if value == "credits" {
        MarkerKind::Credits
    } else {
        MarkerKind::Intro
    }
}

pub(crate) fn marker_fingerprints(
    connection: &Connection,
    media_key: &str,
    kind: MarkerKind,
) -> Result<Option<(String, String)>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT media_fingerprint, group_fingerprint
               FROM media_markers
              WHERE media_key = ?1 AND marker_type = ?2 AND source = 'detected'
              LIMIT 1",
            params![media_key, kind.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
}

pub(crate) fn replace_detected_markers(
    connection: &Connection,
    media_key: &str,
    kind: MarkerKind,
    range: Option<&DetectedRange>,
    media_fingerprint: &str,
    group_fingerprint: &str,
    duration_ms: i64,
) -> Result<(), rusqlite::Error> {
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        "DELETE FROM media_markers
          WHERE media_key = ?1 AND marker_type = ?2 AND source = 'detected'",
        params![media_key, kind.as_str()],
    )?;
    if let Some(range) = range {
        insert_marker(
            &transaction,
            media_key,
            kind,
            range.start_ms,
            range.end_ms,
            MarkerSource::Detected,
            media_fingerprint,
            group_fingerprint,
            duration_ms,
        )?;
    }
    transaction.commit()
}

pub(crate) fn create_user_marker(
    connection: &Connection,
    media_key: &str,
    kind: MarkerKind,
    start_ms: i64,
    end_ms: i64,
    duration_ms: i64,
) -> Result<SkipMarker, rusqlite::Error> {
    let marker_id = insert_marker(
        connection,
        media_key,
        kind,
        start_ms,
        end_ms,
        MarkerSource::User,
        "",
        "",
        duration_ms,
    )?;
    Ok(SkipMarker {
        id: marker_id,
        kind,
        start_ms,
        end_ms,
        final_marker: is_final(kind, end_ms, duration_ms),
    })
}

pub(crate) fn update_marker(
    connection: &Connection,
    marker_id: i64,
    kind: MarkerKind,
    start_ms: i64,
    end_ms: i64,
    duration_ms: i64,
) -> Result<Option<SkipMarker>, rusqlite::Error> {
    let final_marker = is_final(kind, end_ms, duration_ms);
    let updated = connection.execute(
        "UPDATE media_markers
            SET marker_type = ?1, start_ms = ?2, end_ms = ?3, final_marker = ?4
          WHERE marker_id = ?5",
        params![
            kind.as_str(),
            start_ms,
            end_ms,
            i64::from(final_marker),
            marker_id
        ],
    )?;
    if updated == 0 {
        return Ok(None);
    }
    Ok(Some(SkipMarker {
        id: marker_id,
        kind,
        start_ms,
        end_ms,
        final_marker,
    }))
}

pub(crate) fn delete_marker(
    connection: &Connection,
    media_key: &str,
    marker_id: i64,
) -> Result<bool, rusqlite::Error> {
    Ok(connection.execute(
        "DELETE FROM media_markers WHERE media_key = ?1 AND marker_id = ?2",
        params![media_key, marker_id],
    )? > 0)
}

pub(crate) fn delete_section_markers(
    connection: &Connection,
    section_id: u64,
) -> Result<usize, rusqlite::Error> {
    Ok(connection.execute(
        "DELETE FROM media_markers WHERE media_key LIKE ?1",
        [format!("{section_id}-%")],
    )?)
}

#[allow(clippy::too_many_arguments)]
fn insert_marker(
    connection: &Connection,
    media_key: &str,
    kind: MarkerKind,
    start_ms: i64,
    end_ms: i64,
    source: MarkerSource,
    media_fingerprint: &str,
    group_fingerprint: &str,
    duration_ms: i64,
) -> Result<i64, rusqlite::Error> {
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    connection.execute(
        "INSERT INTO media_markers
             (media_key, marker_type, start_ms, end_ms, final_marker, source,
              media_fingerprint, group_fingerprint, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            media_key,
            kind.as_str(),
            start_ms,
            end_ms,
            i64::from(is_final(kind, end_ms, duration_ms)),
            source.as_str(),
            media_fingerprint,
            group_fingerprint,
            created_at
        ],
    )?;
    Ok(connection.last_insert_rowid())
}

fn is_final(kind: MarkerKind, end_ms: i64, duration_ms: i64) -> bool {
    kind == MarkerKind::Credits && duration_ms > 0 && end_ms >= duration_ms.saturating_sub(1_000)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DetectedRange {
    pub(crate) start_ms: i64,
    pub(crate) end_ms: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct EpisodeAudio {
    pub(crate) frames: Vec<AudioFrame>,
    pub(crate) window_start_ms: i64,
    pub(crate) duration_ms: i64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AudioFrame {
    pub(crate) bands: [f32; BAND_COUNT],
    pub(crate) useful: bool,
}

pub(crate) fn detection_threshold(value: Option<f64>) -> f32 {
    let threshold = value
        .and_then(|value| {
            if value.is_finite() && value > 0.0 {
                Some(value as f32)
            } else {
                None
            }
        })
        .unwrap_or(DEFAULT_THRESHOLD);
    threshold.clamp(0.001, 1.0)
}

pub(crate) async fn decode_audio_window(
    path: &std::path::Path,
    kind: MarkerKind,
    duration_ms: Option<i64>,
) -> Result<EpisodeAudio, String> {
    let ffmpeg = std::env::var("HARBINGER_FFMPEG").unwrap_or_else(|_| "ffmpeg".to_owned());
    let window_ms = SEARCH_WINDOW.as_millis() as i64;
    let window_start_ms = match kind {
        MarkerKind::Intro => 0,
        MarkerKind::Credits => duration_ms
            .unwrap_or(window_ms)
            .saturating_sub(window_ms)
            .max(0),
    };
    let window_seconds = (window_ms as f64) / 1000.0;
    let start_seconds = (window_start_ms as f64) / 1000.0;

    let output = Command::new(ffmpeg)
        .arg("-nostdin")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-ss")
        .arg(start_seconds.to_string())
        .arg("-i")
        .arg(path)
        .arg("-t")
        .arg(window_seconds.to_string())
        .arg("-map")
        .arg("0:a:0")
        .arg("-vn")
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg(SAMPLE_RATE.to_string())
        .arg("-f")
        .arg("s16le")
        .arg("-c:a")
        .arg("pcm_s16le")
        .arg("pipe:1")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .output()
        .await
        .map_err(|error| format!("could not start ffmpeg audio decoder: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            "ffmpeg could not decode the audio stream".to_owned()
        } else {
            format!("ffmpeg could not decode the audio stream: {stderr}")
        });
    }

    let frames = frames_from_pcm(&output.stdout);
    if frames.len() < MIN_RUN_FRAMES {
        return Err("audio window is too short for marker detection".to_owned());
    }

    Ok(EpisodeAudio {
        frames,
        window_start_ms,
        duration_ms: duration_ms.unwrap_or(i64::MAX),
    })
}

pub(crate) fn detect_shared_ranges(
    episodes: &[EpisodeAudio],
    threshold: f32,
) -> Vec<Option<DetectedRange>> {
    let mut best = vec![None; episodes.len()];
    if episodes.len() < 2 {
        return best;
    }

    for first in 0..episodes.len() {
        for second in (first + 1)..episodes.len() {
            let (first_range, second_range) =
                longest_shared_run(&episodes[first], &episodes[second], threshold);
            keep_better(&mut best[first], first_range);
            keep_better(&mut best[second], second_range);
        }
    }
    best
}

fn keep_better(current: &mut Option<DetectedRange>, candidate: Option<DetectedRange>) {
    let Some(candidate) = candidate else { return };
    let replace = match current {
        Some(current) => {
            (candidate.end_ms - candidate.start_ms) > (current.end_ms - current.start_ms)
                || ((candidate.end_ms - candidate.start_ms) == (current.end_ms - current.start_ms)
                    && candidate.start_ms < current.start_ms)
        }
        None => true,
    };
    if replace {
        *current = Some(candidate);
    }
}

fn longest_shared_run(
    first: &EpisodeAudio,
    second: &EpisodeAudio,
    threshold: f32,
) -> (Option<DetectedRange>, Option<DetectedRange>) {
    let (first_frames, second_frames) = (&first.frames, &second.frames);
    if first_frames.len() < MIN_RUN_FRAMES || second_frames.len() < MIN_RUN_FRAMES {
        return (None, None);
    }

    let mut baseline_sum = 0.0f64;
    let mut baseline_count = 0usize;
    for first_frame in first_frames {
        for second_frame in second_frames {
            if first_frame.useful && second_frame.useful {
                baseline_sum += band_distance(&first_frame.bands, &second_frame.bands) as f64;
                baseline_count += 1;
            }
        }
    }
    if baseline_count == 0 {
        return (None, None);
    }
    let baseline = baseline_sum / baseline_count as f64;
    if !baseline.is_finite() || baseline <= f64::EPSILON {
        return (None, None);
    }
    let limit = baseline * threshold as f64;

    let mut best: Option<(usize, f64, usize, usize)> = None;
    let first_len = first_frames.len() as isize;
    let second_len = second_frames.len() as isize;
    for lag in -(second_len - 1)..first_len {
        let (first_base, second_base, overlap) = if lag >= 0 {
            let lag = lag as usize;
            (
                0usize,
                lag,
                first_frames
                    .len()
                    .min(second_frames.len().saturating_sub(lag)),
            )
        } else {
            let lag = (-lag) as usize;
            (
                lag,
                0usize,
                second_frames
                    .len()
                    .min(first_frames.len().saturating_sub(lag)),
            )
        };
        if overlap < MIN_RUN_FRAMES {
            continue;
        }

        let mut prefix_sum = Vec::with_capacity(overlap + 1);
        let mut useful_counts = Vec::with_capacity(overlap + 1);
        prefix_sum.push(0.0f64);
        useful_counts.push(0usize);
        let mut low_frames = 0usize;
        for offset in 0..overlap {
            let first_frame = &first_frames[first_base + offset];
            let second_frame = &second_frames[second_base + offset];
            let useful = first_frame.useful && second_frame.useful;
            let distance = if useful {
                band_distance(&first_frame.bands, &second_frame.bands) as f64
            } else {
                limit * 2.0
            };
            if useful && distance <= limit {
                low_frames += 1;
            }
            prefix_sum.push(prefix_sum[offset] + distance);
            useful_counts.push(useful_counts[offset] + usize::from(useful));
        }
        if low_frames < MIN_RUN_FRAMES {
            continue;
        }

        for window in MIN_RUN_FRAMES..=overlap.min(MAX_RUN_FRAMES) {
            for start in 0..=(overlap - window) {
                let end = start + window;
                let useful = useful_counts[end] - useful_counts[start];
                if (useful as f32) < (window as f32) * MIN_USEFUL_FRAME_RATIO {
                    continue;
                }
                let mean = (prefix_sum[end] - prefix_sum[start]) / window as f64;
                if mean > limit {
                    continue;
                }
                let score = mean * (MIN_RUN_FRAMES as f64 / window as f64).sqrt();
                let better = match best {
                    Some((best_window, best_mean, _, _)) => {
                        let best_score =
                            best_mean * (MIN_RUN_FRAMES as f64 / best_window as f64).sqrt();
                        score < best_score || (score == best_score && window > best_window)
                    }
                    None => true,
                };
                if better {
                    best = Some((window, mean, first_base + start, second_base + start));
                }
            }
        }
    }

    let Some((window, _, first_start, second_start)) = best else {
        return (None, None);
    };
    (
        range_for(first, first_start, window),
        range_for(second, second_start, window),
    )
}

fn range_for(episode: &EpisodeAudio, start_frame: usize, length: usize) -> Option<DetectedRange> {
    let start_ms = episode
        .window_start_ms
        .saturating_add((start_frame as i64).saturating_mul(FRAME_MS));
    let end_ms = episode
        .window_start_ms
        .saturating_add(((start_frame + length) as i64).saturating_mul(FRAME_MS))
        .min(episode.duration_ms);
    if start_ms < 0 || end_ms <= start_ms || end_ms - start_ms < (MIN_RUN_FRAMES as i64) * FRAME_MS
    {
        return None;
    }
    Some(DetectedRange { start_ms, end_ms })
}

fn band_distance(first: &[f32; BAND_COUNT], second: &[f32; BAND_COUNT]) -> f32 {
    first
        .iter()
        .zip(second.iter())
        .map(|(first, second)| {
            let delta = first - second;
            delta * delta
        })
        .sum()
}

fn frames_from_pcm(bytes: &[u8]) -> Vec<AudioFrame> {
    let samples: Vec<f32> = bytes
        .chunks_exact(2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 32768.0)
        .collect();
    if samples.len() < FRAME_SAMPLES {
        return Vec::new();
    }

    let cosines = band_table(true);
    let sines = band_table(false);
    samples
        .chunks_exact(FRAME_SAMPLES)
        .map(|frame| AudioFrame::from_samples(frame, &cosines, &sines))
        .collect()
}

fn band_table(cosine: bool) -> Vec<[f32; FRAME_SAMPLES]> {
    (0..BAND_COUNT)
        .map(|band| {
            let frequency = 80.0 + (band as f32) * 55.0;
            (0..FRAME_SAMPLES)
                .map(|index| {
                    let angle = 2.0 * std::f32::consts::PI * frequency * (index as f32)
                        / (SAMPLE_RATE as f32);
                    if cosine {
                        angle.cos()
                    } else {
                        -angle.sin()
                    }
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap_or_else(|_| unreachable!("fixed-size band table"))
        })
        .collect()
}

impl AudioFrame {
    fn from_samples(
        samples: &[f32],
        cosines: &[[f32; FRAME_SAMPLES]],
        sines: &[[f32; FRAME_SAMPLES]],
    ) -> Self {
        let rms = (samples.iter().map(|sample| sample * sample).sum::<f32>()
            / samples.len() as f32)
            .sqrt();
        let mut bands = [0.0f32; BAND_COUNT];
        for (band, (cosine, sine)) in cosines.iter().zip(sines.iter()).enumerate() {
            let mut real = 0.0f32;
            let mut imaginary = 0.0f32;
            for (sample, (cosine_value, sine_value)) in samples.iter().zip(cosine.iter().zip(sine))
            {
                real += sample * cosine_value;
                imaginary += sample * sine_value;
            }
            bands[band] = real.hypot(imaginary) / samples.len() as f32;
        }
        let norm = bands.iter().map(|value| value * value).sum::<f32>().sqrt();
        if norm > f32::EPSILON {
            for value in &mut bands {
                *value /= norm;
            }
        }
        Self {
            bands,
            useful: rms > MIN_USEFUL_RMS && norm > f32::EPSILON,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_frame(seed: usize) -> AudioFrame {
        let mut bands = [0.0f32; BAND_COUNT];
        for (index, band) in bands.iter_mut().enumerate() {
            let wave = (((seed + index * 3) % 11) as f32) / 10.0;
            *band = 0.2 + wave * 0.8;
        }
        let norm = bands.iter().map(|value| value * value).sum::<f32>().sqrt();
        for value in &mut bands {
            *value /= norm;
        }
        AudioFrame {
            bands,
            useful: true,
        }
    }

    fn episode(frames: Vec<AudioFrame>, window_start_ms: i64, duration_ms: i64) -> EpisodeAudio {
        EpisodeAudio {
            frames,
            window_start_ms,
            duration_ms,
        }
    }

    fn shared_sequence(length: usize) -> Vec<AudioFrame> {
        (0..length).map(synthetic_frame).collect()
    }

    fn padding_frame() -> AudioFrame {
        let mut bands = [0.0f32; BAND_COUNT];
        bands[0] = 1.0;
        AudioFrame {
            bands,
            useful: true,
        }
    }

    #[test]
    fn detects_shared_audio_at_different_offsets() {
        let shared = shared_sequence(40);
        let mut first = vec![synthetic_frame(900); 7];
        first.extend(shared.iter().cloned());
        let mut second = vec![synthetic_frame(901); 19];
        second.extend(shared.iter().cloned());

        let detected = detect_shared_ranges(
            &[episode(first, 0, 60_000), episode(second, 0, 60_000)],
            DEFAULT_THRESHOLD,
        );

        assert_eq!(
            detected[0],
            Some(DetectedRange {
                start_ms: 3_500,
                end_ms: 23_500,
            })
        );
        assert_eq!(
            detected[1],
            Some(DetectedRange {
                start_ms: 9_500,
                end_ms: 29_500,
            })
        );
    }

    #[test]
    fn detects_shared_window_with_imperfect_frames() {
        let shared = (0..48)
            .map(|index| {
                let mut frame = synthetic_frame(index);
                if index % 2 == 0 {
                    frame.bands[0] += 0.01;
                    let norm = frame
                        .bands
                        .iter()
                        .map(|value| value * value)
                        .sum::<f32>()
                        .sqrt();
                    for value in &mut frame.bands {
                        *value /= norm;
                    }
                }
                frame
            })
            .collect::<Vec<_>>();
        let mut first = vec![padding_frame(); 9];
        first.extend(shared.iter().cloned());
        let mut second = vec![synthetic_frame(700); 21];
        second.extend(shared.iter().cloned());

        let detected = detect_shared_ranges(
            &[episode(first, 0, 60_000), episode(second, 0, 60_000)],
            DEFAULT_THRESHOLD,
        );

        let first_range = detected[0]
            .as_ref()
            .expect("first shared window was not detected");
        let second_range = detected[1]
            .as_ref()
            .expect("second shared window was not detected");
        assert!(first_range.start_ms <= 5_000 && first_range.start_ms >= 4_000);
        assert!(second_range.start_ms <= 11_000 && second_range.start_ms >= 10_000);
        assert!(first_range.end_ms - first_range.start_ms >= 20_000);
        assert!(second_range.end_ms - second_range.start_ms >= 20_000);
    }

    #[test]
    fn detects_ending_audio_with_different_window_offsets() {
        let shared = shared_sequence(36);
        let mut first = vec![padding_frame(); 5];
        first.extend(shared.clone());
        let mut second = vec![padding_frame(); 11];
        second.extend(shared.clone());

        let detected = detect_shared_ranges(
            &[
                episode(first, 30_000, 50_500),
                episode(second, 31_000, 54_500),
            ],
            DEFAULT_THRESHOLD,
        );

        assert!(detected[0].is_some());
        assert_eq!(detected[0].as_ref().unwrap().end_ms, 50_500);
        assert!(detected[1].is_some());
        assert_eq!(detected[1].as_ref().unwrap().end_ms, 54_500);
    }

    #[test]
    fn ignores_silence_and_unrelated_audio() {
        let silence = AudioFrame {
            bands: [0.0; BAND_COUNT],
            useful: false,
        };
        let first = vec![silence; 60];
        let second = (0..60).map(|index| synthetic_frame(index + 30)).collect();

        let detected = detect_shared_ranges(
            &[episode(first, 0, 60_000), episode(second, 0, 60_000)],
            DEFAULT_THRESHOLD,
        );

        assert_eq!(detected, vec![None, None]);
    }

    #[test]
    fn ignores_short_shared_runs() {
        let shared = shared_sequence(12);
        let first = [vec![synthetic_frame(500); 5], shared.clone()].concat();
        let second = [vec![synthetic_frame(501); 17], shared].concat();

        let detected = detect_shared_ranges(
            &[episode(first, 0, 60_000), episode(second, 0, 60_000)],
            DEFAULT_THRESHOLD,
        );

        assert_eq!(detected, vec![None, None]);
    }

    #[test]
    fn ignores_steady_tones() {
        let tone = synthetic_frame(42);
        let first = vec![tone; 60];
        let second = vec![tone; 60];

        let detected = detect_shared_ranges(
            &[episode(first, 0, 60_000), episode(second, 0, 60_000)],
            DEFAULT_THRESHOLD,
        );

        assert_eq!(detected, vec![None, None]);
    }

    #[test]
    fn marker_json_uses_plex_field_names() {
        let marker = SkipMarker {
            id: 7,
            kind: MarkerKind::Credits,
            start_ms: 1_000,
            end_ms: 2_000,
            final_marker: true,
        };
        let json = serde_json::to_value(&marker).unwrap();
        assert_eq!(json["type"], "credits");
        assert_eq!(json["startTimeOffset"], 1_000);
        assert_eq!(json["endTimeOffset"], 2_000);
        assert_eq!(json["final"], true);
    }

    #[test]
    fn decoding_band_table_is_fixed_size() {
        let cosines = band_table(true);
        assert_eq!(cosines.len(), BAND_COUNT);
        assert!(cosines.iter().all(|table| table.len() == FRAME_SAMPLES));
    }

    #[test]
    fn threshold_is_clamped() {
        assert_eq!(detection_threshold(None), DEFAULT_THRESHOLD);
        assert_eq!(detection_threshold(Some(-1.0)), DEFAULT_THRESHOLD);
        assert_eq!(detection_threshold(Some(10.0)), 1.0);
    }
}
