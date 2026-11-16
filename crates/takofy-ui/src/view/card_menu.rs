//! The menu a card posts for the thing it names.
//!
//! A card is a face for one item with no row under it, so its sheet is the one a
//! sidebar row or a band already posts: the item's own verbs, built for the kind
//! the URI names rather than for the shelf the card sits on. A song gets the row
//! sheet instead, because that is the only one that has business queueing a
//! single track.
//!
//! Every shelf that draws a tile posts through here, so the rights a card offers
//! and the surface it is opened from cannot drift apart.

use egui::Context;
use takofy_model::TrackEntry;

use crate::library::Library;
use crate::menu::{Door, LibraryItem};
use crate::naming::Surface;
use crate::verbs::{self, LibraryRights};
use crate::view::carry;
use crate::view::context_menu;
use crate::view::media;

/// Posts the menu for a playlist, album or artist card.
pub(in crate::view) fn post_thing(
    ctx: &Context,
    library: &Library,
    media: &media::Media<'_>,
    door: Door,
) {
    if carry::carrying(ctx) {
        return;
    }
    let item = LibraryItem {
        kind: media.shelf,
        uri: media.uri.to_owned(),
        name: media.name.to_owned(),
    };
    let rights = LibraryRights {
        // The name field lives in the sidebar, so a card offers no rename.
        renamable: false,
        removable: library.can_remove(item.kind),
        saved: library.band_saved(&item.uri),
    };
    // The surface only decides whether a rename would open a row's field or a
    // band's, and a card offers neither.
    context_menu::post(
        ctx,
        verbs::library_request(item, door, rights, library.page().cloned(), Surface::Row),
    );
}

/// Posts the menu for an artist card: the follow, the radio and the link the
/// artist's own band offers.
///
/// The card is not a discovery row, so it never offers the way back out of
/// discovery.
pub(in crate::view) fn post_artist(
    ctx: &Context,
    library: &Library,
    media: &media::Media<'_>,
    door: Door,
) {
    if carry::carrying(ctx) {
        return;
    }
    let artist = LibraryItem {
        kind: media.shelf,
        uri: media.uri.to_owned(),
        name: media.name.to_owned(),
    };
    context_menu::post(
        ctx,
        verbs::artist_request(
            artist,
            door,
            !library.followed_here(media.uri),
            false,
            library.page().cloned(),
        ),
    );
}

/// Posts the menu for a card whose subject is a song.
pub(in crate::view) fn post_track(
    ctx: &Context,
    library: &Library,
    entry: &TrackEntry,
    door: Door,
) {
    if carry::carrying(ctx) {
        return;
    }
    let standing = library.likes.standing(&[entry.uri.as_str()]);
    if let Some(request) = verbs::track_request(entry, door, standing, library.page().cloned()) {
        context_menu::post(ctx, request);
    }
}
