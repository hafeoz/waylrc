use std::{
    collections::HashSet,
    future::{pending, Pending},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use futures::future::Either;
use tokio::time::{sleep, Sleep};
use zbus::{names::OwnedBusName, zvariant::Value};

use crate::{
    lrc::{Lrc, TimeTag},
    output::WaybarCustomModule,
    player::PlayerInformation,
};

use super::current_player_state::CurrentPlayerState;

// Helper function to extract u64 from metadata value
fn extract_u64_from_value(value: &zbus::zvariant::OwnedValue) -> Option<u64> {
    use std::ops::Deref;
    match value.deref() {
        Value::I64(n) => Some(*n as u64),
        Value::U64(n) => Some(*n),
        _ => None,
    }
}

pub struct LyricsManager {
    pub current_player: Option<CurrentPlayerState>,
    pub current_player_timer: Pin<Box<Either<Sleep, Pending<()>>>>,
    pub last_known_position: Option<Duration>,
    pub current_track_id: Option<String>, // Track the current song to reset loop detection on track change
    pub last_loop_count: Option<u32>, // Track the loop count to detect song loops
}

impl LyricsManager {
    pub fn new() -> Self {
        Self {
            current_player: None,
            current_player_timer: Box::pin(Either::Right(pending())),
            last_known_position: None,
            current_track_id: None,
            last_loop_count: None,
        }
    }

    pub fn clear_state(&mut self) {
        tracing::info!("No player active. Clearing previous state");
        self.current_player = None;
        self.current_player_timer = Box::pin(Either::Right(pending()));
        self.last_known_position = None;
        self.current_track_id = None;
        self.last_loop_count = None;
        WaybarCustomModule::empty().print().unwrap();
    }

    pub fn detect_loop_restart(&self, current_pos: Duration, is_position_update: bool, is_metadata_update: bool) -> bool {
        // DEBUG: Always log function entry
        tracing::info!("[DEBUG] detect_loop_restart called - current_pos: {}s, is_position_update: {}, is_metadata_update: {}",
                     current_pos.as_secs(), is_position_update, is_metadata_update);

        // Enhanced loop restart detection:
        // 1. Position update with position reset (manual seek to beginning or automatic loop)
        // 2. Metadata update when current position is near beginning and we had a previous position well into the song
        // 3. Special case: If lyrics have ended (next_lrc_timetag == u64::MAX) and position changes significantly backwards

        if !(is_position_update || is_metadata_update) {
            tracing::info!("[DEBUG] No position or metadata update, returning false");
            return false;
        }

        // Traditional loop detection: position near beginning after being well into the song
        let traditional_loop = current_pos < Duration::from_secs(15) &&
            self.last_known_position.map_or(false, |prev| prev > Duration::from_secs(60));

        // Special case: lyrics have ended and position moved backwards significantly
        let lyrics_ended_loop = self.current_player.as_ref()
            .map_or(false, |p| p.next_lrc_timetag == TimeTag::from(Duration::from_secs(u64::MAX))) &&
            self.last_known_position.map_or(false, |prev| {
                // If position moved backwards by more than 30 seconds, consider it a loop restart
                prev > current_pos && (prev - current_pos) > Duration::from_secs(30)
            });

        tracing::info!("[DEBUG] Traditional loop check: current_pos < 15s? {}, last_known > 60s? {}, result: {}",
                     current_pos < Duration::from_secs(15),
                     self.last_known_position.map_or(false, |prev| prev > Duration::from_secs(60)),
                     traditional_loop);

        tracing::info!("[DEBUG] Lyrics ended loop check: lyrics_at_end? {}, position_moved_back? {}, result: {}",
                     self.current_player.as_ref().map_or(false, |p| p.next_lrc_timetag == TimeTag::from(Duration::from_secs(u64::MAX))),
                     self.last_known_position.map_or(false, |prev| prev > current_pos && (prev - current_pos) > Duration::from_secs(30)),
                     lyrics_ended_loop);

        if traditional_loop {
            tracing::debug!("Traditional loop detected: current={}s, previous={}s",
                          current_pos.as_secs(),
                          self.last_known_position.map(|p| p.as_secs()).unwrap_or(0));
        }

        if lyrics_ended_loop {
            tracing::debug!("Lyrics-ended loop detected: current={}s, previous={}s",
                          current_pos.as_secs(),
                          self.last_known_position.map(|p| p.as_secs()).unwrap_or(0));
        }

        let result = traditional_loop || lyrics_ended_loop;
        tracing::info!("[DEBUG] detect_loop_restart returning: {}", result);
        result
    }    pub fn refresh_lyrics_display(
        &mut self,
        bus: Arc<OwnedBusName>,
        lrc: Lrc,
        info: &PlayerInformation,
        filter_keys: &HashSet<String>,
        track_id: Option<String>,
    ) {
        tracing::info!("Refreshing lyrics display for: {} (track_id: {:?})", bus, track_id);

        // Reset loop detection state if track changed
        if self.current_track_id != track_id {
            tracing::info!("Track changed, resetting loop detection state: {:?} -> {:?}",
                          self.current_track_id, track_id);
            self.last_known_position = None;
            self.current_track_id = track_id;
        }

        tracing::debug!(%bus, ?info, "Current player state refreshed");
        let mut current_timetag = info.get_current_timetag();

        // Handle position beyond song length (common during loops)
        // Get the track length from metadata if available
        let track_length_micros = info.metadata.get("mpris:length")
            .and_then(extract_u64_from_value);

        let mut is_loop_restart = false;
        if let Some(length_micros) = track_length_micros {
            let track_length = Duration::from_micros(length_micros as u64);
            let current_duration: Duration = current_timetag.into();

            if current_duration > track_length {
                tracing::info!(%bus, current_pos_secs = current_duration.as_secs(),
                             track_length_secs = track_length.as_secs(),
                             "Position beyond track length, detecting as loop restart");
                current_timetag = TimeTag::from(Duration::ZERO);
                is_loop_restart = true;
            }
        }

        // Also check if we were at the end and now we're at the beginning (another loop indicator)
        let current_duration: Duration = current_timetag.into();
        if !is_loop_restart &&
           current_duration < Duration::from_secs(10) &&
           self.current_player.as_ref().map_or(false, |p| p.next_lrc_timetag == TimeTag::from(Duration::from_secs(u64::MAX))) {
            tracing::info!(%bus, current_pos_secs = current_duration.as_secs(),
                         "Position near beginning after being at end, detecting as loop restart");
            is_loop_restart = true;
        }

        tracing::debug!(%bus, ?current_timetag, ?is_loop_restart, "Current time tag for lyrics positioning");
        let (lrc_line, next_lrc_timetag) = lrc.get(&current_timetag);
        tracing::debug!(%bus, ?lrc_line, ?next_lrc_timetag, "Found lyrics line at current position");

        WaybarCustomModule::new(
            Some(&lrc_line.join(" ")),
            None,
            Some(&info.format_metadata(filter_keys)),
            None,
            None,
        )
        .print()
        .unwrap();

        let Some(next_lrc_timetag) = next_lrc_timetag else {
            tracing::info!("Lyric has reached ending - keeping player state for loop detection");
            // Don't clear the player state when lyrics end, keep it to detect loops
            self.current_player = Some(CurrentPlayerState {
                bus,
                lrc,
                next_lrc_timetag: TimeTag::from(Duration::from_secs(u64::MAX)), // Use a very large time as a marker
            });
            self.current_player_timer = Box::pin(Either::Right(pending()));
            WaybarCustomModule::new(
                Some(""),
                None,
                Some(&info.format_metadata(filter_keys)),
                None,
                None,
            )
            .print()
            .unwrap();
            return;
        };

        self.current_player = Some(CurrentPlayerState {
            bus,
            lrc,
            next_lrc_timetag,
        });
        let till_next_timetag =
            next_lrc_timetag.duration_from(&current_timetag, info.rate.unwrap_or(1.0));
        self.current_player_timer = Box::pin(Either::Left(sleep(till_next_timetag)));
    }

    pub fn update_position(&mut self, current_pos: Duration, is_position_update: bool, is_metadata_update: bool) {
        // Always update the last known position when we have current position info
        if is_position_update || is_metadata_update {
            self.last_known_position = Some(current_pos);
        }
    }
}
