# Takofy

A fast, lean Spotify client for Windows and Linux, in Rust.

## How it is put together

The crate boundaries, the threading model and the invariants are in
[`docs/architecture.md`](docs/architecture.md).

## Building

```
cargo build --release
cargo test
cargo clippy --workspace --all-targets -- -D warnings
```

Linux needs ALSA development headers (`alsa-lib-devel` on Fedora,
`alsa-lib` on Arch, `libasound2-dev` on Debian/Ubuntu).

## Status

Headless phase 1 runs: `cargo run` completes a PKCE login in the browser,
stores the refresh token in the OS keychain, prints your rootlist, and plays a
selected playlist track to its end. There is no window yet; `takofy-ui` still
builds but the binary does not use it.

## Requirements

- **Premium.** Playback goes through [librespot](https://github.com/librespot-org/librespot),
  which needs a Premium account. A free account fails with `track unavailable`.
- **An OS keychain.** The refresh token is stored with the `keyring` crate:
  Windows Credential Manager, or a Secret Service provider on Linux (GNOME
  Keyring, KWallet's Secret Service bridge, or another daemon). Without one the
  program exits with an error and never writes a token to disk.
- **An audio output device.** Playback probes the default device through rodio
  before loading anything. With no device, `cargo run` prints
  `no audio output device available` and exits with code 3.

## `--paste`

Normal login opens `http://127.0.0.1:8898/login` in the browser. If that port
is busy, or the redirect is rejected, `cargo run -- --paste` switches to the
portless `http://127.0.0.1/login` and reads the pasted redirect URL from stdin.
That portless redirect may not be registered for the client ID, so `--paste` is
a last resort, not the primary path.

## A note on Spotify

Takofy plays audio through [librespot](https://github.com/librespot-org/librespot),
a community implementation of Spotify's own protocol. It needs a **Premium**
account, and it is not built on Spotify's public Web API or their developer
dashboard. It is not affiliated with or endorsed by Spotify.
