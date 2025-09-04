mod scanner;
mod update_listener;

use std::{
    collections::{HashMap, HashSet},
    future::{pending, Pending},
    ops::Deref,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use anyhow::{bail, Result};
use futures::future::Either;
use futures_lite::StreamExt as _;
use tokio::{
    select,
    sync::mpsc,
    time::{sleep, Sleep},
};
use update_listener::get_player_info;
use zbus::{names::OwnedBusName, Connection};

use crate::{
    dbus::{player_buses, BusActivity, BusChange},
    external_lrc_provider::{navidrome::NavidromeConfig, ExternalLrcProvider},
    lrc::{Lrc, TimeTag},
    output::WaybarCustomModule,
    player::{PlayerInformation, PlayerInformationUpdate},
    utils::extract_str,
};

struct CurrentPlayerState {
    bus: Arc<OwnedBusName>,
    lrc: Lrc,
    next_lrc_timetag: TimeTag,
}

pub async fn event_loop(
    conn: Connection,
    refresh_interval: Duration,
    filter_keys: HashSet<String>,
    allowed_players: Vec<String>,
    external_lrc_providers: Vec<ExternalLrcProvider>,
    navidrome_server_url: Option<String>,
    navidrome_username: Option<String>,
    navidrome_password: Option<String>,
) -> Result<()> {
    // Create Navidrome configuration if all required parameters are provided
    let navidrome_config = if external_lrc_providers.contains(&ExternalLrcProvider::NAVIDROME) {
        match (navidrome_server_url, navidrome_username, navidrome_password) {
            (Some(server_url), Some(username), Some(password)) => {
                Some(NavidromeConfig {
                    server_url,
                    username,
                    password,
                })
            }
            _ => {
                tracing::warn!("Navidrome provider selected but missing required configuration (server_url, username, password)");
                None
            }
        }
    } else {
        None
    };

    let mut dbus_stream = player_buses(&conn).await?;

    let (player_update_sender, mut player_update_receiver) = mpsc::channel(1);

    let mut available_players: HashMap<_, (PlayerInformation, _)> = HashMap::new();

    let mut current_player: Option<CurrentPlayerState> = None;
    let mut current_player_timer: Pin<Box<Either<Sleep, Pending<()>>>> =
        Box::pin(Either::Right(pending()));
    let empty_current_player = |current_player: &mut _, current_player_timer: &mut _| {
        tracing::info!("No player active. Clearing previous state");
        *current_player = None;
        *current_player_timer = Box::pin(Either::Right(pending()));
        WaybarCustomModule::empty().print().unwrap();
    };

    // Async helper to get lyrics with external provider support
    async fn get_lyrics_async(
        player_info: &PlayerInformation,
        external_providers: &[ExternalLrcProvider],
        navidrome_config: Option<&NavidromeConfig>,
    ) -> Option<Result<Lrc, anyhow::Error>> {
        player_info.get_lyrics_with_external(external_providers, navidrome_config).await
    }

    let reload_current_player = |bus: Arc<OwnedBusName>,
                                 lrc: Lrc,
                                 info: &PlayerInformation,
                                 current_player: &mut _,
                                 current_player_timer: &mut _| {
        tracing::debug!(%bus, ?info, "Current player state refreshed");
        let current_timetag = info.get_current_timetag();
        tracing::debug!(%bus, ?current_timetag, "Current time tag for lyrics positioning");
        let (lrc_line, next_lrc_timetag) = lrc.get(&current_timetag);
        tracing::debug!(%bus, ?lrc_line, ?next_lrc_timetag, "Found lyrics line at current position");
        WaybarCustomModule::new(
            Some(&lrc_line.join(" ")),
            None,
            Some(&info.format_metadata(&filter_keys)),
            None,
            None,
        )
        .print()
        .unwrap();
        let Some(next_lrc_timetag) = next_lrc_timetag else {
            tracing::info!("Lyric has reached ending");
            return empty_current_player(current_player, current_player_timer);
        };
        *current_player = Some(CurrentPlayerState {
            bus,
            lrc,
            next_lrc_timetag,
        });
        let till_next_timetag =
            next_lrc_timetag.duration_from(&current_timetag, info.rate.unwrap_or(1.0));
        *current_player_timer = Box::pin(Either::Left(sleep(till_next_timetag)));
    };

    loop {
        select! {
            bus_change = dbus_stream.next() => {
                let Some(BusChange { name: bus_name, activity: bus_activity }) = bus_change else {
                    tracing::error!("DBus NameOwnerChanged stream closed");
                    continue
                };

                // Check if this player is allowed
                let bus_change = BusChange::new(bus_name.clone(), bus_activity);
                if !bus_change.matches_players(&allowed_players) {
                    tracing::debug!(%bus_name, "Player not in allowed list, skipping");
                    continue;
                }

                let bus_name = Arc::new(bus_name);
                match bus_change.activity {
                    BusActivity::Created => {
                        tracing::info!(%bus_name, "New player registered");
                        let (player_info, player_updater) = match get_player_info(Arc::clone(&bus_name), conn.clone(), refresh_interval, player_update_sender.clone()).await {
                            Ok(i) => i,
                            Err(e) => {
                                tracing::error!(?e, "Failed to get player information from DBus");
                                continue
                            }
                        };

                        if scanner::is_player_active(&player_info) && current_player.is_none() {
                            if let Some(Ok(lrc)) = get_lyrics_async(&player_info, &external_lrc_providers, navidrome_config.as_ref()).await {
                                reload_current_player(Arc::clone(&bus_name), lrc, &player_info, &mut current_player, &mut current_player_timer);
                            }
                        }

                        available_players.insert(bus_name, (player_info, player_updater));
                    },
                    BusActivity::Destroyed => {
                        let Some((_, updater)) = available_players.remove(&bus_name) else { tracing::error!("Attempting to destroy a non-existent player {bus_name}"); continue };
                        updater.abort();

                        if current_player.as_ref().is_some_and(|p| p.bus == bus_name) {
                            tracing::info!(%bus_name, "Currently active player modified");
                            match scanner::find_active_player_with_lyrics(&available_players, &external_lrc_providers, navidrome_config.as_ref()).await {
                                Some((active_player_name, active_player_lrc)) => {
                                    let active_player_info = &available_players[&active_player_name].0;
                                    reload_current_player(active_player_name, active_player_lrc, active_player_info, &mut current_player, &mut current_player_timer);
                                }
                                None => empty_current_player(&mut current_player, &mut current_player_timer)
                            }
                        }
                    }
                }
            }
            Some((bus_name, player_update)) = player_update_receiver.recv() => {
                tracing::debug!(%bus_name, ?player_update, "Player status updated");
                let Some((info, _)) = available_players.get_mut(&bus_name) else { tracing::error!("Attempting to update a non-existent player {bus_name}"); continue };

                // Store old metadata for comparison
                let old_lrc_url = info.metadata.get("xesam:url").map(Deref::deref).and_then(extract_str).map(ToOwned::to_owned);
                let old_title = info.metadata.get("xesam:title").map(Deref::deref).and_then(extract_str).map(ToOwned::to_owned);
                let old_artist = info.metadata.get("xesam:artist").map(Deref::deref).and_then(extract_str).map(ToOwned::to_owned);
                let old_trackid = info.metadata.get("mpris:trackid").map(Deref::deref).and_then(extract_str).map(ToOwned::to_owned);

                // Check if this is a position update (manual seek) or status change
                let is_position_update = matches!(player_update, PlayerInformationUpdate::Position(_, _));
                let is_status_update = matches!(player_update, PlayerInformationUpdate::Status(_));

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
                let new_lrc_url = info.metadata.get("xesam:url").map(Deref::deref).and_then(extract_str).map(ToOwned::to_owned);
                let new_title = info.metadata.get("xesam:title").map(Deref::deref).and_then(extract_str).map(ToOwned::to_owned);
                let new_artist = info.metadata.get("xesam:artist").map(Deref::deref).and_then(extract_str).map(ToOwned::to_owned);
                let new_trackid = info.metadata.get("mpris:trackid").map(Deref::deref).and_then(extract_str).map(ToOwned::to_owned);

                if current_player.as_ref().map(|p| &p.bus) == Some(&bus_name) {
                    let player = current_player.take().unwrap();
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

                            match get_lyrics_async(info, &external_lrc_providers, navidrome_config.as_ref()).await {
                                Some(Ok(i)) => i,
                                Some(Err(e)) => {
                                    tracing::warn!(%bus_name, ?e, "Failed to load lyrics");
                                    // Lyric loading failed - find new player
                                    match scanner::find_active_player_with_lyrics(&available_players, &external_lrc_providers, navidrome_config.as_ref()).await {
                                        Some((active_player_name, active_player_lrc)) => {
                                            let active_player_info = &available_players[&active_player_name].0;
                                            reload_current_player(active_player_name, active_player_lrc, active_player_info, &mut current_player, &mut current_player_timer);
                                        }
                                        None => empty_current_player(&mut current_player, &mut current_player_timer)
                                    }
                                    continue
                                },
                                None => {
                                    tracing::info!(%bus_name, "Player lyric is inaccessible");
                                    // Lyric is inaccessible - find new player
                                    match scanner::find_active_player_with_lyrics(&available_players, &external_lrc_providers, navidrome_config.as_ref()).await {
                                        Some((active_player_name, active_player_lrc)) => {
                                            let active_player_info = &available_players[&active_player_name].0;
                                            reload_current_player(active_player_name, active_player_lrc, active_player_info, &mut current_player, &mut current_player_timer);
                                        }
                                        None => empty_current_player(&mut current_player, &mut current_player_timer)
                                    }
                                    continue
                                }
                            }
                        };

                        // Handle position updates (manual seek), track changes, or status changes
                        let needs_reload = track_changed || is_position_update || is_status_update;

                        if track_changed {
                            // For track changes, reset position information to avoid stale timing
                            tracing::debug!(%bus_name, "Resetting position info for track change");
                            info.position = 0;
                            info.position_last_refresh = std::time::Instant::now();
                        } else if is_position_update {
                            // For position updates (manual seek), position is already updated correctly by apply_update()
                            tracing::debug!(%bus_name, "Position updated (manual seek detected)");
                        } else if is_status_update {
                            // For status updates (play/pause), timing is already corrected above
                            tracing::debug!(%bus_name, "Status updated (play/pause detected)");
                        }

                        if needs_reload {
                            reload_current_player(bus_name, lrc, info, &mut current_player, &mut current_player_timer);
                        }
                    }
                    else {
                        // This player has gone inactive - find a new active player
                        tracing::info!(%bus_name, "Player has gone inactive");
                        match scanner::find_active_player_with_lyrics(&available_players, &external_lrc_providers, navidrome_config.as_ref()).await {
                            Some((active_player_name, active_player_lrc)) => {
                                let active_player_info = &available_players[&active_player_name].0;
                                reload_current_player(active_player_name, active_player_lrc, active_player_info, &mut current_player, &mut current_player_timer);
                            }
                            None => empty_current_player(&mut current_player, &mut current_player_timer)
                        }
                    }
                } else if current_player.is_none() && scanner::is_player_active(info) {
                    tracing::info!("Player has gone active");
                    if let Some(Ok(lrc)) = get_lyrics_async(info, &external_lrc_providers, navidrome_config.as_ref()).await {
                        reload_current_player(bus_name, lrc, info, &mut current_player, &mut current_player_timer);
                    }
                } else if is_position_update {
                    // Log position updates for non-current players
                    tracing::debug!(%bus_name, "Position updated for non-current player");
                } else if is_status_update {
                    // Log status updates for non-current players
                    tracing::debug!(%bus_name, ?info.status, "Status updated for non-current player");
                }
            }
            () = &mut current_player_timer => {
                let Some(player) = &mut current_player else { tracing::error!("Lyric timer expired but no active player is found"); continue };
                let (lrc, next_timetag) = player.lrc.get(&player.next_lrc_timetag);
                tracing::debug!(%player.bus, ?lrc, ?next_timetag, "Printing lyric");
                let player_info = &available_players[&player.bus].0;
                WaybarCustomModule::new(Some(&lrc.join(" ")), None, Some(&player_info.format_metadata(&filter_keys)), None, None).print().unwrap();
                match next_timetag {
                    None => current_player_timer = Box::pin(Either::Right(pending())),
                    Some(t) => {
                        current_player_timer = Box::pin(Either::Left(sleep(t.duration_from(&player.next_lrc_timetag, player_info.rate.unwrap_or(1.0)))));
                        player.next_lrc_timetag = t;
                    }
                }
            }
            else => { bail!("Player stream closed"); }
        }
    }
}
