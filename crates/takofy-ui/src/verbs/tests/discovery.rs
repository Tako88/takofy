use super::super::*;
use super::test_support::*;
use crate::menu::{Anchor, Device};
use takofy_model::home::{DiscoveryArtist, DiscoveryCard};

fn card(tracks: Vec<TrackEntry>) -> DiscoveryCard {
    DiscoveryCard {
        artist: DiscoveryArtist {
            uri: "spotify:artist:vane".to_owned(),
            name: "Ilse Vane".to_owned(),
        },
        outside: None,
        portrait: None,
        tracks,
        latest_year: None,
    }
}

fn door() -> Door {
    Door {
        anchor: Anchor::Button(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(32.0, 32.0),
        )),
        opener: egui::Id::unique("discovery-more"),
        device: Device::Keyboard,
    }
}

#[test]
fn a_discovery_card_offers_follow_radio_add_and_hide_all_live() {
    let request = discovery_request(
        &card(vec![entry("spotify:track:a"), entry("spotify:track:b")]),
        door(),
    );

    assert_eq!(
        labels(&request.items),
        [
            "Follow Ilse Vane",
            "Start radio",
            "Add to playlist",
            "Hide Ilse Vane from discovery"
        ]
    );
    assert_eq!(
        verbs(&request.items),
        [
            Verb::Follow,
            Verb::StartRadio,
            Verb::AddToPlaylist,
            Verb::HideFromDiscovery
        ]
    );
    assert_eq!(
        request
            .items
            .iter()
            .map(|item| item.live)
            .collect::<Vec<_>>(),
        [true, true, true, true],
        "every row can act"
    );
    assert_eq!(
        request.items.last().map(|item| item.kind),
        Some(Kind::Destructive),
        "Hide is the destructive row"
    );
    assert!(request.items[2].submenu, "the add opens a list");
    assert_eq!(
        request.targets,
        vec![entry("spotify:track:a")],
        "the add acts on the track the card's play names"
    );
    match &request.listing {
        Listing::Band(item) => {
            assert_eq!(item.kind, SidebarKind::Artists);
            assert_eq!(item.uri, "spotify:artist:vane");
        }
        other => panic!("a discovery card is a band: {other:?}"),
    }
}

#[test]
fn an_unplayable_card_offers_no_add_row() {
    let mut episode = entry("spotify:episode:a");
    episode.named = true;
    let request = discovery_request(&card(vec![episode]), door());

    assert!(
        !verbs(&request.items).contains(&Verb::AddToPlaylist),
        "an episode is not a track an add can take"
    );
    assert_eq!(
        labels(&request.items),
        [
            "Follow Ilse Vane",
            "Start radio",
            "Hide Ilse Vane from discovery"
        ]
    );
}

#[test]
fn a_discovery_cards_radio_seeds_the_artist_not_the_track_it_carries() {
    // Two cards for the same artist whose wires named different tracks first.
    let one = discovery_request(&card(vec![entry("spotify:track:a")]), door());
    let other = discovery_request(&card(vec![entry("spotify:track:z")]), door());

    for request in [one, other] {
        let Listing::Band(row) = &request.listing else {
            panic!("a discovery card is a band");
        };
        assert_eq!(
            library_outcome(Verb::StartRadio, row),
            Some(Outcome::StartRadio {
                seed: StationSeed {
                    kind: SeedKind::Artist,
                    uri: "spotify:artist:vane".to_owned(),
                },
            }),
            "the card seeds the artist, whichever track its play names"
        );
    }
}

#[test]
fn hide_and_show_act_on_the_artist_the_row_names() {
    let row = LibraryItem {
        kind: SidebarKind::Artists,
        uri: "spotify:artist:vane".to_owned(),
        name: "Ilse Vane".to_owned(),
    };

    assert_eq!(
        library_outcome(Verb::HideFromDiscovery, &row),
        Some(Outcome::Hide("spotify:artist:vane".to_owned()))
    );
    assert_eq!(
        library_outcome(Verb::ShowInDiscovery, &row),
        Some(Outcome::Show("spotify:artist:vane".to_owned()))
    );
    assert_eq!(
        outcome(Verb::ShowInDiscovery, &[entry("spotify:track:a")]),
        None,
        "a show acts on a row, not on a pick"
    );
}

#[test]
fn a_discovery_menus_every_row_live_lands_the_keyboard_on_follow() {
    use crate::menu::{Gate, Menu};

    let request = discovery_request(&card(vec![entry("spotify:track:a")]), door());
    let mut menu = Menu::default();
    assert!(menu.open(request, Gate::default()));
    assert_eq!(
        menu.lit(),
        Some(0),
        "the keyboard lights the first row, which can act"
    );
    assert!(menu.items().iter().all(|item| item.live));
    assert!(menu.items()[2].submenu, "the add opens the playlist list");
    assert_eq!(
        menu.choose(0).and_then(|closed| closed.chose),
        Some(Verb::Follow),
        "the Follow row fires"
    );
}

#[test]
fn a_card_menu_names_home_so_it_survives_the_frame_it_opened_on() {
    let request = discovery_request(&card(vec![entry("spotify:track:a")]), door());
    assert_eq!(request.page, Some(Page::Home));

    let mut menu = crate::menu::Menu::default();
    assert!(menu.open(request, crate::menu::Gate::default()));
    assert!(
        menu.follow_page(Some(&Page::Home)).is_none(),
        "the menu closed on the page it was opened on"
    );
    assert!(menu.is_open());
    assert!(
        menu.follow_page(Some(&Page::Artist("spotify:artist:other".to_owned())))
            .is_some(),
        "a page change leaves the rows it described behind"
    );
}
