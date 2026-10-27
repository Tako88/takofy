use librespot::core::{SpotifyId, SpotifyUri};
use librespot::metadata::Show;
use librespot::protocol::metadata::Episode as EpisodeMessage;
use librespot::protocol::metadata::Show as ShowMessage;

use super::{detail, episode_uris};

/// A 16-byte gid, the only length `SpotifyId::from_raw` accepts.
fn gid(byte: u8) -> Vec<u8> {
    vec![byte; 16]
}

/// A protocol show whose episodes are named by their gid byte.
fn wire_show(name: &str, publisher: &str, episodes: &[u8]) -> ShowMessage {
    let mut show = ShowMessage::default();
    show.set_gid(gid(1));
    show.set_name(name.to_owned());
    show.set_publisher(publisher.to_owned());
    for byte in episodes {
        let mut episode = EpisodeMessage::default();
        episode.set_gid(gid(*byte));
        show.episode.push(episode);
    }
    show
}

/// The parsed show the wire's message converts to.
fn show(name: &str, publisher: &str, episodes: &[u8]) -> Show {
    Show::try_from(&wire_show(name, publisher, episodes)).expect("a valid show message")
}

/// The URI the wire's gid byte resolves to.
fn uri(byte: u8) -> String {
    SpotifyUri::Episode {
        id: SpotifyId::from_raw(&gid(byte)).expect("a 16-byte id"),
    }
    .to_uri()
    .expect("a uri")
}

#[test]
fn order_is_preserved() {
    let show = show("Record", "Band", &[10, 11, 12]);
    assert_eq!(
        episode_uris(&show),
        vec![uri(10), uri(11), uri(12)],
        "the page never sorts a show's episodes"
    );
}

#[test]
fn a_non_episode_entry_is_skipped() {
    let mut show = show("Record", "Band", &[10, 11]);
    show.episodes.push(SpotifyUri::Track {
        id: SpotifyId::from_raw(&gid(99)).expect("a 16-byte id"),
    });
    assert_eq!(
        episode_uris(&show),
        vec![uri(10), uri(11)],
        "a track in the episode list is not an episode"
    );
}

#[test]
fn an_empty_name_falls_back_to_the_uri() {
    let show = show("", "Band", &[]);
    let detail = detail("spotify:show:x", &show, Vec::new());
    assert_eq!(detail.show.name, "spotify:show:x");
    assert_eq!(detail.show.uri, "spotify:show:x");
}

#[test]
fn a_missing_cover_is_none() {
    let show = show("Record", "Band", &[]);
    assert_eq!(detail("spotify:show:x", &show, Vec::new()).show.cover, None);
}

#[test]
fn a_blank_publisher_is_an_empty_string_not_some_space() {
    let show = show("Record", "   ", &[]);
    assert_eq!(
        detail("spotify:show:x", &show, Vec::new()).show.publisher,
        ""
    );
}
