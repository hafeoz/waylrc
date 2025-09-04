use std::{
    collections::HashMap,
    ops::Deref,
    sync::Arc,
    time::Duration,
};

use anyhow::Result;

use crate::{
    external_lrc_provider::{navidrome::NavidromeConfig, ExternalLrcProvider},
    lrc::TimeTag,
    player::{PlayerInformation, PlayerInformationUpdate},
    utils::extract_str,
};

use super::{scanner, lyrics_manager::LyricsManager, utils::get_lyrics_async};

// Handle player information update
pub async fn handle_player_update(
    bus_name: Arc<zbus::names::OwnedBusName>,
    player_update: PlayerInformationUpdate,
    lyrics_manager: &mut LyricsManager,
    available_players: &mut HashMap<Arc<zbus::names::OwnedBusName>, (PlayerInformation, tokio::task::JoinHandle<Result<(), anyhow::Error>>)>,
    external_lrc_providers: &[ExternalLrcProvider],
    navidrome_config: Option<&NavidromeConfig>,
    filter_keys: &std::collections::HashSet<String>,
) {
    tracing::info!("Player update received: {} - {:?}", bus_name, player_update);

    // DEBUG: Always log what we're doing
    tracing::info!("[DEBUG] handle_player_update called for {}", bus_name);

    let Some((info, _)) = available_players.get_mut(&bus_name) else {
        tracing::error!("Attempting to update a non-existent player {bus_name}");
        tracing::info!("[DEBUG] Player {} not found in available_players, returning early", bus_name);
        return;
    };

    // Store old metadata for comparison
    let old_lrc_url = info.metadata.get("xesam:url").map(Deref::deref).and_then(extract_str).map(std::borrow::ToOwned::to_owned);
    let old_title = info.metadata.get("xesam:title").map(Deref::deref).and_then(extract_str).map(std::borrow::ToOwned::to_owned);
    let old_artist = info.metadata.get("xesam:artist").map(Deref::deref).and_then(extract_str).map(std::borrow::ToOwned::to_owned);
    let old_trackid = info.metadata.get("mpris:trackid").map(Deref::deref).and_then(extract_str).map(std::borrow::ToOwned::to_owned);

    // Check if this is a position update (manual seek) or status change
    let is_position_update = matches!(player_update, PlayerInformationUpdate::Position(_, _));
    let is_status_update = matches!(player_update, PlayerInformationUpdate::Status(_));
    let is_metadata_update = matches!(player_update, PlayerInformationUpdate::Metadata(_));

    // Handle metadata changes BEFORE apply_update to prevent timing contamination
    if is_metadata_update {
        tracing::debug!(%bus_name, "Metadata update detected - pre-resetting timing to prevent cross-track contamination");
        // When metadata changes (track changes), we must reset timing BEFORE applying the update
        // This prevents timing contamination from previous tracks, especially after failed lyrics
        info.position = 0;
        info.position_last_refresh = std::time::Instant::now();
    }

    info.apply_update(player_update);

    // Handle status changes (pause/resume) to maintain accurate timing
    if is_status_update {
        tracing::debug!(%bus_name, ?info.status, "Status changed, updating position timing");
        // When status changes, we need to "freeze" the current position
        // This prevents get_current_timetag() from continuing to accumulate time during pause
        let current_timetag = {
            assert!(info.position >= 0, "Negative timetag encountered");
            let elapsed = Duration::from_secs_f64(
                info.position_last_refresh.elapsed().as_secs_f64() / info.rate.unwrap_or(1.0),
            );
            Duration::from_micros(info.position as u64) + elapsed
        };

        // Update position to the calculated current position and reset timing base
        info.position = current_timetag.as_micros() as i64;
        info.position_last_refresh = std::time::Instant::now();
    }

    // Check new metadata
    let new_lrc_url = info.metadata.get("xesam:url").map(Deref::deref).and_then(extract_str).map(std::borrow::ToOwned::to_owned);
    let new_title = info.metadata.get("xesam:title").map(Deref::deref).and_then(extract_str).map(std::borrow::ToOwned::to_owned);
    let new_artist = info.metadata.get("xesam:artist").map(Deref::deref).and_then(extract_str).map(std::borrow::ToOwned::to_owned);
    let new_trackid = info.metadata.get("mpris:trackid").map(Deref::deref).and_then(extract_str).map(std::borrow::ToOwned::to_owned);

    if lyrics_manager.current_player.as_ref().map(|p| &p.bus) == Some(&bus_name) {
        // TODO: This section can be refactored into handle_current_player_update when type issues are resolved
        let player = lyrics_manager.current_player.take().unwrap();
        // This player is the current player
        tracing::info!(%bus_name, "Currently active player modified");
        if scanner::is_player_active(info) {
            // Check if track has actually changed (URL, title, artist, or trackid)
            let track_changed = old_lrc_url != new_lrc_url ||
                              old_title != new_title ||
                              old_artist != new_artist ||
                              old_trackid != new_trackid;

            tracing::debug!(%bus_name, ?track_changed, ?old_title, ?new_title, ?old_trackid, ?new_trackid, "Track change detection");

            let lrc = if !track_changed {
                // Same track, reuse existing lyrics
                tracing::debug!(%bus_name, "Reusing existing lyrics (no track change detected)");
                player.lrc
            } else {
                // Track changed, reload lyrics
                tracing::info!(%bus_name, ?old_title, ?new_title, ?old_artist, ?new_artist, "Track changed, reloading lyrics");

                match get_lyrics_async(info, external_lrc_providers, navidrome_config).await {
                    Some(Ok(i)) => i,
                    Some(Err(e)) => {
                        tracing::warn!(%bus_name, ?e, "Failed to load lyrics for current track");
                        // Reset player position timing to prevent state corruption
                        info.position = 0;
                        info.position_last_refresh = std::time::Instant::now();

                        // Clear current player state completely
                        lyrics_manager.clear_state();
                        return;
                    },
                    None => {
                        tracing::info!(%bus_name, "Player lyric is inaccessible for current track");
                        // Reset player position timing to prevent state corruption
                        info.position = 0;
                        info.position_last_refresh = std::time::Instant::now();

                        // Clear current player state completely
                        lyrics_manager.clear_state();
                        return;
                    }
                }
            };

            // Handle position updates (manual seek), track changes, or status changes
            let needs_reload = track_changed || is_position_update || is_status_update;

            // Special case: detect song loops by checking for significant position resets
            let current_pos: Duration = info.get_current_timetag().into();

            // DEBUG: Always log current state before loop detection
            tracing::info!("[DEBUG] Before loop detection - bus: {}, current_pos: {}s, is_position_update: {}, is_metadata_update: {}, needs_reload: {}",
                         bus_name, current_pos.as_secs(), is_position_update, is_metadata_update, needs_reload);

            // Log lyrics manager state
            if let Some(current_player) = &lyrics_manager.current_player {
                let is_at_end = current_player.next_lrc_timetag == TimeTag::from(Duration::from_secs(u64::MAX));
                tracing::info!("[DEBUG] Lyrics manager state - current_player exists: true, at_end: {}, last_known_pos: {:?}s",
                             is_at_end, lyrics_manager.last_known_position.map(|p| p.as_secs()));
            } else {
                tracing::info!("[DEBUG] Lyrics manager state - current_player exists: false");
            }

            // Check for loop restart using the lyrics manager
            let is_loop_restart = lyrics_manager.detect_loop_restart(current_pos, is_position_update, is_metadata_update);

            tracing::info!("[DEBUG] Loop detection result: is_loop_restart = {}", is_loop_restart);

            if is_loop_restart {
                let prev_pos_secs = lyrics_manager.last_known_position.map(|p| p.as_secs()).unwrap_or(0);
                tracing::info!("Detected song loop restart: {} - current: {}s, previous: {}s",
                    bus_name, current_pos.as_secs(), prev_pos_secs);
            } else {
                tracing::debug!("Position tracking: {} - current: {}s, needs_reload: {}",
                    bus_name, current_pos.as_secs(), needs_reload);
            }

            if is_loop_restart {
                let prev_pos_secs = lyrics_manager.last_known_position.map(|p| p.as_secs()).unwrap_or(0);
                tracing::info!(%bus_name, current_pos_secs = current_pos.as_secs(), prev_pos_secs, "Detected song loop - restarting lyrics");
            }

            if needs_reload || is_loop_restart {
                let track_id = info.metadata.get("mpris:trackid").map(Deref::deref).and_then(extract_str).map(|s| s.to_string());
                lyrics_manager.refresh_lyrics_display(bus_name, lrc, info, filter_keys, track_id);
            }

            // Update position tracking
            lyrics_manager.update_position(current_pos, is_position_update, is_metadata_update);
        } else {
            // This player has gone inactive - find a new active player
            tracing::info!(%bus_name, "Player has gone inactive");
            match scanner::find_active_player_with_lyrics(available_players, external_lrc_providers, navidrome_config).await {
                Some((active_player_name, active_player_lrc)) => {
                    let active_player_info = &available_players[&active_player_name].0;
                    let track_id = active_player_info.metadata.get("mpris:trackid").map(Deref::deref).and_then(extract_str).map(|s| s.to_string());
                    lyrics_manager.refresh_lyrics_display(active_player_name, active_player_lrc, active_player_info, filter_keys, track_id);
                }
                None => lyrics_manager.clear_state()
            }
        }
    } else if lyrics_manager.current_player.is_none() && scanner::is_player_active(info) {
        tracing::info!("Player has gone active");

        // When a new player becomes active, ensure we have fresh position timing
        // This is crucial to prevent timing issues from previous failed lyrics attempts
        info.position_last_refresh = std::time::Instant::now();

        if let Some(Ok(lrc)) = get_lyrics_async(info, external_lrc_providers, navidrome_config).await {
            let track_id = info.metadata.get("mpris:trackid").map(Deref::deref).and_then(extract_str).map(|s| s.to_string());
            lyrics_manager.refresh_lyrics_display(bus_name, lrc, info, filter_keys, track_id);
        }
    }
}
