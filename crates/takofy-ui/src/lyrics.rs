//! The lyrics panel's state and the line the transport is on.

use takofy_model::TrackLyrics;

use crate::library::Load;
use crate::message::{Command, Update};
use crate::player::NowPlaying;

/// Which view the panel under the library draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    /// The play queue.
    #[default]
    Queue,
    /// The playing track's lyrics.
    Lyrics,
}

/// The panel's tab, the track its load is about and that load.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LyricsPanel {
    tab: Tab,
    /// The track the load is about, so a stale reply can be told apart.
    track: Option<String>,
    load: Load<TrackLyrics>,
}

impl LyricsPanel {
    /// The tab to draw. The choice is the panel's; nothing else may hide it.
    pub fn showing(&self) -> Tab {
        self.tab
    }

    /// Follows the transport: a different track clears the load and keeps the
    /// tab.
    pub fn sync(&mut self, now: Option<&NowPlaying>) {
        let track = now.map(|now| now.uri.as_str());
        if self.track.as_deref() != track {
            self.track = track.map(str::to_owned);
            self.load = Load::Idle;
        }
    }

    /// The tab a press asked for.
    pub fn select(&mut self, tab: Tab) {
        self.tab = tab;
    }

    /// The fetch the open tab wants, if one is due.
    pub fn request(&mut self, now: Option<&NowPlaying>) -> Option<Command> {
        let now = now?;
        if self.tab != Tab::Lyrics || self.load != Load::Idle {
            return None;
        }
        self.track = Some(now.uri.clone());
        self.load = Load::Loading;
        Some(Command::LoadLyrics {
            track: now.uri.clone(),
        })
    }

    /// Retry after a failure: the load goes back to `Idle` so `request` fires
    /// again.
    pub fn retry(&mut self) {
        if matches!(self.load, Load::Failed(_)) {
            self.load = Load::Idle;
        }
    }

    /// The reply, when it belongs to the track this panel asked about.
    pub fn apply(&mut self, update: &Update) {
        match update {
            Update::Lyrics { track, lyrics } if self.wanted(track) => {
                self.load = Load::Ready(lyrics.clone());
            }
            Update::LyricsFailed { track, failure } if self.wanted(track) => {
                self.load = Load::Failed(*failure);
            }
            _ => {}
        }
    }

    /// The panel's load, for the view to draw.
    pub fn load(&self) -> &Load<TrackLyrics> {
        &self.load
    }

    /// Whether a reply naming `track` belongs to the request in flight.
    fn wanted(&self, track: &str) -> bool {
        self.track.as_deref() == Some(track) && matches!(self.load, Load::Loading)
    }
}

/// The index of the line being sung, for a synced lyric.
pub fn current_line(lyrics: &TrackLyrics, position_ms: u32) -> Option<usize> {
    if !lyrics.synced {
        return None;
    }
    let mut current = None;
    for (index, line) in lyrics.lines.iter().enumerate() {
        if let Some(start) = line.start_ms
            && start <= position_ms
        {
            current = Some(index);
        }
    }
    current
}

#[cfg(test)]
mod tests;
