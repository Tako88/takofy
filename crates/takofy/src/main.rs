//! The Takofy entry point: logging, flags and the two entry points.
//!
//! The window is the default. The terminal driver stays behind `--headless`
//! because `--paste` prompts on the terminal, which a window cannot do.

mod app;
mod bridge;
mod cover;
mod decode;
mod devices;
mod discovery;
mod headless;
mod hidden_file;
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
mod soloist;

use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use librespot::core::SessionConfig;
use log::LevelFilter;
use takofy_core::credentials::{SoloistKey, SoloistKeyStore, StoreError};
use takofy_core::hold::HOLD_LOG_TARGET;
use takofy_core::login::{AuthError, LoginMethod, SourceErrorKind, authenticate};
use takofy_core::oauth::{OAUTH_LOG_TARGET, RedirectMode, SpotifyOAuth};
use takofy_core::receive::RECEIVE_LOG_TARGET;

use crate::bridge::{LANDING_LOG_TARGET, STARTUP_LOG_TARGET};
use crate::preferences::Loaded;
use crate::soloist::{SoloistHost, SoloistPrefs};
use takofy_ui::cover::COVER_LOG_TARGET;
use takofy_ui::message::Failure;
use takofy_ui::prefs::{Prefs, SoundChoice};
use takofy_ui::remote::REMOTE_LOG_TARGET;
use takofy_ui::signin::failure_message;

use crate::keychain::{KeychainStore, SoloistKeychain};

const USAGE: &str = "usage: takofy [--headless [--paste] [--context <uri>]] | takofy --probe-audio | takofy --soloist-key | takofy --soloist-pair";

/// Which entry point the flags select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Window,
    Headless,
    /// Opens the audio device once and says what happened. No login, no window:
    /// the first question a silent install asks is whether this machine can
    /// produce sound at all.
    ProbeAudio,
    /// Reads a Soloist API key from a line of stdin into the keychain, so the
    /// secret never lands in a shell history or an argv of ours.
    SoloistKey,
    /// Runs the daemon once with `--pair`, which signs it in to the account.
    SoloistPair,
}

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Flags {
    redirect: RedirectMode,
    /// Open this context instead of listing the rootlist.
    context: Option<String>,
    /// Use the terminal driver instead of the window.
    headless: bool,
    /// Report the audio device rather than opening a window.
    probe_audio: bool,
    /// Read a Soloist API key from stdin and store it.
    soloist_key: bool,
    /// Pair the daemon with the account, once.
    soloist_pair: bool,
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
        Ok(Mode::ProbeAudio) => run_probe_audio(stored_prefs().engine()),
        Ok(Mode::SoloistKey) => run_soloist_key(),
        Ok(Mode::SoloistPair) => run_soloist_pair(),
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
    if flags.soloist_key {
        // It reads one line and touches nothing else, so any other flag is a typo.
        let alone = !flags.headless
            && !flags.probe_audio
            && !flags.soloist_pair
            && flags.redirect == RedirectMode::Loopback
            && flags.context.is_none();
        return if alone { Ok(Mode::SoloistKey) } else { Err(()) };
    }
    if flags.soloist_pair {
        // It runs the daemon once and exits, so it takes nothing else either.
        let alone = !flags.headless
            && !flags.probe_audio
            && !flags.soloist_key
            && flags.redirect == RedirectMode::Loopback
            && flags.context.is_none();
        return if alone {
            Ok(Mode::SoloistPair)
        } else {
            Err(())
        };
    }
    if flags.probe_audio {
        // It reads nothing else, so any other flag beside it is a typo.
        return if flags.headless || flags.context.is_some() {
            Err(())
        } else {
            Ok(Mode::ProbeAudio)
        };
    }
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
    // Read once, before the window exists: the first frame draws what the file
    // held, and the writer outlives the window only long enough to flush it.
    let (loaded, prefs) = preferences::open();
    // The setting is shared rather than copied: the window stores it every frame
    // and the lookup reads it before any request.
    let gate = Arc::new(AtomicBool::new(app::loaded_discovery(&loaded)));
    // A client that cannot be built leaves the rows unreachable rather than the
    // window unopened: the host then has no outside service to look up with.
    let outside = discovery::http::Http::new()
        .map(|http| Arc::new(discovery::Outside::new(http, Arc::clone(&gate))));
    let runtime = match build_runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: could not start the runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    let engine = engine_of(&loaded);
    // The daemon is started only by the engine that uses it: a window on the
    // built-in player never runs a second sound path.
    let soloist = match engine.engine() {
        SoundChoice::Soloist => soloist_data_dir().map(|dir| {
            Arc::new(SoloistHost::start(
                runtime.handle().clone(),
                SoloistPrefs {
                    path: engine.soloist_path().map(std::path::PathBuf::from),
                    name: engine.soloist_name().to_owned(),
                },
                Arc::new(SoloistKeychain),
                dir,
            ))
        }),
        SoundChoice::BuiltIn => None,
    };
    let host = match &outside {
        Some(outside) => session::LiveSession::new(cfg).with_outside(Arc::clone(outside)),
        None => session::LiveSession::new(cfg),
    }
    .with_sound(engine.engine(), soloist);
    let (hidden, hidden_sync) = hidden_file::open();
    let (cached, snapshots) = snapshot::open();

    let result = bridge::run_window(runtime, host, store, source, move |ui_end, slot| {
        eframe::run_native(
            "Takofy",
            app::native_options(),
            Box::new(move |cc| {
                slot.install(Arc::new(cc.egui_ctx.clone()));
                cc.egui_ctx.request_repaint();
                let app: Box<dyn eframe::App> = Box::new(app::TakofyApp::new(
                    cc,
                    ui_end,
                    loaded,
                    prefs,
                    cached,
                    snapshots,
                    gate,
                    hidden,
                    hidden_sync,
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

/// Answers for the engine the preference chose, in one line, without a session.
///
/// The exit code is the answer: 0 for a device that opened, 1 for one that did
/// not. A machine where this succeeds and Takofy is still silent has told us the
/// fault is above the device.
fn run_probe_audio(engine: SoundChoice) -> ExitCode {
    match engine {
        SoundChoice::BuiltIn => run_probe_device(),
        SoundChoice::Soloist => run_probe_soloist(),
    }
}

/// Opens the output device and says what happened.
///
/// The built-in player is the only engine that opens one, so this is the whole
/// answer for it.
fn run_probe_device() -> ExitCode {
    let started = std::time::Instant::now();
    match takofy_core::playback::probe_device() {
        Ok(()) => {
            println!(
                "audio device: opened in {} ms",
                started.elapsed().as_millis()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!("audio device: {}", error);
            ExitCode::FAILURE
        }
    }
}

/// Reports the daemon's program and build, and never opens an audio device.
///
/// Soloist owns the output, so a device this process opened would prove
/// nothing about whether sound can come out.
fn run_probe_soloist() -> ExitCode {
    let prefs = stored_prefs();
    let found = soloist::locate::find(
        prefs.soloist_path().map(std::path::Path::new),
        std::env::var_os("PATH").as_deref(),
        soloist::locate::exists,
    );
    let Some(exe) = found.path() else {
        println!("soloist: not found");
        return ExitCode::FAILURE;
    };
    let build = soloist::launch::Launch::version(exe)
        .map(|output| takofy_core::soloist::expiry::classify(&output, soloist::now_unix()))
        .unwrap_or(takofy_core::soloist::expiry::Build::Unknown);
    let words = match build {
        takofy_core::soloist::expiry::Build::Fresh => "fresh".to_owned(),
        takofy_core::soloist::expiry::Build::Expiring { days_left } => {
            format!("expiring in {days_left} days")
        }
        takofy_core::soloist::expiry::Build::Expired => "expired".to_owned(),
        takofy_core::soloist::expiry::Build::Unknown => "unknown".to_owned(),
    };
    println!("soloist: found at {}, build {words}", exe.display());
    if build.playable() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
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
        probe_audio: false,
        soloist_key: false,
        soloist_pair: false,
    };
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--headless" => flags.headless = true,
            "--paste" => flags.redirect = RedirectMode::Paste,
            "--context" => flags.context = Some(args.next().ok_or(())?),
            "--probe-audio" => flags.probe_audio = true,
            "--soloist-key" => flags.soloist_key = true,
            "--soloist-pair" => flags.soloist_pair = true,
            _ => return Err(()),
        }
    }
    Ok(flags)
}

/// Reads one line of stdin and stores it as the Soloist API key.
///
/// The value is never echoed, and an error names only the store's kind, so a
/// terminal that scrolls back cannot show the secret.
fn run_soloist_key() -> ExitCode {
    let mut line = String::new();
    if let Err(error) = std::io::stdin().read_line(&mut line) {
        eprintln!("could not read the key: {error}");
        return ExitCode::FAILURE;
    }
    let Some(key) = SoloistKey::new(line.trim().to_owned()) else {
        eprintln!("the key is empty");
        return ExitCode::FAILURE;
    };
    match SoloistKeychain.save(&key) {
        Ok(()) => {
            println!("saved");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("could not save the key: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The engine and its own values, as the file holds them.
fn engine_of(loaded: &Loaded) -> Prefs {
    match loaded {
        Loaded::Found(prefs) => prefs.clone(),
        Loaded::Absent => Prefs::default(),
        Loaded::Refused(_) | Loaded::Unreadable => Prefs::default(),
    }
}

/// Reads the preferences without starting the writer a window would need.
fn stored_prefs() -> Prefs {
    let Some(path) = preferences::config_file(cfg!(windows), |key| std::env::var_os(key)) else {
        return Prefs::default();
    };
    engine_of(&preferences::load(&path))
}

/// The daemon's own directory, beside the preferences file, so its port file
/// is where this window looks for it.
fn soloist_data_dir() -> Option<std::path::PathBuf> {
    Some(preferences::config_dir(cfg!(windows), |key| std::env::var_os(key))?.join("soloist"))
}

/// Runs the daemon once so it can sign in to the account, and says only how
/// that ended.
///
/// Soloist has no sign-in of its own: without this a fresh install stays
/// signed out for ever, and only the exit code is printed so nothing of the
/// run leaks into a terminal.
fn run_soloist_pair() -> ExitCode {
    let prefs = stored_prefs();
    let Some(data_dir) = soloist_data_dir() else {
        eprintln!("no configuration directory");
        return ExitCode::FAILURE;
    };
    let key = match SoloistKeychain.load() {
        Ok(Some(key)) => key,
        Ok(None) => {
            eprintln!("no key stored: run --soloist-key first");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("could not read the key: {error}");
            return ExitCode::FAILURE;
        }
    };
    let found = soloist::locate::find(
        prefs.soloist_path().map(std::path::Path::new),
        std::env::var_os("PATH").as_deref(),
        soloist::locate::exists,
    );
    let Some(exe) = found.path() else {
        eprintln!("soloist: not found");
        return ExitCode::FAILURE;
    };
    if std::fs::create_dir_all(&data_dir).is_err() {
        eprintln!("the soloist directory could not be made");
        return ExitCode::FAILURE;
    }
    let launch = soloist::launch::Launch::pair(exe, prefs.soloist_name(), &key, &data_dir);
    let mut child = match launch.spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    match child.wait() {
        Ok(status) if status.success() => {
            println!("paired");
            ExitCode::SUCCESS
        }
        Ok(status) => {
            println!("{}", status.code().unwrap_or(-1));
            ExitCode::FAILURE
        }
        Err(_) => {
            eprintln!("the daemon could not be waited for");
            ExitCode::FAILURE
        }
    }
}

fn logger_builder() -> env_logger::Builder {
    let mut builder = env_logger::Builder::new();
    builder.filter_level(LevelFilter::Warn);
    // Read after the default, so `RUST_LOG` raises what this build prints, and
    // before the filters below, so the two modules that can log a credential or
    // a track keep their silence whatever the environment says.
    builder.parse_default_env();
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
    // What the daemon is doing, in kinds: never a URI, a frame, a key or an
    // argv.
    builder.filter_module("takofy::soloist", LevelFilter::Info);
    builder.filter_module("takofy_core::soloist", LevelFilter::Info);
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
