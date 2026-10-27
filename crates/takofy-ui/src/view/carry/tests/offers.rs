use egui::{Rect, pos2, vec2};

use super::super::*;
use super::test_support::*;
use crate::theme::extent;

fn zone() -> Zone {
    let rows = Rect::from_min_size(pos2(0.0, 10.0), vec2(300.0, 4.0 * extent::ROW_H));
    Zone {
        hit: rows,
        list: rows,
        row_h: extent::ROW_H,
        first: 0,
        count: 4,
    }
}

#[test]
fn an_offer_is_read_once() {
    let ctx = context();
    let id = Id::unique("list");
    let mut list = List::open(&ctx, id, 4);
    list.offer("spotify:playlist:a", zone());
    assert_eq!(
        take_offer(&ctx),
        None,
        "the frame's offer is set when the list finishes"
    );

    list.finish(&ctx);
    assert_eq!(
        take_offer(&ctx),
        Some(Offer {
            list: id,
            playlist: "spotify:playlist:a".to_owned(),
            zone: zone(),
        }),
        "the frame's offer names the list, the playlist and the rows"
    );
    assert_eq!(take_offer(&ctx), None, "an offer is read once");

    // A page that draws but offers nothing leaves nothing behind, so the
    // frame after it cannot act on the frame before.
    let list = List::open(&ctx, id, 4);
    list.finish(&ctx);
    assert_eq!(
        take_offer(&ctx),
        None,
        "a page that offers nothing offers nothing"
    );
}

#[test]
fn a_column_offer_is_read_once() {
    let ctx = context();
    offer_column(
        &ctx,
        ColumnOffer {
            zone: zone(),
            drawn: extent::ROW_H,
            rows: None,
        },
    );

    assert_eq!(
        take_column_offer(&ctx),
        Some(ColumnOffer {
            zone: zone(),
            drawn: extent::ROW_H,
            rows: None,
        })
    );
    assert_eq!(take_column_offer(&ctx), None, "an offer is read once");
}

#[test]
fn a_frame_that_offers_no_column_leaves_none_behind() {
    let ctx = context();
    offer_column(
        &ctx,
        ColumnOffer {
            zone: zone(),
            drawn: extent::ROW_H,
            rows: None,
        },
    );
    let _ = take_column_offer(&ctx);

    assert_eq!(
        take_column_offer(&ctx),
        None,
        "a frame that offers nothing offers nothing"
    );
}

/// The column's offer of the rows a query left, drawn at the test zone.
fn column_offer(rows: Option<Vec<usize>>) -> ColumnOffer {
    ColumnOffer {
        zone: zone(),
        drawn: extent::ROW_H,
        rows,
    }
}

#[test]
fn source_row_is_identity_without_rows() {
    let offer = column_offer(None);

    assert_eq!(offer.source_row(0), Some(0));
    assert_eq!(offer.source_row(5), Some(5), "a whole list maps to itself");
}

#[test]
fn source_row_maps_a_slot_to_its_true_row() {
    let offer = column_offer(Some(vec![3, 7]));

    assert_eq!(offer.source_row(0), Some(3), "the first drawn row");
    assert_eq!(offer.source_row(1), Some(7), "the second drawn row");
}

#[test]
fn source_row_past_the_end_is_none() {
    let offer = column_offer(Some(vec![3, 7]));

    assert_eq!(
        offer.source_row(2),
        None,
        "a slot the offer does not hold names no row"
    );
}
