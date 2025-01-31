//! A small parser for LRC files.

#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, BufRead, BufReader},
    ops::Bound::{Included, Unbounded},
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use anyhow::{anyhow, bail, Context as _, Result};
use lofty::file::TaggedFileExt as _;

/// A time offset from the start of the song.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
impl TimeTag {
    #[must_use]
    pub fn duration_from(&self, from: &Self, rate: f64) -> Duration {
        Duration::from_secs_f64((self.0 - from.0).as_secs_f64() / rate)
    }
}

pub struct LrcLine {
    pub time: Vec<TimeTag>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lrc(pub Vec<BTreeMap<TimeTag, String>>);

impl FromStr for TimeTag {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Parse both mm:ss.xx and mm:ss:xx formats
        let [minutes, seconds]: [&str; 2] = s
            .split(':')
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_vec| anyhow!("Invalid time tag {s}"))?;
        let minutes = minutes
            .parse::<u64>()
            .with_context(|| format!("Failed to parse {minutes} as u64"))?;
        let seconds = seconds
            .parse::<f64>()
            .with_context(|| format!("Failed to parse {seconds} as f64"))?;
        Ok(Self(Duration::from_secs_f64(
            (minutes as f64).mul_add(60.0, seconds),
        )))
    }
}

impl LrcLine {
    pub fn from_str(mut s: &str) -> Self {
        let mut time = Vec::with_capacity(1);
        loop {
            s = s.trim_start();
            if s.is_empty() {
                break;
            }
            if let Some((Ok(tag), rest)) = s
                .split_once(']')
                .and_then(|(tag, rest)| tag.strip_prefix('[').map(|tag| (tag, rest)))
                .map(|(tag, rest)| (tag.parse(), rest))
            {
                time.push(tag);
                s = rest;
            } else {
                break;
            }
        }

        // Remove Walaoke gender extension
        s = s
            .trim_start_matches("F:")
            .trim_start_matches("M:")
            .trim_start_matches("D:")
            .trim_start();

        // Remove A2 world time extension
        let mut text = String::with_capacity(s.len());
        let mut s = s.chars();
        while let Some(c) = s.next() {
            if c == '<' {
                // Match <mm:ss.xx> format
                let mut tag = String::with_capacity(8);
                loop {
                    match s.next() {
                        Some(c) if c != '>' => tag.push(c),
                        _ => break,
                    }
                }
                if tag.parse::<TimeTag>().is_ok() {
                    // Skip following whitespace
                    for c in s.by_ref() {
                        if !c.is_whitespace() {
                            text.push(c);
                            break;
                        }
                    }
                } else {
                    text.push('<');
                    text.push_str(&tag);
                    text.push('>');
                }
            } else {
                text.push(c);
            }
        }

        Self { time, text }
    }
}

impl Lrc {
    /// Parse an LRC file from a reader.
    pub fn from_reader<R: BufRead>(r: R) -> Result<Self, io::Error> {
        let lines = r
            .lines()
            .map(|l| l.map(|l| LrcLine::from_str(&l)))
            .collect::<Result<Vec<_>, _>>()?;
        let mut lrc = vec![BTreeMap::<_, String>::new()];
        for line in lines {
            // Unwrap: lrc is guaranteed to have at least one element
            let lrc_last = lrc.last_mut().unwrap();
            match line.time.len() {
                0 => {
                    if let Some(mut entry_last) = lrc_last.last_entry() {
                        entry_last.get_mut().push_str(&line.text);
                    } else {
                        lrc_last.insert(TimeTag(Duration::ZERO), line.text);
                    }
                }
                1 => match lrc_last.last_entry() {
                    Some(l) if l.key() > &line.time[0] => {
                        lrc.push(BTreeMap::new());
                        // Unwrap: we've just pushed an element
                        lrc.last_mut().unwrap().insert(line.time[0], line.text);
                    }
                    _ => {
                        lrc_last.insert(line.time[0], line.text);
                    }
                },
                _ => {
                    for time in line.time {
                        lrc_last.insert(time, line.text.clone());
                    }
                }
            }
        }

        Ok(Self(lrc))
    }

    pub fn audio_url_to_path(url: &str) -> Result<PathBuf> {
        let url = match urlencoding::decode(url) {
            Ok(i) => i,
            Err(e) => bail!("Failed to decode URL {url}: {e:?}"),
        };
        url.strip_prefix("file://")
            .ok_or_else(|| anyhow!("URL is not file"))
            .map(PathBuf::from)
    }
    pub fn audio_path_to_lrc(path: &Path) -> PathBuf {
        path.with_extension("lrc")
    }

    pub fn from_lrc_path(path: &Path) -> Result<Self> {
        File::open(path)
            .map(BufReader::new)
            .context("Failed to read {lrc_url}")
            .and_then(|f| Self::from_reader(f).context("Failed to parse {lrc_url}"))
    }

    pub fn from_audio_path(path: &Path) -> Result<Self> {
        lofty::read_from_path(path)
            .context("Failed to read {url}")
            .map(|f| {
                f.tags()
                    .iter()
                    .filter_map(|t| t.get(&lofty::tag::ItemKey::Lyrics))
                    .filter_map(|t| t.value().text())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .and_then(|f| Self::from_reader(f.as_bytes()).context("Failed to parse {url}"))
    }

    #[must_use]
    pub fn floor(&self, time: TimeTag) -> TimeTag {
        let mut floor_time = time;
        for lines in &self.0 {
            let Some((time, _)) = lines.range((Unbounded, Included(time))).next() else { continue; };
            if floor_time < *time {
                floor_time = *time;
            }
        }
        floor_time
    }

    #[must_use]
    pub fn get(&self, time: &TimeTag) -> (Vec<&str>, Option<TimeTag>) {
        let mut next_time = None;
        let mut texts = Vec::with_capacity(self.0.len());
        for lines in &self.0 {
            let mut lines = lines.range(time..);
            let Some((_, text)) = lines.next() else {
                continue;
            };
            let time = lines.next().map(|(t, _)| *t);
            if let Some(t) = time {
                if next_time.is_none_or(|n| t < n) {
                    next_time = Some(t);
                }
            }
            texts.push(text.as_str());
        }
        (texts, next_time)
    }
}
