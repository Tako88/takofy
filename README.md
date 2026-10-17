# Takofy

A fast, lean Spotify client for Windows and Linux, in Rust.

<!--
Two or three lines once phase 1 has something to show. Right now the honest
description is: a window and a dependency tree.
-->

## Building

```
cargo build --release
cargo test
cargo clippy --workspace --all-targets -- -D warnings
```

Linux needs ALSA development headers (`alsa-lib-devel` on Fedora,
`alsa-lib` on Arch, `libasound2-dev` on Debian/Ubuntu).

## Status

Nothing works yet. This repository is a skeleton: the workspace, the token
layer, the layout module, and a window that opens. Work is tracked in issues —
one per phase.

## A note on Spotify

Takofy plays audio through [librespot](https://github.com/librespot-org/librespot),
a community implementation of Spotify's own protocol. It needs a **Premium**
account, and it is not built on Spotify's public Web API or their developer
dashboard. It is not affiliated with or endorsed by Spotify.
