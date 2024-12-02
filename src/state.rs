//! Internal state of the lyric daemon

use core::time::Duration;
use std::path::PathBuf;

use itertools::Itertools;
use lofty::TaggedFileExt;
use mpris::{DBusError, Metadata, Player, PlayerFinder};
use percent_encoding::percent_decode;

use crate::{out::WaybarCustomModule, parser::Lrc};

/// Cached information about a song
struct SongInfo {
    /// Formatted metadata available for display
    pub metadata: String,
    /// The parsed lyrics
    pub lyrics: Option<Lrc>,
}

pub struct State {
    /// An MPRIS player finder
    mpris_finder: PlayerFinder,
    /// An active MPRIS player
    player: Option<Player>,
    /// The current song's data
    song: Option<(String, SongInfo)>,
    /// The maximum time to sleep between metadata updates
    max_sleep: Duration,
}

impl SongInfo {
    /// Format the metadata for display
    fn format_metadata(metadata: &Metadata) -> String {
        let mut result = String::new();
        if let Some(name) = metadata.album_name() {
            result.push_str("album: ");
            result.push_str(name);
            result.push('\n');
        }
        if let Some(name) = metadata.title() {
            result.push_str("title: ");
            result.push_str(name);
            result.push('\n');
        }
        if let Some(name) = metadata.artists() {
            result.push_str("artists: ");
            result.push_str(name.join(", ").as_str());
            result.push('\n');
        }
        result
    }
    /// Create a new ``SongInfo`` from metadata
    pub fn new(metadata: &Metadata) -> Self {
        let url = metadata
            .url()
            .and_then(|s| s.strip_prefix("file://"))
            .map(|s| percent_decode(s.as_bytes()).decode_utf8_lossy().to_string());

        let lyrics = url.and_then(|url| {
            // First, try to load external lyrics
            let lrc_url = PathBuf::from(&url).with_extension("lrc");
            if lrc_url.exists() {
                Lrc::from_file(&lrc_url)
            } else {
                // If that fails, try to load embedded lyrics
                let file = lofty::read_from_path(&url)
                    .inspect_err(|e| tracing::warn!("Failed to read file {}: {}", url, e))
                    .ok()?;
                let tags = file
                    .tags()
                    .iter()
                    .filter_map(|tag| tag.get(&lofty::ItemKey::Lyrics))
                    .filter_map(|item| item.value().text())
                    .join("\n");
                Lrc::from_str(&tags)
            }
            .inspect_err(|e| tracing::warn!("Failed to parse lyrics {}: {}", url, e))
            .inspect(|l| tracing::info!("Loaded lyrics for {}: {:?}", url, l))
            .ok()
        });
        let metadata = Self::format_metadata(metadata);
        Self { metadata, lyrics }
    }
}

impl State {
    /// Create a new, empty player state
    ///
    /// # Panics
    ///
    /// Panics if the `DBus` connection cannot be established.
    #[must_use]
    pub fn new(max_sleep: Duration) -> Self {
        Self {
            mpris_finder: PlayerFinder::new().unwrap(),
            player: None,
            song: None,
            max_sleep,
        }
    }

    /// Find the active player
    fn try_find_player(&mut self) -> Result<Option<&mut Player>, DBusError> {
        if self.player.is_none() {
            self.player = match self.mpris_finder.find_active() {
                Ok(player) => Some(player),
                Err(mpris::FindingError::NoPlayerFound) => None,
                Err(mpris::FindingError::DBusError(err)) => return Err(err),
            };
        }
        Ok(self.player.as_mut())
    }

    /// Get the current lyrics and duration until the next refresh
    ///
    /// # Errors
    ///
    /// Returns an error if the `DBus` connection fails.
    pub fn update(&mut self) -> Result<(Option<WaybarCustomModule>, Duration), DBusError> {
        let Some(player) = self.try_find_player()? else { return Ok((None, self.max_sleep)) };
        let metadata = player.get_metadata()?;
        let position = player.get_position()?.into();

        if let Some((uri, _)) = &self.song {
            if uri != metadata.url().unwrap_or_default() {
                self.song = None;
            }
        }
        let song = self.song.get_or_insert_with(|| {
            (
                metadata.url().unwrap_or_default().to_owned(),
                SongInfo::new(&metadata),
            )
        });

        // Get the current lyrics
        let (lyrics, next_timetag) = song
            .1
            .lyrics
            .as_ref()
            .map(|l| l.get_lyrics(position))
            .map(|(l, timetag)| (l.into_iter().map(|l| &l.text).join(" "), timetag))
            .unwrap_or_default();

        let mut next_timetag_min = self.max_sleep;
        if let Some(next_timetag) = next_timetag {
            next_timetag_min = next_timetag_min.min(next_timetag.0 - position.0);
        }

        let module =
            WaybarCustomModule::new(Some(&lyrics), None, Some(&song.1.metadata), None, None);

        Ok((Some(module), next_timetag_min))
    }
}
