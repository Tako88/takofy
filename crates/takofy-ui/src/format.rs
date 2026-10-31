//! The reference's wording for the words and numbers the shell prints: the
//! playbar's clock, the hero's byline, the playlist band's description, and the
//! age a playlist row gives its date.

use takofy_model::home::SeedKind;
use takofy_model::{AddedAt, Owner, PlaylistEntry, PublishedAt, ReleaseKind, SavedAt, TrackEntry};

use crate::discography::Chip;
use crate::library::SidebarWrite;
use crate::library_filter::RootKind;
use crate::search_index::tracks_phrase;
use crate::sidebar_kind::SidebarKind;

/// One day in milliseconds.
const DAY_MS: u64 = 86_400_000;

/// Formats a position as minutes and seconds.
pub fn format_position(position_ms: u32) -> String {
    let seconds = position_ms / 1_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Formats the time left, in the reference's `-m:ss` form.
pub fn format_remaining(position_ms: u32, duration_ms: u32) -> String {
    format!(
        "-{}",
        format_position(duration_ms.saturating_sub(position_ms))
    )
}

/// The song count as the reference writes it: `1 song`, `N songs`.
///
/// Zero is `0 songs`, not a special case.
pub fn count_label(count: usize) -> String {
    count_noun(count, "song")
}

/// A count and its noun, singular only for one. Zero is `0 <noun>s`.
///
/// One wording rule, two nouns: the playlist counts songs and the album counts
/// tracks, and neither may drift from the other's pluralisation.
fn count_noun(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// An album's track count as its band writes it: `1 track`, `N tracks`.
pub fn track_count_label(count: usize) -> String {
    count_noun(count, "track")
}

/// A release count as the discography's aside writes it.
pub fn release_count_label(count: usize) -> String {
    count_noun(count, "release")
}

/// The word a release block's facts lead with.
pub fn release_kind_label(kind: ReleaseKind) -> &'static str {
    match kind {
        ReleaseKind::Album => "Album",
        ReleaseKind::Ep => "EP",
        ReleaseKind::Single => "Single",
        ReleaseKind::Compilation => "Compilation",
        ReleaseKind::Other => "Release",
    }
}

/// The word a filter chip carries.
///
/// Plural, because a chip counts a set while a block's own facts line names one
/// release. `Other` never gets a chip; its word is here so the match stays
/// exhaustive and no future kind can be added without deciding one.
pub fn filter_label(chip: Chip) -> &'static str {
    match chip {
        Chip::All => "All",
        Chip::Kind(ReleaseKind::Album) => "Albums",
        Chip::Kind(ReleaseKind::Ep) => "EPs",
        Chip::Kind(ReleaseKind::Single) => "Singles",
        Chip::Kind(ReleaseKind::Compilation) => "Compilations",
        Chip::Kind(ReleaseKind::Other) => "Releases",
    }
}

/// A release block's facts: the kind, the year, the count and the total.
///
/// The parts are separate so the painter can place the dot separator between
/// them, exactly as [`album_meta`] does for the band. An unknown year and an
/// absent total are left out rather than drawn as blanks.
pub fn release_meta(kind: ReleaseKind, year: Option<u16>, tracks: (usize, u32)) -> Vec<String> {
    let mut parts = vec![release_kind_label(kind).to_owned()];
    if let Some(year) = year {
        parts.push(year.to_string());
    }
    let (count, total_ms) = tracks;
    parts.push(track_count_label(count));
    if let Some(runtime) = runtime_label(total_ms) {
        parts.push(runtime);
    }
    parts
}

/// The discography's aside: how many releases and how many tracks it holds.
///
/// Empty when there is no release to count, so an artist with no discography
/// draws no aside rather than `0 releases · 0 tracks`.
pub fn discography_meta(releases: usize, tracks: usize) -> String {
    if releases == 0 {
        return String::new();
    }
    format!(
        "{} · {}",
        release_count_label(releases),
        track_count_label(tracks)
    )
}

/// The album's total length as the reference writes it, or `None` for no total.
///
/// Minutes and then seconds, always both, with the seconds truncated rather than
/// rounded: `38 min 58 sec`. Minutes are never carried into hours, so a long
/// album reads `72 min 5 sec`. A total of exactly zero is the absent total.
pub fn runtime_label(total_ms: u32) -> Option<String> {
    if total_ms == 0 {
        return None;
    }
    let seconds = total_ms / 1_000;
    Some(format!("{} min {} sec", seconds / 60, seconds % 60))
}

/// The album band's plain facts: the year, the count and the total.
///
/// The parts are separate so the painter can place the dot separator between
/// them and leave no trailing dot when a part is absent. The artist is not one
/// of them: it is the band's credit, which may be a link. An unknown total is
/// left out.
pub fn album_meta(year: Option<u16>, tracks: (usize, u32)) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some(year) = year {
        parts.push(year.to_string());
    }
    let (count, total_ms) = tracks;
    parts.push(track_count_label(count));
    if let Some(runtime) = runtime_label(total_ms) {
        parts.push(runtime);
    }
    parts
}

/// The sum of the track lengths, in milliseconds, or zero when any length is
/// unknown.
///
/// The model holds each length as an `Option`: a track that could not be
/// labelled carries none, exactly as it carries no album. A sum over only the
/// lengths that happen to be present is a lower bound, not the runtime, but the
/// byline would read an exact figure with nothing to mark it as a bound; the
/// missing lengths are already blank on their own rows, so a total that hides
/// its gap is worse than no total at all. A list with any unknown length
/// therefore reports zero — the absent total the band leaves out — and a wholly
/// known list reports its honest sum. Saturating addition keeps an absurd list
/// from wrapping.
pub fn total_duration_ms(tracks: &[TrackEntry]) -> u32 {
    if tracks.iter().any(|entry| entry.duration_ms.is_none()) {
        return 0;
    }
    tracks
        .iter()
        .filter_map(|entry| entry.duration_ms)
        .fold(0u32, u32::saturating_add)
}

/// The total runtime as the reference writes it, or `None` when there is no
/// total.
///
/// Past an hour it is `H hr M min`; under one it is `M min`. It rounds to the
/// nearest whole minute, so 59.5 minutes carries into `1 hr 0 min` rather than
/// becoming a 60th minute. A total of exactly zero is the absent total.
pub fn total_label(total_ms: u32) -> Option<String> {
    if total_ms == 0 {
        return None;
    }
    let minutes = (f64::from(total_ms) / 60_000.0).round() as u32;
    if minutes < 60 {
        Some(format!("{minutes} min"))
    } else {
        Some(format!("{} hr {} min", minutes / 60, minutes % 60))
    }
}

/// The hero's byline parts: the song count, then the total when there is one.
///
/// The parts are separate so the painter can place the dot separator between
/// them and leave no trailing dot when the total is absent.
pub fn hero_meta(songs: usize, total_ms: u32) -> Vec<String> {
    let mut parts = vec![count_label(songs)];
    if let Some(total) = total_label(total_ms) {
        parts.push(total);
    }
    parts
}

/// The reference's relative wording for a date, or `None` for a date after
/// `now_ms`.
///
/// `then_ms` is the date in epoch milliseconds and `now_ms` is the app's clock,
/// which reads once per frame and does not tick while idle. A day is a fixed 24
/// hour span rather than a calendar day, so the function has only an instant and
/// a clock, and a local calendar would need a timezone. Months are `days / 30`;
/// past eleven of them the wording moves to `last year` and then to whole years.
/// The 28–59 band is its own arm so the run after three weeks reads as one month
/// instead of falling through to `0 months ago`.
pub fn age_label(then_ms: u64, now_ms: u64) -> Option<String> {
    let days = now_ms.checked_sub(then_ms)? / DAY_MS;
    Some(match days {
        0 => "Today".to_owned(),
        1 => "1 day ago".to_owned(),
        2..=6 => format!("{days} days ago"),
        7..=13 => "1 week ago".to_owned(),
        14..=27 => format!("{} weeks ago", days / 7),
        28..=59 => "1 month ago".to_owned(),
        _ => match days / 30 {
            months @ 2..=11 => format!("{months} months ago"),
            12..=23 => "last year".to_owned(),
            // 24 months is the first value that reaches this arm, so the count
            // is never one.
            months => format!("{} years ago", months / 12),
        },
    })
}

/// The age a playlist row gives its date, through [`age_label`].
///
/// The row test for the shared rule is the existing `added_label` test, which
/// now reads the same branch as the episode row.
pub fn added_label(added: AddedAt, now_ms: u64) -> Option<String> {
    age_label(added.as_millis(), now_ms)
}

/// What a Home card says under its name: what it is and when it was saved.
///
/// A save dated after the clock has no age, so the card carries the kind word
/// alone rather than a broken sentence.
pub fn saved_sub(kind: &str, at: SavedAt, now_ms: u64) -> String {
    library_kind(kind, age_label(at.as_millis(), now_ms).as_deref())
}

/// The Home shelf's title.
pub const HOME_SHELF_TITLE: &str = "Recently added";

/// What the shelf says when things are saved but none of them is recent.
pub const HOME_QUIET: &str = "Nothing recent to show yet.";

/// The day-one page's heading.
pub const HOME_DAY_ONE_TITLE: &str = "Home is built from what you save";

/// The day-one page's one sentence under the heading.
pub const HOME_DAY_ONE_BODY: &str =
    "Save an album, keep a playlist or like a song, and it appears here.";

/// The day-one page's door into the catalogue.
pub const HOME_DAY_ONE_DOOR: &str = "Search the catalogue";

/// The stations shelf's title, which names the seeds rather than a count.
pub const HOME_STATIONS_TITLE: &str = "Stations built from your library";

/// What a station's head says it was built from, under the seed's own name.
///
/// The reference adds the station's length after it; this client drops the
/// count and keeps the sentence that makes the shelf honest about its source.
pub fn station_source(kind: SeedKind) -> &'static str {
    match kind {
        SeedKind::Playlist => "From your playlist",
        SeedKind::Artist => "From an artist you follow",
        SeedKind::Album => "From an album you saved",
        SeedKind::Song => "From the songs you liked",
    }
}

/// The related shelf's title, before it names the artist it follows.
pub const HOME_BECAUSE_TITLE: &str = "Because you follow";

/// The counted shelf's title: the ranking, said without a number.
pub const HOME_COUNTED_TITLE: &str = "The artists you have the most of";

/// The episodes shelf's title.
pub const HOME_EPISODES_TITLE: &str = "New from the shows you follow";

/// The dated shelf's foot: the door into the whole library.
pub const HOME_DOOR_SAVED: &str = "Show all saved";

/// The stations shelf's foot: the reference's own words for it.
pub const HOME_DOOR_LIBRARY: &str = "Your library";

/// The counted shelf's foot: every artist the ranking was drawn from.
pub const HOME_DOOR_ARTISTS: &str = "Every artist in your library";

/// The episodes shelf's foot: the shows the episodes came from.
pub const HOME_DOOR_SHOWS: &str = "All your shows";

/// The because shelf's foot, naming the artist whose neighbours it drew.
pub fn home_door_because(seed: &str) -> String {
    format!("More artists near {seed}")
}

/// An episode count as the show page's head writes it: `1 episode`, `N episodes`.
pub fn episode_count_label(count: usize) -> String {
    count_noun(count, "episode")
}

/// The show page's band byline: the episode count, then the publisher.
///
/// The name is not repeated: the band's title is already the name. The publisher
/// is dropped when it is blank and when it is the show's own name, which the
/// probe's shows do, so the byline never prints the same string twice.
pub fn show_meta(name: &str, publisher: &str, episodes: usize) -> Vec<String> {
    let mut parts = vec![episode_count_label(episodes)];
    let name = name.trim();
    let publisher = publisher.trim();
    if !publisher.is_empty() && publisher != name {
        parts.push(publisher.to_owned());
    }
    parts
}

/// The show page's episode byline, in the two parts the row draws.
///
/// The show's name is a link and the date is not, so they are kept apart rather
/// than joined here. The show is dropped when blank and the age when unknown or
/// in the future, so a row with neither draws nothing and never a trailing dot.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct EpisodeMeta {
    /// The show's name, trimmed, when it has one.
    pub show: Option<String>,
    /// How long ago the episode was published, when known and not in the future.
    pub age: Option<String>,
}

impl EpisodeMeta {
    /// The byline as one line, `Show · 3 days ago`.
    ///
    /// A blank or whitespace-only part is dropped, so the separator only ever
    /// stands between two words. The ghost and the spoken label print the row
    /// whole; the row itself splits the line so only the show's name is a link.
    pub fn joined(&self) -> String {
        [self.show.as_deref(), self.age.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// The show page's episode row byline: the show, then when it was published.
pub fn episode_meta(show: &str, published: Option<PublishedAt>, now_ms: u64) -> EpisodeMeta {
    let show = show.trim();
    EpisodeMeta {
        show: (!show.is_empty()).then(|| show.to_owned()),
        age: published.and_then(|published| age_label(published.as_millis(), now_ms)),
    }
}

/// The root's head note: how many cards the chosen chip shows.
///
/// Each card dates itself, so the note counts them rather than picking one
/// date: `3 saved`.
pub fn library_note(count: usize) -> String {
    format!("{count} saved")
}

/// The note under a playlist when some of its rows could not be named.
///
/// `None` when every row is named, so a healthy list adds no line.
pub fn unnamed_note(entries: &[TrackEntry]) -> Option<String> {
    let unnamed = entries.iter().filter(|entry| !entry.named).count();
    if unnamed == 0 {
        return None;
    }
    Some(format!(
        "{unnamed} of {} could not be named",
        count_noun(entries.len(), "track")
    ))
}

/// The results page's head note: how many tracks the answer holds.
///
/// "Top" is a statement about a cap, so it appears only when the answer reaches
/// `cap`, the most the route returns; a shorter answer has nothing hidden.
pub fn search_note(tracks: usize, cap: usize) -> String {
    if tracks >= cap {
        format!("Top {tracks} tracks")
    } else {
        tracks_phrase(tracks)
    }
}

/// The heading and hint the results page shows when nothing matched.
pub fn search_empty_copy() -> (&'static str, &'static str) {
    (
        "Nothing in the catalogue matches that",
        "Check the spelling, drop the year, or search an artist instead.",
    )
}

/// The sidebar's size readout: the whole library, or what a filter leaves.
pub fn library_count(shown: usize, total: usize) -> String {
    if shown == total {
        format!("{total} saved")
    } else {
        format!("{shown} of {total}")
    }
}

/// The rail's empty line when a query left no row: `Nothing matches “har”`.
///
/// The query is the user's own words, so it keeps its case and is trimmed: a
/// trailing space is not part of what they asked for. The quotes are the
/// reference's curly pair, as the search menu's line uses.
pub fn rail_nothing(query: &str) -> String {
    format!("Nothing matches “{}”", query.trim())
}

/// A filtered playlist's count line: `12 of 62 tracks`.
///
/// The unit follows the total, not the shown count, so a one-track playlist
/// reads `1 of 1 track` while a filter is on.
pub fn filter_count_label(shown: usize, total: usize) -> String {
    let noun = if total == 1 { "track" } else { "tracks" };
    format!("{shown} of {total} {noun}")
}

/// A filtered discography's count line: `3 of 4 releases`.
///
/// The unit follows the total, not the shown count, so a single-release
/// discography reads `1 of 1 release` while a query is on.
pub fn release_filter_count_label(shown: usize, total: usize) -> String {
    let noun = if total == 1 { "release" } else { "releases" };
    format!("{shown} of {total} {noun}")
}

/// A filtered list's sentence when a query leaves no row.
///
/// The query is the user's own words, so it keeps its case and is trimmed. The
/// quotes are the reference's curly pair, as the rail's line uses.
pub fn list_nothing(query: &str) -> String {
    format!("Nothing in this list matches “{}”", query.trim())
}

/// The heading and sentence a format chip shows when it leaves no release.
///
/// The artist's name is part of the sentence because the section is about that
/// artist: "Vela Moreno has no releases of that type."
pub fn format_empty_copy(name: &str) -> (&'static str, String) {
    (
        "Nothing in this format yet",
        format!("{name} has no releases of that type."),
    )
}

/// A library row's second line: what the thing is, and whose it is.
///
/// The reference writes `Playlist · Vela Moreno` for a playlist and the
/// publisher alone for a show; a row with nothing to add carries the kind word
/// by itself rather than a dangling separator.
pub fn library_kind(kind: &str, detail: Option<&str>) -> String {
    match detail.map(str::trim).filter(|detail| !detail.is_empty()) {
        Some(detail) => format!("{kind} · {detail}"),
        None => kind.to_owned(),
    }
}

/// What a playlist row says under its name.
///
/// A playlist that can take an add reads its own track count, because the count
/// is the answer an add gets. Every other row reads its owner's name; our own
/// lists and the wire's numeric ids have none, so the row says the kind alone.
pub fn playlist_detail(entry: &PlaylistEntry) -> Option<String> {
    match entry.add_basis() {
        Some((_, tracks)) => Some(track_count_label(tracks)),
        None => entry
            .owner
            .as_ref()
            .and_then(Owner::name)
            .map(str::to_owned),
    }
}

/// What the sidebar says when one of its writes fails.
pub fn sidebar_notice(write: SidebarWrite) -> &'static str {
    match write {
        SidebarWrite::Reorder => "Could not move that playlist. Try again.",
        SidebarWrite::Add => "Could not add those tracks. Try again.",
        SidebarWrite::Create => "Could not create that playlist. Try again.",
        SidebarWrite::Link => "Created it, but could not add it to your list.",
        SidebarWrite::Unfilled => "Created it, but could not add the tracks.",
        SidebarWrite::Unnamed => "Created it, but could not keep its name.",
        SidebarWrite::Remove => "Could not remove that from your library. Try again.",
        SidebarWrite::Save => "Could not save that to your library. Try again.",
        SidebarWrite::Rename => "Could not rename that playlist. Try again.",
        SidebarWrite::Radio => "Could not start that radio. Try again.",
    }
}

/// What the library says while it draws a copy it could not refresh.
///
/// It is drawn wherever the library's own failure would be said, and it is the
/// only sentence in the window that describes rows rather than a write.
pub fn copy_note() -> &'static str {
    "Showing last time's library. Could not refresh it."
}

/// The root's filter row labels each chip as the reference writes it.
pub fn root_filter_label(kind: RootKind) -> &'static str {
    match kind {
        RootKind::All => "All",
        RootKind::Playlists => "Playlists",
        RootKind::Artists => "Artists",
        RootKind::Albums => "Albums",
        RootKind::Podcasts => "Podcasts",
    }
}

/// The sidebar's kind row labels each chip as the reference writes it.
pub fn sidebar_kind_label(kind: SidebarKind) -> &'static str {
    match kind {
        SidebarKind::Playlists => "Playlists",
        SidebarKind::Artists => "Artists",
        SidebarKind::Albums => "Albums",
        SidebarKind::Podcasts => "Podcasts",
    }
}

/// The message the root draws when the chosen chip shows nothing.
///
/// All and Playlists show playlists, Podcasts shows shows, and the two waiting
/// chips show nothing yet, so each gets a message that fits what it would draw.
pub fn root_empty_label(kind: RootKind) -> &'static str {
    match kind {
        RootKind::All | RootKind::Playlists => "No playlists yet.",
        RootKind::Podcasts => "You don't follow any shows yet.",
        RootKind::Artists => "You don't follow any artists yet.",
        RootKind::Albums => "You haven't saved any albums yet.",
    }
}

/// A library item's Type cell: the one word the reference puts there.
pub fn media_type(kind: RootKind) -> &'static str {
    match kind {
        RootKind::All | RootKind::Playlists => "Playlist",
        RootKind::Artists => "Artist",
        RootKind::Albums => "Album",
        RootKind::Podcasts => "Podcast",
    }
}

/// A library item's Size cell, in the reference's own words.
///
/// The unit follows the kind: a playlist counts songs, an album tracks, a show
/// episodes. An artist has no count here — the reference puts a track total in
/// that cell and this client holds no such number for an artist — and a count
/// the wire did not name is an empty cell rather than a zero.
pub fn media_size(kind: RootKind, count: Option<usize>) -> String {
    let Some(count) = count else {
        return String::new();
    };
    match kind {
        RootKind::All | RootKind::Playlists => songs(count),
        RootKind::Albums => tracks(count),
        RootKind::Podcasts => episodes(count),
        RootKind::Artists => String::new(),
    }
}

fn songs(count: usize) -> String {
    if count == 1 {
        "1 song".to_owned()
    } else {
        format!("{count} songs")
    }
}

fn tracks(count: usize) -> String {
    if count == 1 {
        "1 track".to_owned()
    } else {
        format!("{count} tracks")
    }
}

fn episodes(count: usize) -> String {
    if count == 1 {
        "1 ep".to_owned()
    } else {
        format!("{count} eps")
    }
}

/// What the root says under its cards while a chip's list is being fetched.
pub fn root_loading_label(kind: RootKind) -> &'static str {
    match kind {
        RootKind::All | RootKind::Playlists => "Loading your playlists…",
        RootKind::Podcasts => "Loading your shows…",
        RootKind::Artists => "Loading your artists…",
        RootKind::Albums => "Loading your albums…",
    }
}

/// The band's byline parts: the owner, then the count and the total.
///
/// Our own list reads `Made by you`; another account is named. A blank or
/// numeric owner never reaches here, because the model refuses to build one.
pub fn byline(owner: Option<&Owner>, tracks: Option<(usize, u32)>) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some(owner) = owner {
        parts.push(match owner.name() {
            Some(name) => format!("Made by {name}"),
            None => "Made by you".to_owned(),
        });
    }
    if let Some((songs, total_ms)) = tracks {
        parts.extend(hero_meta(songs, total_ms));
    }
    parts
}

/// The playlist's description as one line: whitespace collapsed and trimmed.
///
/// `None` when nothing is left, so a blank description draws no line at all.
/// This collapses whitespace and nothing else: markup in the wire string — HTML
/// entities such as `&#x27;` or `&amp;`, and `<a href="spotify:…">` tags — is
/// passed through unchanged and draws literally. The name promises no more than
/// that.
pub fn plain_description(raw: &str) -> Option<String> {
    let line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() { None } else { Some(line) }
}

#[cfg(test)]
mod tests;
