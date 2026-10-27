use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use librespot::protocol::playlist4_external::{Item, ListItems, SelectedListContent};
use protobuf::{Message, MessageField};

use super::{
    MAX_ITEMS, PAGE_SIZE, RootlistError, RootlistPage, RootlistSource, RootlistWire, fetch_all,
    parse_rootlist,
};
use takofy_model::{PlaylistAccess, Revision};

const PLAYLIST_A: &str = "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M";
const PLAYLIST_B: &str = "spotify:playlist:37i9dQZF1DX0XUsuxWHRQd";
const FOLDER: &str = "spotify:user:someone:folder:abc";

fn item(uri: &str) -> Item {
    Item {
        uri: Some(uri.to_owned()),
        ..Default::default()
    }
}

fn binary(items: Vec<Item>, length: Option<i32>) -> Vec<u8> {
    binary_at(items, length, None)
}

fn binary_at(items: Vec<Item>, length: Option<i32>, revision: Option<&[u8]>) -> Vec<u8> {
    let mut list = ListItems::new();
    list.pos = Some(0);
    list.truncated = Some(false);
    list.items = items;
    let mut content = SelectedListContent::new();
    content.length = length;
    content.revision = revision.map(<[u8]>::to_vec);
    content.contents = MessageField::some(list);
    content.write_to_bytes().expect("serialize fixture")
}

fn revision(byte: u8) -> Revision {
    Revision::from_wire(&[byte]).expect("a revision")
}

fn page_with(count: usize, total: Option<usize>) -> RootlistPage {
    RootlistPage {
        playlists: (0..count)
            .map(|index| format!("spotify:playlist:{index:022}"))
            .collect(),
        scanned: count,
        total,
        revision: Some(revision(1)),
    }
}

/// One page of `count` playlists, at `revision`, reporting `total`.
fn page_at(
    count: usize,
    total: Option<usize>,
    revision: Option<Revision>,
    scanned: usize,
) -> RootlistPage {
    RootlistPage {
        playlists: (0..count)
            .map(|index| format!("spotify:playlist:{index:022}"))
            .collect(),
        scanned,
        total,
        revision,
    }
}

struct Scripted {
    pages: Mutex<VecDeque<RootlistPage>>,
    repeat: Option<RootlistPage>,
    calls: AtomicUsize,
    froms: Mutex<Vec<usize>>,
    fail: bool,
}

impl Scripted {
    fn new(pages: Vec<RootlistPage>) -> Self {
        Self {
            pages: Mutex::new(pages.into()),
            repeat: None,
            calls: AtomicUsize::new(0),
            froms: Mutex::new(Vec::new()),
            fail: false,
        }
    }

    fn repeating(page: RootlistPage) -> Self {
        Self {
            pages: Mutex::new(VecDeque::new()),
            repeat: Some(page),
            calls: AtomicUsize::new(0),
            froms: Mutex::new(Vec::new()),
            fail: false,
        }
    }

    fn failing() -> Self {
        Self {
            pages: Mutex::new(VecDeque::new()),
            repeat: None,
            calls: AtomicUsize::new(0),
            froms: Mutex::new(Vec::new()),
            fail: true,
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn froms(&self) -> Vec<usize> {
        self.froms.lock().expect("lock").clone()
    }
}

impl RootlistSource for Scripted {
    async fn page(&self, from: usize, _len: usize) -> Result<RootlistPage, RootlistError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.froms.lock().expect("lock").push(from);
        if self.fail {
            return Err(RootlistError::Source("boom".to_owned()));
        }
        if let Some(page) = self.pages.lock().expect("lock").pop_front() {
            return Ok(page);
        }
        match &self.repeat {
            Some(page) => Ok(page.clone()),
            None => Ok(RootlistPage {
                playlists: Vec::new(),
                scanned: 0,
                total: None,
                revision: None,
            }),
        }
    }
}

#[test]
fn parses_playlist_uris() {
    let bytes = binary(vec![item(PLAYLIST_A), item(PLAYLIST_B)], Some(2));
    let page = parse_rootlist(&bytes, RootlistWire::Binary).expect("valid fixture");
    assert_eq!(
        page.playlists,
        vec![PLAYLIST_A.to_owned(), PLAYLIST_B.to_owned()]
    );
    assert_eq!(page.scanned, 2);
}

#[test]
fn total_comes_from_length_field() {
    let bytes = binary(vec![item(PLAYLIST_A)], Some(7));
    let page = parse_rootlist(&bytes, RootlistWire::Binary).expect("valid fixture");
    assert_eq!(page.total, Some(7));
}

#[test]
fn unset_or_negative_length_is_none() {
    let unset = parse_rootlist(&binary(vec![], None), RootlistWire::Binary).expect("fixture");
    assert_eq!(unset.total, None);
    let negative =
        parse_rootlist(&binary(vec![], Some(-1)), RootlistWire::Binary).expect("fixture");
    assert_eq!(negative.total, None);
}

#[test]
fn folder_markers_are_skipped_but_counted() {
    let bytes = binary(vec![item(FOLDER), item(PLAYLIST_A)], Some(2));
    let page = parse_rootlist(&bytes, RootlistWire::Binary).expect("valid fixture");
    assert_eq!(page.playlists, vec![PLAYLIST_A.to_owned()]);
    assert_eq!(page.scanned, 2);
}

#[test]
fn garbage_bytes_are_a_parse_error() {
    let error = parse_rootlist(&[0xff, 0xff, 0xff], RootlistWire::Binary).expect_err("garbage");
    assert!(matches!(
        error,
        RootlistError::Parse {
            wire: RootlistWire::Binary,
            len: 3
        }
    ));
}

#[test]
fn json_wire_parses_the_same_fixture() {
    let bytes = binary_at(
        vec![item(PLAYLIST_A), item(FOLDER)],
        Some(2),
        Some(&[0xde, 0xad]),
    );
    let content = SelectedListContent::parse_from_bytes(&bytes).expect("fixture");
    let json = protobuf_json_mapping::print_to_string(&content).expect("serialize json");
    let page = parse_rootlist(json.as_bytes(), RootlistWire::Json).expect("valid json");
    assert_eq!(page.playlists, vec![PLAYLIST_A.to_owned()]);
    assert_eq!(page.scanned, 2);
    assert_eq!(page.total, Some(2));
    assert_eq!(page.revision, Revision::from_wire(&[0xde, 0xad]));
}

#[tokio::test]
async fn paging_stops_at_total() {
    let source = Scripted::new(vec![page_with(2, Some(2)), page_with(5, None)]);
    let result = fetch_all(&source).await.expect("walk");
    assert_eq!(result.playlists.len(), 2);
    assert_eq!(source.calls(), 1);
}

#[tokio::test]
async fn exact_multiple_of_page_size_terminates() {
    let total = 2 * PAGE_SIZE;
    let source = Scripted::new(vec![
        page_with(PAGE_SIZE, Some(total)),
        page_with(PAGE_SIZE, Some(total)),
        page_with(1, None),
    ]);
    let result = fetch_all(&source).await.expect("walk");
    assert_eq!(result.playlists.len(), total);
    assert_eq!(source.calls(), 2);
}

#[tokio::test]
async fn fetch_all_advances_by_scanned_not_playlists_kept() {
    let all_playlists = Scripted::new(vec![
        page_with(PAGE_SIZE, Some(3 * PAGE_SIZE)),
        page_with(PAGE_SIZE, Some(3 * PAGE_SIZE)),
        page_with(PAGE_SIZE, Some(3 * PAGE_SIZE)),
    ]);
    fetch_all(&all_playlists).await.expect("walk");
    assert_eq!(all_playlists.froms(), vec![0, PAGE_SIZE, 2 * PAGE_SIZE]);

    let folder_heavy = Scripted::new(vec![RootlistPage {
        playlists: vec![PLAYLIST_A.to_owned()],
        scanned: 2,
        total: None,
        revision: None,
    }]);
    fetch_all(&folder_heavy).await.expect("walk");
    assert_eq!(folder_heavy.froms(), vec![0, 2]);
}

#[tokio::test]
async fn empty_page_stops_without_total() {
    let source = Scripted::new(vec![page_with(0, None)]);
    let result = fetch_all(&source).await.expect("walk");
    assert!(result.playlists.is_empty());
    assert_eq!(source.calls(), 1);
}

#[tokio::test]
async fn server_that_ignores_from_hits_the_cap() {
    let source = Scripted::repeating(page_with(PAGE_SIZE, None));
    let error = fetch_all(&source).await.expect_err("cap");
    assert!(matches!(error, RootlistError::TooLarge));
    assert_eq!(source.calls(), MAX_ITEMS / PAGE_SIZE + 1);
}

#[test]
fn parse_keeps_the_revision_of_the_page() {
    let bytes = binary_at(vec![item(PLAYLIST_A)], Some(1), Some(&[0xde, 0xad]));
    let page = parse_rootlist(&bytes, RootlistWire::Binary).expect("valid fixture");
    assert_eq!(page.revision, Revision::from_wire(&[0xde, 0xad]));
}

#[test]
fn a_page_without_a_revision_has_none() {
    let bytes = binary(vec![item(PLAYLIST_A)], Some(1));
    let page = parse_rootlist(&bytes, RootlistWire::Binary).expect("valid fixture");
    assert_eq!(page.revision, None);
}

#[test]
fn an_empty_revision_is_none() {
    let bytes = binary_at(vec![item(PLAYLIST_A)], Some(1), Some(&[]));
    let page = parse_rootlist(&bytes, RootlistWire::Binary).expect("valid fixture");
    assert_eq!(page.revision, None);
}

#[tokio::test]
async fn a_whole_read_of_playlists_is_index_faithful() {
    let source = Scripted::new(vec![page_at(2, Some(2), Some(revision(7)), 2)]);
    let walked = fetch_all(&source).await.expect("walk");
    assert_eq!(walked.playlists.len(), 2);
    assert_eq!(
        walked.access,
        Some(PlaylistAccess {
            revision: revision(7),
            editable: true,
        })
    );
}

#[tokio::test]
async fn a_dropped_item_makes_the_read_not_faithful() {
    // A folder marker keeps its place in `scanned` and takes none in the
    // playlists, so row 1 on screen is row 2 on the wire.
    let source = Scripted::new(vec![page_at(1, Some(2), Some(revision(7)), 2)]);
    let walked = fetch_all(&source).await.expect("walk");
    assert_eq!(walked.playlists.len(), 1);
    assert_eq!(
        walked.access,
        Some(PlaylistAccess {
            revision: revision(7),
            editable: false,
        })
    );
}

#[tokio::test]
async fn an_unknown_total_is_not_faithful() {
    let source = Scripted::new(vec![page_at(1, None, Some(revision(7)), 1)]);
    let walked = fetch_all(&source).await.expect("walk");
    assert!(
        !walked.access.expect("a revision").editable,
        "an unknown total cannot say the read reached the end"
    );
}

#[tokio::test]
async fn a_total_the_read_did_not_reach_is_not_faithful() {
    // The list ends early, so the rows on screen are not the whole list.
    let source = Scripted::new(vec![
        page_at(2, Some(9), Some(revision(7)), 2),
        page_at(0, None, Some(revision(7)), 0),
    ]);
    let walked = fetch_all(&source).await.expect("walk");
    assert_eq!(walked.playlists.len(), 2);
    assert!(!walked.access.expect("a revision").editable);
}

#[tokio::test]
async fn a_revision_that_changes_between_pages_is_not_faithful() {
    let source = Scripted::new(vec![
        page_at(2, Some(4), Some(revision(7)), 2),
        page_at(2, Some(4), Some(revision(8)), 2),
    ]);
    let walked = fetch_all(&source).await.expect("walk");
    assert!(
        !walked.access.expect("a revision").editable,
        "the list changed under the walk"
    );
}

#[tokio::test]
async fn a_page_that_names_no_revision_breaks_the_walk() {
    let source = Scripted::new(vec![
        page_at(2, Some(4), Some(revision(7)), 2),
        page_at(2, Some(4), None, 2),
    ]);
    let walked = fetch_all(&source).await.expect("walk");
    assert!(!walked.access.expect("a revision").editable);
}

#[tokio::test]
async fn the_first_pages_revision_is_the_one_held() {
    let source = Scripted::new(vec![
        page_at(2, Some(4), Some(revision(7)), 2),
        page_at(2, Some(4), Some(revision(7)), 2),
    ]);
    let walked = fetch_all(&source).await.expect("walk");
    assert_eq!(walked.access.expect("a revision").revision, revision(7));
}

#[tokio::test]
async fn a_read_with_no_revision_offers_no_access() {
    let source = Scripted::new(vec![page_at(2, Some(2), None, 2)]);
    let walked = fetch_all(&source).await.expect("walk");
    assert_eq!(walked.access, None);
}

#[tokio::test]
async fn source_error_propagates() {
    let source = Scripted::failing();
    let error = fetch_all(&source).await.expect_err("source failure");
    assert!(matches!(error, RootlistError::Source(_)));
}
