use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::Result;

use crate::{lrc::TimeTag, player::PlayerInformation};

use super::lyrics_manager::LyricsManager;

// Handle loop check timer tick
pub fn handle_loop_check_timer(
    lyrics_manager: &mut LyricsManager,
    available_players: &mut HashMap<
        Arc<zbus::names::OwnedBusName>,
        (
            PlayerInformation,
            tokio::task::JoinHandle<Result<(), anyhow::Error>>,
        ),
    >,
    filter_keys: &std::collections::HashSet<String>,
) {
    // tracing::debug!("Loop check timer triggered");

    // Debug current player position information and detect loops
    let mut should_refresh_lyrics = None;
    let mut reset_player_position = None;

    for (bus_name, (info, _)) in available_players.iter() {
        let raw_pos = Duration::from_micros(info.position as u64);
        let calc_pos: Duration = info.get_current_timetag().into();
        let status = info
            .status
            .as_ref()
            .map(|s| format!("{:?}", s))
            .unwrap_or_else(|| "Unknown".to_string());

        // Only output debug info when there's a current player
        let is_current_player = lyrics_manager
            .current_player
            .as_ref()
            .map_or(false, |current_player| current_player.bus == *bus_name);

        if is_current_player {
            tracing::debug!(
                "Current player {}: raw={}s, calc={}s, status={}",
                bus_name,
                raw_pos.as_secs(),
                calc_pos.as_secs(),
                status
            );
        }

        // Check if this is the current player
        if let Some(current_player) = &lyrics_manager.current_player {
            if current_player.bus == *bus_name {
                // Detect loop count changes
                let (current_loop_count, position_in_loop) = info.get_loop_count();

                let should_refresh_due_to_loop =
                    if let Some(last_loop_count) = lyrics_manager.last_loop_count {
                        if current_loop_count > last_loop_count {
                            tracing::info!(
                                "[LOOP COUNT] Detected song loop: {} -> {}, position in loop: {}s",
                                last_loop_count,
                                current_loop_count,
                                position_in_loop.as_secs()
                            );
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                // Original lyrics end detection logic
                let should_refresh_due_to_position = if current_player.next_lrc_timetag
                    == TimeTag::from(Duration::from_secs(u64::MAX))
                {
                    // Check if position has significantly decreased (loop detection)
                    if let Some(last_pos) = lyrics_manager.last_known_position {
                        let current_pos = calc_pos;
                        if last_pos > current_pos
                            && (last_pos - current_pos) > Duration::from_secs(30)
                        {
                            tracing::info!("[MANUAL] Detected position reset in event loop: {}s -> {}s, triggering manual refresh",
                                         last_pos.as_secs(), current_pos.as_secs());
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                // If lyrics refresh is needed, record relevant information
                if should_refresh_due_to_loop || should_refresh_due_to_position {
                    let track_id = info
                        .metadata
                        .get("mpris:trackid")
                        .map(std::ops::Deref::deref)
                        .and_then(crate::utils::extract_str)
                        .map(|s| s.to_string());
                    should_refresh_lyrics = Some((
                        current_player.bus.clone(),
                        current_player.lrc.clone(),
                        track_id,
                        should_refresh_due_to_loop,
                    ));

                    // If it's a loop refresh, record player position reset info
                    if should_refresh_due_to_loop {
                        reset_player_position = Some((bus_name.clone(), position_in_loop));
                    }
                }

                break; // Exit loop after finding current player
            }
        }
    }

    // First reset player position (if needed)
    if let Some((bus_name, position_in_loop)) = reset_player_position {
        if let Some((info, _)) = available_players.get_mut(&bus_name) {
            info.position = position_in_loop.as_micros() as i64;
            info.position_last_refresh = std::time::Instant::now();
            tracing::info!(
                "Reset player {} position to {}s after loop detection",
                bus_name,
                position_in_loop.as_secs()
            );
        }
    }

    // Execute lyrics refresh and state update
    if let Some((bus, lrc, track_id, was_loop_refresh)) = should_refresh_lyrics {
        if let Some((info, _)) = available_players.get(&bus) {
            lyrics_manager.refresh_lyrics_display(bus.clone(), lrc, info, filter_keys, track_id);

            // Update loop count and position
            let (current_loop_count, _) = info.get_loop_count();
            let calc_pos: Duration = info.get_current_timetag().into();
            lyrics_manager.last_loop_count = Some(current_loop_count);
            lyrics_manager.last_known_position = Some(calc_pos);

            if was_loop_refresh {
                tracing::info!("Loop-based lyrics refresh completed");
            } else {
                tracing::info!("Position-based lyrics refresh completed");
            }
        }
    } else {
        // When no lyrics refresh, still need to update loop count and position
        if let Some(current_player) = &lyrics_manager.current_player {
            if let Some((info, _)) = available_players.get(&current_player.bus) {
                let (current_loop_count, _) = info.get_loop_count();
                let calc_pos: Duration = info.get_current_timetag().into();
                lyrics_manager.last_loop_count = Some(current_loop_count);
                lyrics_manager.last_known_position = Some(calc_pos);
            }
        }
    }
}
