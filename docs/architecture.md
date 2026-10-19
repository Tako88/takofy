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
app lands in Development Mode, capped at five users and without the batch
endpoints, so reading a 500-track playlist costs 500 requests.

The price is a Premium account and a footing outside Spotify's developer terms.

## The crates

| crate | does | never does |
| --- | --- | --- |
| `takofy-model` | holds plain data | depends on anything |
| `takofy-core` | talks to Spotify | draws |
| `takofy-ui` | draws | reaches the network |
| `takofy` | owns threads, wiring, platform | talks to Spotify, or draws a widget |

The split is enforced by the compiler, not by convention. `takofy-ui` does not
depend on `takofy-core` or on librespot, so a view cannot reach the network by
accident. The two share only `takofy-model`, which has no dependencies at all.
That is what lets the window hold the types it draws without the split ceasing
to be a boundary.

## How data moves

The UI holds a snapshot of what it draws and emits commands. The binary owns the
engine, runs it on a tokio runtime, drains commands, and sends updates back.

The UI never calls into core. That keeps the crate boundary real, keeps the frame
loop free of awaits, and keeps the cursor types that borrow a session out of the
UI.

The updates have to stay fine-grained. An update that reports one event is
testable; one that carries the whole state is not: it moves the UI's state into
core and leaves the reducer with nothing to assert. The first shape is what
earns the boilerplate the second one avoids.

Playback events carry the loaded track's duration and the position a seek
landed on, so the window can draw the time left and follow the player's truth
rather than a guess. The play queue is a window reducer too: it owns the index
and the advance rule, folds the same updates the transport does, and hands back
an ordinary `Play`, so the worker stays a dumb executor and never learns about
the queue. None of this changes the shutdown order — the command task still
owns the engine and the player, and teardown still joins it before the runtime
drops.

Cover art runs through the same split. The binary fetches and decodes; the
window only holds textures. A frame's protocol is `begin_frame` → `look` →
`requests` → `next_wake`: the app starts the frame, the draw `look`s each cover
box it is about to paint, the app asks the store for the fetches the draw said
it wanted and sends at most a fixed number, and then it asks how long until the
next retry or in-flight deadline so an idle window can wake itself for a retry
it would otherwise never make. Nothing in the window fetches, and a box that is
not drawn is never requested. Decode runs on a detached thread rather than
tokio's blocking pool for the same reason sign-in does: a decode that hangs must
not hold up the runtime drop, and a detached thread dies with the process
instead.

Following an artist is the first command that changes the account rather than
reading it, and it crosses the same seam as everything else: the page sends a
write and reports only what the server confirmed, so the label moves on the
answer and never on the press.

## Who owns what

- The binary owns the tokio runtime, and the `Engine` in headless mode. In the
  window, the command task owns the engine and the player, and the task's exit
  path shuts both down.
- The session work the task drives sits behind one seam, so the task's exit
  paths can be exercised without a network or an audio device. A real `Playback`
  needs a live session and an audio device, so the exit path would otherwise be
  untestable.
- The window's end of the bridge, `UiEnd`, is moved into the app and dropped
  with it when the window closes.
- `Worker` is owned outside the app, by the teardown that joins it. eframe drops
  the app before `run_native` returns, so a `Worker` the app held would be gone
  before shutdown could use it.
- The waker is installed only once the window exists. An update that arrives
  first waits in the channel, and the first frame drains it.
- `Playback` is single-owner by design. `Player::drop` joins the player thread
  only when the last `Arc` drops, so a stray clone skips the join silently.
  `shutdown` enforces this with `Arc::try_unwrap` and reports `PlayerShared`.
- librespot owns its playback threads. Nothing is shared behind a lock the UI
  could block on.
- Playback events leave librespot on a task spawned on the runtime and reach the
  window through one plain drain thread. That thread is detached, so it cannot
  delay the runtime drop.

Shutdown runs in a fixed order: the window closes, teardown tells the command
task to stop, the task shuts the player down, and then the runtime drops. The
player's shutdown is awaited, so the order ends in joining the player thread. It
depends on `NativeOptions::run_and_return`, so that eframe returns instead of
exiting the process; an exit would skip both teardown and the runtime drop.

In the window, sign-in calls run on detached threads, not `spawn_blocking`:
one parked on the browser callback can hold up the runtime drop, which waits
on the blocking pool, and a detached thread dies with the process instead.

## Invariants

- No token, auth code or credential reaches a log, a panic message or stdout.
- No credential is written to disk. The refresh token lives in the OS keychain;
  there is no fallback to a file or to a store keyed by something the same user
  can read.
- `takofy-ui` does not link `takofy-core` or librespot.
- The UI thread never blocks on I/O.

## Why egui

egui is immediate mode, so `ui()` re-runs each frame and reads a plain struct.
That suits a music player, which is lists and a transport bar rather than an
animation showcase, and it has AccessKit support by default.

The icons are data. The reference draws them from an SVG sprite, so
`crates/takofy-ui/src/icon/data.rs` holds the flattened coordinates and the
binary carries no SVG parser: `convex_polygon` for the filled glyphs, polylines
for the stroked ones, and two pure functions that round the corners and cap the
ends at draw time, because the sprite asks for round joins and epaint mitres.
The table is generated, and the generator is a local script rather than repo
content, so `assets/icons/README.md` documents the conversion and a test
re-checks every glyph against `docs/reference/main.js`.

What it costs: egui has no stylesheet. The token layer in
`crates/takofy-ui/src/theme.rs` is therefore the only place a colour, spacing
step, type size or radius is written, and `clippy.toml` fails the build on a
`Color32` constructor anywhere else. Window geometry and the breakpoints that
decide region visibility live in `crates/takofy-ui/src/layout.rs`, which
expresses every region width in tokens.
