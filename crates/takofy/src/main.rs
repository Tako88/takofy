//! The headless Takofy driver: logging, flags and login.
//!
//! Keychain and OAuth run before any Tokio runtime, so blocking HTTP and
//! keychain calls never sit inside async code.

mod keychain;
mod prompt;

use std::process::ExitCode;

use librespot::core::SessionConfig;
use log::LevelFilter;
use takofy_core::credentials::AccessToken;
use takofy_core::engine::{Engine, EngineError};
use takofy_core::login::{AuthError, LoginMethod, SourceErrorKind, authenticate};
use takofy_core::oauth::{OAUTH_LOG_TARGET, RedirectMode, SpotifyOAuth};
use takofy_core::{Outcome, PlaybackError, PlaybackEvent, TrackEntry, TrackPages, drive};

use crate::keychain::KeychainStore;
use crate::prompt::{Choice, format_playlist_line, format_track_line, parse_choice, read_line};

const USAGE: &str = "usage: takofy [--paste]";

fn main() -> ExitCode {
    logger_builder().init();

    let mode = match redirect_mode(std::env::args().skip(1)) {
        Ok(mode) => mode,
        Err(()) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

    let cfg = SessionConfig::default();
    let store = KeychainStore;

    let (access, method) = match authenticate(&store, &SpotifyOAuth::new(&cfg.client_id, mode)) {
        Ok(result) => result,
        Err(error) => {
            print_error(&error);
            return ExitCode::FAILURE;
        }
    };
    match method {
        LoginMethod::Resumed => println!("Logged in (resumed)"),
        LoginMethod::Interactive => println!("Logged in (browser)"),
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: could not start the runtime: {error}");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(run(cfg, &access)) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cfg: SessionConfig, token: &AccessToken) -> Result<ExitCode, EngineError> {
    let engine = Engine::connect(cfg, token).await?;
    let playlists = engine.rootlist().await?;
    if playlists.is_empty() {
        println!("no playlists found");
        return Ok(ExitCode::SUCCESS);
    }
    for (index, entry) in playlists.iter().enumerate() {
        println!("{}", format_playlist_line(index + 1, entry));
    }
    let Some(index) = prompt_playlist(playlists.len()).await else {
        return Ok(ExitCode::SUCCESS);
    };
    let Some(track) = choose_track(engine.tracks(&playlists[index].uri)).await? else {
        return Ok(ExitCode::SUCCESS);
    };

    let (playback, events) = match engine.playback() {
        Ok(pair) => pair,
        Err(PlaybackError::NoAudioDevice) => {
            eprintln!("error: no audio output device available");
            return Ok(ExitCode::from(3));
        }
        Err(error) => {
            eprintln!("error: {error}");
            return Ok(ExitCode::FAILURE);
        }
    };

    if let Err(error) = playback.play(&track.uri) {
        eprintln!("error: {error}");
        if let Err(error) = playback.shutdown().await {
            eprintln!("error: {error}");
        }
        return Ok(ExitCode::FAILURE);
    }

    let loop_result =
        tokio::task::spawn_blocking(move || drive(events, print_playback_event)).await;
    let exit = match loop_result {
        Ok(Ok(Outcome::Ended | Outcome::Stopped)) => ExitCode::SUCCESS,
        Ok(Ok(Outcome::Unavailable)) => {
            println!("track unavailable");
            ExitCode::FAILURE
        }
        Ok(Err(error)) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("error: player event loop failed: {error}");
            ExitCode::FAILURE
        }
    };
    if let Err(error) = playback.shutdown().await {
        eprintln!("error: {error}");
        return Ok(ExitCode::FAILURE);
    }
    Ok(exit)
}

async fn choose_track(mut pages: TrackPages<'_>) -> Result<Option<TrackEntry>, EngineError> {
    let mut shown: Vec<TrackEntry> = Vec::new();
    match pages.next().await? {
        Some(tracks) => push_page(&mut shown, &tracks),
        None => {
            println!("no tracks");
            return Ok(None);
        }
    }
    if shown.is_empty() {
        println!("no tracks to pick");
        return Ok(None);
    }
    loop {
        println!(
            "pick a track (1-{}), n for next page, q to quit",
            shown.len()
        );
        let Some(input) = read_line().await else {
            return Ok(None);
        };
        match parse_choice(&input, shown.len()) {
            Choice::Pick(number) => return Ok(Some(shown[number - 1].clone())),
            Choice::Quit => return Ok(None),
            Choice::Next => match pages.next().await? {
                Some(tracks) => push_page(&mut shown, &tracks),
                None => println!("end of tracks"),
            },
            Choice::Invalid => {
                println!("enter a track number, n for next page, or q to quit");
            }
        }
    }
}

fn push_page(shown: &mut Vec<TrackEntry>, tracks: &[TrackEntry]) {
    if tracks.is_empty() {
        println!("(empty page)");
    }
    for track in tracks {
        shown.push(track.clone());
        println!("{}", format_track_line(shown.len(), track));
    }
}

fn print_playback_event(event: &PlaybackEvent) {
    match event {
        PlaybackEvent::Loading => println!("loading"),
        PlaybackEvent::Playing { position_ms } => println!("playing at {position_ms} ms"),
        PlaybackEvent::Paused { position_ms } => println!("paused at {position_ms} ms"),
        PlaybackEvent::Ended => println!("ended"),
        PlaybackEvent::Unavailable => println!("unavailable"),
        PlaybackEvent::Stopped => println!("stopped"),
    }
}

async fn prompt_playlist(count: usize) -> Option<usize> {
    loop {
        println!("pick a playlist (1-{count}, q to quit)");
        let input = read_line().await?;
        match parse_choice(&input, count) {
            Choice::Pick(number) => return Some(number - 1),
            Choice::Quit => return None,
            Choice::Next | Choice::Invalid => {}
        }
    }
}

fn logger_builder() -> env_logger::Builder {
    let mut builder = env_logger::Builder::new();
    builder.filter_level(LevelFilter::Warn);
    // librespot-oauth logs tokens and auth codes at trace level.
    builder.filter_module(OAUTH_LOG_TARGET, LevelFilter::Off);
    builder
}

fn redirect_mode(args: impl Iterator<Item = String>) -> Result<RedirectMode, ()> {
    let mut mode = RedirectMode::Loopback;
    for arg in args {
        match arg.as_str() {
            "--paste" => mode = RedirectMode::Paste,
            _ => return Err(()),
        }
    }
    Ok(mode)
}

fn print_error(error: &AuthError) {
    if let AuthError::Source(source) = error
        && source.kind == SourceErrorKind::PortBusy
    {
        eprintln!("error: port 8898 is already in use");
        eprintln!(
            "free the port, or re-run with --paste (which needs http://127.0.0.1/login registered)"
        );
        return;
    }
    eprintln!("error: {error}");
}

#[cfg(test)]
mod tests {
    use log::{Level, Metadata, Record};

    use super::logger_builder;
    use takofy_core::oauth::OAUTH_LOG_TARGET;

    fn record<'a>(target: &'a str, level: Level) -> Record<'a> {
        let metadata = Metadata::builder().target(target).level(level).build();
        Record::builder().metadata(metadata).build()
    }

    #[test]
    fn oauth_log_target_is_silenced() {
        let logger = logger_builder().build();
        assert!(!logger.matches(&record(OAUTH_LOG_TARGET, Level::Error)));
        assert!(!logger.matches(&record(OAUTH_LOG_TARGET, Level::Trace)));
    }

    #[test]
    fn unrelated_warn_target_is_kept() {
        let logger = logger_builder().build();
        assert!(logger.matches(&record("takofy", Level::Warn)));
    }
}
