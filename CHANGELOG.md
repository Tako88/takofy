# Changelog

What changed in each release, written for the people using Takofy rather than
for the people building it. Newest first. Anything older than the first entry
here is on the [releases page](https://github.com/Tako88/takofy/releases).

## [Unreleased]

## [0.4.0] — 2026-11-20

### Added

- **The sound can come from Spotify Soloist.** Settings now names the program
  that plays your audio. Takofy's own engine is the default and is unchanged.
  The other is Spotify Soloist — Spotify's playback program, installed
  separately — which Takofy starts and drives while it keeps doing everything
  else: the library, search, the queue, and what is playing. Three steps set it
  up: point Takofy at the program, paste the API key Spotify gives you, and pair
  the device, which the phone does by choosing **Takofy** from Spotify's device
  list. A Soloist build stops working 90 days after it was built, and the page
  says how many days yours has left.
- **A settings page**, reached from the sidebar, because a choice that only
  exists in a file is not a choice. It says where each piece stands in words —
  whether the program was found, whether the key is stored, whether the device
  is paired, whether the build has expired — and offers the step that comes
  next.

## [0.3.2] — 2026-11-17

### Added

- **A one-line check of the sound.** `takofy --probe-audio` opens the output
  device and says whether this machine can play anything at all — no sign-in, no
  window. It is the first thing to try when a machine is silent, and it is what a
  bug report should start with.

### Fixed

- **A play that fails says why.** When the audio device is missing, or the player
  stops, the playbar's second line now carries the reason. Before, the bar showed
  a play button that did nothing, and the only sentence in the window lived in the
  Now Playing pane — which does not exist on a narrow window.
- **A playback failure no longer reports itself as a sign-in failure.** It used to
  say "Sign-in failed. Try again, or run with --headless for details."
- **Tracks that all fail stop and say so.** Five in a row with nothing playing
  between them used to leave the bar silent; it now reads "Nothing would play
  here."
- **Fewer interruptions when Spotify is slow.** A track that took longer than a
  second and a half to start, and then failed, was treated as a lost connection:
  the client reconnected and asked for the track again. Spotify's own key request
  times out at that same mark, so the two were indistinguishable.

## [0.3.1] — 2026-11-16

### Added

- **The cover is a menu too.** Right-click the picture in Now Playing for the same
  sheet the pane's "…" opens on the track that is playing — copy link, start
  radio, go to the album or the artist — which is the quickest way to hand
  someone what you are listening to.

## [0.3.0] — 2026-11-16

### Added

- **Just outside your library.** Home opens with artists next to the ones you
  already keep — a few at a time, each with a face, what they last released and
  the year it came out. Follow one, hide one, play one, or press the square for a
  different set, and "Turn discovery off" removes the band entirely.
- **A row you can keep pulling on.** Each discovery row ends with "Load more",
  which walks one ring further out from the artists already shown. When there is
  nobody out there it says "Nothing deeper from here" and stops offering.
- **Something you did not know you wanted.** One shelf draws a single thing out
  of your library and sets eight cards beside it: the songs and albums of that
  thing's neighbourhood that you already have. Press its square and it draws
  again, out of something else.
- **What the artists you follow just released.** A shelf of their newest albums,
  each with the day it came out.
- **Every card has its own "…".** The art in the library's grid, on Home and in
  search results now opens a menu: copy link, save, start radio, add to
  playlist. Start radio is there on any card whose album, artist or playlist can
  seed a station.
- **Keep a playlist someone sends you.** A playlist you open from a link shows
  its name, description, owner and cover even when it is not in your library,
  and its heart adds it to the sidebar. The page stays put when the library
  refreshes instead of falling back to your playlists, and once the list is
  yours the menu offers rename and remove as it does anywhere else.
- **Open a link someone sent you.** Paste a Spotify link into the search field
  and Takofy acts on it: a playlist, album, artist or show opens its page, and a
  track or an episode goes to the front of what is waiting. A pasted link is
  acted on at once, Enter does the same for one you typed, and anything that is
  not a link is still a search. A pasted track starts playing only when this
  window has the sound; with another device playing it queues and waits.
- **Play next.** A track's "…" menu can put it ahead of everything that is
  waiting, including what another device queued, without interrupting what is
  playing.

## [0.2.0] — 2026-10-31

### Added

- **A radio from anything.** A track, an album, an artist or a playlist now has
  "Start radio" in its "…" menu: the queue fills with that thing's station and
  starts playing.
- **Music that keeps going.** A radio no longer stops after its first batch:
  when it nears the end it asks for more that follow from what it has just
  played. A playlist, album or pick that reaches its end continues the same way
  instead of falling silent, and a queue that repeats is left alone — a loop
  stays a loop.

## [0.1.3] — 2026-10-31

### Added

- **Your library is there when the window opens.** Takofy remembers the last
  session's playlists, liked songs, saved albums, followed artists and followed
  shows, shows them while it reconnects, then quietly updates them. The copy is
  kept in your own cache folder, belongs to your account, is never used to make
  changes on Spotify, and a first run (or a copy it cannot read) starts exactly
  as before.
- **A failed refresh no longer blanks the library.** If the internet is down or
  Spotify is slow, what you had stays on screen with a note saying it is from
  your last session, and a Retry. It no longer replaces your library with an
  error.

### Changed

- **The library fills in sooner.** The lists no longer wait for every playlist
  name to arrive; they are usually on screen before the playlist names are.
- **The sidebar is lighter to fill.** Naming a playlist no longer downloads the
  whole playlist — only its first few tracks, which is all the row and its
  cover need.
- **Covers load in bigger batches and give up honestly.** A page full of covers
  arrives in fewer rounds, and a cover that cannot be loaded shows as a missing
  cover instead of sitting in a loading box, retrying behind the scenes.

### Known issues

- Starting Takofy with no internet connection still asks you to sign in again
  instead of telling you that you are offline
  ([#79](https://github.com/Tako88/takofy/issues/79)).

[Unreleased]: https://github.com/Tako88/takofy/compare/v0.3.2...HEAD
[0.3.2]: https://github.com/Tako88/takofy/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/Tako88/takofy/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/Tako88/takofy/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Tako88/takofy/compare/v0.1.3...v0.2.0
[0.1.3]: https://github.com/Tako88/takofy/compare/v0.1.2...v0.1.3
