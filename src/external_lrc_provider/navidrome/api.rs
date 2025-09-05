use anyhow::{anyhow, Result};
use md5;
use reqwest::Client;
use std::collections::HashMap;
use tracing::{debug, error, warn};

use crate::external_lrc_provider::navidrome::{
    metadata::TrackMetadata,
    types::*,
    utils::{calculate_similarity, convert_to_lrc},
};

/// Navidrome API client
pub struct NavidromeClient {
    config: NavidromeConfig,
    client: Client,
}

impl NavidromeClient {
    /// Create new Navidrome client
    pub fn new(config: NavidromeConfig) -> Self {
        Self {
            config,
            client: Client::new(),
        }
    }

    /// Fetch lyrics for the given track metadata
    pub async fn fetch_lyrics(&self, metadata: &TrackMetadata) -> Result<String> {
        debug!(
            "Fetching lyrics for: {} - {}",
            metadata.artist, metadata.title
        );

        // First, search for the song to get its ID
        let song_id = self.search_song(metadata).await?;
        debug!("Found song ID: {}", song_id);

        // Then fetch lyrics using the song ID
        self.get_lyrics_by_id(&song_id).await
    }

    /// Search for a song and return the best matching song ID
    async fn search_song(&self, metadata: &TrackMetadata) -> Result<String> {
        let search_query = format!("{} {}", metadata.artist, metadata.title);
        let url = format!("{}/rest/search3", self.config.server_url);

        let auth_params = self.generate_auth_params();
        let mut params = vec![
            ("query", search_query.as_str()),
            ("songCount", "10"),
            ("f", "json"),
        ];
        params.extend(auth_params.iter().map(|(k, v)| (k.as_str(), v.as_str())));

        debug!("Search URL: {}", url);
        debug!("Search params: {:?}", params);

        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Search request failed: {} - {}", status, body);
            return Err(anyhow!("Search request failed: {}", status));
        }

        let search_response: SearchResponse = response.json().await?;
        debug!("Search response: {:?}", search_response);

        if search_response.subsonic_response.status != "ok" {
            return Err(anyhow!("Search API returned error status"));
        }

        let search_result = search_response
            .subsonic_response
            .search_result3
            .ok_or_else(|| anyhow!("No search results"))?;

        if search_result.song.is_empty() {
            return Err(anyhow!("No songs found"));
        }

        // Find the best matching song
        let mut best_match: Option<(&Song, f64)> = None;
        for song in &search_result.song {
            let similarity = calculate_similarity(metadata, song);
            debug!(
                "Song: {} - {} (similarity: {:.2})",
                song.artist.as_deref().unwrap_or("Unknown"),
                song.title,
                similarity
            );

            if similarity > 0.5 {
                // Only consider songs with >50% similarity
                if let Some((_, best_score)) = best_match {
                    if similarity > best_score {
                        best_match = Some((song, similarity));
                    }
                } else {
                    best_match = Some((song, similarity));
                }
            }
        }

        if let Some((song, score)) = best_match {
            debug!(
                "Selected song: {} - {} (score: {:.2})",
                song.artist.as_deref().unwrap_or("Unknown"),
                song.title,
                score
            );
            Ok(song.id.clone())
        } else {
            Err(anyhow!("No suitable match found"))
        }
    }

    /// Get lyrics by song ID
    async fn get_lyrics_by_id(&self, song_id: &str) -> Result<String> {
        let url = format!("{}/rest/getLyricsBySongId", self.config.server_url);

        let auth_params = self.generate_auth_params();
        let mut params = vec![("id", song_id), ("f", "json")];
        params.extend(auth_params.iter().map(|(k, v)| (k.as_str(), v.as_str())));

        debug!("Lyrics URL: {}", url);
        debug!("Lyrics params: {:?}", params);

        // Generate curl command for debugging
        let curl_cmd = self.generate_curl_command(&url, &params);
        debug!("Equivalent curl command: {}", curl_cmd);

        let response = self.client.get(&url).query(&params).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Lyrics request failed: {} - {}", status, body);
            return Err(anyhow!("Lyrics request failed: {}", status));
        }

        let lyrics_response: SubsonicResponse = response.json().await?;
        debug!("Lyrics response: {:?}", lyrics_response);

        if lyrics_response.subsonic_response.status != "ok" {
            return Err(anyhow!("Lyrics API returned error status"));
        }

        let lyrics_list = lyrics_response
            .subsonic_response
            .lyrics_list
            .ok_or_else(|| anyhow!("No lyrics found"))?;

        if lyrics_list.structured_lyrics.is_empty() {
            return Err(anyhow!("No structured lyrics available"));
        }

        // Convert the first set of structured lyrics to LRC format
        let structured_lyrics = &lyrics_list.structured_lyrics[0];
        let lrc_content = convert_to_lrc(structured_lyrics)?;

        if lrc_content.trim().is_empty() {
            warn!("Lyrics content is empty");
            return Err(anyhow!("Empty lyrics content"));
        }

        debug!("Successfully converted lyrics to LRC format");
        Ok(lrc_content)
    }

    /// Generate authentication parameters for Subsonic API
    fn generate_auth_params(&self) -> HashMap<String, String> {
        use std::time::{SystemTime, UNIX_EPOCH};

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let salt = format!("{:x}", timestamp);

        let token_input = format!("{}{}", self.config.password, salt);
        let token = format!("{:x}", md5::compute(token_input.as_bytes()));

        let mut params = HashMap::new();
        params.insert("u".to_string(), self.config.username.clone());
        params.insert("t".to_string(), token);
        params.insert("s".to_string(), salt);
        params.insert("v".to_string(), "1.16.1".to_string());
        params.insert("c".to_string(), "waylrc".to_string());

        params
    }

    /// Generate curl command for debugging
    fn generate_curl_command(&self, url: &str, params: &[(&str, &str)]) -> String {
        let mut cmd = format!("curl -X GET '{}", url);
        for (i, (key, value)) in params.iter().enumerate() {
            if i == 0 {
                cmd.push('?');
            } else {
                cmd.push('&');
            }
            cmd.push_str(&format!("{}={}", key, value));
        }
        cmd.push('\'');
        cmd
    }
}
