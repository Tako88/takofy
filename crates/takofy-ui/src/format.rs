//! The reference's wording for the words and numbers the shell prints: the
//! playbar's clock, the hero's byline, the playlist band's description, and the
//! age a playlist row gives its date.

use takofy_model::{AddedAt, Owner, PlaylistEntry, PublishedAt, ReleaseKind, TrackEntry};

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

/// The album band's byline parts: the artists, the year, the count and the total.
///
/// The parts are separate so the painter can place the dot separator between
/// them and leave no trailing dot when a part is absent. A blank artist line is
/// dropped rather than drawn as an empty part, and an unknown total is left out.
pub fn album_meta(artists: &str, year: Option<u16>, tracks: (usize, u32)) -> Vec<String> {
    let mut parts = Vec::new();
    let artists = artists.trim();
    if !artists.is_empty() {
        parts.push(artists.to_owned());
    }
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

/// The show page's episode row meta: the show, then when it was published.
///
/// `Show · 3 days ago`. The show part is dropped when blank and the age part
/// when unknown or in the future, so a row with neither draws nothing and the
/// painter draws no trailing dot.
pub fn episode_meta(show: &str, published: Option<PublishedAt>, now_ms: u64) -> Vec<String> {
    let mut parts = Vec::new();
    let show = show.trim();
    if !show.is_empty() {
        parts.push(show.to_owned());
    }
    if let Some(published) = published
        && let Some(age) = age_label(published.as_millis(), now_ms)
    {
        parts.push(age);
    }
    parts
}

/// The root's head note: how many cards the chosen chip shows.
///
/// Only the shows the user follows carry an added date, and a rootlist playlist
/// carries none, so the note counts cards rather than dating any of them: `3
/// saved`.
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
    }
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
mod tests {
    use super::*;

    use takofy_model::{Owner, OwnerName, PlaylistAccess, Revision};

    #[test]
    fn the_search_note_says_top_only_when_the_answer_reaches_the_cap() {
        let note = |tracks| search_note(tracks, 20);
        assert_eq!(note(0), "0 tracks");
        assert_eq!(note(1), "1 track");
        assert_eq!(note(7), "7 tracks");
        assert_eq!(note(19), "19 tracks");
        assert_eq!(note(20), "Top 20 tracks");
        assert_eq!(note(25), "Top 25 tracks");
    }

    #[test]
    fn the_empty_copy_is_the_references_words() {
        assert_eq!(
            search_empty_copy(),
            (
                "Nothing in the catalogue matches that",
                "Check the spelling, drop the year, or search an artist instead."
            )
        );
    }

    fn entry(
        owner: Option<&str>,
        tracks: Option<usize>,
        access: Option<PlaylistAccess>,
    ) -> PlaylistEntry {
        PlaylistEntry {
            composed: Vec::new(),
            uri: "spotify:playlist:a".to_owned(),
            name: "A".to_owned(),
            named: true,
            cover: None,
            description: None,
            tracks,
            owner: owner.and_then(OwnerName::from_wire).map(Owner::Named),
            access,
        }
    }

    fn named(name: &str) -> Owner {
        Owner::Named(OwnerName::from_wire(name).expect("a name"))
    }

    fn editable(editable: bool) -> Option<PlaylistAccess> {
        Some(PlaylistAccess {
            revision: Revision::from_wire(&[1]).expect("a revision"),
            editable,
        })
    }

    #[test]
    fn an_addable_playlist_reads_its_track_count() {
        assert_eq!(
            playlist_detail(&entry(Some("Vela Moreno"), Some(12), editable(true))),
            Some("12 tracks".to_owned())
        );
        assert_eq!(
            playlist_detail(&entry(None, Some(0), editable(true))),
            Some("0 tracks".to_owned()),
            "an empty playlist still says how empty"
        );
    }

    #[test]
    fn one_track_is_singular() {
        assert_eq!(
            playlist_detail(&entry(Some("Vela Moreno"), Some(1), editable(true))),
            Some("1 track".to_owned())
        );
    }

    #[test]
    fn a_playlist_that_cannot_be_edited_keeps_its_owner() {
        assert_eq!(
            playlist_detail(&entry(Some("Vela Moreno"), Some(12), editable(false))),
            Some("Vela Moreno".to_owned())
        );
        assert_eq!(
            playlist_detail(&entry(None, Some(12), editable(false))),
            None
        );
    }

    #[test]
    fn an_editable_playlist_with_no_count_keeps_its_owner() {
        assert_eq!(
            playlist_detail(&entry(Some("Vela Moreno"), None, editable(true))),
            Some("Vela Moreno".to_owned())
        );
    }

    #[test]
    fn a_playlist_of_ours_names_no_owner_under_its_name() {
        let mut ours = entry(None, Some(12), editable(false));
        ours.owner = Some(Owner::You);
        assert_eq!(
            playlist_detail(&ours),
            None,
            "our own list draws no name under its title"
        );
    }

    #[test]
    fn a_numeric_or_missing_owner_names_no_owner_under_its_name() {
        let mut numeric = entry(None, Some(12), editable(false));
        numeric.owner = OwnerName::from_wire("31415926535").map(Owner::Named);
        assert_eq!(numeric.owner, None, "a numeric id is not an owner");
        assert_eq!(playlist_detail(&numeric), None);
        assert_eq!(
            playlist_detail(&entry(None, Some(12), editable(false))),
            None
        );
    }

    #[test]
    fn the_notices_name_what_could_not_be_done() {
        assert_eq!(
            sidebar_notice(SidebarWrite::Reorder),
            "Could not move that playlist. Try again."
        );
        assert_eq!(
            sidebar_notice(SidebarWrite::Add),
            "Could not add those tracks. Try again."
        );
        assert_eq!(
            sidebar_notice(SidebarWrite::Unfilled),
            "Created it, but could not add the tracks."
        );
        assert_eq!(
            sidebar_notice(SidebarWrite::Unnamed),
            "Created it, but could not keep its name."
        );
        assert_eq!(
            sidebar_notice(SidebarWrite::Rename),
            "Could not rename that playlist. Try again."
        );
        assert_eq!(
            sidebar_notice(SidebarWrite::Remove),
            "Could not remove that from your library. Try again."
        );
    }

    #[test]
    fn sidebar_notice_words_for_create_and_link() {
        assert_eq!(
            sidebar_notice(SidebarWrite::Create),
            "Could not create that playlist. Try again."
        );
        assert_eq!(
            sidebar_notice(SidebarWrite::Link),
            "Created it, but could not add it to your list.",
            "the playlist exists, so the sentence says what is missing"
        );
        assert_ne!(
            sidebar_notice(SidebarWrite::Create),
            sidebar_notice(SidebarWrite::Link),
            "the two failures are not the same failure"
        );
    }

    #[test]
    fn release_count_label_counts_releases() {
        assert_eq!(release_count_label(1), "1 release");
        assert_eq!(release_count_label(0), "0 releases");
        assert_eq!(release_count_label(3), "3 releases");
    }

    #[test]
    fn release_kind_label_names_every_kind() {
        assert_eq!(release_kind_label(ReleaseKind::Album), "Album");
        assert_eq!(release_kind_label(ReleaseKind::Single), "Single");
        assert_eq!(release_kind_label(ReleaseKind::Compilation), "Compilation");
        assert_eq!(release_kind_label(ReleaseKind::Ep), "EP");
        assert_eq!(release_kind_label(ReleaseKind::Other), "Release");
    }

    #[test]
    fn filter_labels_are_plural_and_match_the_reference_words() {
        assert_eq!(filter_label(Chip::All), "All");
        assert_eq!(filter_label(Chip::Kind(ReleaseKind::Album)), "Albums");
        assert_eq!(filter_label(Chip::Kind(ReleaseKind::Ep)), "EPs");
        assert_eq!(filter_label(Chip::Kind(ReleaseKind::Single)), "Singles");
        assert_eq!(
            filter_label(Chip::Kind(ReleaseKind::Compilation)),
            "Compilations"
        );
    }

    #[test]
    fn release_meta_reads_kind_year_count_and_runtime() {
        assert_eq!(
            release_meta(ReleaseKind::Album, Some(2024), (11, 2_338_000)),
            vec![
                "Album".to_owned(),
                "2024".to_owned(),
                "11 tracks".to_owned(),
                "38 min 58 sec".to_owned(),
            ]
        );
    }

    #[test]
    fn release_meta_drops_a_blank_year_and_a_zero_total() {
        assert_eq!(
            release_meta(ReleaseKind::Single, None, (1, 0)),
            vec!["Single".to_owned(), "1 track".to_owned()]
        );
    }

    #[test]
    fn discography_meta_counts_releases_and_tracks() {
        assert_eq!(discography_meta(3, 24), "3 releases · 24 tracks");
        assert_eq!(discography_meta(1, 1), "1 release · 1 track");
    }

    #[test]
    fn discography_meta_is_empty_without_releases() {
        assert_eq!(discography_meta(0, 0), "");
    }

    /// A fixed clock: 2023-11-14T22:13:20Z.
    const NOW: u64 = 1_700_000_000_000;
    const D: u64 = 86_400_000;

    fn at(millis: u64) -> AddedAt {
        AddedAt::from_wire(&millis.to_string()).expect("a date above the floor")
    }

    /// The label for a track `age` milliseconds old at `NOW`.
    fn aged(age: u64) -> Option<String> {
        added_label(at(NOW - age), NOW)
    }

    #[test]
    fn added_label_under_24_hours_is_today() {
        assert_eq!(aged(0).as_deref(), Some("Today"));
        assert_eq!(aged(86_399_999).as_deref(), Some("Today"));
    }

    #[test]
    fn added_label_one_day_is_singular() {
        assert_eq!(aged(D).as_deref(), Some("1 day ago"));
        assert_eq!(aged(2 * D - 1).as_deref(), Some("1 day ago"));
    }

    #[test]
    fn added_label_two_to_six_days_counts_days() {
        assert_eq!(aged(2 * D).as_deref(), Some("2 days ago"));
        assert_eq!(aged(6 * D + 86_399_999).as_deref(), Some("6 days ago"));
    }

    #[test]
    fn added_label_weeks_cover_7_to_27_days() {
        assert_eq!(aged(7 * D).as_deref(), Some("1 week ago"));
        assert_eq!(aged(13 * D).as_deref(), Some("1 week ago"));
        assert_eq!(aged(14 * D).as_deref(), Some("2 weeks ago"));
        assert_eq!(aged(20 * D).as_deref(), Some("2 weeks ago"));
        assert_eq!(aged(21 * D).as_deref(), Some("3 weeks ago"));
        assert_eq!(aged(27 * D).as_deref(), Some("3 weeks ago"));
    }

    #[test]
    fn added_label_28_to_59_days_is_one_month() {
        assert_eq!(aged(28 * D - 1).as_deref(), Some("3 weeks ago"));
        assert_eq!(aged(28 * D).as_deref(), Some("1 month ago"));
        assert_eq!(aged(59 * D).as_deref(), Some("1 month ago"));
    }

    #[test]
    fn added_label_months_run_to_eleven() {
        assert_eq!(aged(60 * D).as_deref(), Some("2 months ago"));
        assert_eq!(aged(89 * D).as_deref(), Some("2 months ago"));
        assert_eq!(aged(90 * D).as_deref(), Some("3 months ago"));
        assert_eq!(aged(330 * D).as_deref(), Some("11 months ago"));
        assert_eq!(aged(359 * D).as_deref(), Some("11 months ago"));
    }

    #[test]
    fn added_label_12_to_23_months_is_last_year() {
        assert_eq!(aged(360 * D).as_deref(), Some("last year"));
        assert_eq!(aged(510 * D).as_deref(), Some("last year"));
        assert_eq!(aged(719 * D).as_deref(), Some("last year"));
    }

    #[test]
    fn added_label_24_months_is_two_years() {
        assert_eq!(aged(720 * D).as_deref(), Some("2 years ago"));
    }

    #[test]
    fn added_label_years_count_whole_years() {
        assert_eq!(aged(1080 * D).as_deref(), Some("3 years ago"));
        assert_eq!(aged(1439 * D).as_deref(), Some("3 years ago"));
        assert_eq!(aged(1440 * D).as_deref(), Some("4 years ago"));
        assert_eq!(aged(4_380 * D).as_deref(), Some("12 years ago"));
    }

    #[test]
    fn added_label_in_the_future_is_none() {
        assert_eq!(added_label(at(NOW + 1), NOW), None);
        assert_eq!(added_label(at(NOW + 365 * D), NOW), None);
        assert_eq!(added_label(at(NOW), NOW).as_deref(), Some("Today"));
    }

    #[test]
    fn added_label_extremes_do_not_overflow() {
        assert_eq!(added_label(at(u64::MAX), 0), None);
        assert_eq!(
            added_label(at(1_000_000_000_000), u64::MAX).as_deref(),
            Some("593066585 years ago")
        );
    }

    #[test]
    fn format_remaining_counts_down_and_saturates() {
        assert_eq!(format_remaining(74_000, 222_000), "-2:28");
        assert_eq!(format_remaining(0, 222_000), "-3:42");
        assert_eq!(format_remaining(222_000, 222_000), "-0:00");
        assert_eq!(format_remaining(300_000, 222_000), "-0:00");
        assert_eq!(
            format_remaining(73_250, 222_000),
            "-2:28",
            "sub-second truncates"
        );
    }

    #[test]
    fn a_position_formats_as_minutes_and_seconds() {
        assert_eq!(format_position(0), "0:00");
        assert_eq!(format_position(5_000), "0:05");
        assert_eq!(format_position(65_000), "1:05");
        assert_eq!(format_position(3_600_000), "60:00");
    }

    #[test]
    fn count_label_is_singular_only_for_one() {
        assert_eq!(count_label(1), "1 song");
        assert_eq!(count_label(0), "0 songs");
        assert_eq!(count_label(2), "2 songs");
        assert_eq!(count_label(62), "62 songs");
    }

    fn track(uri: &str, duration_ms: Option<u32>) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms,
            named: true,
            cover: None,
        }
    }

    fn unnamed(uri: &str) -> TrackEntry {
        TrackEntry {
            named: false,
            ..track(uri, None)
        }
    }

    #[test]
    fn unnamed_note_is_none_when_every_row_is_named() {
        let entries = [track("a", None), track("b", None)];
        assert_eq!(unnamed_note(&entries), None);
    }

    #[test]
    fn unnamed_note_is_none_for_an_empty_list() {
        assert_eq!(unnamed_note(&[]), None);
    }

    #[test]
    fn unnamed_note_counts_unnamed_rows_against_the_whole_list() {
        let mut entries: Vec<TrackEntry> = (0..10)
            .map(|index| track(&format!("t{index}"), None))
            .collect();
        entries[1].named = false;
        entries[4].named = false;
        entries[9].named = false;
        assert_eq!(
            unnamed_note(&entries).as_deref(),
            Some("3 of 10 tracks could not be named")
        );
    }

    #[test]
    fn unnamed_note_says_track_for_a_list_of_one() {
        assert_eq!(
            unnamed_note(&[unnamed("a")]).as_deref(),
            Some("1 of 1 track could not be named")
        );
    }

    #[test]
    fn unnamed_note_counts_a_repeated_unnamed_uri_per_position() {
        let entries = [unnamed("a"), unnamed("a")];
        assert_eq!(
            unnamed_note(&entries).as_deref(),
            Some("2 of 2 tracks could not be named")
        );
    }

    #[test]
    fn count_noun_is_singular_only_for_one() {
        assert_eq!(count_noun(1, "song"), "1 song");
        assert_eq!(count_noun(0, "song"), "0 songs");
        assert_eq!(count_noun(2, "song"), "2 songs");
    }

    #[test]
    fn track_count_label_counts_tracks() {
        assert_eq!(track_count_label(1), "1 track");
        assert_eq!(track_count_label(0), "0 tracks");
        assert_eq!(track_count_label(12), "12 tracks");
    }

    #[test]
    fn runtime_label_reads_minutes_and_seconds_the_reference_way() {
        assert_eq!(runtime_label(2_338_000).as_deref(), Some("38 min 58 sec"));
        // Minutes are never carried into hours, so 72 minutes stays minutes.
        assert_eq!(runtime_label(4_325_000).as_deref(), Some("72 min 5 sec"));
        assert_eq!(runtime_label(1_000).as_deref(), Some("0 min 1 sec"));
    }

    #[test]
    fn runtime_label_truncates_whole_seconds() {
        assert_eq!(runtime_label(2_338_999).as_deref(), Some("38 min 58 sec"));
        assert_eq!(runtime_label(1_999).as_deref(), Some("0 min 1 sec"));
    }

    #[test]
    fn runtime_label_is_none_for_an_absent_total() {
        assert_eq!(runtime_label(0), None);
    }

    #[test]
    fn album_meta_reads_artist_year_count_and_runtime() {
        assert_eq!(
            album_meta("Band", Some(1997), (12, 2_338_000)),
            vec![
                "Band".to_owned(),
                "1997".to_owned(),
                "12 tracks".to_owned(),
                "38 min 58 sec".to_owned(),
            ]
        );
    }

    #[test]
    fn album_meta_drops_a_blank_artist_and_an_absent_year() {
        assert_eq!(
            album_meta("  ", None, (1, 0)),
            vec!["1 track".to_owned()],
            "a blank artist and a zero runtime are both absent"
        );
        assert_eq!(
            album_meta("Band", None, (2, 0)),
            vec!["Band".to_owned(), "2 tracks".to_owned()]
        );
    }

    #[test]
    fn the_total_is_withheld_when_any_length_is_unknown() {
        assert_eq!(total_duration_ms(&[]), 0, "no tracks is the absent total");
        assert_eq!(
            total_duration_ms(&[track("a", Some(1_000)), track("b", Some(2_000))]),
            3_000
        );
        assert_eq!(
            total_duration_ms(&[track("a", Some(1_000)), track("b", None)]),
            0,
            "one unknown length withholds the whole total"
        );
    }

    #[test]
    fn the_total_saturates_instead_of_wrapping() {
        assert_eq!(
            total_duration_ms(&[track("a", Some(u32::MAX)), track("b", Some(u32::MAX))]),
            u32::MAX,
            "two maximal lengths saturate rather than wrap"
        );
    }

    #[test]
    fn total_label_reads_hours_and_minutes_past_an_hour() {
        assert_eq!(total_label(13_860_000).as_deref(), Some("3 hr 51 min"));
    }

    #[test]
    fn total_label_reads_minutes_only_under_an_hour() {
        assert_eq!(total_label(2_700_000).as_deref(), Some("45 min"));
    }

    #[test]
    fn total_label_is_none_for_an_absent_total() {
        assert_eq!(total_label(0), None);
    }

    #[test]
    fn total_label_rounds_to_the_nearest_minute_and_carries_into_the_hour() {
        // 59.5 minutes rounds up to the hour, not to a 60th minute.
        assert_eq!(total_label(3_570_000).as_deref(), Some("1 hr 0 min"));
        // 59.4 minutes stays in the minute form.
        assert_eq!(total_label(3_564_000).as_deref(), Some("59 min"));
    }

    #[test]
    fn hero_meta_omits_the_total_when_there_is_none() {
        // One part means the painter draws no trailing dot.
        assert_eq!(hero_meta(62, 0), vec!["62 songs".to_owned()]);
        assert_eq!(hero_meta(1, 0), vec!["1 song".to_owned()]);
    }

    #[test]
    fn hero_meta_puts_the_count_before_the_total() {
        assert_eq!(
            hero_meta(62, 13_860_000),
            vec!["62 songs".to_owned(), "3 hr 51 min".to_owned()]
        );
    }

    #[test]
    fn byline_says_made_by_you_for_our_own() {
        assert_eq!(
            byline(Some(&Owner::You), Some((62, 13_860_000))),
            vec![
                "Made by you".to_owned(),
                "62 songs".to_owned(),
                "3 hr 51 min".to_owned()
            ]
        );
    }

    #[test]
    fn byline_names_another_owner() {
        assert_eq!(
            byline(Some(&named("alice")), Some((62, 13_860_000))),
            vec![
                "Made by alice".to_owned(),
                "62 songs".to_owned(),
                "3 hr 51 min".to_owned()
            ]
        );
    }

    #[test]
    fn byline_draws_the_count_alone_without_an_owner() {
        assert_eq!(byline(None, Some((1, 0))), vec!["1 song".to_owned()]);
        assert_eq!(byline(None, Some((0, 0))), vec!["0 songs".to_owned()]);
    }

    #[test]
    fn byline_is_empty_without_an_owner_or_tracks() {
        assert!(byline(None, None).is_empty());
    }

    #[test]
    fn byline_keeps_the_owner_while_the_tracks_are_unknown() {
        assert_eq!(
            byline(Some(&named("alice")), None),
            vec!["Made by alice".to_owned()]
        );
    }

    #[test]
    fn plain_description_collapses_whitespace_and_drops_a_blank() {
        assert_eq!(plain_description(""), None);
        assert_eq!(plain_description("  \n "), None);
        assert_eq!(plain_description("a\n\nb   c").as_deref(), Some("a b c"));
        assert_eq!(
            plain_description("  a single line  ").as_deref(),
            Some("a single line")
        );
    }

    /// A fixed clock: 2023-11-14T22:13:20Z.
    const NOW_MS: u64 = 1_700_000_000_000;
    const DAY: u64 = 86_400_000;

    #[test]
    fn age_label_today_days_and_future() {
        // The existing added_label tests read this function through added_label;
        // these pin the seam directly.
        assert_eq!(age_label(NOW_MS, NOW_MS).as_deref(), Some("Today"));
        assert_eq!(
            age_label(NOW_MS - DAY, NOW_MS).as_deref(),
            Some("1 day ago")
        );
        assert_eq!(
            age_label(NOW_MS - 2 * DAY, NOW_MS).as_deref(),
            Some("2 days ago")
        );
        assert_eq!(
            age_label(NOW_MS + 1, NOW_MS),
            None,
            "a date after now draws nothing"
        );
    }

    #[test]
    fn episode_count_label_singular_only_for_one() {
        assert_eq!(episode_count_label(1), "1 episode");
        assert_eq!(episode_count_label(0), "0 episodes");
        assert_eq!(episode_count_label(211), "211 episodes");
    }

    #[test]
    fn show_meta_drops_a_blank_publisher() {
        assert_eq!(
            show_meta("Lateral with Tom Scott", "", 211),
            vec!["211 episodes".to_owned()],
            "the band's title already carries the name"
        );
    }

    #[test]
    fn show_meta_drops_a_publisher_equal_to_the_name() {
        assert_eq!(
            show_meta("Lateral", "Lateral", 5),
            vec!["5 episodes".to_owned()],
            "a publisher that is the show's own name prints once, in the title"
        );
    }

    fn published(millis: u64) -> Option<PublishedAt> {
        PublishedAt::from_millis(millis as i64)
    }

    #[test]
    fn an_episode_meta_ages_into_last_year() {
        assert_eq!(
            episode_meta("Lateral", published(NOW_MS - 510 * DAY), NOW_MS),
            vec!["Lateral".to_owned(), "last year".to_owned()],
            "the show row reads the same tail as the added column"
        );
    }

    #[test]
    fn episode_meta_omits_missing_parts() {
        assert_eq!(
            episode_meta("Lateral", published(NOW_MS - 3 * DAY), NOW_MS),
            vec!["Lateral".to_owned(), "3 days ago".to_owned()]
        );
        assert!(
            episode_meta("", None, NOW_MS).is_empty(),
            "neither part draws"
        );
        assert_eq!(
            episode_meta("Lateral", published(NOW_MS + DAY), NOW_MS),
            vec!["Lateral".to_owned()],
            "a future age is omitted"
        );
    }

    #[test]
    fn library_counts() {
        assert_eq!(library_count(0, 0), "0 saved");
        assert_eq!(library_count(28, 28), "28 saved");
        assert_eq!(library_count(3, 28), "3 of 28");
    }

    #[test]
    fn nothing_matches_names_the_trimmed_query() {
        assert_eq!(rail_nothing("har"), "Nothing matches “har”");
        assert_eq!(rail_nothing("  har  "), "Nothing matches “har”");
    }

    #[test]
    fn the_quote_marks_are_the_curly_pair() {
        let line = rail_nothing("har");

        assert!(line.contains('“') && line.contains('”'), "{line}");
        assert!(
            !line.contains('"'),
            "straight quotes are not the reference's"
        );
    }

    #[test]
    fn the_query_keeps_its_case() {
        assert_eq!(
            rail_nothing("Midnight City"),
            "Nothing matches “Midnight City”"
        );
    }

    #[test]
    fn release_filter_count_labels() {
        assert_eq!(release_filter_count_label(3, 4), "3 of 4 releases");
        assert_eq!(release_filter_count_label(1, 1), "1 of 1 release");
        assert_eq!(release_filter_count_label(0, 4), "0 of 4 releases");
        assert_eq!(release_filter_count_label(1, 4), "1 of 4 releases");
    }

    #[test]
    fn list_nothing_names_the_trimmed_query() {
        assert_eq!(
            list_nothing("  Midnight City  "),
            "Nothing in this list matches “Midnight City”"
        );
        let line = list_nothing("har");
        assert!(line.contains('“') && line.contains('”'), "{line}");
        assert!(
            !line.contains('"'),
            "straight quotes are not the reference's"
        );
    }

    #[test]
    fn format_empty_copy_matches_the_reference() {
        assert_eq!(
            format_empty_copy("Vela Moreno"),
            (
                "Nothing in this format yet",
                "Vela Moreno has no releases of that type.".to_owned()
            )
        );
    }

    #[test]
    fn filter_count_labels() {
        assert_eq!(filter_count_label(12, 62), "12 of 62 tracks");
        assert_eq!(filter_count_label(1, 1), "1 of 1 track");
        assert_eq!(filter_count_label(0, 62), "0 of 62 tracks");
        assert_eq!(filter_count_label(1, 2), "1 of 2 tracks");
    }

    #[test]
    fn library_kinds() {
        assert_eq!(
            library_kind("Playlist", Some("Vela Moreno")),
            "Playlist · Vela Moreno"
        );
        assert_eq!(library_kind("Podcast", None), "Podcast");
        assert_eq!(library_kind("Playlist", Some("   ")), "Playlist");
        assert_eq!(library_kind("Album", Some(" Hana Sō ")), "Album · Hana Sō");
    }

    #[test]
    fn library_note_counts() {
        assert_eq!(library_note(0), "0 saved");
        assert_eq!(library_note(1), "1 saved");
        assert_eq!(library_note(8), "8 saved");
    }

    #[test]
    fn root_filter_labels_match_the_reference() {
        assert_eq!(root_filter_label(RootKind::All), "All");
        assert_eq!(root_filter_label(RootKind::Playlists), "Playlists");
        assert_eq!(root_filter_label(RootKind::Artists), "Artists");
        assert_eq!(root_filter_label(RootKind::Albums), "Albums");
        assert_eq!(root_filter_label(RootKind::Podcasts), "Podcasts");
    }

    #[test]
    fn sidebar_kind_labels_match_the_reference() {
        assert_eq!(sidebar_kind_label(SidebarKind::Playlists), "Playlists");
        assert_eq!(sidebar_kind_label(SidebarKind::Artists), "Artists");
        assert_eq!(sidebar_kind_label(SidebarKind::Albums), "Albums");
        assert_eq!(sidebar_kind_label(SidebarKind::Podcasts), "Podcasts");
    }

    #[test]
    fn root_empty_labels_per_chip() {
        assert_eq!(root_empty_label(RootKind::All), "No playlists yet.");
        assert_eq!(root_empty_label(RootKind::Playlists), "No playlists yet.");
        assert_eq!(
            root_empty_label(RootKind::Podcasts),
            "You don't follow any shows yet."
        );
        assert_eq!(
            root_empty_label(RootKind::Artists),
            "You don't follow any artists yet."
        );
        assert_eq!(
            root_empty_label(RootKind::Albums),
            "You haven't saved any albums yet."
        );
    }

    #[test]
    fn the_type_cell_is_one_word_per_kind() {
        assert_eq!(media_type(RootKind::All), "Playlist");
        assert_eq!(media_type(RootKind::Playlists), "Playlist");
        assert_eq!(media_type(RootKind::Artists), "Artist");
        assert_eq!(media_type(RootKind::Albums), "Album");
        assert_eq!(media_type(RootKind::Podcasts), "Podcast");
    }

    #[test]
    fn the_size_cell_counts_in_the_kind_own_unit() {
        assert_eq!(media_size(RootKind::Playlists, Some(62)), "62 songs");
        assert_eq!(media_size(RootKind::Albums, Some(11)), "11 tracks");
        assert_eq!(media_size(RootKind::Podcasts, Some(128)), "128 eps");
        assert_eq!(media_size(RootKind::Playlists, Some(1)), "1 song");
        assert_eq!(media_size(RootKind::Albums, Some(1)), "1 track");
        assert_eq!(media_size(RootKind::Podcasts, Some(1)), "1 ep");
        assert_eq!(
            media_size(RootKind::Playlists, Some(0)),
            "0 songs",
            "an empty playlist counts zero rather than showing nothing"
        );
    }

    #[test]
    fn the_size_cell_is_empty_when_the_count_is_missing_or_the_kind_has_none() {
        assert_eq!(media_size(RootKind::Playlists, None), "");
        assert_eq!(media_size(RootKind::Albums, None), "");
        assert_eq!(media_size(RootKind::Podcasts, None), "");
        assert_eq!(
            media_size(RootKind::Artists, Some(12)),
            "",
            "a release count is not the track count the reference draws"
        );
    }

    #[test]
    fn every_chip_says_what_it_is_loading() {
        for kind in crate::library_filter::ROW {
            assert!(
                root_loading_label(kind).starts_with("Loading your "),
                "{kind:?}"
            );
        }
        assert_eq!(
            root_loading_label(RootKind::Artists),
            "Loading your artists…"
        );
        assert_eq!(root_loading_label(RootKind::Albums), "Loading your albums…");
    }
}
