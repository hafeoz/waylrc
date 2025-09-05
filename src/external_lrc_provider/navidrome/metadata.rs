use anyhow::{anyhow, Result};
use std::collections::HashMap;
use zbus::zvariant::Value;

/// Extracted track metadata from MPRIS
#[derive(Debug, Clone)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration: Option<u32>, // Duration in seconds
}

/// Extract metadata from MPRIS metadata HashMap
pub fn extract_metadata(metadata: &HashMap<String, Value>) -> Result<TrackMetadata> {
    let title = get_string_value(metadata, "xesam:title")?;
    let artist = get_artist_value(metadata)?;
    let album = get_string_value(metadata, "xesam:album").ok();
    let duration = get_duration_value(metadata);

    Ok(TrackMetadata {
        title,
        artist,
        album,
        duration,
    })
}

/// Get string value from metadata, handling both String and Array<String> types
fn get_string_value(metadata: &HashMap<String, Value>, key: &str) -> Result<String> {
    let value = metadata
        .get(key)
        .ok_or_else(|| anyhow!("Missing key: {}", key))?;

    match value {
        Value::Str(s) => Ok(s.as_str().to_string()),
        Value::Array(arr) => {
            // Try to get the first element
            match arr.get(0) {
                Ok(Some(first_value)) => {
                    if let Value::Str(s) = first_value {
                        Ok(s.as_str().to_string())
                    } else {
                        Err(anyhow!("Array element is not a string for key: {}", key))
                    }
                }
                Ok(None) => Err(anyhow!("Empty array for key: {}", key)),
                Err(e) => Err(anyhow!(
                    "Failed to access array element for key {}: {}",
                    key,
                    e
                )),
            }
        }
        _ => Err(anyhow!("Unsupported value type for key: {}", key)),
    }
}

/// Get artist value, handling both String and Array<String> types
fn get_artist_value(metadata: &HashMap<String, Value>) -> Result<String> {
    // Try xesam:artist first
    if let Ok(artist) = get_string_value(metadata, "xesam:artist") {
        return Ok(artist);
    }

    // Try xesam:albumArtist as fallback
    if let Ok(artist) = get_string_value(metadata, "xesam:albumArtist") {
        return Ok(artist);
    }

    Err(anyhow!("No artist information found"))
}

/// Get duration value from metadata (in seconds)
fn get_duration_value(metadata: &HashMap<String, Value>) -> Option<u32> {
    // Try mpris:length (in microseconds)
    if let Some(value) = metadata.get("mpris:length") {
        if let Value::U64(microseconds) = value {
            return Some((*microseconds / 1_000_000) as u32);
        }
    }

    // Try xesam:duration (typically in seconds)
    if let Some(value) = metadata.get("xesam:duration") {
        match value {
            Value::U32(seconds) => return Some(*seconds),
            Value::U64(seconds) => return Some(*seconds as u32),
            Value::I32(seconds) if *seconds > 0 => return Some(*seconds as u32),
            Value::I64(seconds) if *seconds > 0 => return Some(*seconds as u32),
            _ => {}
        }
    }

    None
}
