mod current_player_state;
mod loop_check_handler;
mod lyrics_manager;
mod player_update_handler;
mod scanner;
mod update_listener;
mod utils;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use anyhow::{bail, Result};
use futures_lite::StreamExt as _;
use tokio::{select, sync::mpsc};
use zbus::Connection;

use crate::{
    dbus::{player_buses, BusActivity, BusChange},
    external_lrc_provider::ExternalLrcProvider,
    player::PlayerInformation,
};

use lyrics_manager::LyricsManager;
use loop_check_handler::handle_loop_check_timer;
use player_update_handler::handle_player_update;
use utils::*;

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
    let navidrome_config = setup_navidrome_config(
        &external_lrc_providers,
        navidrome_server_url,
        navidrome_username,
        navidrome_password,
    );

    let mut dbus_stream = player_buses(&conn).await?;

    let (player_update_sender, mut player_update_receiver) = mpsc::channel(1);

    let mut available_players: HashMap<_, (PlayerInformation, _)> = HashMap::new();
    let mut lyrics_manager = LyricsManager::new();

    // Add timer to forcefully check loop status periodically
    let mut loop_check_timer = tokio::time::interval(Duration::from_secs(3));
    loop_check_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        // Reduce debug log frequency, only print when there are actual events
        // tracing::debug!("Event loop iteration starting...");

        select! {
            bus_change = dbus_stream.next() => {
                tracing::info!("Bus change event received");
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
                        if let Err(e) = handle_bus_created(
                            bus_name,
                            &conn,
                            refresh_interval,
                            &player_update_sender,
                            &mut lyrics_manager,
                            &mut available_players,
                            &external_lrc_providers,
                            navidrome_config.as_ref(),
                            &filter_keys,
                        ).await {
                            tracing::error!(?e, "Failed to handle bus creation");
                        }
                    },
                    BusActivity::Destroyed => {
                        handle_bus_destroyed(
                            &bus_name,
                            &mut lyrics_manager,
                            &mut available_players,
                            &external_lrc_providers,
                            navidrome_config.as_ref(),
                            &filter_keys,
                        ).await;
                    }
                }
            }
            Some((bus_name, player_update)) = player_update_receiver.recv() => {
                tracing::info!("Player update event received: {}", bus_name);
                handle_player_update(
                    bus_name,
                    player_update,
                    &mut lyrics_manager,
                    &mut available_players,
                    &external_lrc_providers,
                    navidrome_config.as_ref(),
                    &filter_keys,
                ).await;
            }
            () = &mut lyrics_manager.current_player_timer => {
                tracing::info!("Main event loop: lyrics timer triggered");
                handle_lyrics_timer(&mut lyrics_manager, &available_players, &filter_keys);
            }
            _ = loop_check_timer.tick() => {
                handle_loop_check_timer(&mut lyrics_manager, &mut available_players, &filter_keys);
            }
            else => { bail!("Player stream closed"); }
        }
    }
}
