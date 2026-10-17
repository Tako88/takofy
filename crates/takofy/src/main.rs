//! The headless Takofy driver: logging, flags and login.
//!
//! Keychain and OAuth run before any Tokio runtime, so blocking HTTP and
//! keychain calls never sit inside async code.

mod keychain;

use std::process::ExitCode;

use librespot::core::SessionConfig;
use log::LevelFilter;
use takofy_core::login::{AuthError, LoginMethod, SourceErrorKind, authenticate};
use takofy_core::oauth::{OAUTH_LOG_TARGET, RedirectMode, SpotifyOAuth};

use crate::keychain::KeychainStore;

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

    match authenticate(&store, &SpotifyOAuth::new(&cfg.client_id, mode)) {
        Ok((_access, method)) => {
            match method {
                LoginMethod::Resumed => println!("Logged in (resumed)"),
                LoginMethod::Interactive => println!("Logged in (browser)"),
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            print_error(&error);
            ExitCode::FAILURE
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
