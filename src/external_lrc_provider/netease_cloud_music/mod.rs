use anyhow::{anyhow, Result};
use netease_cloud_music_api::MusicApi;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info};

use crate::lrc::{LrcLine, TimeTag};

#[derive(Clone)]
pub struct NetEaseProvider {
    api: MusicApi,
}

impl std::fmt::Debug for NetEaseProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetEaseProvider")
            .field("api", &"MusicApi(...)") // MusicApi doesn't implement Debug, so we show a placeholder
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: u64,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration: u64, // Duration in milliseconds
    pub similarity_score: f64,
}

impl NetEaseProvider {
    pub fn new() -> Self {
        Self {
            api: MusicApi::new(10), // Use 10 as max connections
        }
    }

    /// Search for songs based on title and artist
    pub async fn search_songs(
        &self,
        title: &str,
        artist: &str,
        limit: u16,
    ) -> Result<Vec<SearchResult>> {
        let keywords = format!("{} {}", title, artist);

        debug!("Searching NetEase for: '{}'", keywords);

        // Search for songs using NetEase API
        let songs = self
            .api
            .search_song(keywords, 0, limit)
            .await
            .map_err(|e| anyhow!("NetEase search failed: {}", e))?;

        debug!("NetEase returned {} results", songs.len());

        let mut results = Vec::new();

        for song in songs {
            let artists_str = song.singer.clone();

            let similarity_score = calculate_similarity(title, artist, &song.name, &artists_str);

            let result = SearchResult {
                id: song.id,
                name: song.name,
                artist: artists_str,
                album: song.album,
                duration: song.duration,
                similarity_score,
            };

            results.push(result);
        }

        // Sort by similarity score (highest first)
        results.sort_by(|a, b| b.similarity_score.partial_cmp(&a.similarity_score).unwrap());

        Ok(results)
    }

    /// Get lyrics for a specific song ID
    pub async fn get_lyrics(&self, song_id: u64) -> Result<Vec<LrcLine>> {
        debug!("Fetching lyrics for NetEase song ID: {}", song_id);

        let lyrics = self
            .api
            .song_lyric(song_id)
            .await
            .map_err(|e| anyhow!("Failed to fetch lyrics from NetEase: {}", e))?;

        debug!("NetEase returned {} lyric lines", lyrics.lyric.len());

        let mut lrc_lines = Vec::new();

        // Parse lyrics from NetEase format to LrcLine format
        for line in lyrics.lyric {
            if let Some(lrc_line) = parse_netease_lyric_line(&line) {
                lrc_lines.push(lrc_line);
            }
        }

        // Sort by timestamp
        lrc_lines.sort_by(|a, b| a.time[0].cmp(&b.time[0]));

        info!(
            "Successfully parsed {} LRC lines from NetEase",
            lrc_lines.len()
        );

        Ok(lrc_lines)
    }

    /// Search and get lyrics for the best matching song
    pub async fn search_and_get_lyrics(
        &self,
        title: &str,
        artist: &str,
        duration_ms: Option<u64>,
    ) -> Result<Vec<LrcLine>> {
        // Search for songs
        let search_results = self.search_songs(title, artist, 10).await?;

        if search_results.is_empty() {
            return Err(anyhow!(
                "No songs found on NetEase for '{}' by '{}'",
                title,
                artist
            ));
        }

        // Find the best match
        let best_match = if let Some(target_duration) = duration_ms {
            // If we have duration info, factor it into the selection
            search_results
                .iter()
                .min_by_key(|result| {
                    let duration_diff = (result.duration as i64 - target_duration as i64).abs();
                    let duration_score =
                        1.0 - (duration_diff as f64 / target_duration as f64).min(1.0);
                    let combined_score = result.similarity_score * 0.7 + duration_score * 0.3;
                    -(combined_score * 1000.0) as i64 // Negative for min_by
                })
                .ok_or_else(|| anyhow!("No suitable match found"))?
        } else {
            // Use the highest similarity score
            &search_results[0]
        };

        info!(
            "Selected NetEase song: '{}' by '{}' (ID: {}, similarity: {:.2})",
            best_match.name, best_match.artist, best_match.id, best_match.similarity_score
        );

        // Get lyrics for the best match
        self.get_lyrics(best_match.id).await
    }
}

/// Parse a single NetEase lyric line in LRC format
fn parse_netease_lyric_line(line: &str) -> Option<LrcLine> {
    // NetEase lyrics are already in LRC format: [mm:ss.xxx]lyrics
    if line.trim().is_empty() {
        return None;
    }

    // Parse LRC timestamp format: [mm:ss.xxx]
    let time_end = line.find(']')?;
    let time_str = &line[1..time_end]; // Remove the opening '['
    let text = line[time_end + 1..].to_string();

    // Parse mm:ss.xxx format
    let parts: Vec<&str> = time_str.split(':').collect();
    if parts.len() != 2 {
        return None;
    }

    let minutes: u64 = parts[0].parse().ok()?;
    let seconds_parts: Vec<&str> = parts[1].split('.').collect();
    if seconds_parts.len() != 2 {
        return None;
    }

    let seconds: u64 = seconds_parts[0].parse().ok()?;
    let milliseconds: u64 = seconds_parts[1].parse().ok()?;

    let timestamp_ms = minutes * 60 * 1000 + seconds * 1000 + milliseconds;
    let time_tag = TimeTag::from(Duration::from_millis(timestamp_ms));

    Some(LrcLine {
        time: vec![time_tag],
        text,
    })
}

/// Calculate similarity between search query and result
fn calculate_similarity(
    query_title: &str,
    query_artist: &str,
    result_title: &str,
    result_artist: &str,
) -> f64 {
    let title_similarity = string_similarity(query_title, result_title);
    let artist_similarity = string_similarity(query_artist, result_artist);

    // Weight title similarity more heavily
    title_similarity * 0.7 + artist_similarity * 0.3
}

/// Calculate similarity between two strings using a simple algorithm
fn string_similarity(a: &str, b: &str) -> f64 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();

    if a_lower == b_lower {
        return 1.0;
    }

    // Check if one contains the other
    if a_lower.contains(&b_lower) || b_lower.contains(&a_lower) {
        return 0.8;
    }

    // Simple Levenshtein-like distance
    let max_len = a_lower.len().max(b_lower.len());
    if max_len == 0 {
        return 1.0;
    }

    let distance = levenshtein_distance(&a_lower, &b_lower);
    1.0 - (distance as f64 / max_len as f64)
}

/// Calculate Levenshtein distance between two strings
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut matrix = vec![vec![0; b_len + 1]; a_len + 1];

    for i in 0..=a_len {
        matrix[i][0] = i;
    }
    for j in 0..=b_len {
        matrix[0][j] = j;
    }

    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }

    matrix[a_len][b_len]
}
