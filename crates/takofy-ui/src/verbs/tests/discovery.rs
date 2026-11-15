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
fn a_discovery_card_offers_follow_add_and_hide_all_live() {
    let request = discovery_request(
        &card(vec![entry("spotify:track:a"), entry("spotify:track:b")]),
        door(),
        None,
    );

    assert_eq!(
        labels(&request.items),
        [
            "Follow Ilse Vane",
            "Add to playlist",
            "Hide Ilse Vane from discovery"
        ]
    );
    assert_eq!(
        verbs(&request.items),
        [Verb::Follow, Verb::AddToPlaylist, Verb::HideFromDiscovery]
    );
    assert_eq!(
        request
            .items
            .iter()
            .map(|item| item.live)
            .collect::<Vec<_>>(),
        [true, true, true],
        "every row can act"
    );
    assert_eq!(
        request.items.last().map(|item| item.kind),
        Some(Kind::Destructive),
        "Hide is the destructive row"
    );
    assert!(request.items[1].submenu, "the add opens a list");
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
    let request = discovery_request(&card(vec![episode]), door(), None);

    assert!(
        !verbs(&request.items).contains(&Verb::AddToPlaylist),
        "an episode is not a track an add can take"
    );
    assert_eq!(
        labels(&request.items),
        ["Follow Ilse Vane", "Hide Ilse Vane from discovery"]
    );
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

    let request = discovery_request(&card(vec![entry("spotify:track:a")]), door(), None);
    let mut menu = Menu::default();
    assert!(menu.open(request, Gate::default()));
    assert_eq!(
        menu.lit(),
        Some(0),
        "the keyboard lights the first row, which can act"
    );
    assert!(menu.items().iter().all(|item| item.live));
    assert!(menu.items()[1].submenu, "the add opens the playlist list");
    assert_eq!(
        menu.choose(0).and_then(|closed| closed.chose),
        Some(Verb::Follow),
        "the Follow row fires"
    );
}
