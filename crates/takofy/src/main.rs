//! The Takofy entry point: logging, flags, login and the runtime.
//!
//! Keychain and OAuth run before any Tokio runtime, so blocking HTTP and
//! keychain calls never sit inside async code.

mod headless;
mod keychain;
mod prompt;

use std::process::ExitCode;

use librespot::core::SessionConfig;
use log::LevelFilter;
use takofy_core::login::{AuthError, LoginMethod, SourceErrorKind, authenticate};
use takofy_core::oauth::{OAUTH_LOG_TARGET, RedirectMode, SpotifyOAuth};

use crate::keychain::KeychainStore;

const USAGE: &str = "usage: takofy [--paste] [--context <uri>]";

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Flags {
    redirect: RedirectMode,
    /// Open this context instead of listing the rootlist.
    context: Option<String>,
}

fn main() -> ExitCode {
    logger_builder().init();

    let flags = match parse_flags(std::env::args().skip(1)) {
        Ok(flags) => flags,
        Err(()) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };

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

    match runtime.block_on(headless::run(cfg, &access, flags.context)) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_flags(args: impl Iterator<Item = String>) -> Result<Flags, ()> {
    let mut flags = Flags {
        redirect: RedirectMode::Loopback,
        context: None,
    };
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
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
    eprintln!("error: {error}");
}

#[cfg(test)]
mod tests {
    use std::vec::IntoIter;

    use log::{Level, Metadata, Record};
    use takofy_core::oauth::{OAUTH_LOG_TARGET, RedirectMode};

    use super::{Flags, USAGE, logger_builder, parse_flags};

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
    fn context_without_a_value_is_rejected() {
        assert!(parse_flags(args(&["--context"])).is_err());
    }

    #[test]
    fn unknown_arguments_are_rejected() {
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
        for flag in ["--paste", "--context"] {
            assert!(
                USAGE.contains(flag),
                "{flag} is missing from the usage line"
            );
        }
    }
}
