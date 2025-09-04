use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    io::BufReader,
    ops::Deref,
    str::FromStr,
    time::Instant,
};

use anyhow::{anyhow, Context as _, Result};
use futures_lite::{stream::Fuse, StreamExt as _};
use tokio::{
    select,
    time::{interval, Duration, Interval},
};
use zbus::{
    proxy::PropertyStream,
    zvariant::{OwnedValue, Value},
};

use crate::{
    dbus::player::{PlayerProxy, SeekedStream},
    external_lrc_provider::{
        navidrome::{fetch_lyrics_from_navidrome, NavidromeConfig},
        ExternalLrcProvider,
    },
    lrc::{Lrc, TimeTag},
    utils::extract_str,
};

const MAX_METADATA_VALUE_LEN: usize = 256;

/// Current playback status of a MPRIS-compliant player
#[derive(Eq, PartialEq, Debug)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}
impl FromStr for PlaybackStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_ref() {
            "playing" => Ok(Self::Playing),
            "paused" => Ok(Self::Paused),
            "stopped" => Ok(Self::Stopped),
            _ => Err(anyhow!("Unknown PlaybackStatus {s}")),
        }
    }
}

#[derive(Debug)]
pub struct PlayerInformation {
    pub metadata: std::collections::HashMap<String, OwnedValue>,
    pub position: i64,
    pub position_last_refresh: Instant,
    pub rate: Option<f64>,
    pub status: Option<PlaybackStatus>,
}
impl PlayerInformation {
    #[must_use]
    fn format_value<'a>(v: &'a Value<'_>) -> Cow<'a, str> {
        match v {
            zbus::zvariant::Value::U8(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::Bool(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::I16(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::U16(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::I32(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::U32(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::I64(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::U64(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::F64(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::Str(v) => Cow::Owned(v.to_string()),
            zbus::zvariant::Value::Signature(s) => Cow::Owned(s.to_string()),
            zbus::zvariant::Value::ObjectPath(o) => Cow::Borrowed(o.as_str()),
            zbus::zvariant::Value::Value(v) => Self::format_value(v),
            zbus::zvariant::Value::Array(a) => Cow::Owned(
                a.iter()
                    .map(Self::format_value)
                    .collect::<Vec<_>>()
                    .join(";"),
            ),
            zbus::zvariant::Value::Dict(d) => Cow::Owned(
                d.iter()
                    .map(|(k, v)| format!("{}={}", Self::format_value(k), Self::format_value(v)))
                    .collect::<Vec<_>>()
                    .join(";"),
            ),
            zbus::zvariant::Value::Structure(s) => Cow::Owned(
                s.fields()
                    .iter()
                    .map(Self::format_value)
                    .collect::<Vec<_>>()
                    .join(";"),
            ),
            zbus::zvariant::Value::Fd(_) => Cow::Borrowed("fd"),
        }
    }
    pub fn metadata<'a>(
        &'a self,
        filter_keys: &'a HashSet<String>,
    ) -> impl Iterator<Item = (&'a String, Cow<'a, str>)> {
        self.metadata
            .iter()
            .filter(|(k, _)| filter_keys.get(k.as_str()).is_none())
            .map(|(k, v)| (k, Self::format_value(v)))
    }
    pub fn format_metadata(&self, filter_keys: &HashSet<String>) -> String {
        self.metadata(filter_keys)
            .map(|(k, v)| {
                if v.len() > MAX_METADATA_VALUE_LEN {
                    (k, Cow::Owned(format!("({} bytes blob)", v.len())))
                } else {
                    (k, v)
                }
            })
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn has_lyrics(&self) -> bool {
        // First check if lyrics are exposed from MPRIS metadata
        if self
            .metadata
            .get("xesam:asText")
            .map(Deref::deref)
            .and_then(extract_str)
            .is_some()
        {
            return true;
        }

        // Then check if we have a file URL that might contain lyrics
        if let Some(audio_url) = self
            .metadata
            .get("xesam:url")
            .map(Deref::deref)
            .and_then(extract_str)
        {
            match Lrc::audio_url_to_path(audio_url) {
                Ok(i) if i.is_file() => return true,
                Err(e) => {
                    tracing::debug!(%e, "URL is not a file path, but will continue processing");
                }
                _ => {}
            }
        }

        // If we have any metadata at all, assume we might be able to work with it
        // This prevents the program from stopping when URL is not a file://
        !self.metadata.is_empty()
    }
    pub fn get_lyrics(&self) -> Option<Result<Lrc>> {
        // Attempt to extract lyrics from MPRIS
        let mpris_lrc;
        if let Some(lrc) = self
            .metadata
            .get("xesam:asText")
            .map(Deref::deref)
            .and_then(extract_str)
        {
            tracing::debug!("Using lyrics from MPRIS asText metadata");
            if lrc.lines().count() == 1 {
                // Lines are concatenated for some reason - parse them on best-effort basis
                // Only parse them when really needed - no LRC or audio files
                mpris_lrc = Some(|| {
                    tracing::warn!(
                        "Lyric lines are concatenated - parsing them might be inaccurate"
                    );
                    let lrc = lrc
                        .split(" [")
                        .map(|l| format!(" [{l}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    Lrc::from_reader(BufReader::new(lrc.as_bytes()))
                        .context("Failed to parse lrc from MPRIS metadata")
                });
            } else {
                return Some(
                    Lrc::from_reader(BufReader::new(lrc.as_bytes()))
                        .context("Failed to parse lrc from MPRIS metadata"),
                );
            }
        } else {
            mpris_lrc = None;
        }

        // Try to get lyrics from file if URL is a file path
        if let Some(audio_url) = self
            .metadata
            .get("xesam:url")
            .map(Deref::deref)
            .and_then(extract_str)
        {
            match Lrc::audio_url_to_path(audio_url) {
                Ok(audio_path) => {
                    // Attempt to extract lyrics from discrete LRC file
                    let lrc_path = Lrc::audio_path_to_lrc(&audio_path);
                    if lrc_path.is_file() {
                        tracing::debug!("Using lyrics from LRC file");
                        return Some(Lrc::from_lrc_path(&lrc_path));
                    }
                    tracing::debug!("Using lyrics from media tags");
                    // Attempt to extract lyrics from media tags
                    return Some(Lrc::from_audio_path(&audio_path));
                }
                Err(e) => {
                    tracing::debug!(%e, "URL is not a file path, trying MPRIS metadata fallback");
                }
            }
        }

        // Fall back to MPRIS lyrics if available
        if let Some(mpris_lrc) = mpris_lrc {
            return Some(mpris_lrc());
        }

        tracing::warn!("No lyrics found but get_lyrics is called");
        None
    }

    /// Get lyrics with external provider support (async version)
    pub async fn get_lyrics_with_external(
        &self,
        external_providers: &[ExternalLrcProvider],
        navidrome_config: Option<&NavidromeConfig>,
    ) -> Option<Result<Lrc>> {
        // First try local sources (same as get_lyrics)
        if let Some(result) = self.get_lyrics() {
            return Some(result);
        }

        // If no local lyrics found, try external providers
        for provider in external_providers {
            match provider {
                ExternalLrcProvider::NAVIDROME => {
                    if let Some(config) = navidrome_config {
                        tracing::debug!("Trying to fetch lyrics from Navidrome");

                        match fetch_lyrics_from_navidrome(
                            &config.server_url,
                            &config.username,
                            &config.password,
                            &self.metadata,
                        )
                        .await
                        {
                            Ok(lyrics_text) => {
                                tracing::info!("Successfully fetched lyrics from Navidrome");
                                // Parse the lyrics text into LRC format
                                return Some(
                                    Lrc::from_reader(BufReader::new(lyrics_text.as_bytes()))
                                        .context("Failed to parse lyrics from Navidrome"),
                                );
                            }
                            Err(e) => {
                                tracing::warn!("Failed to fetch lyrics from Navidrome: {:?}", e);
                            }
                        }
                    } else {
                        tracing::warn!("Navidrome provider selected but no configuration provided");
                    }
                }
            }
        }

        None
    }
}
pub struct PlayerInformationUpdateListener<'a> {
    player: PlayerProxy<'a>,
    metadata_stream: Fuse<PropertyStream<'a, HashMap<String, OwnedValue>>>,
    rate_stream: Fuse<PropertyStream<'a, f64>>,
    status_stream: Fuse<PropertyStream<'a, String>>,
    seeked: SeekedStream,
    position_refresh_stream: Interval,
}
#[derive(Debug)]
pub enum PlayerInformationUpdate {
    Metadata(HashMap<String, OwnedValue>),
    Rate(f64),
    Status(PlaybackStatus),
    Position(i64, Instant),
}
impl PlayerInformation {
    pub async fn new(player: &PlayerProxy<'_>) -> Result<Self> {
        Ok(Self {
            metadata: player
                .metadata()
                .await
                .inspect_err(|e| {
                    tracing::warn!(?e, "Failed to get player metadata");
                })
                .ok()
                .unwrap_or_default(),
            position: player
                .position()
                .await
                .context("Failed to get player position")?,
            rate: player
                .rate()
                .await
                .inspect_err(|e| {
                    tracing::warn!(?e, "Failed to get player playback rate");
                })
                .ok(),
            status: player
                .playback_status()
                .await
                .inspect_err(|e| {
                    tracing::warn!(?e, "Failed to get player playback status");
                })
                .ok()
                .as_deref()
                .map(str::parse)
                .transpose()
                .context("Failed to parse player playback status")?,
            position_last_refresh: Instant::now(),
        })
    }

    pub fn apply_update(&mut self, update: PlayerInformationUpdate) {
        match update {
            PlayerInformationUpdate::Metadata(metadata) => {
                self.metadata = metadata;
            }
            PlayerInformationUpdate::Rate(rate) => {
                self.rate = Some(rate);
            }
            PlayerInformationUpdate::Status(status) => {
                self.status = Some(status);
            }
            PlayerInformationUpdate::Position(position, instant) => {
                self.position = position;
                self.position_last_refresh = instant;
            }
        }
    }

    /// Calculate the total elapsed time including accumulated time since last position update
    #[must_use]
    fn calculate_total_elapsed(&self) -> Duration {
        assert!(self.position >= 0, "Negative timetag encountered");

        // Only accumulate elapsed time if the player is actually playing
        let elapsed = if matches!(self.status, Some(PlaybackStatus::Playing)) {
            Duration::from_secs_f64(
                self.position_last_refresh.elapsed().as_secs_f64() / self.rate.unwrap_or(1.0),
            )
        } else {
            Duration::ZERO
        };

        Duration::from_micros(self.position as u64) + elapsed
    }

    #[must_use]
    pub fn get_current_timetag(&self) -> TimeTag {
        let calculated_position = self.calculate_total_elapsed();

        // Get track length from metadata to prevent position from exceeding track duration
        let track_length = self.metadata.get("mpris:length")
            .and_then(|value| {
                use std::ops::Deref;
                match value.deref() {
                    zbus::zvariant::Value::I64(micros) => Some(Duration::from_micros(*micros as u64)),
                    zbus::zvariant::Value::U64(micros) => Some(Duration::from_micros(*micros)),
                    _ => None,
                }
            });

        // If we have track length and calculated position exceeds it, clamp to track length
        // This prevents endless time accumulation when song loops but MPRIS hasn't updated position yet
        let final_position = if let Some(length) = track_length {
            if calculated_position > length {
                tracing::debug!("Position {}s exceeds track length {}s, clamping to track length",
                              calculated_position.as_secs(), length.as_secs());
                length
            } else {
                calculated_position
            }
        } else {
            calculated_position
        };

        TimeTag(final_position)
    }

    /// Get the current loop count (how many times the song has looped)
    /// Returns (loop_count, position_within_current_loop)
    #[must_use]
    pub fn get_loop_count(&self) -> (u32, Duration) {
        let total_elapsed = self.calculate_total_elapsed();

        // Get track length from metadata
        let track_length = self.metadata.get("mpris:length")
            .and_then(|value| {
                use std::ops::Deref;
                match value.deref() {
                    zbus::zvariant::Value::I64(micros) => Some(Duration::from_micros(*micros as u64)),
                    zbus::zvariant::Value::U64(micros) => Some(Duration::from_micros(*micros)),
                    _ => None,
                }
            });

        if let Some(length) = track_length {
            if length.as_millis() > 0 {
                let total_millis = total_elapsed.as_millis();
                let length_millis = length.as_millis();
                let loop_count = (total_millis / length_millis) as u32;
                let position_in_loop = Duration::from_millis((total_millis % length_millis) as u64);
                return (loop_count, position_in_loop);
            }
        }

        // If no track length available, assume no loops
        (0, total_elapsed)
    }
}

impl<'a> PlayerInformationUpdateListener<'a> {
    pub async fn new(player: PlayerProxy<'a>, refresh_interval: Duration) -> Result<Self> {
        Ok(Self {
            metadata_stream: player.receive_metadata_changed().await.fuse(),
            rate_stream: player.receive_rate_changed().await.fuse(),
            status_stream: player.receive_playback_status_changed().await.fuse(),
            seeked: player
                .receive_seeked()
                .await
                .context("Failed to receive seek signal")?,
            position_refresh_stream: interval(refresh_interval),
            player,
        })
    }
    pub async fn update(&mut self) -> Result<PlayerInformationUpdate> {
        select! {
            metadata = self.metadata_stream.next() => {
                metadata.context("Failed to receive metadata update event")?.get().await.context("Failed to get player metadata").map(PlayerInformationUpdate::Metadata)
            },
            rate = self.rate_stream.next() => {
                rate.context("Failed to receive rate update event")?.get().await.context("Failed to get player playback rate").map(PlayerInformationUpdate::Rate)
            },
            status = self.status_stream.next() => {
                status.context("Failed to receive status update event")?.get().await.context("Failed to get player playback status")?.parse().map(PlayerInformationUpdate::Status)
            }
            seek = self.seeked.next() => {
                seek.context("Failed to receive seek signal")?.args().context("Failed to get player seeked position").map(|p| PlayerInformationUpdate::Position(p.position, Instant::now()))
            }
            _ = self.position_refresh_stream.tick() => {
                self.player.position().await.context("Failed to get player position").map(|p| PlayerInformationUpdate::Position(p, Instant::now()))
            }
        }
    }
}
