use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use futures::future::Either;
use tokio::sync::mpsc;
use zbus::Connection;

use crate::{
    external_lrc_provider::{navidrome::NavidromeConfig, ExternalLrcProvider},
    lrc::{Lrc, TimeTag},
    output::WaybarCustomModule,
    player::{PlayerInformation, PlayerInformationUpdate},
    utils::extract_str,
};

use super::{lyrics_manager::LyricsManager, scanner, update_listener::get_player_info};

// Async helper to get lyrics with external provider support
pub async fn get_lyrics_async(
    player_info: &PlayerInformation,
    external_providers: &[ExternalLrcProvider],
    navidrome_config: Option<&NavidromeConfig>,
) -> Option<Result<Lrc, anyhow::Error>> {
    player_info
        .get_lyrics_with_external(external_providers, navidrome_config)
        .await
}

// Setup Navidrome configuration
pub fn setup_navidrome_config(
    external_lrc_providers: &[ExternalLrcProvider],
    navidrome_server_url: Option<String>,
    navidrome_username: Option<String>,
    navidrome_password: Option<String>,
) -> Option<NavidromeConfig> {
    if external_lrc_providers.contains(&ExternalLrcProvider::NAVIDROME) {
        match (navidrome_server_url, navidrome_username, navidrome_password) {
            (Some(server_url), Some(username), Some(password)) => Some(NavidromeConfig {
                server_url,
                username,
                password,
            }),
            _ => {
                tracing::warn!("Navidrome provider selected but missing required configuration (server_url, username, password)");
                None
            }
        }
    } else {
        None
    }
}

// Handle new player creation
pub async fn handle_bus_created(
    bus_name: Arc<zbus::names::OwnedBusName>,
    conn: &Connection,
    refresh_interval: Duration,
    player_update_sender: &mpsc::Sender<(Arc<zbus::names::OwnedBusName>, PlayerInformationUpdate)>,
    lyrics_manager: &mut LyricsManager,
    available_players: &mut HashMap<
        Arc<zbus::names::OwnedBusName>,
        (
            PlayerInformation,
            tokio::task::JoinHandle<Result<(), anyhow::Error>>,
        ),
    >,
    external_lrc_providers: &[ExternalLrcProvider],
    navidrome_config: Option<&NavidromeConfig>,
    filter_keys: &HashSet<String>,
) -> Result<()> {
    tracing::info!(%bus_name, "New player registered");
    let (player_info, player_updater) = match get_player_info(
        Arc::clone(&bus_name),
        conn.clone(),
        refresh_interval,
        player_update_sender.clone(),
    )
    .await
    {
        Ok(i) => i,
        Err(e) => {
            tracing::error!(?e, "Failed to get player information from DBus");
            return Err(e);
        }
    };

    let mut player_info = player_info;

    if scanner::is_player_active(&player_info) && lyrics_manager.current_player.is_none() {
        // Ensure fresh timing when attempting to get lyrics for new player
        player_info.position_last_refresh = std::time::Instant::now();

        if let Some(Ok(lrc)) =
            get_lyrics_async(&player_info, external_lrc_providers, navidrome_config).await
        {
            let track_id = player_info
                .metadata
                .get("mpris:trackid")
                .map(Deref::deref)
                .and_then(extract_str)
                .map(|s| s.to_string());
            lyrics_manager.refresh_lyrics_display(
                Arc::clone(&bus_name),
                lrc,
                &player_info,
                filter_keys,
                track_id,
            );
        }
    }

    available_players.insert(bus_name, (player_info, player_updater));
    Ok(())
}

// Handle player destruction
pub async fn handle_bus_destroyed(
    bus_name: &Arc<zbus::names::OwnedBusName>,
    lyrics_manager: &mut LyricsManager,
    available_players: &mut HashMap<
        Arc<zbus::names::OwnedBusName>,
        (
            PlayerInformation,
            tokio::task::JoinHandle<Result<(), anyhow::Error>>,
        ),
    >,
    external_lrc_providers: &[ExternalLrcProvider],
    navidrome_config: Option<&NavidromeConfig>,
    filter_keys: &HashSet<String>,
) {
    let Some((_, updater)) = available_players.remove(bus_name) else {
        tracing::error!("Attempting to destroy a non-existent player {bus_name}");
        return;
    };
    updater.abort();

    if lyrics_manager
        .current_player
        .as_ref()
        .is_some_and(|p| &p.bus == bus_name)
    {
        tracing::info!(%bus_name, "Currently active player modified");
        match scanner::find_active_player_with_lyrics(
            available_players,
            external_lrc_providers,
            navidrome_config,
        )
        .await
        {
            Some((active_player_name, active_player_lrc)) => {
                let active_player_info = &available_players[&active_player_name].0;
                let track_id = active_player_info
                    .metadata
                    .get("mpris:trackid")
                    .map(Deref::deref)
                    .and_then(extract_str)
                    .map(|s| s.to_string());
                lyrics_manager.refresh_lyrics_display(
                    active_player_name,
                    active_player_lrc,
                    active_player_info,
                    filter_keys,
                    track_id,
                );
            }
            None => lyrics_manager.clear_state(),
        }
    }
}

// Handle lyrics timer expiration
pub fn handle_lyrics_timer(
    lyrics_manager: &mut LyricsManager,
    available_players: &HashMap<
        Arc<zbus::names::OwnedBusName>,
        (
            PlayerInformation,
            tokio::task::JoinHandle<Result<(), anyhow::Error>>,
        ),
    >,
    filter_keys: &HashSet<String>,
) {
    tracing::info!("Lyrics timer expired - processing next line");
    let Some(player) = &mut lyrics_manager.current_player else {
        tracing::error!("Lyric timer expired but no active player is found");
        return;
    };

    let (lrc, next_timetag) = player.lrc.get(&player.next_lrc_timetag);
    let player_info = &available_players[&player.bus].0;
    WaybarCustomModule::new(
        Some(&lrc.join(" ")),
        None,
        Some(&player_info.format_metadata(filter_keys)),
        None,
        None,
    )
    .print()
    .unwrap();

    match next_timetag {
        None => {
            // Lyric has reached the end, keep player state but stop timer
            tracing::info!("Lyric has reached ending - keeping player state for loop detection");
            player.next_lrc_timetag = TimeTag::from(Duration::from_secs(u64::MAX));
            lyrics_manager.current_player_timer = Box::pin(Either::Right(std::future::pending()));
            // Display empty lyrics
            WaybarCustomModule::new(
                Some(""),
                None,
                Some(&player_info.format_metadata(filter_keys)),
                None,
                None,
            )
            .print()
            .unwrap();
        }
        Some(t) => {
            lyrics_manager.current_player_timer = Box::pin(Either::Left(tokio::time::sleep(
                t.duration_from(&player.next_lrc_timetag, player_info.rate.unwrap_or(1.0)),
            )));
            player.next_lrc_timetag = t;
        }
    }
}
