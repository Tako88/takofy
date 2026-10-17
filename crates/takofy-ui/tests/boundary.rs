//! The window crate must not reach the network, the credential store or the
//! Spotify session.
//!
//! A view is handed its data. This test fails if a dependency ever gives a view
//! a way to fetch or decrypt one itself.

use std::process::Command;

/// The package name at the head of every line of `cargo tree --prefix none`.
fn package_names() -> Vec<String> {
    let output = Command::new(env!("CARGO"))
        .args([
            "tree",
            "-p",
            "takofy-ui",
            "--edges",
            "normal",
            "--prefix",
            "none",
        ])
        .output()
        .expect("running cargo tree");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("cargo tree writes UTF-8")
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

#[test]
fn ui_does_not_link_core_or_librespot() {
    for name in package_names() {
        assert_ne!(
            name, "takofy-core",
            "takofy-ui must not depend on the engine crate"
        );
        assert!(
            name != "librespot" && !name.starts_with("librespot-"),
            "takofy-ui must not depend on librespot (found {name})"
        );
    }
}

#[test]
fn ui_links_the_model() {
    assert!(
        package_names().iter().any(|name| name == "takofy-model"),
        "takofy-ui draws model types and must depend on takofy-model"
    );
}
