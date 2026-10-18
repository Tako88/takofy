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

The window is the default: `cargo run` opens it, completes the PKCE sign-in in
the browser once, and stores the refresh token in the OS keychain. The sidebar
lists your playlists; clicking one shows its tracks, double-clicking a track
plays it, and the transport bar names what is playing with a position that
advances. `cargo run -- --headless` still drives the terminal driver unchanged.

Not there yet: search, folders, album art, and seeking or transport controls
([#13](https://github.com/Tako88/takofy/issues/13)).

## Requirements

- **Premium.** Playback goes through [librespot](https://github.com/librespot-org/librespot),
  which needs a Premium account. A free account fails with `track unavailable`.
- **An OS keychain.** The refresh token is stored with the `keyring` crate:
  Windows Credential Manager, or a Secret Service provider on Linux (GNOME
  Keyring, KWallet's Secret Service bridge, or another daemon). Without one,
  sign-in fails and no token is written to disk; `--headless` exits with that
  error.
- **An audio output device.** Playback probes the default device through rodio
  before loading anything. With no device the window says so on the transport
  bar; `--headless` prints `no audio output device available` and exits with
  code 3.

## `--paste`

Normal login opens `http://127.0.0.1:8898/login` in the browser. If that port
is busy, or the redirect is rejected, `cargo run -- --headless --paste` switches
to the portless `http://127.0.0.1/login` and reads the pasted redirect URL from
stdin.
That portless redirect may not be registered for the client ID, so `--paste` is
a last resort, not the primary path.

## A note on Spotify

Takofy plays audio through [librespot](https://github.com/librespot-org/librespot),
a community implementation of Spotify's own protocol. It needs a **Premium**
account, and it is not built on Spotify's public Web API or their developer
dashboard. It is not affiliated with or endorsed by Spotify.
