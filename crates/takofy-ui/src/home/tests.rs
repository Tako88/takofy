use super::*;
use takofy_model::home::{HomeAnswer, HomeAsk};

fn named(uri: &str, album: Option<&str>) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: album.map(str::to_owned),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

fn uris(values: &[&str]) -> Vec<String> {
    values.iter().map(|uri| (*uri).to_owned()).collect()
}

#[test]
fn opening_home_twice_draws_two_seeds() {
    let mut home = Home::default();
    home.open(1_700_000_000_000);
    let first = home.seed();
    home.open(1_700_000_000_000);
    assert_ne!(first, home.seed(), "a second open is a second draw");
    assert_eq!(home.opens, 2);
}

#[test]
fn a_named_track_with_an_album_is_usable() {
    let mut home = Home::default();
    let track = named("spotify:track:a", Some("spotify:album:r"));
    home.apply_labels(&uris(&["spotify:track:a"]), std::slice::from_ref(&track));

    assert_eq!(home.label("spotify:track:a"), Some(&track));
    assert!(home.answered("spotify:track:a"));
}

#[test]
fn an_unnamed_track_or_one_with_no_album_is_unusable() {
    let mut home = Home::default();
    let mut unnamed = named("spotify:track:a", Some("spotify:album:r"));
    unnamed.named = false;
    let albumless = named("spotify:track:b", None);
    home.apply_labels(
        &uris(&["spotify:track:a", "spotify:track:b"]),
        &[unnamed, albumless],
    );

    assert_eq!(home.label("spotify:track:a"), None);
    assert_eq!(home.label("spotify:track:b"), None);
    assert!(home.answered("spotify:track:a"));
    assert!(home.answered("spotify:track:b"));
}

#[test]
fn a_track_the_read_did_not_name_is_unusable() {
    let mut home = Home::default();
    home.apply_labels(&uris(&["spotify:track:a"]), &[]);

    assert_eq!(home.label("spotify:track:a"), None);
    assert!(home.answered("spotify:track:a"));
}

#[test]
fn an_unasked_uri_is_absent() {
    let mut home = Home::default();
    home.apply_labels(
        &uris(&["spotify:track:a"]),
        &[named("spotify:track:b", Some("spotify:album:r"))],
    );

    assert!(!home.asked("spotify:track:b"));
    assert!(!home.answered("spotify:track:b"));
    assert_eq!(home.label("spotify:track:b"), None);
}

#[test]
fn a_sent_uri_is_asked_but_not_yet_answered() {
    let mut home = Home::default();
    home.mark_asked(&uris(&["spotify:track:a"]));

    assert!(home.asked("spotify:track:a"), "the read was sent for it");
    assert!(!home.answered("spotify:track:a"), "no reply has landed yet");
    assert_eq!(home.label("spotify:track:a"), None);

    home.apply_labels(
        &uris(&["spotify:track:a"]),
        &[named("spotify:track:a", Some("spotify:album:r"))],
    );
    assert!(home.answered("spotify:track:a"), "the reply landed");
}

/// The four shelves below the dated one, each in one presence.
const ABSENT: [Presence; 4] = [Presence::Absent; 4];
const ONE_READY: [Presence; 4] = [
    Presence::Ready,
    Presence::Absent,
    Presence::Absent,
    Presence::Absent,
];
const ONE_WAITING: [Presence; 4] = [
    Presence::Absent,
    Presence::Waiting,
    Presence::Absent,
    Presence::Absent,
];
const ONE_FAILED: [Presence; 4] = [
    Presence::Absent,
    Presence::Absent,
    Presence::Failed,
    Presence::Absent,
];

#[test]
fn the_dated_shelf_draws_while_it_waits_and_when_it_has_cards() {
    assert!(dated_draws(&Shelf::Waiting, ABSENT));
    assert!(
        dated_draws(&Shelf::Waiting, ONE_READY),
        "another shelf has content"
    );
    assert!(dated_draws(&Shelf::Cards(Vec::new()), ABSENT));
    assert!(dated_draws(&Shelf::Cards(Vec::new()), ONE_READY));
}

#[test]
fn an_unreadable_library_draws_once_whatever_else_is_present() {
    for others in [ABSENT, ONE_READY, ONE_WAITING, ONE_FAILED] {
        assert!(dated_draws(&Shelf::Unreadable(Failure::Load), others));
    }
}

#[test]
fn an_actionable_failure_draws_beside_a_shelf_with_content() {
    for others in [ABSENT, ONE_READY, ONE_WAITING, ONE_FAILED] {
        assert!(dated_draws(
            &Shelf::Incomplete(Failure::SavedAlbums),
            others
        ));
    }
}

#[test]
fn the_quiet_shelf_draws_only_when_nothing_else_has_content() {
    assert!(dated_draws(&Shelf::Quiet, ABSENT));
    assert!(
        dated_draws(&Shelf::Quiet, ONE_FAILED),
        "a failure is not content"
    );
    assert!(!dated_draws(&Shelf::Quiet, ONE_READY));
    assert!(
        !dated_draws(&Shelf::Quiet, ONE_WAITING),
        "a shelf still coming will have content"
    );
}

#[test]
fn the_day_one_invite_draws_only_beside_four_absent_shelves() {
    assert!(dated_draws(&Shelf::DayOne, ABSENT));
    assert!(!dated_draws(&Shelf::DayOne, ONE_READY));
    assert!(!dated_draws(&Shelf::DayOne, ONE_WAITING));
    assert!(
        !dated_draws(&Shelf::DayOne, ONE_FAILED),
        "a followed artist would make the invite lie"
    );
    assert!(
        !dated_draws(
            &Shelf::DayOne,
            [
                Presence::Absent,
                Presence::Absent,
                Presence::Absent,
                Presence::Ready
            ]
        ),
        "a shelf at the bottom of the page is still a shelf with content"
    );
}

#[test]
fn an_open_refills_the_budget() {
    let mut home = Home::default();
    for _ in 0..Budget::per_open(Slot::Episodes) {
        assert!(home.budget.take(Slot::Episodes));
    }
    assert_eq!(home.budget.left(Slot::Episodes), 0);

    home.open(1_700_000_000_000);

    assert_eq!(
        home.budget,
        Budget::default(),
        "an open is a fresh allowance"
    );
}

#[test]
fn forgetting_drops_the_labels_and_the_failure() {
    let mut home = Home::default();
    home.mark_asked(&uris(&["spotify:track:a"]));
    home.apply_labels(
        &uris(&["spotify:track:a"]),
        &[named("spotify:track:a", Some("spotify:album:r"))],
    );
    home.fail_labels(&uris(&["spotify:track:b"]), Failure::Load);
    assert!(home.asked("spotify:track:a"));
    assert!(home.answered("spotify:track:a"));
    assert_eq!(home.failed(), Some(Failure::Load));

    home.forget();

    assert!(!home.asked("spotify:track:a"));
    assert!(!home.answered("spotify:track:a"));
    assert_eq!(home.label("spotify:track:a"), None);
    assert_eq!(home.failed(), None);
}

#[test]
fn forgetting_one_slot_keeps_the_others_and_the_budget() {
    let mut home = Home::default();
    let uri = "spotify:track:a";
    home.mark_asked(&uris(&[uri]));
    home.apply_labels(&uris(&[uri]), &[named(uri, Some("spotify:album:r"))]);
    for _ in 0..Budget::per_open(Slot::Counted) {
        assert!(home.budget.take(Slot::Counted));
    }
    let counted = home.counted.clone();

    home.forget_slot(Slot::Episodes);

    assert_eq!(home.counted, counted, "another shelf's state was dropped");
    assert!(home.answered(uri), "another shelf's label was dropped");
    assert_eq!(
        home.budget.left(Slot::Counted),
        0,
        "another shelf's read was returned"
    );

    home.forget_slot(Slot::Dated);

    assert!(!home.asked(uri), "the dated shelf asks again");
    assert!(!home.answered(uri));
}

#[test]
fn a_counted_label_answer_is_not_the_dated_shelves() {
    let mut home = Home::default();
    let uri = "spotify:track:a";
    home.mark_asked(&uris(&[uri]));

    home.land(&HomeAnswer::Labelled {
        asked: uris(&[uri]),
        named: vec![named(uri, Some("spotify:album:r"))],
    });

    assert!(
        !home.answered(uri),
        "only the dated shelf's own read fills its labels"
    );
    assert_eq!(home.label(uri), None);
    assert_eq!(home.failed(), None);
}

#[test]
fn a_failed_counted_read_is_not_the_dated_shelves() {
    let mut home = Home::default();
    let uri = "spotify:track:a";
    home.mark_asked(&uris(&[uri]));

    home.fail(
        &HomeAsk::Labels {
            tracks: uris(&[uri]),
        },
        Failure::Load,
    );

    assert!(!home.answered(uri));
    assert_eq!(home.label(uri), None);
    assert_eq!(home.failed(), None);
}

#[test]
fn the_page_draws_its_shelves_in_a_fixed_order() {
    assert_eq!(
        Slot::ORDER,
        [
            Slot::Dated,
            Slot::Stations,
            Slot::Because,
            Slot::Counted,
            Slot::Episodes
        ]
    );
}

/// Each draw on the page is a stream of its own, told apart by its salt alone:
/// the counted ranking's, the two the related shelf uses to try artists and to
/// pick their cards, and the episodes pool's. A copied salt would make two draws
/// take the same positions from pools of the same size.
#[test]
fn every_draw_stream_is_its_own() {
    let page = Seed::for_open(1_700_000_000_000, 1);
    let streams = [
        page.split(counted::SALT),
        page.split(because::ORDER_SALT),
        page.split(because::PICK_SALT),
        page.split(episodes::SALT),
    ];

    for (at, stream) in streams.iter().enumerate() {
        assert_ne!(*stream, page, "a shelf draws from the page's own stream");
        assert!(
            !streams[..at].contains(stream),
            "two shelves share a stream"
        );
    }
}

#[test]
fn a_sections_presence_reads_the_state_it_settled_from() {
    assert_eq!(Section::<()>::Waiting.presence(), Presence::Waiting);
    assert_eq!(
        Section::<()>::Failed(Failure::Load).presence(),
        Presence::Failed
    );
    assert_eq!(Section::<()>::Absent.presence(), Presence::Absent);
    assert_eq!(Section::<()>::Ready(()).presence(), Presence::Ready);
}

/// The dated shelf is the page's one hand-written state, so its presence is
/// pinned against the same four words the settled sections use.
#[test]
fn the_dated_shelf_says_the_same_four_things() {
    assert_eq!(dated_presence(&Shelf::Waiting), Presence::Waiting);
    assert_eq!(
        dated_presence(&Shelf::Unreadable(Failure::Load)),
        Presence::Failed
    );
    assert_eq!(
        dated_presence(&Shelf::Incomplete(Failure::SavedAlbums)),
        Presence::Failed
    );
    assert_eq!(dated_presence(&Shelf::DayOne), Presence::Absent);
    assert_eq!(dated_presence(&Shelf::Quiet), Presence::Absent);
    assert_eq!(
        dated_presence(&Shelf::Cards(Vec::new())),
        Presence::Ready,
        "cards are cards whether or not the pool drew any"
    );
}

#[test]
fn a_label_answer_names_the_lengths_of_what_it_asked_for() {
    let mut home = Home::default();
    let asked = uris(&["spotify:track:a", "spotify:track:b"]);
    let mut timed = named("spotify:track:a", None);
    timed.duration_ms = Some(252_000);

    home.land(&HomeAnswer::Labelled {
        asked: asked.clone(),
        named: vec![timed, named("spotify:track:b", None)],
    });

    assert_eq!(
        home.duration("spotify:track:a"),
        Some(252_000),
        "a label read says how long the track it named runs"
    );
    assert_eq!(
        home.duration("spotify:track:b"),
        None,
        "a track with no length answers none"
    );
}

#[test]
fn a_label_answer_from_another_shelf_does_not_fill_the_dated_shelves_cards() {
    let mut home = Home::default();
    let asked = uris(&["spotify:track:a"]);

    home.land(&HomeAnswer::Labelled {
        asked: asked.clone(),
        named: vec![named("spotify:track:a", Some("spotify:album:r"))],
    });

    assert!(
        !home.answered("spotify:track:a"),
        "only the dated shelf's own read names a card"
    );
}
