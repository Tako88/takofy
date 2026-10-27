use super::*;

#[test]
fn the_row_is_the_four_reference_chips_in_order() {
    assert_eq!(
        ROW,
        [
            SidebarKind::Playlists,
            SidebarKind::Artists,
            SidebarKind::Albums,
            SidebarKind::Podcasts,
        ]
    );
    assert_eq!(
        SidebarKind::default(),
        SidebarKind::Playlists,
        "the list shows playlists until the user picks another kind"
    );
}

#[test]
fn each_kind_owns_its_own_control() {
    assert_eq!(SidebarKind::Playlists.control(), Control::FilterPlaylists);
    assert_eq!(SidebarKind::Artists.control(), Control::FilterArtists);
    assert_eq!(SidebarKind::Albums.control(), Control::FilterAlbums);
    assert_eq!(SidebarKind::Podcasts.control(), Control::FilterPodcasts);
}

#[test]
fn acts_follows_the_availability_of_the_control() {
    for kind in ROW {
        assert_eq!(
            kind.acts(),
            availability(kind.control()) == Availability::Ready,
            "{kind:?}"
        );
    }
}

#[test]
fn every_chip_has_data_behind_it() {
    for kind in ROW {
        assert!(kind.acts(), "{kind:?}");
    }
}

#[test]
fn choosing_a_live_chip_sticks() {
    let mut filter = SidebarFilter::default();
    assert!(filter.choose(SidebarKind::Podcasts));
    assert_eq!(filter.chosen(), SidebarKind::Podcasts);
}

#[test]
fn a_pressed_chip_can_be_replaced_by_any_other() {
    let mut filter = SidebarFilter::default();
    assert!(filter.choose(SidebarKind::Podcasts));

    for kind in [SidebarKind::Artists, SidebarKind::Albums] {
        assert!(filter.choose(kind), "{kind:?}");
        assert_eq!(filter.chosen(), kind, "{kind:?}");
    }
}
