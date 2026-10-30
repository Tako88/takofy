# Home — what there is to draw

A brief for the designer. No code, no implementation detail. It says what data
this client can actually obtain, what it cannot, and therefore which shelves a
Home page could honestly carry. The pictures come after that, because the
pictures are the easy half.

**How to read the confidence markers**, since a lot of this document is a
measurement rather than an opinion:

- **Measured** — read off the real account, with the date.
- **In the app** — the app already reads it and draws it today.
- **Unverified** — the route exists in the library this client is built on, but
  it has not yet been tried against the account. Treat every "unverified" line as
  a maybe, and do not design a page that collapses if it turns out to be a no.

---

## 1. The one hard fact this page has to live with

There is no feed.

Spotify's own landing page — the greeting, "Made for you", Daily Mix, Discover
Weekly, Release Radar, "Because you listened to", "Fans also like", "Browse all"
— is assembled by a private GraphQL API (`pathfinder`) that the official client
speaks. The protocol this client speaks has no home endpoint, no browse endpoint
and no pathfinder client. That is not a matter of finding the right call: the
call is not there. It was checked once already, carefully, and the result is
written down in #17.

So Home cannot be a window onto Spotify's curation. It can only be a page the app
*assembles*, out of three things:

1. **Your library** — what you have saved and followed. All of it is already read.
2. **A few "more like this" generators** that do exist on the wire, seeded by
   something in your library (§4).
3. Nothing else. There is no editorial voice available to this client, and no
   second API worth adding one from (§4.5).

That is the whole design problem: a page that feels alive without a curating
third party, using a library and a similarity engine.

> One note before any of this. #17 was closed because the design target moved —
> the reference's Home was replaced by Your Library, and the roadmap says Home "is
> answered, not pending". A Home page is therefore a reversal of a recorded
> decision, and it needs to be taken deliberately, not discovered halfway through
> the pictures.

---

## 2. What is in the library, and what comes with it

**Measured 2026-10-19 and 2026-10-21 on the test account, and read by the app
today.** The counts are one person's library — treat them as orders of magnitude,
not as constants.

| Kind | Count | What arrives with it |
| --- | --- | --- |
| Playlists | 23 | name, own cover or a derived mosaic of up to four album covers, owner, whether this user may edit it, track count |
| Liked tracks | 1,461 | the track, and **when it was liked**, in seconds |
| Saved albums | mixed into the same saved set as liked tracks | name, artist line, year, cover, track count |
| Followed artists | 19 | name, portrait, how many current releases |
| Followed shows | 8 | name, publisher, description, cover |
| Episodes | only through a show | name, duration, description, cover, publish date |

Three properties of this data matter to the design more than the counts:

**Names and artwork arrive late, and separately.** A list is read as a set of
URIs first; the names, artist lines and covers come from a second, batched read
afterwards, and art is a third fetch again. Every list in the app therefore fills
in place. A Home shelf does not get to be an exception — plan for the skeleton
state as the *normal* state of the first quarter second.

**Dates exist in two places, and one of them is not drawn.** Liked tracks carry
the date they were liked (seconds). A track inside a playlist carries the date it
joined that playlist, but the app does not read it yet (#20). "Recently liked" is
available today; "recently added to this playlist" nearly.

**Art is many-sized.** Every cover arrives as a set of variants and the app picks
by the pixels it is about to fill, so a 144pt card and a 40pt queue row draw
different, correctly sized images from the same set.

---

## 3. What is not there, and cannot be faked

This list is longer than §2 and it constrains the design more.

- **No play history.** Nothing counts plays, nothing records skips, nothing
  remembers completions. The only things that survive a restart are volume, mute,
  shuffle and repeat. So "because you listened to X" is not available, and
  neither is "your top artists this month", "recently played", or "you might be
  tired of this one".
- **No play counts, no monthly listeners, no "verified" flag** on any entity.
- **No editorial anything**: no charts, no curated playlists by browsable name,
  no "new releases for you", no categories, no genres.
- **No mixed search.** Search answers 20 tracks and nothing else, so it cannot be
  mined for artists or albums to build a shelf out of.
- **No folders** (#3). Nested playlists are skipped rather than shown, so a Home
  shelf counting "your playlists" counts a flat list.

The consequence, stated plainly: **every shelf on Home has to be justified by
something the user saved.** The app can say "because you follow X" or "more like
X". It cannot say "because you have been playing X", because it does not know
that. Any shelf that implies listening behaviour is a lie the app cannot back up
— and it is the kind of lie a user notices in a week.

---

## 4. The generators that do exist

Four of them. The first is in the app. The rest are routes inside the library
this client is built on: the calls exist in its source, one of them is called by
the library itself every time a playlist ends, and none of them has been tried
against this account yet.

| # | Generator | What it answers with | Seed | Confidence |
| --- | --- | --- | --- | --- |
| 1 | Related artists | up to 12 artists "fans also like" per artist | any artist | **In the app** (it is the artist page's second row) |
| 2 | Station / radio | a station: its own title, subtitle, picture, and 20–55 tracks **with names, artists and cover ids** | an artist, album, track or playlist | **Measured 2026-10-29** |
| 3 | Autoplay continuation | a page of tracks that continue a context | a context (e.g. one of your playlists) plus what was just played | **Unverified, but in live use by the library itself** |
| 4 | "Inspired by" | **playlists**, not tracks | a track | **Unverified** |

Notes that change how they can be used:

- **2 is deterministic per seed.** Asked twice, it answered the same twenty
  tracks both times, and passing the previous answer back as `prev_tracks`
  changed nothing. So a station cannot be made to differ by asking again — the
  way to make Home differ between opens is to rotate **which seeds** are asked,
  exactly as this reference already does. That is a measurement, not a
  preference: it is why the rotation lives in the seed list and not in the call.
- **3 returns tracks in exactly the shape the app already renders and plays.**
  It is the cheapest of the three to turn into a working shelf, because the app
  already knows how to draw and play that shape.
- **4 returns playlists**, so it belongs in a card shelf, not a row shelf.
- **1 is a graph you can walk.** From 19 followed artists you can reach up to 228
  artist names, most of them not in your library. One more hop reaches their top
  tracks and their releases. Nothing about that is unverified: it is today's
  artist page, used as a generator.
- **Any context URI resolves.** Anything the app can name — a playlist, an album,
  an artist, a show — can be opened as a playable list, so a shelf can be built
  from a seed *context* rather than from a single track.

### 4.5 What was rejected, so nobody spends a week on it

- **The public Web API** (`/v1/me/top/tracks`, `/v1/recommendations`). Answered
  429 to this client's token when it was tried. It is also a second door, a
  second rate limit and a second way to fail, for a result no better than §4's.
- **Last.fm, ListenBrainz, MusicBrainz.** Each needs a credential the user has to
  go and get, each ships their listening somewhere else, and each recommends
  worse than the station generator above. For a private desktop client the
  privacy cost buys nothing.
- **Spotify's editorial shelves, by guessing URLs.** Playlist URIs could be
  hardcoded, and it would look like a feed for a month and then rot. It is also
  not "based on your library", which is the point.

---

## 5. The shelf archetypes that follow

Seven that the data above can actually carry. Each is one section head plus one
body, both of which already exist (§6). **On counts**: the app's own blocks
settle at ten top tracks and up to twelve related artists, each with a reveal row
at its foot that asks for the rest. A shelf should stay inside that range — eight
or ten items, then a reveal — rather than inventing a scale of its own.

| Shelf | Source | Body | The "why" line | Network | Empty when |
| --- | --- | --- | --- | --- | --- |
| **A. Recently added** | liked tracks, saved albums and playlists by date (Measured: each carries one) | track rows, or cards | "the newest things you saved" | none | nothing saved at all |
| **B. Artists you have a lot of** | counted holdings per artist (local counting, not a route) | artist cards | "the artists you have the most of" | none | a library with no repeats |
| **C. More like <artist>** | generator 1 | artist cards | "because you follow <artist>" | 1 call per artist | never, if the artist has neighbours |
| **D. <artist> station** | generator 2 | track rows | "a station built from <artist>" | 1 call | seed has no station |
| **E. Where <playlist> would go next** | generator 3 | track rows | "after your playlist <name>" | 1 call | playlist too short or empty |
| **F. Saved and untouched** | date only (Measured) | cards or rows | "in your library since 2023" | none | a library younger than a year |
| **G. Your shows** | followed shows (+ episodes) | episode rows or cards | "you follow 8 shows" | none / 1 call | no podcasts followed |

Two of these (A, F) are pure arithmetic over data the app already holds, which
means they work on day one, offline-first, with no failure mode. Three (C, D, E)
are seeded by something the user can see on screen. B is the only one that needs
a new local computation, and it is the one that most resembles "taste" without
claiming to know any.

**The rule worth writing into the page**: a shelf's seed must be visible in its
own title. "Because you follow Bonobo" is honest and cannot break. "Made for you"
is neither, and it puts the app in competition with a curation engine it does not
have.

---

## 6. What the app can already draw

The design job here is composition, not new components. Everything a shelf needs
exists:

**The section head** — bold title at the `lg` size on the left, optional trailing
aside and chips at the right, a step of space below. It is the same head the
artist and show pages use.

**The card grid** — the reference's `auto-fill, minmax(144pt, 1fr)` with a 12pt
gap: as many 144pt-minimum columns as fit, sharing the remainder exactly, art
square on top, name and one subline beneath. Because it fills, a shelf's card
count is a *design* decision, not a layout one — at the default 1240pt window the
main pane holds 680pt, which is four such cards; at 1120pt the Now Playing pane
is gone and the same shelf fits five. The library's own tiles use a 176pt
minimum, so they sit at three and four instead.

**The track row** — 44pt tall, and it already carries nine things: number, 36pt
art, title, artist, album, added date, duration, like, "more". A Home row shelf
could use the full row or a reduced version; the reduced one is the queue's 48pt
row (40pt art, title, artist, duration).

**The library tile** — 36pt art beside a name and a kind line, for a list-density
shelf.

**The reveal row** ("show all 24") — a full-width row at a block's foot, already
built, already keyboard-reachable.

**The window**, in tokens: sidebar 256pt, Now Playing pane 312pt (272pt when
narrow), playbar 64pt, topbar 56pt. Breakpoints that matter: the pane disappears
at ≤1120pt, the sidebar collapses to a rail at ≤900pt, the track table loses
columns at ≤1000pt and again at ≤760pt. Default window is 1240×720; the minimum is
640×500.

**Spacing** is a 4pt scale (4, 8, 12, 16, 24, 32, 48, 64). **Radii** are 0, 1, 2,
3, 4 and pill. **Rules** are 1pt or 2pt and nothing else. **Motion** is 110ms
fast, 180ms base, 300ms slow. Text sizes run 10, 11, 12, 13, 15, 18, 22, 28, 40,
56.

**Green is spent.** `#1ed760` means "sound is coming out" — the playing row's 2pt
left bar, the play glyph, the focused field's border. It is not a brand colour and
it never paints a surface. A Home page is the most tempting place in the app to
break that rule; the token file's own words are that green is a functional
signal, so if Home wants an accent it needs a ruling written down, not a shade
picked on the day.

---

## 7. The awkward parts, where a ruling is wanted

These are the questions the pictures cannot answer by being pretty. Answer them
in a sentence each and the visual decisions follow.

**1. Where does Home live?** The root of the app is Your Library today, by
decision. Home can be: a fifth thing in the sidebar above the playlists, a page
behind the app's name in the topbar, or two shelves at the top of Your Library
that scroll away. These are three different pages with three different budgets,
and picking one changes what "the app opens on" means.

**2. What makes it differ between opens?** **Answered: the shelves are fixed and
their contents are drawn.** The five shelves appear in the same order under the
same titles every time; what changes is every choice inside them — each
station's seed, which three rows of a station are shown, which followed artist
seeds the related shelf and which of its artists are drawn, and which cards fill
each shelf inside its own claim (§5). The generator cannot help here: it answers
the same twenty tracks every time the same seed is asked (§4), so the variety has
to come from the draw, not the request.

Three constraints on the draw, and all three are visual. A shelf's title stays
true after a draw (the dated shelf's cards always run downhill by date, drawn from
the newest end;
a ranked shelf stays inside its ranking). **Nothing may move while the page is on
screen** — one draw per open, never per repaint, so no card changes under the
pointer. And a shelf is absent only when its library is empty, never because the
draw skipped it: the page is five shelves, or four when the account has no shows,
and it has to look intentional both ways.

So the layout is stable and the page height barely changes — which is the point:
a page the user can learn, whose contents are new each time.

**3. What does it look like on day one?** A fresh account has no likes, no
follows, no playlists. That is not an edge case: it is the first thing every new
user sees, and it is the only screen where a page of empty shelves is worse than
no page. Either Home has an honest empty state, or it does not exist for a new
account and the app opens on Your Library until there is something to say.

**4. How do shelves fill in?** §2: names and art arrive after the URIs. Design
the skeleton: how many placeholder rows, do the cards shimmer, does the section
head appear before or after its body, and what happens when one shelf's network
call fails while the other five are fine.

**5. Vertical page, vertical shelves.** Nothing in the app scrolls sideways
today: lists scroll, pages scroll, but no row of cards ever moves horizontally.
A shelf that scrolls sideways would be the app's first. It is also what every
music app does. Pick one deliberately.

**6. What is the budget?** Each wire-backed shelf is one request per open. Three
is a reasonable ceiling: the page then takes three round trips to be complete,
which on a 50ms connection is invisible and on a bad one is a second and a half.

**7. What happens when a seed disappears?** "Because you follow Bonobo" survives
unfollowing Bonobo until the page refreshes. Say whether the shelf vanishes,
goes quiet, or lingers — and whether that is allowed to be visible mid-session.

---

## 8. What to hand back

1. A page in the reference — `docs/reference/home.html` — drawn with the app's
   own classes and tokens, showing: the full page at 1240×720; the same at
   1120×720 with no Now Playing pane; the same at 760 wide; one shelf in its
   skeleton state; and the day-one page with an empty library.
2. Per shelf: which of §6's bodies it uses, or the name of the new token or
   component it would need, in the token file's own one-line style.
3. A ruling on §7.1 (where Home lives), §7.2 (what makes it differ) and §7.5
   (horizontal or not). Those three decide everything else, in that order.
4. The "why" line's voice: one example sentence per shelf archetype, so the
   strings have a style before they have a translation.

---

## Appendix — the routes, exactly

For whoever builds it, so the brief is checkable. Everything here is read from
this client's own library source; the last column is what has been *verified*.

| Route | Method | Parameters | What the field is |
| --- | --- | --- | --- |
| `/radio-apollo/v3/{scope}/{seed}` | GET | `scope` = `stations` or `tracks`; `count` (asked 20 → 20, asked 100 → 55; it returns what exists), `prev_tracks`, `autoplay` | **measured**: answers for playlist, track, artist and album seeds; `tracks` returns `{tracks[], next_page_url}`; `stations` adds the station's own `uri` (`spotify:station:<seed>`), `title`, `subtitle`, a single or `spotify:mosaic:` `imageUri`, `seeds[]` and `related_artists[]`. Each track carries `uri`, `uid`, `artist_uri`, `album_uri` and `metadata{title, artist_name, album_title, artist_uri, image_url}` — **and no duration**. Deterministic per seed; `prev_tracks` changed nothing |
| `/context-resolve/v1/autoplay` | POST | protobuf `{context_uri, recent_track_uri[], is_video}` | **measured**: 50 rows in one page, and its `uri` came back as `spotify:station:<seed>` — the autoplay context *is* a station |
| `/context-resolve/v1/spotify:station:<seed>` | GET | — | **measured**: 50 rows in one page, paging follows to `hm://radio-apollo/…` and answers 50 more. The rows carry `uri` and `uid` only, so labels come from the usual batched read |
| `/inspiredby-mix/v2/seed_to_playlist/{uri}` | GET | `response-format=json` | **measured, and not worth using**: `{total: 1, mediaItems: [{uri: "spotify:playlist:…"}]}` — one bare playlist URI with no name, cover or description, so it costs a second read to learn what it even is |
| `/user-profile-view/v3/profile/{user}` | GET | `playlist_limit`, `artist_limit` (both ignored) | **measured**: `account_id, allow_follows, color, followers_count, following_count, is_current_user, name, public_playlists, total_public_playlists_count, uri`. No top artists, no listening anything, so a "most of" shelf must be counted locally |
| artist metadata | — | — | `related` (fans also like, up to 12), `top_tracks` (10), releases split into albums/singles/compilations/appears-on — **measured, in the app** |
| collection sets | POST | paging, 500 at a time | liked tracks and saved albums together (split by URI prefix), followed artists, followed shows; each item carries its added date — **measured** |

Three things this measurement settles and one it does not:

- **Shelves D and E are commitments.** Both routes answer, for every seed kind the
  page uses, and a station arrives with its names and cover ids already attached —
  a shelf of three rows is one call, not one call plus a label read.
- **The collection is not a seed.** `spotify:user:<name>:collection` is refused
  with 400, so "From the songs you liked" cannot be a station seed. A *track*
  seed works, so the fourth station becomes "from a song you liked" with the song
  named in its head — which is more honest anyway: it names the thing instead of
  the pile.
- **Durations are not in it.** The trailing mono value in a station row needs one
  batched label read for the twelve rows shown, or it shows the album's name
  instead of a time.
- **Settled since:** the `hm://radio-router/…` `next_page_url` in the JSON is not
  reachable through the client's page fetch (404), while the *context's* next page
  on `hm://radio-apollo/…` answers 50 more tracks. Page through the context, not
  the JSON.

Three more things measured afterwards, all of which unblock a shelf:

- **A playlist has a save date.** The rootlist read has to ask for it: with
  `decorate=…,timestamp` each entry carries an epoch-millisecond date (some
  entries answer `0`, so the card needs a fallback). Liked tracks and saved
  albums carry theirs already; the app currently reads and drops them.
- **A show can be followed.** The same collection write the app already uses for
  artists, with the set named `show`: unfollow answered 0 bytes with the followed
  count going 7 → 6, and re-follow went back to 7. So the day-one page's second
  door is real, not aspiration.
- **Labels and durations are cheap.** One batched read named 500 liked tracks in
  **282 ms**, durations included. That is what a "most of" shelf needs to rank
  artists, and it is also where a station row's `4:12` comes from — one batch
  serves both.
