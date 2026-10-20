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
mod tests {
    use takofy_model::{LyricLine, TrackLyrics};

    use super::*;
    use crate::message::Failure;

    fn now_playing(uri: &str) -> NowPlaying {
        NowPlaying {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: String::new(),
            cover: None,
        }
    }

    fn track(uri: &str) -> String {
        uri.to_owned()
    }

    fn lyrics(words: &[&str]) -> TrackLyrics {
        TrackLyrics {
            lines: words
                .iter()
                .map(|words| LyricLine {
                    start_ms: None,
                    end_ms: None,
                    words: (*words).to_owned(),
                })
                .collect(),
            synced: false,
        }
    }

    fn timed(lines: &[(Option<u32>, &str)]) -> TrackLyrics {
        TrackLyrics {
            lines: lines
                .iter()
                .map(|(start_ms, words)| LyricLine {
                    start_ms: *start_ms,
                    end_ms: None,
                    words: (*words).to_owned(),
                })
                .collect(),
            synced: true,
        }
    }

    fn command(uri: &str) -> Command {
        Command::LoadLyrics { track: track(uri) }
    }

    #[test]
    fn the_panel_opens_on_the_queue() {
        assert_eq!(LyricsPanel::default().showing(), Tab::Queue);
    }

    #[test]
    fn the_lyrics_tab_asks_once_for_the_playing_track() {
        let mut panel = LyricsPanel::default();
        let now = now_playing("spotify:track:a");
        panel.select(Tab::Lyrics);

        assert_eq!(panel.request(Some(&now)), Some(command("spotify:track:a")));
        assert_eq!(panel.load(), &Load::Loading);
        assert_eq!(
            panel.request(Some(&now)),
            None,
            "one request per track, not one per frame"
        );
    }

    #[test]
    fn the_queue_tab_never_asks() {
        let mut panel = LyricsPanel::default();
        let now = now_playing("spotify:track:a");

        assert_eq!(panel.request(Some(&now)), None);
        assert_eq!(panel.load(), &Load::Idle);
    }

    #[test]
    fn nothing_playing_asks_nothing() {
        let mut panel = LyricsPanel::default();
        panel.select(Tab::Lyrics);

        assert_eq!(panel.request(None), None);
        assert_eq!(panel.load(), &Load::Idle);
    }

    #[test]
    fn a_new_track_clears_the_load_and_keeps_the_tab() {
        let mut panel = LyricsPanel::default();
        panel.select(Tab::Lyrics);
        panel.request(Some(&now_playing("spotify:track:a")));
        panel.apply(&Update::Lyrics {
            track: track("spotify:track:a"),
            lyrics: lyrics(&["one"]),
        });

        panel.sync(Some(&now_playing("spotify:track:b")));

        assert_eq!(panel.showing(), Tab::Lyrics, "the choice survives");
        assert_eq!(panel.load(), &Load::Idle);
    }

    #[test]
    fn the_same_track_changes_nothing() {
        let mut panel = LyricsPanel::default();
        panel.select(Tab::Lyrics);
        let now = now_playing("spotify:track:a");
        panel.request(Some(&now));
        let ready = lyrics(&["one"]);
        panel.apply(&Update::Lyrics {
            track: track("spotify:track:a"),
            lyrics: ready.clone(),
        });

        panel.sync(Some(&now));

        assert_eq!(panel.load(), &Load::Ready(ready));
    }

    #[test]
    fn nothing_playing_clears_the_load() {
        let mut panel = LyricsPanel::default();
        panel.select(Tab::Lyrics);
        panel.request(Some(&now_playing("spotify:track:a")));

        panel.sync(None);

        assert_eq!(panel.load(), &Load::Idle);
        assert_eq!(panel.showing(), Tab::Lyrics);
    }

    #[test]
    fn a_reply_lands_only_on_the_asked_track() {
        let mut panel = LyricsPanel::default();
        panel.select(Tab::Lyrics);
        panel.request(Some(&now_playing("spotify:track:a")));

        panel.apply(&Update::Lyrics {
            track: track("spotify:track:b"),
            lyrics: lyrics(&["elsewhere"]),
        });
        assert_eq!(panel.load(), &Load::Loading, "another track's reply");

        let asked = lyrics(&["here"]);
        panel.apply(&Update::Lyrics {
            track: track("spotify:track:a"),
            lyrics: asked.clone(),
        });
        assert_eq!(panel.load(), &Load::Ready(asked));
    }

    #[test]
    fn the_first_reply_wins() {
        let mut panel = LyricsPanel::default();
        panel.select(Tab::Lyrics);
        panel.request(Some(&now_playing("spotify:track:a")));
        let first = lyrics(&["first"]);
        panel.apply(&Update::Lyrics {
            track: track("spotify:track:a"),
            lyrics: first.clone(),
        });

        panel.apply(&Update::Lyrics {
            track: track("spotify:track:a"),
            lyrics: lyrics(&["second"]),
        });

        assert_eq!(panel.load(), &Load::Ready(first));
    }

    #[test]
    fn a_failure_lands_only_while_loading() {
        let mut panel = LyricsPanel::default();
        panel.select(Tab::Lyrics);
        panel.request(Some(&now_playing("spotify:track:a")));

        panel.apply(&Update::LyricsFailed {
            track: track("spotify:track:b"),
            failure: Failure::Lyrics,
        });
        assert_eq!(panel.load(), &Load::Loading, "another track's failure");

        panel.apply(&Update::LyricsFailed {
            track: track("spotify:track:a"),
            failure: Failure::Lyrics,
        });
        assert_eq!(panel.load(), &Load::Failed(Failure::Lyrics));

        panel.apply(&Update::LyricsFailed {
            track: track("spotify:track:a"),
            failure: Failure::Load,
        });
        assert_eq!(
            panel.load(),
            &Load::Failed(Failure::Lyrics),
            "a settled load is not replaced"
        );
    }

    #[test]
    fn retry_moves_a_failure_back_to_idle() {
        let mut panel = LyricsPanel::default();
        panel.select(Tab::Lyrics);
        panel.request(Some(&now_playing("spotify:track:a")));
        panel.apply(&Update::LyricsFailed {
            track: track("spotify:track:a"),
            failure: Failure::Lyrics,
        });

        panel.retry();

        assert_eq!(panel.load(), &Load::Idle);
        assert_eq!(
            panel.request(Some(&now_playing("spotify:track:a"))),
            Some(command("spotify:track:a")),
            "the retry asks again"
        );
    }

    #[test]
    fn retry_leaves_anything_else_alone() {
        let mut panel = LyricsPanel::default();
        panel.retry();
        assert_eq!(panel.load(), &Load::Idle);

        panel.select(Tab::Lyrics);
        panel.request(Some(&now_playing("spotify:track:a")));
        panel.retry();
        assert_eq!(panel.load(), &Load::Loading);

        let ready = lyrics(&["one"]);
        panel.apply(&Update::Lyrics {
            track: track("spotify:track:a"),
            lyrics: ready.clone(),
        });
        panel.retry();
        assert_eq!(panel.load(), &Load::Ready(ready));
    }

    #[test]
    fn an_untimed_lyric_has_no_current_line() {
        assert_eq!(current_line(&lyrics(&["one", "two"]), 1_000), None);
    }

    #[test]
    fn before_the_first_line_none_is_current() {
        let lyrics = timed(&[(Some(1_000), "one"), (Some(2_000), "two")]);
        assert_eq!(current_line(&lyrics, 0), None);
        assert_eq!(current_line(&lyrics, 999), None);
    }

    #[test]
    fn a_lyric_with_no_starts_has_no_current_line() {
        let lyrics = timed(&[(None, "one"), (None, "two")]);
        assert_eq!(current_line(&lyrics, 10_000), None);
    }

    #[test]
    fn the_last_line_that_has_started_is_current() {
        let lyrics = timed(&[
            (Some(1_000), "one"),
            (Some(2_000), "two"),
            (Some(3_000), "three"),
        ]);
        assert_eq!(current_line(&lyrics, 1_000), Some(0));
        assert_eq!(current_line(&lyrics, 2_500), Some(1));
        assert_eq!(current_line(&lyrics, 3_000), Some(2));
        assert_eq!(current_line(&lyrics, 9_000), Some(2));
    }

    #[test]
    fn a_line_without_a_start_is_never_current() {
        let lyrics = timed(&[(Some(1_000), "one"), (None, "gap"), (Some(3_000), "three")]);
        assert_eq!(current_line(&lyrics, 2_000), Some(0));
        assert_eq!(current_line(&lyrics, 3_500), Some(2));
    }
}
