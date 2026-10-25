//! What a search answer contains besides its tracks: the albums they sit on
//! and the artists they credit.
//!
//! Pure, so the derivation and its caps are testable without a window.

use takofy_model::{CoverSet, TrackEntry};

/// How many rows of each kind a surface shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub tracks: usize,
    pub albums: usize,
    pub artists: usize,
}

/// The drop-down's caps: a surface property, not the derivation's, so the
/// results page asks for its own from the same shape.
pub const MENU: Caps = Caps {
    tracks: 3,
    albums: 2,
    artists: 2,
};

/// The results page's caps: ten of each kind until a block's reveal is taken.
pub const PAGE: Caps = Caps {
    tracks: 10,
    albums: 10,
    artists: 10,
};

/// One album an answer's tracks sit on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Album {
    pub uri: String,
    pub name: String,
    /// The first crediting track's first artist: the payload names no album
    /// performer of its own.
    pub artist: String,
    pub cover: Option<CoverSet>,
}

/// One artist credited on an answer's tracks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artist {
    pub uri: String,
    pub name: String,
    /// How many answer tracks credit this artist; a track crediting them twice
    /// counts once.
    pub tracks: usize,
    /// The cover of the record the match sits on, for an artist with no portrait.
    pub record_cover: Option<CoverSet>,
}

/// The lists an answer derives, in first-appearance order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Index {
    /// The answer's length, which is not capped: a band's total is the truth.
    pub tracks: usize,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}

impl Index {
    /// Derives the albums and artists of `answer`.
    pub fn derive(answer: &[TrackEntry]) -> Self {
        let mut index = Self {
            tracks: answer.len(),
            ..Self::default()
        };
        for entry in answer {
            if let Some(album) = album_of(entry) {
                match index.albums.iter_mut().find(|found| found.uri == album.uri) {
                    // A later track fills the cover only when the first had none.
                    Some(found) => {
                        if found.cover.is_none() {
                            found.cover = album.cover;
                        }
                    }
                    None => index.albums.push(album),
                }
            }
            for (position, (uri, name)) in entry
                .artist_uris
                .iter()
                .zip(&entry.artist_names)
                .enumerate()
            {
                // A track crediting the same artist twice is one credit.
                if entry.artist_uris[..position].contains(uri) {
                    continue;
                }
                match index.artists.iter_mut().find(|found| &found.uri == uri) {
                    Some(found) => {
                        found.tracks += 1;
                        if found.record_cover.is_none() {
                            found.record_cover = entry.cover.clone();
                        }
                    }
                    None => index.artists.push(Artist {
                        uri: uri.clone(),
                        name: name.clone(),
                        tracks: 1,
                        record_cover: entry.cover.clone(),
                    }),
                }
            }
        }
        index
    }

    /// The bands `caps` draws from this index, in band order, empty bands
    /// omitted.
    pub fn shape(&self, caps: Caps) -> Shape {
        let bands: Vec<Band> = [
            (Kind::Tracks, self.tracks, caps.tracks),
            (Kind::Albums, self.albums.len(), caps.albums),
            (Kind::Artists, self.artists.len(), caps.artists),
        ]
        .into_iter()
        .filter(|(_, total, _)| *total > 0)
        .map(|(kind, total, cap)| Band {
            kind,
            shown: cap.min(total),
            total,
        })
        .collect();
        let floor = bands.iter().any(|band| band.shown < band.total);
        Shape { bands, floor }
    }
}

/// The album one track contributes, or `None` when it names no album.
fn album_of(entry: &TrackEntry) -> Option<Album> {
    Some(Album {
        uri: entry.album_uri.clone()?,
        name: entry.album.clone()?,
        artist: entry
            .artist_names
            .first()
            .cloned()
            .unwrap_or_else(|| entry.artists.clone()),
        cover: entry.cover.clone(),
    })
}

/// `"1 track"`, or `"N tracks"` for any other count.
pub fn tracks_phrase(tracks: usize) -> String {
    if tracks == 1 {
        "1 track".to_owned()
    } else {
        format!("{tracks} tracks")
    }
}

/// The kinds of row the menu walks, in the order their bands appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Tracks,
    Albums,
    Artists,
}

/// One band: a kind, how many of its rows a surface shows, and how many exist.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Band {
    pub kind: Kind,
    pub shown: usize,
    pub total: usize,
}

impl Kind {
    /// The kind's heading: what a menu band or a page block is called.
    pub fn heading(self) -> &'static str {
        match self {
            Kind::Tracks => "Tracks",
            Kind::Albums => "Albums",
            Kind::Artists => "Artists",
        }
    }
}

impl Band {
    /// The band's heading, in the menu's head grammar.
    pub fn heading(&self) -> &'static str {
        self.kind.heading()
    }

    /// The showing-versus-total phrase beside the heading.
    pub fn share(&self) -> String {
        if self.shown < self.total {
            format!("{} of {}", self.shown, self.total)
        } else {
            self.total.to_string()
        }
    }

    /// The heading and share as one phrase, for a screen reader: `Tracks, 10 of 20`.
    pub fn spoken(&self) -> String {
        format!("{}, {}", self.heading(), self.share())
    }

    /// The reveal row's words, or `None` when nothing is hidden.
    pub fn more(&self) -> Option<String> {
        let hidden = self
            .total
            .checked_sub(self.shown)
            .filter(|hidden| *hidden > 0)?;
        let noun = match (self.kind, hidden) {
            (Kind::Tracks, 1) => "track",
            (Kind::Tracks, _) => "tracks",
            (Kind::Albums, 1) => "album",
            (Kind::Albums, _) => "albums",
            (Kind::Artists, 1) => "artist",
            (Kind::Artists, _) => "artists",
        };
        Some(format!("Show {hidden} more {noun}"))
    }
}

/// The bands a surface draws, and whether a floor row sits under them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    pub bands: Vec<Band>,
    pub floor: bool,
}

impl Shape {
    /// How many rows the keyboard walks: every shown row, then the floor.
    pub fn len(&self) -> usize {
        self.bands.iter().map(|band| band.shown).sum::<usize>() + usize::from(self.floor)
    }

    /// Whether there is nothing to walk.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The row at `ordinal`, walking bands in order and the floor last.
    pub fn row(&self, ordinal: usize) -> Option<Row> {
        let mut walked = 0;
        for band in &self.bands {
            if ordinal < walked + band.shown {
                let index = ordinal - walked;
                return Some(match band.kind {
                    Kind::Tracks => Row::Track(index),
                    Kind::Albums => Row::Album(index),
                    Kind::Artists => Row::Artist(index),
                });
            }
            walked += band.shown;
        }
        (self.floor && ordinal == walked).then_some(Row::Floor)
    }

    /// The ordinal `row` sits at, or `None` when the shape does not draw it.
    pub fn ordinal(&self, row: Row) -> Option<usize> {
        let (kind, index) = match row {
            Row::Track(index) => (Kind::Tracks, index),
            Row::Album(index) => (Kind::Albums, index),
            Row::Artist(index) => (Kind::Artists, index),
            Row::Floor => return self.floor.then(|| self.len().saturating_sub(1)),
        };
        let mut walked = 0;
        for band in &self.bands {
            if band.kind == kind {
                return (index < band.shown).then_some(walked + index);
            }
            walked += band.shown;
        }
        None
    }

    /// The shown rows per band, as the layout stacks them.
    pub fn rows_per_band(&self) -> Vec<usize> {
        self.bands.iter().map(|band| band.shown).collect()
    }
}

/// One row of the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Row {
    /// An index into the whole answer, which is a prefix of it.
    Track(usize),
    /// An index into [`Index::albums`].
    Album(usize),
    /// An index into [`Index::artists`].
    Artist(usize),
    Floor,
}

#[cfg(test)]
mod tests {
    use takofy_model::{CoverSet, CoverUrl, TrackEntry};

    use super::{Album, Artist, Band, Caps, Index, Kind, MENU, PAGE, Row, tracks_phrase};

    fn entry(uri: &str) -> TrackEntry {
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

    fn cover(name: &str) -> CoverSet {
        CoverSet::single(
            CoverUrl::from_wire(&format!("https://i.scdn.co/image/{name}")).expect("a cover url"),
        )
    }

    fn on_album(mut entry: TrackEntry, name: &str, uri: &str) -> TrackEntry {
        entry.album = Some(name.to_owned());
        entry.album_uri = Some(uri.to_owned());
        entry
    }

    fn credited(mut entry: TrackEntry, artists: &[(&str, &str)]) -> TrackEntry {
        entry.artists = artists
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ");
        entry.artist_names = artists.iter().map(|(name, _)| (*name).to_owned()).collect();
        entry.artist_uris = artists.iter().map(|(_, uri)| (*uri).to_owned()).collect();
        entry
    }

    fn covered(mut entry: TrackEntry, name: &str) -> TrackEntry {
        entry.cover = Some(cover(name));
        entry
    }

    fn album_uris(index: &Index) -> Vec<&str> {
        index
            .albums
            .iter()
            .map(|album| album.uri.as_str())
            .collect()
    }

    fn artist_uris(index: &Index) -> Vec<&str> {
        index
            .artists
            .iter()
            .map(|artist| artist.uri.as_str())
            .collect()
    }

    #[test]
    fn albums_are_deduplicated_by_uri_in_first_appearance_order() {
        let answer = [
            on_album(entry("t1"), "B", "spotify:album:b"),
            on_album(entry("t2"), "A", "spotify:album:a"),
            on_album(entry("t3"), "B again", "spotify:album:b"),
        ];
        let index = Index::derive(&answer);
        assert_eq!(
            album_uris(&index),
            vec!["spotify:album:b", "spotify:album:a"]
        );
        assert_eq!(index.albums[0].name, "B", "the first answer names it");
    }

    #[test]
    fn an_answer_full_of_one_album_gives_one_album() {
        let answer: Vec<TrackEntry> = (1..=4)
            .map(|n| on_album(entry(&format!("t{n}")), "One", "spotify:album:one"))
            .collect();
        let index = Index::derive(&answer);
        assert_eq!(album_uris(&index), vec!["spotify:album:one"]);
        assert_eq!(index.albums[0].name, "One");
    }

    #[test]
    fn an_unnamed_first_track_leaves_the_album_to_a_named_one() {
        let mut nameless = entry("t1");
        nameless.album_uri = Some("spotify:album:a".to_owned());
        let answer = [nameless, on_album(entry("t2"), "Named", "spotify:album:a")];
        let index = Index::derive(&answer);
        assert_eq!(album_uris(&index), vec!["spotify:album:a"]);
        assert_eq!(index.albums[0].name, "Named");
    }

    #[test]
    fn an_album_artist_is_the_first_credits_first_artist() {
        let answer = [on_album(
            credited(
                entry("t1"),
                &[
                    ("First", "spotify:artist:1"),
                    ("Second", "spotify:artist:2"),
                ],
            ),
            "Record",
            "spotify:album:a",
        )];
        let index = Index::derive(&answer);
        assert_eq!(index.albums[0].artist, "First");
    }

    #[test]
    fn an_album_artist_falls_back_to_the_joined_credit_when_no_names_are_kept() {
        let mut track = on_album(entry("t1"), "Record", "spotify:album:a");
        track.artists = "A, B".to_owned();
        let index = Index::derive(&[track]);
        assert_eq!(index.albums[0].artist, "A, B");
    }

    #[test]
    fn a_track_without_an_album_uri_or_name_contributes_no_album() {
        let mut named_only = entry("t1");
        named_only.album = Some("Record".to_owned());
        let mut uri_only = entry("t2");
        uri_only.album_uri = Some("spotify:album:a".to_owned());
        let index = Index::derive(&[named_only, uri_only]);
        assert!(index.albums.is_empty());
    }

    #[test]
    fn an_album_cover_is_the_first_one_the_answer_has() {
        let answer = [
            on_album(entry("t1"), "Record", "spotify:album:a"),
            on_album(covered(entry("t2"), "later"), "Record", "spotify:album:a"),
            on_album(covered(entry("t3"), "latest"), "Record", "spotify:album:a"),
        ];
        let index = Index::derive(&answer);
        assert_eq!(index.albums[0].cover, Some(cover("later")));
    }

    #[test]
    fn a_three_credit_track_gives_three_artists() {
        let answer = [credited(
            entry("t1"),
            &[
                ("A", "spotify:artist:a"),
                ("B", "spotify:artist:b"),
                ("C", "spotify:artist:c"),
            ],
        )];
        let index = Index::derive(&answer);
        assert_eq!(
            artist_uris(&index),
            vec!["spotify:artist:a", "spotify:artist:b", "spotify:artist:c"]
        );
    }

    #[test]
    fn artists_are_deduplicated_by_uri_in_first_appearance_order() {
        let answer = [
            credited(entry("t1"), &[("B", "spotify:artist:b")]),
            credited(entry("t2"), &[("A", "spotify:artist:a")]),
            credited(entry("t3"), &[("B", "spotify:artist:b")]),
        ];
        let index = Index::derive(&answer);
        assert_eq!(
            artist_uris(&index),
            vec!["spotify:artist:b", "spotify:artist:a"]
        );
        assert_eq!(index.artists[0].name, "B");
        assert_eq!(index.artists[0].tracks, 2);
        assert_eq!(index.artists[1].tracks, 1);
    }

    #[test]
    fn a_track_crediting_one_uri_twice_counts_once() {
        let answer = [credited(
            entry("t1"),
            &[("A", "spotify:artist:a"), ("A again", "spotify:artist:a")],
        )];
        let index = Index::derive(&answer);
        assert_eq!(artist_uris(&index), vec!["spotify:artist:a"]);
        assert_eq!(index.artists[0].tracks, 1);
        assert_eq!(index.artists[0].name, "A", "the first credit names the row");
    }

    #[test]
    fn the_track_count_is_the_answers_length() {
        for length in [0, 3, 7, 25] {
            let answer: Vec<TrackEntry> = (0..length).map(|n| entry(&format!("t{n}"))).collect();
            assert_eq!(Index::derive(&answer).tracks, length);
        }
    }

    #[test]
    fn an_empty_answer_gives_an_empty_index() {
        let index = Index::derive(&[]);
        assert_eq!(index, Index::default());
    }

    #[test]
    fn the_totals_are_the_derived_lengths_whatever_the_answer_size() {
        for length in [3, 7, 25] {
            let answer: Vec<TrackEntry> = (0..length)
                .map(|n| {
                    credited(
                        on_album(
                            entry(&format!("t{n}")),
                            &format!("Record {n}"),
                            &format!("spotify:album:{n}"),
                        ),
                        &[("A", "spotify:artist:a")],
                    )
                })
                .collect();
            let index = Index::derive(&answer);
            assert_eq!(index.tracks, length);
            assert_eq!(index.albums.len(), length);
            assert_eq!(index.artists.len(), 1, "one artist, counted per track");
            assert_eq!(index.artists[0].tracks, length);
        }
    }

    #[test]
    fn a_record_cover_is_the_first_crediting_tracks_own_cover() {
        let answer = [
            credited(entry("t1"), &[("A", "spotify:artist:a")]),
            credited(covered(entry("t2"), "first"), &[("A", "spotify:artist:a")]),
            credited(covered(entry("t3"), "second"), &[("A", "spotify:artist:a")]),
        ];
        let index = Index::derive(&answer);
        assert_eq!(index.artists[0].record_cover, Some(cover("first")));
    }

    #[test]
    fn a_track_phrase_is_singular_only_for_one() {
        assert_eq!(tracks_phrase(0), "0 tracks");
        assert_eq!(tracks_phrase(1), "1 track");
        assert_eq!(tracks_phrase(2), "2 tracks");
    }

    fn index(tracks: usize, albums: usize, artists: usize) -> Index {
        Index {
            tracks,
            albums: (0..albums)
                .map(|n| Album {
                    uri: format!("spotify:album:{n}"),
                    name: format!("Record {n}"),
                    artist: "A".to_owned(),
                    cover: None,
                })
                .collect(),
            artists: (0..artists)
                .map(|n| Artist {
                    uri: format!("spotify:artist:{n}"),
                    name: format!("Artist {n}"),
                    tracks: 1,
                    record_cover: None,
                })
                .collect(),
        }
    }

    #[test]
    fn the_menu_caps_show_three_tracks_two_albums_and_two_artists_in_order() {
        let shape = index(20, 6, 4).shape(MENU);
        let bands: Vec<(Kind, usize, usize)> = shape
            .bands
            .iter()
            .map(|band| (band.kind, band.shown, band.total))
            .collect();
        assert_eq!(
            bands,
            vec![
                (Kind::Tracks, 3, 20),
                (Kind::Albums, 2, 6),
                (Kind::Artists, 2, 4),
            ]
        );
        assert!(shape.floor);
    }

    #[test]
    fn a_band_with_nothing_in_it_is_omitted() {
        let shape = index(2, 0, 2).shape(MENU);
        let kinds: Vec<Kind> = shape.bands.iter().map(|band| band.kind).collect();
        assert_eq!(kinds, vec![Kind::Tracks, Kind::Artists]);
        assert!(!shape.floor, "nothing was hidden");
    }

    #[test]
    fn the_floor_exists_exactly_when_a_band_hid_something() {
        assert!(!index(3, 2, 2).shape(MENU).floor, "exactly at the caps");
        assert!(!index(1, 1, 1).shape(MENU).floor);
        assert!(index(4, 2, 2).shape(MENU).floor, "one track over");
        assert!(index(0, 3, 0).shape(MENU).floor, "one album over");
        assert!(index(0, 0, 3).shape(MENU).floor, "one artist over");
    }

    #[test]
    fn a_share_says_what_is_hidden_and_what_is_not() {
        let shape = index(20, 2, 0).shape(MENU);
        assert_eq!(shape.bands[0].share(), "3 of 20");
        assert_eq!(shape.bands[1].share(), "2", "a band showing everything");
        let single = index(1, 0, 0).shape(MENU);
        assert_eq!(single.bands[0].share(), "1");
    }

    #[test]
    fn a_band_heading_names_its_kind() {
        let shape = index(3, 2, 2).shape(MENU);
        let headings: Vec<&str> = shape.bands.iter().map(|band| band.heading()).collect();
        assert_eq!(headings, vec!["Tracks", "Albums", "Artists"]);
    }

    #[test]
    fn the_walk_and_the_ordinal_round_trip_for_every_shape() {
        let cases = [
            (index(2, 0, 0).shape(MENU), false),
            (index(20, 0, 0).shape(MENU), true),
            (index(3, 2, 0).shape(MENU), false),
            (index(20, 4, 0).shape(MENU), true),
            (index(3, 2, 2).shape(MENU), false),
            (index(20, 6, 5).shape(MENU), true),
        ];
        for (shape, floor) in cases {
            assert_eq!(shape.floor, floor);
            for ordinal in 0..shape.len() {
                let row = shape.row(ordinal).expect("every ordinal is a row");
                assert_eq!(shape.ordinal(row), Some(ordinal));
            }
            assert_eq!(shape.row(shape.len()), None, "past the end");
            assert_eq!(shape.ordinal(Row::Track(shape.len())), None, "past the end");
        }
    }

    #[test]
    fn the_walk_crosses_tracks_then_albums_then_artists_then_the_floor() {
        let shape = index(20, 6, 5).shape(MENU);
        let rows: Vec<Row> = (0..shape.len()).filter_map(|n| shape.row(n)).collect();
        assert_eq!(
            rows,
            vec![
                Row::Track(0),
                Row::Track(1),
                Row::Track(2),
                Row::Album(0),
                Row::Album(1),
                Row::Artist(0),
                Row::Artist(1),
                Row::Floor,
            ]
        );
    }

    #[test]
    fn without_a_floor_the_walk_ends_on_the_last_shown_row() {
        let shape = index(3, 2, 2).shape(MENU);
        assert_eq!(shape.row(shape.len() - 1), Some(Row::Artist(1)));
        assert_eq!(shape.ordinal(Row::Floor), None);
    }

    #[test]
    fn a_track_row_is_an_index_into_the_whole_answer() {
        let shape = index(20, 0, 0).shape(MENU);
        assert_eq!(shape.row(2), Some(Row::Track(2)));
        assert_eq!(shape.ordinal(Row::Track(2)), Some(2));
    }

    #[test]
    fn the_caps_are_a_parameter_not_a_property_of_the_derivation() {
        let wide = Caps {
            tracks: 10,
            albums: 10,
            artists: 10,
        };
        let shape = index(25, 12, 11).shape(wide);
        let shown: Vec<usize> = shape.rows_per_band();
        assert_eq!(shown, vec![10, 10, 10]);
        assert!(shape.floor);
        assert_eq!(shape.len(), 31);
        assert_eq!(shape.row(30), Some(Row::Floor));

        let all = Caps {
            tracks: 25,
            albums: 12,
            artists: 11,
        };
        assert!(!index(25, 12, 11).shape(all).floor);
    }

    #[test]
    fn the_page_shows_ten_of_each_kind() {
        assert_eq!((PAGE.tracks, PAGE.albums, PAGE.artists), (10, 10, 10));
        let shape = index(25, 12, 11).shape(PAGE);
        let shares: Vec<String> = shape.bands.iter().map(Band::share).collect();
        assert_eq!(shares, ["10 of 25", "10 of 12", "10 of 11"]);
    }

    fn band(kind: Kind, shown: usize, total: usize) -> Band {
        Band { kind, shown, total }
    }

    #[test]
    fn a_band_offers_the_rest_only_while_something_is_hidden() {
        assert_eq!(band(Kind::Tracks, 10, 10).more(), None);
        assert_eq!(band(Kind::Tracks, 3, 3).more(), None);
        assert_eq!(
            band(Kind::Tracks, 10, 20).more().as_deref(),
            Some("Show 10 more tracks")
        );
        assert_eq!(
            band(Kind::Albums, 10, 13).more().as_deref(),
            Some("Show 3 more albums")
        );
        assert_eq!(
            band(Kind::Artists, 10, 11).more().as_deref(),
            Some("Show 1 more artist")
        );
        assert_eq!(
            band(Kind::Tracks, 10, 11).more().as_deref(),
            Some("Show 1 more track")
        );
        assert_eq!(
            band(Kind::Albums, 10, 11).more().as_deref(),
            Some("Show 1 more album")
        );
    }

    #[test]
    fn a_band_speaks_its_heading_and_share() {
        assert_eq!(band(Kind::Tracks, 10, 20).spoken(), "Tracks, 10 of 20");
        assert_eq!(band(Kind::Albums, 6, 6).spoken(), "Albums, 6");
    }
}
