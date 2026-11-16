use super::super::*;
use super::test_support::*;
use crate::menu::{Anchor, Device};

fn door() -> Door {
    Door {
        anchor: Anchor::Button(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(32.0, 32.0),
        )),
        opener: egui::Id::unique("card-more"),
        device: Device::Pointer,
    }
}

#[test]
fn a_song_cards_sheet_is_the_rows_and_can_remove_nothing() {
    let entry = entry("spotify:track:a");
    let request = track_request(&entry, door(), Standing::Unknown, None).expect("a sheet");

    assert_eq!(request.listing, Listing::Page, "a card sits in no list");
    assert_eq!(request.targets, vec![entry]);
    assert!(request.rows.is_empty(), "a card is not a row of a list");
    assert!(
        !verbs(&request.items).contains(&Verb::Remove),
        "a card cannot be removed from a list it is not in: {:?}",
        verbs(&request.items)
    );
}

#[test]
fn a_song_cards_sheet_offers_its_radio_and_its_queue() {
    let entry = entry("spotify:track:a");
    let request = track_request(&entry, door(), Standing::Unknown, None).expect("a sheet");
    let offered = verbs(&request.items);

    for verb in [Verb::StartRadio, Verb::AddToQueue, Verb::PlayNext] {
        assert!(offered.contains(&verb), "{verb:?} is out of {offered:?}");
    }
}

#[test]
fn a_local_file_offers_no_radio_and_no_link() {
    let local = entry("spotify:local:a:b:c:1");
    let request = track_request(&local, door(), Standing::Unsaved, None).expect("a sheet");
    let offered = verbs(&request.items);

    assert!(
        !offered.contains(&Verb::StartRadio),
        "a file seeds no station"
    );
    assert!(!offered.contains(&Verb::CopyLink), "a file has no page");
}

#[test]
fn a_cards_sheet_carries_the_page_it_was_opened_on() {
    let entry = entry("spotify:track:a");
    let page = Page::Home;
    let request =
        track_request(&entry, door(), Standing::Unknown, Some(page.clone())).expect("a sheet");

    assert_eq!(request.page, Some(page));
}
