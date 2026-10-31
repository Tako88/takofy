//! The Takofy entry point: logging, flags and the two entry points.
//!
//! The window is the default. The terminal driver stays behind `--headless`
//! because `--paste` prompts on the terminal, which a window cannot do.

mod app;
mod bridge;
mod cover;
mod decode;
mod devices;
mod headless;
mod keychain;
mod latest_writer;
mod playhead;
mod preferences;
mod prompt;
mod recovery;
mod session;
mod small_file;
mod snapshot;
mod snapshot_format;

use std::process::ExitCode;
use std::sync::Arc;

use librespot::core::SessionConfig;
use log::LevelFilter;
use takofy_core::credentials::StoreError;
use takofy_core::hold::HOLD_LOG_TARGET;
use takofy_core::login::{AuthError, LoginMethod, SourceErrorKind, authenticate};
use takofy_core::oauth::{OAUTH_LOG_TARGET, RedirectMode, SpotifyOAuth};
use takofy_core::receive::RECEIVE_LOG_TARGET;

use crate::bridge::{LANDING_LOG_TARGET, STARTUP_LOG_TARGET};
use takofy_ui::cover::COVER_LOG_TARGET;
use takofy_ui::message::Failure;
use takofy_ui::remote::REMOTE_LOG_TARGET;
use takofy_ui::signin::failure_message;

use crate::keychain::KeychainStore;

const USAGE: &str = "usage: takofy [--headless [--paste] [--context <uri>]]";

/// Which entry point the flags select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Window,
    Headless,
}

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Flags {
    redirect: RedirectMode,
    /// Open this context instead of listing the rootlist.
    context: Option<String>,
    /// Use the terminal driver instead of the window.
    headless: bool,
}

fn main() -> ExitCode {
    logger_builder().init();

    let flags = match parse_flags(std::env::args().skip(1)) {
        Ok(flags) => flags,
        Err(()) => return usage_error(),
    };

    match mode_for(&flags) {
        Ok(Mode::Window) => run_window(&flags),
        Ok(Mode::Headless) => run_headless(flags),
        Err(()) => usage_error(),
    }
}

fn usage_error() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

/// The window has no terminal to prompt on, so the paste fallback and a chosen
/// context only make sense headless.
fn mode_for(flags: &Flags) -> Result<Mode, ()> {
    if !flags.headless && (flags.redirect == RedirectMode::Paste || flags.context.is_some()) {
        return Err(());
    }
    Ok(if flags.headless {
        Mode::Headless
    } else {
        Mode::Window
    })
}

fn run_headless(flags: Flags) -> ExitCode {
    let cfg = SessionConfig::default();
    let store = KeychainStore;

    let (access, method) =
        match authenticate(&store, &SpotifyOAuth::new(&cfg.client_id, flags.redirect)) {
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

    let runtime = match build_runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: could not start the runtime: {error}");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(headless::run(cfg, &access, flags.context)) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Opens the window and drives sign-in through the worker. Failure to open a
/// window is reported as a plain line: the underlying error may embed a
/// response body and is not printed.
fn run_window(flags: &Flags) -> ExitCode {
    debug_assert!(flags.context.is_none());
    let cfg = SessionConfig::default();
    let store = Arc::new(KeychainStore);
    // A window has no terminal, so its authorize path is loopback by
    // construction; a Paste source would block on a prompt nobody can answer.
    let source = Arc::new(SpotifyOAuth::new(&cfg.client_id, RedirectMode::Loopback));
    let host = session::LiveSession::new(cfg);
    // Read once, before the window exists: the first frame draws what the file
    // held, and the writer outlives the window only long enough to flush it.
    let (loaded, prefs) = preferences::open();
    let (cached, snapshots) = snapshot::open();

    let runtime = match build_runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: could not start the runtime: {error}");
            return ExitCode::FAILURE;
        }
    };

    let result = bridge::run_window(runtime, host, store, source, move |ui_end, slot| {
        eframe::run_native(
            "Takofy",
            app::native_options(),
            Box::new(move |cc| {
                slot.install(Arc::new(cc.egui_ctx.clone()));
                cc.egui_ctx.request_repaint();
                let app: Box<dyn eframe::App> = Box::new(app::TakofyApp::new(
                    cc, ui_end, loaded, prefs, cached, snapshots,
                ));
                Ok(app)
            }),
        )
    });

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("could not open a window; try --headless");
            ExitCode::FAILURE
        }
    }
}

fn build_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
}

fn parse_flags(args: impl Iterator<Item = String>) -> Result<Flags, ()> {
    let mut flags = Flags {
        redirect: RedirectMode::Loopback,
        context: None,
        headless: false,
    };
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--headless" => flags.headless = true,
            "--paste" => flags.redirect = RedirectMode::Paste,
            "--context" => flags.context = Some(args.next().ok_or(())?),
            _ => return Err(()),
        }
    }
    Ok(flags)
}

fn logger_builder() -> env_logger::Builder {
    let mut builder = env_logger::Builder::new();
    builder.filter_level(LevelFilter::Warn);
    // librespot-oauth logs tokens and auth codes at trace level.
    builder.filter_module(OAUTH_LOG_TARGET, LevelFilter::Off);
    // The player logs a track's URI and title when it skips one. The whole
    // module is off, not a message match, so a reword cannot start leaking it.
    builder.filter_module("librespot_playback::player", LevelFilter::Off);
    // The one line that says a device asked this window to play something. It is
    // counts and kinds, never a URI, and it is the only evidence that a request
    // arrived at all.
    builder.filter_module(RECEIVE_LOG_TARGET, LevelFilter::Info);
    // The one line that says what the account answered to a hold: which baseline
    // the loss rule took, and whether the reply named this device. Both words,
    // never a URI.
    builder.filter_module(HOLD_LOG_TARGET, LevelFilter::Info);
    // The hand-over's decisions: which arm refused and whether the put was even
    // reached. Without it a failed hand-over leaves no trace but a retry.
    builder.filter_module(LANDING_LOG_TARGET, LevelFilter::Info);
    // Which queue order the window refused and why. A phone's window that is
    // refused leaves the queue disagreeing with the phone, and the kind is the
    // only thing that says whose rows were wrong.
    builder.filter_module(REMOTE_LOG_TARGET, LevelFilter::Info);
    // Which size each uploaded cover texture was. Dimensions only: a cover URL
    // names a file and stays out of the log.
    builder.filter_module(COVER_LOG_TARGET, LevelFilter::Info);
    // Per-stage startup durations and counts, never a name or URI.
    builder.filter_module(STARTUP_LOG_TARGET, LevelFilter::Info);
    builder
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
    if matches!(error, AuthError::Store(StoreError::Unavailable)) {
        eprintln!("error: {}", failure_message(Failure::Keychain));
        return;
    }
    eprintln!("error: {error}");
}

#[cfg(test)]
mod tests;
