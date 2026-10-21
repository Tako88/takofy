//! The command-line driver: pick a playlist, pick a track, play it.
//!
//! The only live run of the playback, rootlist and context paths, and the only
//! way to reach the `--paste` sign-in fallback — which cannot work from a
//! window, because with no port in the redirect librespot prompts on the
//! terminal instead.

use std::process::ExitCode;

use librespot::core::SessionConfig;
use takofy_core::credentials::AccessToken;
use takofy_core::engine::{Engine, EngineError};
use takofy_core::{Outcome, PlaybackError, PlaybackEvent, TrackPages, drive};
use takofy_model::TrackEntry;

use crate::prompt::{Choice, format_playlist_line, format_track_line, parse_choice, read_line};

/// Drives the terminal, against `context` when one was given and against the
/// rootlist otherwise.
pub(crate) async fn run(
    cfg: SessionConfig,
    token: &AccessToken,
    context: Option<String>,
) -> Result<ExitCode, EngineError> {
    let engine = Engine::connect(cfg, token).await?;

    let uri = match context {
        Some(uri) => uri,
        None => match pick_playlist(&engine).await? {
            Some(uri) => uri,
            None => return Ok(ExitCode::SUCCESS),
        },
    };

    let Some(track) = choose_track(engine.tracks(&uri)).await? else {
        return Ok(ExitCode::SUCCESS);
    };
    play(&engine, &track).await
}

/// Lists the rootlist and returns the URI of the chosen playlist.
async fn pick_playlist(engine: &Engine) -> Result<Option<String>, EngineError> {
    let playlists = engine.rootlist().await?.entries;
    if playlists.is_empty() {
        println!("no playlists found");
        return Ok(None);
    }
    for (index, entry) in playlists.iter().enumerate() {
        println!("{}", format_playlist_line(index + 1, entry));
    }
    let Some(index) = prompt_playlist(playlists.len()).await else {
        return Ok(None);
    };
    Ok(Some(playlists[index].uri.clone()))
}

async fn play(engine: &Engine, track: &TrackEntry) -> Result<ExitCode, EngineError> {
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
        PlaybackEvent::Duration { duration_ms } => println!("track length {duration_ms} ms"),
        PlaybackEvent::Seeked { position_ms } => println!("seeked to {position_ms} ms"),
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
