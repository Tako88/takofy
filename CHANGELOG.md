# Changelog

What changed in each release, written for the people using Takofy rather than
for the people building it. Newest first. Anything older than the first entry
here is on the [releases page](https://github.com/Tako88/takofy/releases).

## [Unreleased]

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

[Unreleased]: https://github.com/Tako88/takofy/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/Tako88/takofy/compare/v0.1.2...v0.1.3
