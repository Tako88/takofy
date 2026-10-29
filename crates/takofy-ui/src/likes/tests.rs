use super::*;
use takofy_model::SavedAt;

const A: &str = "spotify:track:a";
const B: &str = "spotify:track:b";
const C: &str = "spotify:track:c";

/// A read's answer for `uris`, none of them dated.
fn undated(uris: &[&str]) -> Vec<LikedTrack> {
    uris.iter()
        .map(|uri| LikedTrack {
            uri: (*uri).to_owned(),
            saved_at: None,
        })
        .collect()
}

/// A liked track saved `secs` after the epoch.
fn dated(uri: &str, secs: i64) -> LikedTrack {
    LikedTrack {
        uri: uri.to_owned(),
        saved_at: SavedAt::from_secs(secs),
    }
}

fn likes(tracks: &[&str]) -> Likes {
    let mut likes = Likes::default();
    assert!(likes.request(true));
    likes.apply(&Event::Set(undated(tracks)));
    likes
}

fn set(likes: &mut Likes, uris: &[&str], liked: bool) -> Option<Change> {
    let uris: Vec<String> = uris.iter().map(|uri| (*uri).to_owned()).collect();
    likes.set(&uris, liked)
}

#[test]
fn a_new_set_knows_nothing() {
    let mut likes = Likes::default();
    assert_eq!(likes.state(A), None);
    assert_eq!(likes.standing(&[A]), Standing::Unknown);
    assert_eq!(set(&mut likes, &[A], true), None);
    assert_eq!(likes.notice(), None);
}

#[test]
fn a_read_makes_the_set_ready_and_answers_per_uri() {
    let likes = likes(&[A, C]);
    assert_eq!(likes.state(A), Some(true));
    assert_eq!(likes.state(B), Some(false));
    assert_eq!(likes.state(C), Some(true));
}

#[test]
fn only_track_uris_have_a_state() {
    for uri in [
        "spotify:episode:e",
        "spotify:album:a",
        "spotify:local:A:B:C:1000",
        "spotify:track:",
        "track:a",
        "",
    ] {
        assert_eq!(likes(&[A]).state(uri), None, "{uri:?}");
    }
}

#[test]
fn a_bare_track_address_is_not_a_track() {
    let mut set_likes = likes(&[]);

    assert_eq!(
        set(&mut set_likes, &["spotify:track:"], true),
        None,
        "an address naming no track cannot be liked"
    );
    assert_eq!(set_likes.state("spotify:track:"), None);
    assert_eq!(
        likes(&["spotify:track:a:b"]).state("spotify:track:a:b"),
        Some(true),
        "a colon in the id is part of the id"
    );
}

#[test]
fn set_flips_every_effective_uri_and_returns_one_change_in_order() {
    let mut likes = likes(&[]);
    assert_eq!(
        set(&mut likes, &[A, B], true),
        Some(Change {
            tracks: vec![A.to_owned(), B.to_owned()],
            liked: true,
        })
    );
    assert_eq!(likes.state(A), Some(true));
    assert_eq!(likes.state(B), Some(true));
}

#[test]
fn a_pick_that_is_partly_liked_writes_only_the_unliked() {
    let mut likes = likes(&[A]);
    assert_eq!(
        set(&mut likes, &[A, B, C], true),
        Some(Change {
            tracks: vec![B.to_owned(), C.to_owned()],
            liked: true,
        }),
        "the already-liked row is not re-dated"
    );
    assert_eq!(likes.state(A), Some(true));
    assert_eq!(likes.state(B), Some(true));
}

#[test]
fn a_set_that_changes_nothing_returns_none_and_stays_idle() {
    let mut likes = likes(&[A, B]);
    assert_eq!(set(&mut likes, &[A, B], true), None);
    assert_eq!(set(&mut likes, &[C], false), None);
    assert_eq!(
        set(&mut likes, &[A], false),
        Some(Change {
            tracks: vec![A.to_owned()],
            liked: false
        }),
        "nothing was stuck pending by the writes that changed nothing"
    );
}

#[test]
fn a_second_set_while_one_is_pending_is_refused_and_changes_nothing() {
    let mut likes = likes(&[A]);
    assert!(set(&mut likes, &[B], true).is_some());
    assert_eq!(set(&mut likes, &[C], true), None);
    assert_eq!(likes.state(C), Some(false));
}

#[test]
fn duplicate_uris_in_a_pick_are_written_once() {
    let mut likes = likes(&[]);
    assert_eq!(
        set(&mut likes, &[A, A, B], true),
        Some(Change {
            tracks: vec![A.to_owned(), B.to_owned()],
            liked: true,
        })
    );
}

#[test]
fn a_non_track_in_the_pick_refuses_the_whole_set() {
    let mut likes = likes(&[]);
    assert_eq!(set(&mut likes, &[A, "spotify:episode:e"], true), None);
    assert_eq!(likes.state(A), Some(false), "nothing was flipped");
    assert_eq!(
        set(&mut likes, &[A], true),
        Some(Change {
            tracks: vec![A.to_owned()],
            liked: true
        }),
        "the refusal stuck nothing pending"
    );
}

#[test]
fn a_confirmed_write_ends_the_wait_and_clears_the_notice() {
    let mut likes = likes(&[A]);
    let change = set(&mut likes, &[B], true).expect("a change");
    likes.apply(&Event::Confirmed {
        tracks: change.tracks,
        liked: true,
    });
    assert_eq!(likes.notice(), None);
    assert!(
        set(&mut likes, &[C], true).is_some(),
        "the wait ended, so the next like is sent"
    );
}

#[test]
fn a_confirmation_that_names_other_tracks_is_ignored() {
    let mut likes = likes(&[A]);
    assert!(set(&mut likes, &[B], true).is_some());
    likes.apply(&Event::Confirmed {
        tracks: vec![C.to_owned()],
        liked: true,
    });
    assert_eq!(
        set(&mut likes, &[C], true),
        None,
        "the write is still the one in flight"
    );
}

#[test]
fn a_failed_write_restores_every_uri_sets_save_and_marks_stale() {
    let mut likes = likes(&[A]);
    let change = set(&mut likes, &[A, B], false).expect("a change");
    assert_eq!(
        change.tracks,
        vec![A.to_owned()],
        "only the row the write would change is named"
    );
    assert_eq!(likes.state(A), Some(false), "the flip was optimistic");

    likes.apply(&Event::Failed {
        tracks: change.tracks,
        failure: Failure::Save,
    });
    assert_eq!(likes.state(A), Some(true), "the revert put A back");
    assert_eq!(likes.state(B), Some(false));
    assert_eq!(likes.notice(), Some(Failure::Save));
    assert!(likes.request(true), "a failed write asks for one read");
}

#[test]
fn a_failure_that_names_other_tracks_is_ignored() {
    let mut likes = likes(&[A]);
    assert!(set(&mut likes, &[B], true).is_some());
    likes.apply(&Event::Failed {
        tracks: vec![C.to_owned()],
        failure: Failure::Save,
    });
    assert_eq!(likes.notice(), None);
    assert_eq!(
        set(&mut likes, &[C], true),
        None,
        "the write is still the one in flight"
    );
}

#[test]
fn request_emits_one_load_after_a_failure_and_then_waits_for_the_reply() {
    let mut likes = likes(&[A]);
    assert!(set(&mut likes, &[B], true).is_some());
    let _ = likes.asking.take();
    likes.apply(&Event::Failed {
        tracks: vec![B.to_owned()],
        failure: Failure::Save,
    });
    assert!(!likes.request(false), "the playlists are not ready yet");
    assert!(likes.request(true));
    assert!(!likes.request(true), "one read is in flight");
    likes.apply(&Event::Set(undated(&[A])));
    assert!(!likes.request(true), "the read answered");
}

#[test]
fn a_read_landing_during_a_pending_write_keeps_the_write() {
    let mut likes = likes(&[A]);
    likes.reload();
    assert!(likes.request(true));
    assert!(set(&mut likes, &[B], true).is_some());
    likes.apply(&Event::Set(undated(&[A])));
    assert_eq!(likes.state(B), Some(true), "the pending flip survived");
    assert_eq!(
        set(&mut likes, &[C], true),
        None,
        "and the write is still in flight"
    );
}

#[test]
fn a_stale_read_landing_after_a_confirmed_like_keeps_the_like() {
    let mut likes = likes(&[]);
    likes.reload();
    assert!(likes.request(true), "the first sweep is in flight");
    let change = set(&mut likes, &[A], true).expect("a change");
    likes.apply(&Event::Confirmed {
        tracks: change.tracks,
        liked: true,
    });
    // The read was issued before the like, so it carries the truth from
    // before it: adopting it would wipe a confirmed like with no healing
    // read behind it.
    likes.apply(&Event::Set(undated(&[])));
    assert_eq!(likes.state(A), Some(true));
}

#[test]
fn a_write_failure_during_a_read_heals_after_the_stale_reply() {
    let mut likes = likes(&[A]);
    likes.reload();
    assert!(likes.request(true), "a refresh is in flight");
    let change = set(&mut likes, &[B], true).expect("a change");
    likes.apply(&Event::Failed {
        tracks: change.tracks,
        failure: Failure::Save,
    });
    assert_eq!(likes.notice(), Some(Failure::Save));
    assert!(
        !likes.request(true),
        "a second read must not overlap the one already in flight"
    );

    // The stale read was issued before the flip: dropping it must ask again,
    // or the sentence would stay forever.
    likes.apply(&Event::Set(undated(&[A])));
    assert!(likes.request(true), "the stale reply did not ask again");
    likes.apply(&Event::Set(undated(&[A])));
    assert_eq!(likes.notice(), None);
}

#[test]
fn a_read_with_no_pending_write_replaces_the_set_wholesale() {
    let mut likes = likes(&[A]);
    likes.reload();
    assert!(likes.request(true));
    likes.apply(&Event::Set(undated(&[B])));
    assert_eq!(likes.state(A), Some(false));
    assert_eq!(likes.state(B), Some(true));
}

#[test]
fn an_adopted_read_clears_the_notice() {
    let mut likes = likes(&[A]);
    assert!(set(&mut likes, &[B], true).is_some());
    likes.apply(&Event::Failed {
        tracks: vec![B.to_owned()],
        failure: Failure::Save,
    });
    assert_eq!(likes.notice(), Some(Failure::Save));
    assert!(likes.request(true));
    likes.apply(&Event::Set(undated(&[A])));
    assert_eq!(likes.notice(), None);
}

#[test]
fn a_failed_sweep_after_a_failed_write_stays_stale() {
    let mut likes = likes(&[A]);
    likes.reload();
    assert!(likes.request(true), "a refresh is in flight");
    let change = set(&mut likes, &[B], true).expect("a change");
    likes.apply(&Event::Failed {
        tracks: change.tracks,
        failure: Failure::Save,
    });

    // The read was issued before the revert, so its failure is not the
    // healing read's answer: the set stays stale and is read again.
    likes.apply(&Event::SetFailed(Failure::Load));
    assert!(likes.request(true), "the healing read was buried");
    likes.apply(&Event::Set(undated(&[A])));
    assert_eq!(likes.notice(), None);
}

#[test]
fn a_failed_refresh_keeps_the_set() {
    let mut likes = likes(&[A]);
    likes.reload();
    assert!(likes.request(true));
    likes.apply(&Event::SetFailed(Failure::Load));
    assert_eq!(likes.state(A), Some(true), "a failed refresh hid a heart");
}

#[test]
fn a_failed_first_read_is_failed_and_every_state_is_unknown() {
    let mut likes = Likes::default();
    assert!(likes.request(true));
    likes.apply(&Event::SetFailed(Failure::Load));
    assert_eq!(likes.state(A), None);
    assert_eq!(likes.standing(&[A]), Standing::Unknown);
    assert!(
        !likes.request(true),
        "a failed read is not retried every frame"
    );
}

#[test]
fn reload_marks_a_ready_set_stale_without_hiding_any_heart() {
    let mut likes = likes(&[A]);
    likes.reload();
    assert_eq!(likes.state(A), Some(true));
    assert!(likes.request(true), "the reload asks for the set");
}

#[test]
fn reload_rearms_a_failed_read() {
    let mut likes = Likes::default();
    assert!(likes.request(true));
    likes.apply(&Event::SetFailed(Failure::Load));
    likes.reload();
    assert!(likes.request(true));
}

#[test]
fn reload_while_a_write_is_pending_keeps_the_wait() {
    let mut likes = likes(&[A]);
    assert!(set(&mut likes, &[A], false).is_some());
    likes.reload();
    likes.apply(&Event::Failed {
        tracks: vec![A.to_owned()],
        failure: Failure::Save,
    });
    assert_eq!(
        likes.state(A),
        Some(true),
        "the reload dropped the write the failure had to revert"
    );
}

#[test]
fn standing_of_a_pick() {
    let likes = likes(&[A, B]);
    assert_eq!(likes.standing(&[A, B]), Standing::Saved);
    assert_eq!(likes.standing(&[C]), Standing::Unsaved);
    assert_eq!(likes.standing(&[A, C]), Standing::Mixed);
    assert_eq!(likes.standing(&[]), Standing::Unknown);
    assert_eq!(likes.standing(&[A, "spotify:episode:e"]), Standing::Unknown);
    assert_eq!(Likes::default().standing(&[A]), Standing::Unknown);
}

#[test]
fn pending_does_not_change_standing() {
    let mut likes = likes(&[A]);
    assert!(set(&mut likes, &[B], true).is_some());
    assert_eq!(likes.standing(&[A, B]), Standing::Saved);
}

#[test]
fn the_first_request_waits_for_the_playlists() {
    let mut likes = Likes::default();
    assert!(!likes.request(false));
    assert!(likes.request(true));
}

#[test]
fn recent_is_none_until_a_read_lands() {
    assert!(Likes::default().recent().is_none());
    let mut likes = Likes::default();
    assert!(likes.request(true));
    likes.apply(&Event::SetFailed(Failure::Load));
    assert!(
        likes.recent().is_none(),
        "a failed first read has no set to be recent in"
    );
}

#[test]
fn an_adopted_read_keeps_each_tracks_date_in_the_reads_order() {
    let mut likes = Likes::default();
    assert!(likes.request(true));
    let (b, a) = (dated(B, 200), dated(A, 100));
    likes.apply(&Event::Set(vec![b.clone(), a.clone()]));
    assert_eq!(
        likes.recent().expect("a read landed").collect::<Vec<_>>(),
        vec![&b, &a]
    );
}

#[test]
fn a_stale_read_is_not_adopted_into_recent() {
    let mut likes = Likes::default();
    assert!(likes.request(true));
    likes.apply(&Event::Set(vec![dated(A, 100)]));
    likes.reload();
    assert!(likes.request(true));
    let change = set(&mut likes, &[B], true).expect("a change");
    likes.apply(&Event::Confirmed {
        tracks: change.tracks,
        liked: true,
    });
    // Issued before the like, so it names no tracks; adopting it would empty recent.
    likes.apply(&Event::Set(Vec::new()));
    assert_eq!(
        likes.recent().expect("a set").collect::<Vec<_>>(),
        vec![&dated(A, 100)],
        "the stale read was not adopted, so the dated track stays recent"
    );
}

#[test]
fn a_track_unliked_leaves_recent_and_its_failed_write_brings_it_back() {
    let mut likes = Likes::default();
    assert!(likes.request(true));
    let (a, b) = (dated(A, 100), dated(B, 200));
    likes.apply(&Event::Set(vec![a.clone(), b.clone()]));

    let change = set(&mut likes, &[A], false).expect("a change");
    assert_eq!(
        likes.recent().expect("a set").collect::<Vec<_>>(),
        vec![&b],
        "an unliked track is no longer recent"
    );

    likes.apply(&Event::Failed {
        tracks: change.tracks,
        failure: Failure::Save,
    });
    assert_eq!(
        likes.recent().expect("a set").collect::<Vec<_>>(),
        vec![&a, &b],
        "the revert put it back in the read's order"
    );
}
