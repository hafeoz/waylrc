use serde::Deserialize;

/// Configuration for Navidrome connection
#[derive(Debug, Clone)]
pub struct NavidromeConfig {
    pub server_url: String,
    pub username: String,
    pub password: String,
}

/// Response structures for Navidrome API
#[derive(Debug, Deserialize)]
pub struct SubsonicResponse {
    #[serde(rename = "subsonic-response")]
    pub subsonic_response: SubsonicResponseData,
}

#[derive(Debug, Deserialize)]
pub struct SubsonicResponseData {
    pub status: String,
    #[serde(rename = "lyricsList")]
    pub lyrics_list: Option<LyricsList>,
}

#[derive(Debug, Deserialize)]
pub struct LyricsList {
    #[serde(rename = "structuredLyrics")]
    pub structured_lyrics: Vec<StructuredLyrics>,
}

#[derive(Debug, Deserialize)]
pub struct StructuredLyrics {
    pub line: Vec<LyricsLine>,
    pub synced: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct LyricsLine {
    pub start: Option<u64>,
    pub value: String,
}

/// Search result structures
#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    #[serde(rename = "subsonic-response")]
    pub subsonic_response: SearchResponseData,
}

#[derive(Debug, Deserialize)]
pub struct SearchResponseData {
    pub status: String,
    #[serde(rename = "searchResult3")]
    pub search_result3: Option<SearchResult>,
}

#[derive(Debug, Deserialize)]
pub struct SearchResult {
    pub song: Vec<Song>,
}

#[derive(Debug, Deserialize)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<u32>,
}
