use crate::external_lrc_provider::navidrome::{types::{Song, LyricsLine}, metadata::TrackMetadata};

/// Calculate similarity score between track metadata and search result
pub fn calculate_similarity(metadata: &TrackMetadata, song: &Song) -> f64 {
    let mut score = 0.0;
    let mut total_weight = 0.0;

    // Title similarity (highest weight)
    let title_weight = 3.0;
    total_weight += title_weight;
    if is_similar(&metadata.title, &song.title) {
        score += title_weight;
    }

    // Artist similarity
    let artist_weight = 2.0;
    total_weight += artist_weight;
    if let Some(song_artist) = &song.artist {
        if is_similar(&metadata.artist, song_artist) {
            score += artist_weight;
        }
    }

    // Album similarity (lower weight since it's optional)
    if let (Some(metadata_album), Some(song_album)) = (&metadata.album, &song.album) {
        let album_weight = 1.0;
        total_weight += album_weight;
        if is_similar(metadata_album, song_album) {
            score += album_weight;
        }
    }

    // Duration similarity (moderate weight)
    if let (Some(metadata_duration), Some(song_duration)) = (metadata.duration, song.duration) {
        let duration_weight = 1.5;
        total_weight += duration_weight;
        if is_duration_similar(metadata_duration, song_duration) {
            score += duration_weight;
        }
    }

    score / total_weight
}

/// Check if two strings are similar (case-insensitive)
fn is_similar(a: &str, b: &str) -> bool {
    let a_normalized = a.to_lowercase().trim().to_string();
    let b_normalized = b.to_lowercase().trim().to_string();

    // Exact match
    if a_normalized == b_normalized {
        return true;
    }

    // Check if one contains the other
    a_normalized.contains(&b_normalized) || b_normalized.contains(&a_normalized)
}

/// Check if two durations are similar (within 10 seconds tolerance)
fn is_duration_similar(duration1: u32, duration2: u32) -> bool {
    let diff = if duration1 > duration2 {
        duration1 - duration2
    } else {
        duration2 - duration1
    };

    // Consider durations similar if they're within 10 seconds of each other
    diff <= 10
}

/// Convert Navidrome lyrics to LRC format
pub fn convert_to_lrc(lyrics: &[LyricsLine]) -> String {
    lyrics
        .iter()
        .filter_map(|line| {
            if let Some(start_ms) = line.start {
                let total_centiseconds = start_ms / 10; // Convert milliseconds to centiseconds
                let minutes = total_centiseconds / 6000; // 60 seconds * 100 centiseconds
                let remaining_centiseconds = total_centiseconds % 6000;
                let seconds = remaining_centiseconds / 100;
                let centiseconds = remaining_centiseconds % 100;
                Some(format!("[{:02}:{:02}.{:02}]{}", minutes, seconds, centiseconds, line.value))
            } else {
                // Lines without timestamps
                Some(line.value.clone())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_lrc_provider::navidrome::{types::{Song, LyricsLine}, metadata::TrackMetadata};

    #[test]
    fn test_is_similar() {
        assert!(is_similar("Hello World", "hello world"));
        assert!(is_similar("Test Song", "Test"));
        assert!(is_similar("Artist Name", "artist"));
        assert!(!is_similar("Completely Different", "Nothing Similar"));
    }

    #[test]
    fn test_is_duration_similar() {
        assert!(is_duration_similar(180, 185)); // 5 seconds difference
        assert!(is_duration_similar(200, 190)); // 10 seconds difference
        assert!(!is_duration_similar(180, 200)); // 20 seconds difference
        assert!(is_duration_similar(120, 120)); // Exact match
    }

    #[test]
    fn test_calculate_similarity_with_duration() {
        let metadata = TrackMetadata {
            title: "Test Song".to_string(),
            artist: "Test Artist".to_string(),
            album: Some("Test Album".to_string()),
            duration: Some(180), // 3 minutes
        };

        let song = Song {
            id: "1".to_string(),
            title: "Test Song".to_string(),
            artist: Some("Test Artist".to_string()),
            album: Some("Test Album".to_string()),
            duration: Some(185), // 3:05, within 10 seconds tolerance
        };

        let similarity = calculate_similarity(&metadata, &song);
        // Should be 1.0 since all fields match (including duration within tolerance)
        assert!((similarity - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_convert_to_lrc() {
        let lyrics = vec![
            LyricsLine {
                start: Some(0),
                value: "First line".to_string(),
            },
            LyricsLine {
                start: Some(4020), // Should be [00:04.02]
                value: "煌く水面の上を".to_string(),
            },
            LyricsLine {
                start: Some(8640), // Should be [00:08.64]
                value: "夢中で風切り翔る".to_string(),
            },
            LyricsLine {
                start: Some(22290), // Should be [00:22.29]
                value: "僕はそう小さなツバメ".to_string(),
            },
        ];

        let result = convert_to_lrc(&lyrics);
        let expected = "[00:00.00]First line\n[00:04.02]煌く水面の上を\n[00:08.64]夢中で風切り翔る\n[00:22.29]僕はそう小さなツバメ";
        assert_eq!(result, expected);
    }
}
