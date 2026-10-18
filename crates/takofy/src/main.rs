//! The Takofy entry point: logging, flags and the two entry points.
//!
//! The window is the default. The terminal driver stays behind `--headless`
//! because `--paste` prompts on the terminal, which a window cannot do.

mod app;
mod bridge;
mod headless;
mod keychain;
mod prompt;
mod session;

use std::process::ExitCode;
use std::sync::Arc;

use librespot::core::SessionConfig;
use log::LevelFilter;
use takofy_core::credentials::StoreError;
use takofy_core::login::{AuthError, LoginMethod, SourceErrorKind, authenticate};
use takofy_core::oauth::{OAUTH_LOG_TARGET, RedirectMode, SpotifyOAuth};
use takofy_ui::message::Failure;
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

    let runtime = match build_runtime() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: could not start the runtime: {error}");
            return ExitCode::FAILURE;
        }
    };

    let result = bridge::run_window(runtime, host, store, source, |ui_end, slot| {
        eframe::run_native(
            "Takofy",
            app::native_options(),
            Box::new(move |cc| {
                slot.install(Arc::new(cc.egui_ctx.clone()));
                cc.egui_ctx.request_repaint();
                let app: Box<dyn eframe::App> = Box::new(app::TakofyApp::new(cc, ui_end));
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
mod tests {
    use std::vec::IntoIter;

    use log::{Level, Metadata, Record};
    use takofy_core::oauth::{OAUTH_LOG_TARGET, RedirectMode};

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
    fn unrelated_warn_target_is_kept() {
        let logger = logger_builder().build();
        assert!(logger.matches(&record("takofy", Level::Warn)));
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

        let context_first = parse_flags(args(&["--context", "spotify:artist:x", "--headless"]))
            .expect("flags parse");
        assert_eq!(context_first.context.as_deref(), Some("spotify:artist:x"));
        assert_eq!(mode_for(&context_first), Ok(Mode::Headless));

        let context_last = parse_flags(args(&["--headless", "--context", "spotify:artist:x"]))
            .expect("flags parse");
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
}
