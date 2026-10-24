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
the queue. It also owns the edits to its own order and the selection over its
rows, since both are indices into it. None of this changes the shutdown order —
the command task still owns the engine and the player, and teardown still joins
it before the runtime drops.

The device list is read the same way. This client announces itself as a device
that cannot play and cannot be handed the sound, so seeing the account's
devices costs an entry in that list. The roster arrives whole, and the holder's
playback arrives as its own update so a position move does not rewrite the
rows. A remote transport sits beside the local one and steering goes through
one gated request path, so a skip cannot overlap an announce or a withdraw.
The costs are explicit: announcing is required to see the list, the phone
shows this client greyed until it can play, and local playback is not announced
at all.

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

A box that composes several covers is still one box. The playlist band's mosaic
looks at up to four album covers in the frame's own pass and paints only once
every one of them is held, drawing today's single cover until then, so a picture
never arrives in pieces and nothing in the band moves while it lands.

Following an artist is the first command that changes the account rather than
reading it, and it crosses the same seam as everything else: the page sends a
write and reports only what the server confirmed, so the label moves on the
answer and never on the press.

Labels branch on URI kind: a track and an episode resolve through their own
batch, and an episode is a label like a track with its show as its credit.

A playlist reorder is the second write, and it is applied locally first: the
rows take their new order as the drop lands, and then the moves go out through
librespot's spclient on the revision that read named. A block of rows is still
one POST: contiguous picks are one `MOV` of their own length, and gapped picks
are a sequence of `MOV`s in one delta, each naming indices in the list the
previous move left, so the order on screen is the replay of the moves that were
written. That request is made in a single attempt — `TryTimes(1)` for the one
POST, the default strategy restored after it — because a `MOV` is not
idempotent: a retry after a 5xx or a lost reply can move the row twice on a
server that has already applied it, so the one request that might have landed is
not repeated. A remote skip follows the same rule for the same reason — it is
relative, so a retry can skip twice — while pause, resume and seek name the
state they want and repeat safely. While a write is in flight no
read's order is adopted; a probe recorded the server answering a write with the
new revision and the old order, so adopting on revision equality would undo the
drag, and the worker answers in the order its commands were sent, which is what
makes the gate correct. The page reads only the first page of its context and a
move touches indices inside it, so every boundary the marker draws is a wire
index. A failed write puts the rows back where they were and the page says so
above them: the sentence is the whole affordance, and trying again means
dragging again rather than a button that could write twice for one gesture. Two
devices editing one list are last-write-wins, each naming the revision it read.

The library is four kinds over three collection sets. Followed shows, followed
artists and saved albums each come from their own set — saved albums share the
`collection` set with liked songs, so the read keeps the album URIs and drops
the rest — and every item is then named by its own metadata call, eight at a
time. An artist is one `Artist::get`; an album is one batched `ALBUM_V4`
request per fifty, because that endpoint answers a whole discography in one
reply and a short answer costs only its own chunk.

The two newest reads are want-driven rather than chained onto the first: the
artist and album lists are asked for when their chip is chosen, and the artists
an artist is heard with when that artist's own page has landed and named
somebody. A page therefore draws its hero and its discography without waiting on
a dozen likeness reads, and an artist the wire names nobody for asks for nothing.
Lyrics follow the same rule from the other side: the pane asks for the playing
track's lines when the tab is first opened, so playback never spends a request
nobody asked to see.

The sidebar column has two meanings, told apart by what is carried: a playlist
row dragged to a gap moves the playlists, and a track dragged onto a playlist
row is appended to it. One pointer position therefore cannot mean both, and a
carried playlist never reaches the queue, so a mis-drop cannot quietly reorder
a library. One pointer position never draws both a rule and a fill either: the
boundary target draws the rule, the row target draws the fill, and the two are
mutually exclusive by construction.

Both are writes through the same single-attempt path as a page's move — the
rootlist's own changes route for a move, the playlist's for an append — because
none of the three is idempotent, and a retry after a lost reply would apply the
change twice. A row index is only a wire index when the rootlist read kept every
item and reached the list's end, so a read that dropped one makes the whole
column read-only rather than move a different playlist. Length and revision
always move together: a confirmed append replaces the entry's revision and adds
one to its count, so the next append names a revision whose length it holds.

The library allows one write at a time across the page move and the sidebar's
two. While a sidebar write is in flight the sidebar's own read is dropped — the
part of it that carries an order, because a read taken after the write reached
the server can carry the new revision with the old order or the old count, and
the confirmation cannot tell which side of the write it came from. A dropped
read takes its access with it, since a revision only means something beside the
order it was read with. An access that lands while an add is pending is taken
all the same, because an add writes one playlist and not the rootlist; a
reorder's access waits for the reply that replaces its revision. A page's read
is not dropped: one that lands while that page's own move is pending keeps the
order the drag made. A queue insert is outside that gate: it is local and names
no revision, so no pending write can invalidate it. What the gate cannot fix is
a reply that was lost after the server applied the write: the held revision is
then old, the next write fails, and the library has to be reloaded. That is
preferred to retrying a write that is not idempotent.

## Narrowing a list, and strips that stay

A list can be narrowed by name as well as by kind, and the two axes multiply.
The matching is pure and lives beside the list it narrows, not in a view: a
track matches on its title, artists and album; a rail row on its name; a release
on its name only. A field is one widget with one id and one focus, drawn by the
three surfaces that have one, and it reads its own text after the frame's edit
so a keystroke resolves in the frame that typed it.

A narrowed row keeps the index it has in the whole list. That is what lets a
removal name true rows while a move is refused: a gap on screen has no true
position once hidden rows can sit between two visible ones, so the drag is
switched off rather than made up. The queue has neither a field nor that rule,
because its order is the window's own.

Two strips pin: the action row under a band, which condenses and keeps the
page's name, and a section's own row of chips, which holds under the action row
and releases at the end of the section it filters so it never bands over the
next one. Both are drawn once, at the pinned position, never copied. What egui
cannot do is reveal a focused row clear of them: it measures against the scroll
area's own clip, and a child's shrunken clip never reaches it. So the pinned
strips report how far down they reach, one seam sums that per frame, and every
row that asks to be revealed is shifted by it.

## One kind of result, three kinds of row

The search route answers twenty tracks and nothing else; there is no mixed
index this client can speak. So the menu and the results page derive the rest
from those twenty: the albums the matches sit on, and every artist credited on
them. The derivation is pure and lives beside the search state, and its bands
are data — a kind, a shown count and a total — capped by the surface that draws
them, three/two/two in the menu and ten apiece on the page, so another kind
would be an entry rather than a redesign. The page can therefore only say what
the twenty carry: the record a match sits on, and how many of the twenty name an
artist. An artist or album that no track matches is not found.

The faces are the one thing the answer cannot carry, so they arrive in a second,
speculative read: one batched artist request for every artist a surface will
draw, keyed by the record's own id rather than by the URI the answer echoes, and
bounded in time because the worker answers commands in order and a speculative
read must not hold a play behind it. A reply counts only while the surface that
asked is the one on screen — with one exception, a menu's reply filling the page
when Enter hands the overlay over, because the cache is keyed by artist and the
faces are the same faces. An artist whose record carries no portrait, whose
face never arrived, or whom the answer omitted falls back to the cover of the
record the match sits on; only a read still in flight draws a raised slot.

A revealed block belongs to the read on screen, not to the query: a new query
starts every block at ten and at the top of the page, while a retry or a
re-entry keeps both what was revealed and where the reader was.

## Who owns what

- The binary owns the tokio runtime, and the `Engine` in headless mode. In the
  window, the command task owns the engine and the player, and the task's exit
  path shuts both down.
- The session work the task drives sits behind one seam, so the task's exit
  paths can be exercised without a network or an audio device. A real `Playback`
  needs a live session and an audio device, so the exit path would otherwise be
  untestable.
- The device watch is a runtime task that reads the dealer's streams and never
  outlives the withdraw, and a plain drain thread that turns its events into
  updates. On every exit path the command task withdraws the announced device
  before it shuts the player down.
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

Preferences are a file read once before the window opens and written by its own
`takofy-prefs` thread, so the UI thread never touches the filesystem. A value is
handed over after it has been quiet for 400 ms, and `on_exit` flushes whatever
is still unsettled. The writer's `Drop` then waits a bounded two seconds for the
thread and detaches it: a hung filesystem cannot hold the window open longer
than that.

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

A page's contents are drawn in either of two densities the library page shares:
the same item as a card, or as a row under a type and size column. The density
is one field on the library, so it survives a page and a Back, and both
densities draw the same `Item` — the filter, the loads and the click handling
exist once.

What it costs: egui has no stylesheet. The token layer in
`crates/takofy-ui/src/theme.rs` is therefore the only place a colour, spacing
step, type size or radius is written, and `clippy.toml` fails the build on a
`Color32` constructor anywhere else. Window geometry and the breakpoints that
decide region visibility live in `crates/takofy-ui/src/layout.rs`, which
expresses every region width in tokens.
