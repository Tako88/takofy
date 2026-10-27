use super::super::*;

pub(super) fn track(uri: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: String::new(),
        album: None,
        album_uri: None,
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

pub(super) fn tracks(uris: &[&str]) -> Vec<TrackEntry> {
    uris.iter().map(|uri| track(uri)).collect()
}

pub(super) fn reply(query: &str, uris: &[&str]) -> Update {
    Update::Search {
        query: query.to_owned(),
        tracks: tracks(uris),
    }
}

pub(super) fn failed(query: &str) -> Update {
    Update::SearchFailed {
        query: query.to_owned(),
        failure: Failure::Search,
    }
}

pub(super) fn search(query: &str) -> Command {
    Command::Search {
        query: query.to_owned(),
    }
}

pub(super) fn rich(uri: &str, album: &str, artist: &str) -> TrackEntry {
    TrackEntry {
        uri: uri.to_owned(),
        title: uri.to_owned(),
        artists: artist.to_owned(),
        album: Some(album.to_owned()),
        album_uri: Some(format!("spotify:album:{album}")),
        artist_uris: vec![format!("spotify:artist:{artist}")],
        artist_names: vec![artist.to_owned()],
        added_at: None,
        duration_ms: None,
        named: true,
        cover: None,
    }
}

pub(super) fn rich_reply(query: &str, count: usize) -> Update {
    Update::Search {
        query: query.to_owned(),
        tracks: (0..count)
            .map(|index| {
                rich(
                    &format!("t{index}"),
                    &format!("a{index}"),
                    &format!("r{index}"),
                )
            })
            .collect(),
    }
}

pub(super) fn banded() -> Search {
    let mut state = typed("abc");
    state.apply(&rich_reply("abc", 8));
    state
}

/// Types `text` into the field, as one frame of the real path would.
pub(super) fn type_text(state: &mut Search, text: &str) {
    state.text = text.to_owned();
    let input = KeyInput {
        typed: true,
        focused: true,
        ..KeyInput::default()
    };
    assert_eq!(
        state.step(input),
        Act::None,
        "typing only changes the field"
    );
}

/// Types `text` and hands back the state the field would be left in.
pub(super) fn typed(text: &str) -> Search {
    let mut state = Search::default();
    type_text(&mut state, text);
    state
}

/// A frame with the field focused and only `key` pressed.
pub(super) fn key(name: &str) -> KeyInput {
    KeyInput {
        down: name == "down",
        up: name == "up",
        enter: name == "enter",
        escape: name == "escape",
        typed: false,
        clicked_field: false,
        focused: true,
    }
}

/// The page ready on `abc` with `count` distinct artists, and the menu
/// closed, as landing on the page leaves it.
pub(super) fn paged(count: usize) -> Search {
    let mut state = typed("abc");
    state.open_results("abc");
    state.apply(&rich_reply("abc", count));
    state.close();
    state
}
