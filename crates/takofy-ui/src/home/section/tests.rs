use super::*;
use crate::home::Intake;
use crate::home::outlook::{Outlook, Tally, outlook};

const UNREADABLE: Failure = Failure::Load;
const ALBUMS: Failure = Failure::SavedAlbums;

/// The phases a read can be in, one of each kind.
const PHASES: [Phase; 3] = [Phase::Pending, Phase::Arrived, Phase::Failed(Failure::Load)];

/// What a state says, in the vocabulary the dated shelf's outlook uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Said {
    Waiting,
    Failed(Failure),
    Absent,
    Ready,
}

fn section_says<T>(section: &Section<T>) -> Said {
    match section {
        Section::Waiting => Said::Waiting,
        Section::Failed(failure) => Said::Failed(*failure),
        Section::Absent => Said::Absent,
        Section::Ready(_) => Said::Ready,
    }
}

fn outlook_says(outlook: Outlook) -> Said {
    match outlook {
        Outlook::Waiting => Said::Waiting,
        Outlook::Unreadable(failure) | Outlook::Incomplete(failure) => Said::Failed(failure),
        Outlook::Cards => Said::Ready,
        // Neither is anything to draw, which is what an absent shelf means.
        Outlook::DayOne | Outlook::Quiet => Said::Absent,
    }
}

#[test]
fn a_pending_feed_outranks_every_other_feed() {
    for held in [None, Some(())] {
        assert_eq!(
            settle(Phase::Pending, &[Phase::Arrived, Phase::Arrived], held),
            Section::Waiting
        );
        assert_eq!(
            settle(
                Phase::Arrived,
                &[Phase::Failed(ALBUMS), Phase::Pending],
                held
            ),
            Section::Waiting
        );
    }
}

#[test]
fn a_held_payload_outranks_a_failed_feed() {
    assert_eq!(
        settle(
            Phase::Arrived,
            &[Phase::Failed(ALBUMS), Phase::Arrived],
            Some(7)
        ),
        Section::Ready(7),
        "one failed source hid what another supplied"
    );
}

#[test]
fn a_failed_prerequisite_outranks_a_held_payload() {
    assert_eq!(
        settle(Phase::Failed(UNREADABLE), &[Phase::Arrived], Some(7)),
        Section::Failed(UNREADABLE),
        "nothing behind a failed prerequisite can draw"
    );
}

#[test]
fn a_failed_feed_outranks_an_empty_shelf() {
    assert_eq!(
        settle::<()>(
            Phase::Arrived,
            &[Phase::Failed(ALBUMS), Phase::Arrived],
            None
        ),
        Section::Failed(ALBUMS)
    );
    assert_eq!(
        settle::<()>(
            Phase::Arrived,
            &[Phase::Arrived, Phase::Failed(UNREADABLE)],
            None
        ),
        Section::Failed(UNREADABLE)
    );
    assert_eq!(
        settle::<()>(
            Phase::Arrived,
            &[Phase::Failed(ALBUMS), Phase::Failed(UNREADABLE)],
            None,
        ),
        Section::Failed(ALBUMS),
        "the feeds are named in the order their failures are read"
    );
}

#[test]
fn every_feed_arriving_with_nothing_is_absent() {
    assert_eq!(
        settle::<()>(Phase::Arrived, &[Phase::Arrived; 3], None),
        Section::Absent
    );
    assert_eq!(
        settle::<()>(Phase::Arrived, &[], None),
        Section::Absent,
        "a shelf that reads nothing"
    );
}

#[test]
fn a_failed_rootlist_gates_the_reads_behind_it() {
    assert_eq!(
        inherit(Phase::Failed(UNREADABLE), Phase::Pending),
        Phase::Failed(UNREADABLE)
    );
    assert_eq!(
        inherit(Phase::Failed(UNREADABLE), Phase::Arrived),
        Phase::Failed(UNREADABLE)
    );
    assert_eq!(inherit(Phase::Pending, Phase::Arrived), Phase::Arrived);
    assert_eq!(
        inherit(Phase::Arrived, Phase::Failed(ALBUMS)),
        Phase::Failed(ALBUMS)
    );
}

#[test]
fn the_budget_caps_each_slot_and_its_total() {
    let full = Budget::default();

    assert_eq!(
        Budget::per_open(Slot::Dated),
        0,
        "the dated shelf reads nothing"
    );
    assert_eq!(Budget::per_open(Slot::Stations), 5);
    assert_eq!(Budget::per_open(Slot::Because), 4);
    assert_eq!(
        Budget::per_open(Slot::Counted),
        1 + crate::home::counted::DRAWN,
        "the labels, then a face per artist the shelf draws"
    );
    assert_eq!(Budget::per_open(Slot::Episodes), 8);
    assert_eq!(Budget::allowance(), 24);

    let all: usize = Slot::ORDER.into_iter().map(|slot| full.left(slot)).sum();
    assert_eq!(all, Budget::allowance());
    assert_eq!(full.total(), Budget::allowance());
}

#[test]
fn a_taken_read_is_spent_and_refilling_returns_it() {
    let mut budget = Budget::default();

    for _ in 0..Budget::per_open(Slot::Counted) {
        assert!(budget.take(Slot::Counted), "the counted read is allowed");
    }
    assert!(
        !budget.take(Slot::Counted),
        "one open is the counted shelf's whole allowance"
    );
    assert_eq!(budget.left(Slot::Counted), 0);
    assert_eq!(
        budget.total(),
        Budget::allowance() - Budget::per_open(Slot::Counted)
    );

    budget.refill(Slot::Counted);
    assert!(budget.take(Slot::Counted), "a refilled slot may read again");

    assert!(
        !budget.take(Slot::Dated),
        "the dated shelf never sends a read"
    );
    budget.refill(Slot::Dated);
    assert_eq!(budget.left(Slot::Dated), 0);
}

/// One table pins both machines: the dated shelf's outlook and `settle` must
/// read the same phases the same way, or the page's rules disagree with the
/// state the shelves are drawn from.
#[test]
fn outlook_and_settle_agree_on_every_phase_triple() {
    for playlists in PHASES {
        for albums in PHASES {
            for likes in PHASES {
                // The reads behind a failed rootlist are reported as failed too,
                // which is what the dated shelf's intake does.
                let feeds = [inherit(playlists, albums), inherit(playlists, likes)];
                for held in [None, Some(())] {
                    let intake = Intake {
                        playlists,
                        albums: feeds[0],
                        likes: feeds[1],
                    };
                    let tally = Tally {
                        any: true,
                        unanswered: 0,
                        usable: usize::from(held.is_some()),
                        label_failed: None,
                    };
                    assert_eq!(
                        section_says(&settle(playlists, &feeds, held)),
                        outlook_says(outlook(intake, tally)),
                        "{playlists:?} {albums:?} {likes:?} held {held:?}"
                    );
                }
            }
        }
    }
}
