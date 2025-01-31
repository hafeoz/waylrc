use std::{collections::HashMap, ops::Deref, sync::Arc};

use anyhow::Result;
use tokio::task::JoinHandle;
use zbus::{names::OwnedBusName, zvariant::Value};

use crate::{
    lrc::Lrc,
    player::{PlaybackStatus, PlayerInformation},
};

pub fn is_player_active(player: &PlayerInformation) -> bool {
    if player.status != PlaybackStatus::Playing {
        return false;
    }
    let Some(Value::Str(audio_url)) = player.metadata.get("xesam:url").map(Deref::deref) else {
        return false;
    };
    match Lrc::audio_url_to_path(audio_url) {
        Ok(i) if i.is_file() => true,
        Err(e) => {
            tracing::warn!(%e, "Failed to decode URL");
            false
        }
        Ok(_) => false,
    }
}

pub fn find_active_player(
    available_players: &HashMap<Arc<OwnedBusName>, (PlayerInformation, JoinHandle<Result<()>>)>,
) -> Option<(Arc<OwnedBusName>, &PlayerInformation, Lrc)> {
    available_players
        .iter()
        .filter(|(_, (i, _))| is_player_active(i))
        .find_map(|(name, (player, _))| {
            let name = Arc::clone(name);
            let Value::Str(audio_url) = player.metadata.get("xesam:url")?.deref()
            else {
                unreachable!()
            };
            let audio_path = Lrc::audio_url_to_path(audio_url).unwrap();
            match Lrc::from_audio_path(&audio_path) {
                Ok(i) => return Some((name, player, i)),
                Err(e) => {
                    tracing::debug!(?e, ?audio_path, bus_name=%name, "Failed to load lyric from audio file");
                }
            };
            let lrc_path = Lrc::audio_path_to_lrc(&audio_path);
            if lrc_path.exists() {
                match Lrc::from_lrc_path(&lrc_path) {
                    Ok(i) => return Some((name, player, i)),
                    Err(e) => {
                        tracing::debug!(?e, ?lrc_path, bus_name=%name, "Failed to load lyric from lrc file");
                    }
                }
            }
            None
        })
}
