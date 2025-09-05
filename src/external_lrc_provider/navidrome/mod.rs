pub mod api;
pub mod metadata;
pub mod types;
pub mod utils;

use anyhow::Result;
use std::collections::HashMap;
use tracing::{debug, warn};
use zbus::zvariant::Value;

// Re-export main functionality
pub use api::NavidromeClient;
pub use metadata::extract_metadata;
pub use types::NavidromeConfig;

/// Fetch lyrics from Navidrome server using MPRIS metadata
pub async fn fetch_lyrics_from_navidrome(
    server_url: &str,
    username: &str,
    password: &str,
    metadata: &HashMap<String, zbus::zvariant::OwnedValue>,
) -> Result<String> {
    debug!("Starting Navidrome lyrics fetch");

    // Convert OwnedValue to Value for processing
    let converted_metadata: HashMap<String, Value> = metadata
        .iter()
        .map(|(k, v)| (k.clone(), v.clone().into()))
        .collect();

    // Extract metadata from MPRIS
    let track_metadata = extract_metadata(&converted_metadata)?;
    debug!("Extracted metadata: {:?}", track_metadata);

    // Create Navidrome client
    let config = NavidromeConfig {
        server_url: server_url.to_string(),
        username: username.to_string(),
        password: password.to_string(),
    };
    let client = NavidromeClient::new(config);

    // Fetch lyrics
    match client.fetch_lyrics(&track_metadata).await {
        Ok(lyrics) => {
            debug!("Successfully fetched lyrics from Navidrome");
            Ok(lyrics)
        }
        Err(e) => {
            warn!("Failed to fetch lyrics from Navidrome: {}", e);
            Err(e)
        }
    }
}
