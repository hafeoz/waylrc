use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use tokio::task::JoinHandle;
use zbus::names::OwnedBusName;

use crate::{
    external_lrc_provider::{navidrome::NavidromeConfig, ExternalLrcProvider},
    lrc::Lrc,
    player::{PlaybackStatus, PlayerInformation},
};

pub fn is_player_active(player: &PlayerInformation) -> bool {
    // The player must be playing something ...
    if player.status.is_some() && player.status != Some(PlaybackStatus::Playing) {
        return false;
    }
    // ... and there should be some lyrics we can use
    if !player.has_lyrics() {
        return false;
    }
    true
}

pub async fn find_active_player_with_lyrics(
    available_players: &HashMap<Arc<OwnedBusName>, (PlayerInformation, JoinHandle<Result<()>>)>,
    external_providers: &[ExternalLrcProvider],
    navidrome_config: Option<&NavidromeConfig>,
) -> Option<(Arc<OwnedBusName>, Lrc)> {
    for (name, (player, _)) in available_players.iter() {
        if !is_player_active(player) {
            continue;
        }

        // Try to get lyrics with external provider support
        if let Some(Ok(lrc)) = player.get_lyrics_with_external(external_providers, navidrome_config).await {
            return Some((Arc::clone(name), lrc));
        }
    }
    None
}
