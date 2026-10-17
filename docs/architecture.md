# Architecture

What Takofy is built out of, and why the boundaries sit where they do.

This describes the system as it is, and what has been settled for what is not
built yet. The reason for a change belongs in the issue or the commit that made
it. A decision that is reversible in an afternoon and does not move a boundary
belongs there too, not here.

## The footing

Takofy plays audio through [librespot](https://github.com/librespot-org/librespot),
a community implementation of Spotify's own protocol. It does not use Spotify's
public Web API, and there is no dashboard app behind it.

Two properties of the public API force that. It hands out no audio stream, so a
client built on it can only remote-control an official one. And a new dashboard
app lands in Development Mode, capped at five users and, since February 2026,
without the batch endpoints, so reading a 500-track playlist costs 500 requests.

The price is a Premium account and a footing outside Spotify's developer terms.

## The crates

| crate | does | never does |
| --- | --- | --- |
| `takofy-core` | talks to Spotify | draws |
| `takofy-ui` | draws | reaches the network |
| `takofy` | owns threads, wiring, platform | either of the above |

The split is enforced by the compiler, not by convention. `takofy-ui` does not
depend on `takofy-core` or on librespot, so a view cannot reach the network by
accident.

## How data moves

The UI holds a snapshot of what it draws and emits commands. The binary owns the
engine, runs it on a tokio runtime, drains commands, and sends updates back.

The UI never calls into core. That keeps the crate boundary real, keeps the frame
loop free of awaits, and keeps the cursor types that borrow a session out of the
UI.

The updates have to stay fine-grained. `Update::PlaylistLoaded(Vec<PlaylistEntry>)`
is testable. `Update::State(Library)` is not: it moves the UI's state into core
and leaves the reducer with nothing to assert. The first shape is what earns the
boilerplate the second one avoids.

## Who owns what

- The binary owns the `Engine`, the tokio runtime, and both channels.
- `Playback` is single-owner by design. `Player::drop` joins the player thread
  only when the last `Arc` drops, so a stray clone skips the join silently.
  `shutdown` enforces this with `Arc::try_unwrap` and reports `PlayerShared`.
- librespot owns its playback threads. Nothing is shared behind a lock the UI
  could block on.

## Invariants

- No token, auth code or credential reaches a log, a panic message or stdout.
- No credential is written to disk. The refresh token lives in the OS keychain.
- `takofy-ui` does not link `takofy-core` or librespot.
- The UI thread never blocks on I/O.

## Why egui

egui is immediate mode, so `ui()` re-runs each frame and reads a plain struct.
That suits a music player, which is lists and a transport bar rather than an
animation showcase, and it has AccessKit support by default.

What it costs: egui has no stylesheet. The token layer in
`crates/takofy-ui/src/theme.rs` is therefore the only place a colour or size may
be written, and `clippy.toml` fails the build on a literal anywhere else.
