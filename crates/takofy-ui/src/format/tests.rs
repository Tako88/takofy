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
        saved_at: None,
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
fn album_meta_reads_year_count_and_runtime() {
    assert_eq!(
        album_meta(Some(1997), (12, 2_338_000)),
        vec![
            "1997".to_owned(),
            "12 tracks".to_owned(),
            "38 min 58 sec".to_owned(),
        ]
    );
}

#[test]
fn album_meta_drops_an_absent_year() {
    assert_eq!(
        album_meta(None, (2, 0)),
        vec!["2 tracks".to_owned()],
        "an absent year and a zero runtime are both left out"
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
fn an_episode_byline_ages_into_last_year() {
    let meta = episode_meta("Lateral", published(NOW_MS - 510 * DAY), NOW_MS);
    assert_eq!(meta.show.as_deref(), Some("Lateral"));
    assert_eq!(
        meta.age.as_deref(),
        Some("last year"),
        "the show row reads the same tail as the added column"
    );
    assert_eq!(meta.joined(), "Lateral · last year");
}

#[test]
fn an_episode_byline_omits_missing_parts() {
    let meta = episode_meta("Lateral", published(NOW_MS - 3 * DAY), NOW_MS);
    assert_eq!(meta.joined(), "Lateral · 3 days ago");

    let none = episode_meta("", None, NOW_MS);
    assert_eq!(none, EpisodeMeta::default(), "neither part draws");
    assert_eq!(none.joined(), "");

    let future = episode_meta("Lateral", published(NOW_MS + DAY), NOW_MS);
    assert_eq!(future.age, None, "a future age is omitted");
    assert_eq!(
        future.joined(),
        "Lateral",
        "a dropped age leaves no trailing dot"
    );
}

#[test]
fn a_blank_show_is_none() {
    for blank in ["", "   ", "\t"] {
        let meta = episode_meta(blank, published(NOW_MS - 3 * DAY), NOW_MS);
        assert_eq!(meta.show, None, "{blank:?} names no show");
        assert_eq!(meta.joined(), "3 days ago");
    }
}

#[test]
fn a_joined_byline_drops_blank_parts() {
    // The join takes its two words as given, so a blank one must not leave
    // the separator behind.
    let blank_show = EpisodeMeta {
        show: Some("   ".to_owned()),
        age: Some("last year".to_owned()),
    };
    assert_eq!(blank_show.joined(), "last year");

    let blank_age = EpisodeMeta {
        show: Some("Lateral".to_owned()),
        age: Some(String::new()),
    };
    assert_eq!(blank_age.joined(), "Lateral");
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

#[test]
fn a_home_card_says_what_it_is_and_when_it_was_saved() {
    let at = SavedAt::from_millis(1_700_000_000_000).expect("a date");
    assert_eq!(
        saved_sub("Playlist", at, 1_700_000_000_000 + 3_600_000),
        "Playlist · Today"
    );
    assert_eq!(
        saved_sub("Album", at, 1_700_000_000_000 + 8 * 86_400_000),
        "Album · 1 week ago"
    );
    assert_eq!(
        saved_sub("Playlist", at, 1_699_000_000_000),
        "Playlist",
        "a date after the clock has no age to name"
    );
}

/// Every shelf title names what the shelf was built from, and none prints a
/// number it would have to explain.
#[test]
fn the_home_section_titles_count_nothing() {
    assert_eq!(HOME_STATIONS_TITLE, "Stations built from your library");
    assert_eq!(HOME_BECAUSE_TITLE, "Because you follow");
    assert_eq!(HOME_COUNTED_TITLE, "The artists you have the most of");
    assert_eq!(HOME_EPISODES_TITLE, "New from the shows you follow");
    for title in [
        HOME_STATIONS_TITLE,
        HOME_BECAUSE_TITLE,
        HOME_COUNTED_TITLE,
        HOME_EPISODES_TITLE,
    ] {
        assert!(
            !title.chars().any(|c| c.is_ascii_digit()),
            "{title:?} counts nothing"
        );
        assert!(
            !title.is_empty(),
            "a shelf is never drawn without its title"
        );
        assert_eq!(title.trim(), title, "{title:?} is drawn as it is written");
    }
}

#[test]
fn the_home_copy_has_no_counts() {
    assert_eq!(HOME_SHELF_TITLE, "Recently added");
    assert_eq!(HOME_QUIET, "Nothing recent to show yet.");
    assert_eq!(HOME_DAY_ONE_TITLE, "Home is built from what you save");
    assert_eq!(HOME_DAY_ONE_DOOR, "Search the catalogue");
    for copy in [
        HOME_SHELF_TITLE,
        HOME_QUIET,
        HOME_DAY_ONE_TITLE,
        HOME_DAY_ONE_BODY,
        HOME_DAY_ONE_DOOR,
    ] {
        assert!(
            !copy.chars().any(|c| c.is_ascii_digit()),
            "{copy:?} counts nothing"
        );
    }
}
