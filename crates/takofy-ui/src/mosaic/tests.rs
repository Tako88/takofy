use egui::TextureId;
use takofy_model::{CoverOrigin, CoverSet, CoverUrl, TrackEntry};

use super::*;

fn url(tail: &str) -> CoverSet {
    CoverSet::single(
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url"),
    )
}

fn track(cover: &str) -> TrackEntry {
    TrackEntry {
        uri: format!("spotify:track:{cover}"),
        title: cover.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: Some(url(cover)),
    }
}

fn bare(tail: &str) -> TrackEntry {
    TrackEntry {
        cover: None,
        ..track(tail)
    }
}

fn ready() -> Cover {
    Cover::Ready(TextureId::Managed(1))
}

#[test]
fn the_first_four_distinct_covers_in_track_order() {
    let tracks = [track("a"), track("b"), track("c"), track("d"), track("e")];
    assert_eq!(urls(&tracks), vec![url("a"), url("b"), url("c"), url("d")]);
}

#[test]
fn a_repeated_album_counts_once() {
    let tracks = [track("a"), track("b"), track("a"), track("c")];
    assert_eq!(urls(&tracks), vec![url("a"), url("b"), url("c")]);
}

#[test]
fn tracks_without_a_cover_are_skipped() {
    let tracks = [bare("x"), track("a"), bare("y"), track("b")];
    assert_eq!(urls(&tracks), vec![url("a"), url("b")]);
}

#[test]
fn a_scan_stops_at_four() {
    let tracks: Vec<TrackEntry> = (0..8).map(|index| track(&format!("c{index}"))).collect();
    assert_eq!(urls(&tracks).len(), QUADRANTS);
}

#[test]
fn one_cover_is_no_mosaic() {
    assert!(urls(&[track("a")]).is_empty());
    assert!(urls(&[track("a"), track("a")]).is_empty());
    assert!(wanted(Some(CoverOrigin::Derived), &urls(&[track("a")])).is_empty());
}

#[test]
fn two_covers_are_a_mosaic() {
    assert_eq!(
        wanted(Some(CoverOrigin::Derived), &[url("a"), url("b")]),
        vec![url("a"), url("b")]
    );
}

#[test]
fn a_chosen_cover_is_never_a_mosaic() {
    assert!(
        wanted(
            Some(CoverOrigin::Chosen),
            &[url("a"), url("b"), url("c"), url("d")]
        )
        .is_empty()
    );
}

#[test]
fn a_derived_or_absent_origin_may_be_a_mosaic() {
    let found = vec![url("a"), url("b")];
    assert_eq!(wanted(Some(CoverOrigin::Derived), &found), found);
    assert_eq!(wanted(None, &found), found);
}

#[test]
fn arrange_is_none_until_every_cover_is_ready() {
    assert_eq!(arrange(&[ready(), Cover::Loading]), None);
    assert_eq!(arrange(&[ready(), Cover::Failed]), None);
    assert_eq!(arrange(&[Cover::Loading, ready()]), None);
}

#[test]
fn arrange_pads_missing_quadrants_with_holes() {
    let arranged = arrange(&[ready(), ready()]).expect("two ready covers tile");
    assert_eq!(arranged, [ready(), ready(), Cover::Missing, Cover::Missing]);
}

#[test]
fn arrange_is_none_for_no_covers() {
    assert_eq!(arrange(&[]), None);
}

#[test]
fn kept_answers_only_for_its_own_playlist() {
    let kept = Kept::new("spotify:playlist:a".to_owned(), vec![url("a"), url("b")]);
    assert_eq!(
        kept.urls_for("spotify:playlist:a"),
        Some([url("a"), url("b")].as_slice())
    );
    assert_eq!(kept.urls_for("spotify:playlist:b"), None);
}
