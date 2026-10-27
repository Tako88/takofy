use super::super::*;
use super::test_support::*;

use crate::show::ShowError;
use librespot::core::{SpotifyId, SpotifyUri};
use librespot::metadata::Show;
use librespot::protocol::metadata::image::Size;
use librespot::protocol::metadata::{ImageGroup, Show as ShowMessage};

/// A protocol show, the way `Show::get` answers one.
fn wire_show(name: &str, publisher: &str, cover: Option<ImageGroup>) -> ShowMessage {
    let mut show = ShowMessage::default();
    show.set_gid(gid(1));
    show.set_name(name.to_owned());
    show.set_publisher(publisher.to_owned());
    if let Some(group) = cover {
        show.cover_image = Some(group).into();
    }
    show
}

/// The parsed show the wire's message converts to.
fn show(name: &str, publisher: &str, cover: Option<ImageGroup>) -> Show {
    Show::try_from(&wire_show(name, publisher, cover)).expect("a valid show message")
}

#[test]
fn a_show_maps_name_publisher_and_cover() {
    let card = show_card(&show(
        "A Show",
        "A Publisher",
        Some(grouped_cover(Size::DEFAULT, &[7; 20])),
    ))
    .expect("a named show");
    assert_eq!(card.name, "A Show");
    assert_eq!(card.publisher.as_deref(), Some("A Publisher"));
    assert_eq!(
        card.cover
            .as_ref()
            .and_then(|cover| cover.variants().first())
            .map(|variant| variant.url.as_str()),
        Some("https://i.scdn.co/image/0707070707070707070707070707070707070707")
    );
}

#[test]
fn a_blank_publisher_is_none() {
    let card = show_card(&show("A Show", "  \n", None)).expect("a named show");
    assert_eq!(card.publisher, None);
}

#[test]
fn an_empty_name_is_no_card() {
    assert_eq!(show_card(&show("", "A Publisher", None)), None);
}

#[test]
fn a_show_without_a_cover_has_none() {
    let card = show_card(&show("A Show", "A Publisher", None)).expect("a named show");
    assert_eq!(card.cover, None);
}

#[test]
fn parse_show_uri_accepts_a_show_and_refuses_an_album_and_garbage() {
    let uri = show_uri(1);
    let id = parse_show_uri(&uri).expect("a show uri");
    assert_eq!(
        id,
        SpotifyUri::Show {
            id: SpotifyId::from_raw(&gid(1)).expect("a 16-byte id"),
        }
    );
    assert!(
        matches!(
            parse_show_uri(&format!("spotify:album:{TRACK_ID}")),
            Err(ShowError::NotAShow)
        ),
        "an album uri names no show"
    );
    assert!(
        matches!(parse_show_uri("spotify:album:x"), Err(ShowError::NotAShow)),
        "spotify:album:x is not a show"
    );
    assert!(
        matches!(parse_show_uri("garbage"), Err(ShowError::NotAShow)),
        "garbage is not a show"
    );
}

#[test]
fn needs_labels_is_false_for_an_empty_order() {
    assert!(!needs_labels(&[]));
}

#[test]
fn needs_labels_is_true_for_any_show() {
    assert!(needs_labels(&owned(&[&show_uri(1)])));
}
