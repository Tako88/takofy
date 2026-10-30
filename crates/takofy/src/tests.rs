use std::vec::IntoIter;

use log::{Level, Metadata, Record};
use takofy_core::hold::HOLD_LOG_TARGET;
use takofy_core::oauth::{OAUTH_LOG_TARGET, RedirectMode};
use takofy_core::playback::PLAYBACK_LOG_TARGET;
use takofy_core::receive::RECEIVE_LOG_TARGET;
use takofy_ui::cover::COVER_LOG_TARGET;
use takofy_ui::remote::REMOTE_LOG_TARGET;

use crate::bridge::{LANDING_LOG_TARGET, STARTUP_LOG_TARGET};

use super::{Flags, Mode, USAGE, logger_builder, mode_for, parse_flags};

fn record<'a>(target: &'a str, level: Level) -> Record<'a> {
    let metadata = Metadata::builder().target(target).level(level).build();
    Record::builder().metadata(metadata).build()
}

fn args(values: &[&str]) -> IntoIter<String> {
    values
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<String>>()
        .into_iter()
}

#[test]
fn oauth_log_target_is_silenced() {
    let logger = logger_builder().build();
    assert!(!logger.matches(&record(OAUTH_LOG_TARGET, Level::Error)));
    assert!(!logger.matches(&record(OAUTH_LOG_TARGET, Level::Trace)));
}

#[test]
fn the_receiver_summary_is_the_one_info_line_that_shows() {
    let logger = logger_builder().build();
    assert!(logger.matches(&record(RECEIVE_LOG_TARGET, Level::Info)));
    assert!(logger.matches(&record(RECEIVE_LOG_TARGET, Level::Warn)));
    // Everything else keeps the Warn floor, so the summary cannot become a
    // door for librespot's own chatter.
    assert!(!logger.matches(&record("takofy_core::watch", Level::Info)));
    assert!(!logger.matches(&record("librespot_core", Level::Debug)));
}

#[test]
fn the_hand_over_path_shows_and_its_neighbours_do_not() {
    let logger = logger_builder().build();
    assert!(logger.matches(&record(LANDING_LOG_TARGET, Level::Info)));
    assert!(!logger.matches(&record("takofy::app", Level::Info)));
}

#[test]
fn the_activation_line_shows_and_its_neighbours_do_not() {
    let logger = logger_builder().build();
    assert!(logger.matches(&record(HOLD_LOG_TARGET, Level::Info)));
    assert!(!logger.matches(&record("takofy_core::publish", Level::Info)));
    assert!(!logger.matches(&record("librespot_playback", Level::Info)));
}

#[test]
fn the_order_refusal_line_shows_and_its_neighbours_do_not() {
    let logger = logger_builder().build();
    assert!(logger.matches(&record(REMOTE_LOG_TARGET, Level::Info)));
    assert!(!logger.matches(&record("takofy_ui::report", Level::Info)));
}

#[test]
fn the_cover_size_line_shows_and_its_neighbours_do_not() {
    let logger = logger_builder().build();
    assert!(logger.matches(&record(COVER_LOG_TARGET, Level::Info)));
    assert!(!logger.matches(&record("takofy_ui::view", Level::Info)));
}

#[test]
fn startup_target_logs_at_info() {
    let logger = logger_builder().build();
    assert!(logger.matches(&record(STARTUP_LOG_TARGET, Level::Info)));
    assert!(!logger.matches(&record("takofy::app", Level::Info)));
}

#[test]
fn unrelated_warn_target_is_kept() {
    let logger = logger_builder().build();
    assert!(logger.matches(&record("takofy", Level::Warn)));
}

/// The player's own lines carry a track's URI and title; ours says the same
/// without naming anything, and both the module and its submodules are off.
#[test]
fn librespot_players_own_lines_are_silenced_and_ours_shows() {
    let logger = logger_builder().build();
    for level in [Level::Error, Level::Warn, Level::Info] {
        assert!(
            !logger.matches(&record("librespot_playback::player", level)),
            "the player's own line at {level} must be silenced"
        );
        assert!(
            !logger.matches(&record("librespot_playback::player::sub", level)),
            "a player submodule's line at {level} must be silenced too"
        );
    }
    assert!(logger.matches(&record("librespot_playback::audio_backend", Level::Warn)));
    // The decoder keeps talking: its warnings are how a broken stream shows.
    assert!(logger.matches(&record(
        "librespot_playback::decoder::symphonia_decoder",
        Level::Warn
    )));
    assert!(logger.matches(&record(PLAYBACK_LOG_TARGET, Level::Warn)));
}

#[test]
fn no_arguments_means_loopback_and_the_rootlist() {
    assert_eq!(
        parse_flags(args(&[])).ok(),
        Some(Flags {
            redirect: RedirectMode::Loopback,
            context: None,
            headless: false,
        })
    );
}

#[test]
fn paste_selects_the_stdin_redirect() {
    let flags = parse_flags(args(&["--paste"])).expect("flags parse");
    assert_eq!(flags.redirect, RedirectMode::Paste);
}

#[test]
fn context_takes_the_next_argument() {
    let flags = parse_flags(args(&["--context", "spotify:artist:x"])).expect("flags parse");
    assert_eq!(flags.context.as_deref(), Some("spotify:artist:x"));
}

#[test]
fn a_missing_context_value_is_rejected() {
    assert!(parse_flags(args(&["--context"])).is_err());
}

#[test]
fn an_unknown_flag_is_rejected() {
    assert!(parse_flags(args(&["--nope"])).is_err());
}

#[test]
fn flags_combine() {
    let flags =
        parse_flags(args(&["--paste", "--context", "spotify:search:x"])).expect("flags parse");
    assert_eq!(flags.redirect, RedirectMode::Paste);
    assert_eq!(flags.context.as_deref(), Some("spotify:search:x"));
}

#[test]
fn usage_mentions_every_flag() {
    for flag in ["--headless", "--paste", "--context"] {
        assert!(
            USAGE.contains(flag),
            "{flag} is missing from the usage line"
        );
    }
}

#[test]
fn paste_without_headless_is_rejected() {
    let flags = parse_flags(args(&["--paste"])).expect("flags parse");
    assert!(mode_for(&flags).is_err());
}

#[test]
fn context_without_headless_is_rejected() {
    let flags = parse_flags(args(&["--context", "spotify:artist:x"])).expect("flags parse");
    assert!(mode_for(&flags).is_err());
}

#[test]
fn headless_alone_selects_headless() {
    let flags = parse_flags(args(&["--headless"])).expect("flags parse");
    assert_eq!(mode_for(&flags), Ok(Mode::Headless));
}

#[test]
fn headless_accepts_paste_and_context() {
    let flags = parse_flags(args(&[
        "--headless",
        "--paste",
        "--context",
        "spotify:artist:x",
    ]))
    .expect("flags parse");
    assert_eq!(flags.redirect, RedirectMode::Paste);
    assert_eq!(flags.context.as_deref(), Some("spotify:artist:x"));
    assert_eq!(mode_for(&flags), Ok(Mode::Headless));
}

#[test]
fn headless_is_selected_regardless_of_flag_order() {
    let paste_first = parse_flags(args(&["--paste", "--headless"])).expect("flags parse");
    assert_eq!(paste_first.redirect, RedirectMode::Paste);
    assert_eq!(mode_for(&paste_first), Ok(Mode::Headless));

    let context_first =
        parse_flags(args(&["--context", "spotify:artist:x", "--headless"])).expect("flags parse");
    assert_eq!(context_first.context.as_deref(), Some("spotify:artist:x"));
    assert_eq!(mode_for(&context_first), Ok(Mode::Headless));

    let context_last =
        parse_flags(args(&["--headless", "--context", "spotify:artist:x"])).expect("flags parse");
    assert_eq!(context_last.context.as_deref(), Some("spotify:artist:x"));
    assert_eq!(mode_for(&context_last), Ok(Mode::Headless));
}

#[test]
fn repeated_flags_are_accepted_and_the_last_context_wins() {
    let flags = parse_flags(args(&[
        "--headless",
        "--headless",
        "--context",
        "spotify:artist:first",
        "--context",
        "spotify:artist:second",
    ]))
    .expect("flags parse");
    assert!(flags.headless);
    assert_eq!(flags.context.as_deref(), Some("spotify:artist:second"));
    assert_eq!(mode_for(&flags), Ok(Mode::Headless));
}

#[test]
fn no_arguments_select_the_window() {
    let flags = parse_flags(args(&[])).expect("flags parse");
    assert_eq!(mode_for(&flags), Ok(Mode::Window));
}
