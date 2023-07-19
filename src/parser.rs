//! A small parser for LRC files.
//!
//! # TODO
//!
//! Repeating tags are not currently supported. For example, following line of lyric will not be
//! parsed correctly:
//!
//! ```text
//! [00:21.10][00:45.10]Repeating lyrics (e.g. chorus)
//! ```

use core::{fmt::Debug, str::FromStr, time::Duration};
use std::io::{BufRead, BufReader};

use itertools::Itertools;
use regex::Regex;
use tracing::instrument;

#[cfg(test)]
mod tests;

/// A time offset from the start of the song.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeTag(pub Duration);
impl AsRef<Duration> for TimeTag {
    fn as_ref(&self) -> &Duration {
        &self.0
    }
}
impl From<Duration> for TimeTag {
    fn from(d: Duration) -> Self {
        Self(d)
    }
}
impl From<TimeTag> for Duration {
    fn from(t: TimeTag) -> Self {
        t.0
    }
}

/// A line of lyrics with a time tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub time: TimeTag,
    pub text: String,
}

/// A collection of lines of lyrics.
///
/// It is a two-dimensional vector because lyrics may have multiple "versions" (typically for multiple languages).
///
/// Each inner vector is a list of lines for a single version.
///
/// The outer vector is a list of "versions".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lrc(pub Vec<Vec<Line>>);

pub mod error {
    use thiserror::Error;

    #[derive(Error, Debug)]
    pub enum TimeTagFromStr {
        #[error("invalid format: {0}")]
        InvalidFormat(String),
        #[error("invalid integer {0}: {1}")]
        InvalidInteger(String, #[source] std::num::ParseIntError),
        #[error("invalid float {0}: {1}")]
        InvalidFloat(String, #[source] std::num::ParseFloatError),
    }

    #[derive(Error, Debug)]
    pub enum LineFromStr {
        #[error("no tag present")]
        NoTag,
        #[error("tag is not a valid time tag: {0}")]
        InvalidTimeTag(#[from] TimeTagFromStr),
        #[error("empty text")]
        EmptyText,
    }
}

impl FromStr for TimeTag {
    type Err = error::TimeTagFromStr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // We should parse [mm:ss.xx] and [mm:ss.xxx] formats.
        let [minutes, seconds]: [&str; 2] = s
            .split(':')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| error::TimeTagFromStr::InvalidFormat(s.to_owned()))?;
        let minutes = minutes
            .parse::<u64>()
            .map_err(|e| error::TimeTagFromStr::InvalidInteger(minutes.to_owned(), e))?;
        let seconds = seconds
            .parse::<f64>()
            .map_err(|e| error::TimeTagFromStr::InvalidFloat(seconds.to_owned(), e))?;
        Ok(TimeTag::from(
            Duration::from_secs(minutes * 60) + Duration::from_secs_f64(seconds),
        ))
    }
}

impl FromStr for Line {
    type Err = error::LineFromStr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(error::LineFromStr::EmptyText);
        }

        // Each line should be in the format [mm:ss.xx]text.

        // Remove the leading '['
        let s = s.strip_prefix('[').ok_or(error::LineFromStr::NoTag)?;
        // Split the time tag and the text
        let (tag, text) = s.split_once(']').ok_or(error::LineFromStr::NoTag)?;
        // Parse the time tag
        let time = tag.parse::<TimeTag>()?;
        // Remove Walaoke gender extension
        let text = text
            .trim_start_matches("F:")
            .trim_start_matches("M:")
            .trim_start_matches("D:");
        // Remove A2 world time extension
        // Each line may have many World Time tags with format <mm:ss.xx>
        let a2_world_time_regex = Regex::new(r"<\d{2}:\d{2}\.\d{2}>\s?").unwrap();
        let text = a2_world_time_regex.replace_all(text, "");
        let text = text.trim();
        if text.is_empty() {
            return Err(error::LineFromStr::EmptyText);
        }

        Ok(Line {
            time,
            text: text.to_string(),
        })
    }
}

impl Line {
    /// Append text to the end of the line.
    ///
    /// Some LRC files have lines that are split into multiple lines, but the parser by design
    /// only recognizes one lrc line per file line. This function allows you to append text to
    /// the end of the line.
    pub fn push_text(&mut self, text: &str) {
        self.text.push(' ');
        self.text.push_str(text);
    }
}

impl Lrc {
    /// Parse an LRC file from a reader.
    fn from_reader<R: BufRead>(s: R) -> Result<Self, std::io::Error> {
        let lines = s
            .lines()
            .map_ok(|l| (l.parse::<Line>(), l)) // Parse each line
            .fold_ok(
                (vec![Vec::new()], TimeTag::from(Duration::ZERO)), // Start with an empty vector of versions and a zero time tag.
                |(mut versions, mut last_timestamp), (parsed_line, raw_string)| {
                    // Update the last timestamp
                    if let Ok(parsed_line) = &parsed_line {
                        if last_timestamp.as_ref() > parsed_line.time.as_ref() {
                            // If the last timestamp is greater than the current timestamp, we have a new "version" and should start a new vector.
                            versions.push(Vec::new());
                        }
                        last_timestamp = parsed_line.time;
                    }
                    // Unwrap: we're starting with one element in the vector.
                    let version = versions.last_mut().unwrap();

                    match parsed_line {
                        Ok(l) => {
                            // If the line parsed successfully, add it to the vector.
                            version.push(l);
                            tracing::info!("parsed line: {}", raw_string);
                        }
                        Err(error::LineFromStr::NoTag) => {
                            // If the line has no tag, append it to the last line.
                            if version.is_empty() {
                                // If there is no last line, create one.
                                version.push(Line {
                                    time: TimeTag(Duration::from_secs(0)),
                                    text: String::new(),
                                });
                                tracing::warn!("no time tag present on first line");
                            }
                            // UNWRAP: We just checked that the vector is not empty.
                            version.last_mut().unwrap().push_text(&raw_string);
                            tracing::info!("appended text to last line: {}", raw_string);
                        }
                        Err(e) => {
                            tracing::warn!("failed to parse line: {}", e);
                        }
                    };
                    (versions, last_timestamp)
                },
            )?
            .0;
        Ok(Lrc(lines))
    }

    /// Parse an LRC file from a file.
    #[instrument]
    pub fn from_file<P: AsRef<std::path::Path> + Debug>(path: &P) -> Result<Self, std::io::Error> {
        let mut file = BufReader::new(std::fs::File::open(path)?);
        Self::from_reader(&mut file)
    }

    #[instrument(skip(s))]
    pub fn from_str(s: &str) -> Result<Self, std::io::Error> {
        Self::from_reader(s.as_bytes())
    }

    /// Get lyrics for a given time, and the time tag of the next line.
    #[must_use]
    pub fn get_lyrics(&self, time: TimeTag) -> (Vec<&Line>, Option<TimeTag>) {
        // We want to find the earliest next line in all "versions"
        let mut next_timetag: Option<TimeTag> = None;
        let lines = self
            .0
            .iter()
            .filter_map(|version| {
                let mut lines = version.iter();
                let line = lines
                    .take_while_ref(|line| {
                        // Take all lines that are before the given time
                        line.time.as_ref() <= time.as_ref()
                    })
                    .last();
                // Find the next timetag in this version
                let version_next_timetag = lines.next().map(|line| line.time);
                match (&mut next_timetag, version_next_timetag) {
                    (Some(next_timetag), Some(version_next_timetag))
                        if (version_next_timetag.as_ref() < next_timetag.as_ref()) =>
                    {
                        *next_timetag = version_next_timetag;
                        tracing::info!("found earlier next timetag: {:?}", next_timetag);
                    }
                    (None, Some(version_next_timetag)) => {
                        next_timetag = Some(version_next_timetag);
                        tracing::info!("found next timetag: {:?}", next_timetag);
                    }
                    _ => {}
                }
                line
            })
            .collect();
        (lines, next_timetag)
    }
}
