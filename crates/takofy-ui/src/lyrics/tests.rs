use takofy_model::{LyricLine, TrackLyrics};

use super::*;
use crate::message::Failure;

fn now_playing(uri: &str) -> NowPlaying {
    NowPlaying {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        ..NowPlaying::default()
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
