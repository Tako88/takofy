use std::path::Path;

use takofy_core::credentials::SoloistKey;

use super::{Launch, SpawnError, VersionError, WS_ADDR};

/// The value the tests watch for, so a leak is a failure and not a judgement.
const SENTINEL: &str = "sentinel-key-3f9a";

fn key() -> SoloistKey {
    SoloistKey::new(SENTINEL.to_owned()).expect("a key")
}

fn launch() -> Launch {
    Launch::new(
        Path::new("/opt/soloist"),
        "Takofy Soloist",
        &key(),
        Path::new("/home/x/.config/takofy/soloist"),
    )
}

#[test]
fn the_command_line_names_the_device_the_key_and_the_socket() {
    assert_eq!(
        launch().argv(),
        [
            "-n",
            "Takofy Soloist",
            "-k",
            SENTINEL,
            "-D",
            "/home/x/.config/takofy/soloist",
            "--ws",
            WS_ADDR,
        ]
    );
}

#[test]
fn the_pairing_run_asks_for_no_socket() {
    let argv = Launch::pair(
        Path::new("/opt/soloist"),
        "Takofy Soloist",
        &key(),
        Path::new("/data"),
    )
    .argv();

    assert_eq!(argv.last().map(String::as_str), Some("--pair"));
    assert!(
        !argv.iter().any(|arg| arg == "--ws"),
        "pairing exits before it serves anything"
    );
}

#[test]
fn the_debug_output_names_nothing_but_the_type() {
    let printed = format!("{:?}", launch());

    assert_eq!(printed, "Launch(..)");
    assert!(!printed.contains(SENTINEL), "the key must not be printed");
}

#[test]
fn neither_error_kind_carries_the_key() {
    for error in [SpawnError::NotFound, SpawnError::Denied, SpawnError::Other] {
        let printed = format!("{error} {error:?}");
        assert!(!printed.contains(SENTINEL), "{printed}");
        assert!(!printed.contains("sentinel"), "{printed}");
    }
    for error in [
        VersionError::NotFound,
        VersionError::Denied,
        VersionError::Other,
    ] {
        let printed = format!("{error} {error:?}");
        assert!(!printed.contains(SENTINEL), "{printed}");
    }
}

#[test]
fn a_missing_program_reads_as_a_kind_not_a_path() {
    let error = Launch::version(Path::new("/nonexistent/soloist")).expect_err("no such program");
    assert_eq!(error, VersionError::NotFound);
    assert!(!format!("{error}").contains("nonexistent"));
}
