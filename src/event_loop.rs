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
    lrc::{Lrc, TimeTag},
    output::WaybarCustomModule,
    player::PlayerInformation,
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
) -> Result<()> {
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
    let reload_current_player = |bus: Arc<OwnedBusName>,
                                 lrc: Lrc,
                                 info: &PlayerInformation,
                                 current_player: &mut _,
                                 current_player_timer: &mut _| {
        tracing::debug!(%bus, ?info, "Current player state refreshed");
        let current_timetag = info.get_current_timetag();
        let (lrc_line, next_lrc_timetag) = lrc.get(&current_timetag);
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
                let bus_name = Arc::new(bus_name);
                match bus_activity {
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
                            let Ok(lrc) =  player_info.get_lyrics().unwrap() else { continue };
                            reload_current_player(Arc::clone(&bus_name), lrc, &player_info, &mut current_player, &mut current_player_timer);
                        }

                        available_players.insert(bus_name, (player_info, player_updater));
                    },
                    BusActivity::Destroyed => {
                        let Some((_, updater)) = available_players.remove(&bus_name) else { tracing::error!("Attempting to destroy a non-existent player {bus_name}"); continue };
                        updater.abort();

                        if current_player.as_ref().is_some_and(|p| p.bus == bus_name) {
                            tracing::info!(%bus_name, "Currently active player modified");
                            match scanner::find_active_player(&available_players) {
                                Some((active_player_name, active_player_info, active_player_lrc)) => reload_current_player(active_player_name, active_player_lrc, active_player_info, &mut current_player, &mut current_player_timer),
                                None => empty_current_player(&mut current_player, &mut current_player_timer)
                            }
                        }
                    }
                }
            }
            Some((bus_name, player_update)) = player_update_receiver.recv() => {
                tracing::debug!(%bus_name, ?player_update, "Player status updated");
                let Some((info, _)) = available_players.get_mut(&bus_name) else { tracing::error!("Attempting to update a non-existent player {bus_name}"); continue };
                let old_lrc_url = info.metadata.get("xesam:url").map(Deref::deref).and_then(extract_str).map(ToOwned::to_owned);
                info.apply_update(player_update);
                let new_lrc_url = info.metadata.get("xesam:url").map(Deref::deref).and_then(extract_str).map(ToOwned::to_owned);

                if current_player.as_ref().map(|p| &p.bus) == Some(&bus_name) {
                    let player = current_player.take().unwrap();
                    // This player is the current player
                    tracing::info!(%bus_name, "Currently active player modified");
                    if scanner::is_player_active(info) {
                        // Refresh since metadata hash changed
                        let lrc = if old_lrc_url == new_lrc_url { player.lrc } else {
                            // Reload lyrics first
                            tracing::info!(%bus_name, ?old_lrc_url, ?new_lrc_url, "Currently active player lyrics modified");
                            if let Ok(i) = info.get_lyrics().unwrap() { i } else {
                                // Lyric is inaccessible - find new player
                                tracing::info!(%bus_name, "Player lyric is inaccessible");
                                match scanner::find_active_player(&available_players) {
                                    Some((active_player_name, active_player_info, active_player_lrc)) => reload_current_player(active_player_name, active_player_lrc, active_player_info, &mut current_player, &mut current_player_timer),
                                    None => empty_current_player(&mut current_player, &mut current_player_timer)
                                }
                                continue
                            }
                        };
                        reload_current_player(bus_name, lrc, info, &mut current_player, &mut current_player_timer);
                    }
                    else {
                        // This player has gone inactive - find a new active player
                        tracing::info!(%bus_name, "Player has gone inactive");
                        match scanner::find_active_player(&available_players) {
                            Some((active_player_name, active_player_info, active_player_lrc)) => reload_current_player(active_player_name, active_player_lrc, active_player_info, &mut current_player, &mut current_player_timer),
                            None => empty_current_player(&mut current_player, &mut current_player_timer)
                        }
                    }
                } else if current_player.is_none() && scanner::is_player_active(info) {
                    tracing::info!("Player has gone active");
                    let Ok(lrc) = info.get_lyrics().unwrap() else { continue };
                    reload_current_player(bus_name, lrc, info, &mut current_player, &mut current_player_timer);
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
