//! Rootlist parsing and paging.
//!
//! The rootlist body is assumed to be a binary [`SelectedListContent`]; the JSON
//! branch exists for the day that assumption is wrong.

use librespot::core::SpotifyUri;
use librespot::protocol::playlist4_external::{Item, SelectedListContent};
use protobuf::Message;
use protobuf_json_mapping::ParseOptions;

/// The wire format a rootlist body arrived in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootlistWire {
    Binary,
    Json,
}

/// One page of the rootlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootlistPage {
    /// Playlist URIs kept from this page.
    pub playlists: Vec<String>,
    /// Raw item count, including folder markers and unparsable URIs.
    pub scanned: usize,
    /// `SelectedListContent.length`, when set and non-negative.
    pub total: Option<usize>,
}

/// Why a rootlist read failed.
#[derive(Debug, thiserror::Error)]
pub enum RootlistError {
    /// The source itself failed; the message is the source's.
    #[error("{0}")]
    Source(String),
    /// The body could not be decoded. Only its size and wire kind are kept.
    #[error("could not parse a {wire:?} rootlist body of {len} bytes")]
    Parse { wire: RootlistWire, len: usize },
    /// More than [`MAX_ITEMS`] raw items were returned.
    #[error("rootlist has more than {MAX_ITEMS} items")]
    TooLarge,
}

/// Reads one page of the rootlist. Implemented over librespot in `remote`.
pub(crate) trait RootlistSource {
    async fn page(&self, from: usize, len: usize) -> Result<RootlistPage, RootlistError>;
}

/// Page size requested from the rootlist endpoint.
const PAGE_SIZE: usize = 120;
/// Hard cap on raw items, so a server that ignores `from` cannot loop forever.
const MAX_ITEMS: usize = 10_000;

/// Decodes a rootlist body and keeps its playlist URIs.
pub fn parse_rootlist(bytes: &[u8], wire: RootlistWire) -> Result<RootlistPage, RootlistError> {
    let len = bytes.len();
    let content = match wire {
        RootlistWire::Binary => SelectedListContent::parse_from_bytes(bytes)
            .map_err(|_| RootlistError::Parse { wire, len })?,
        RootlistWire::Json => {
            let json =
                std::str::from_utf8(bytes).map_err(|_| RootlistError::Parse { wire, len })?;
            let options = ParseOptions {
                ignore_unknown_fields: true,
                ..Default::default()
            };
            protobuf_json_mapping::parse_from_str_with_options::<SelectedListContent>(
                json, &options,
            )
            .map_err(|_| RootlistError::Parse { wire, len })?
        }
    };
    Ok(page_from_content(&content))
}

fn page_from_content(content: &SelectedListContent) -> RootlistPage {
    let items = &content.contents.items;
    let playlists = items.iter().filter_map(playlist_uri).collect();
    RootlistPage {
        playlists,
        scanned: items.len(),
        total: content
            .length
            .and_then(|length| usize::try_from(length).ok()),
    }
}

fn playlist_uri(item: &Item) -> Option<String> {
    let uri = item.uri.as_deref()?;
    if matches!(SpotifyUri::from_uri(uri), Ok(SpotifyUri::Playlist { .. })) {
        Some(uri.to_owned())
    } else {
        None
    }
}

/// Walks the rootlist until `length` items are seen, a page comes back empty, or
/// the raw-item cap trips.
pub(crate) async fn fetch_all(source: &impl RootlistSource) -> Result<Vec<String>, RootlistError> {
    let mut playlists = Vec::new();
    let mut from = 0;
    let mut scanned_total = 0usize;
    loop {
        let page = source.page(from, PAGE_SIZE).await?;
        if page.scanned == 0 {
            break;
        }
        scanned_total += page.scanned;
        playlists.extend(page.playlists);
        if let Some(total) = page.total
            && scanned_total >= total
        {
            break;
        }
        if scanned_total > MAX_ITEMS {
            return Err(RootlistError::TooLarge);
        }
        from += page.scanned;
    }
    Ok(playlists)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use librespot::protocol::playlist4_external::{Item, ListItems, SelectedListContent};
    use protobuf::{Message, MessageField};

    use super::{
        MAX_ITEMS, PAGE_SIZE, RootlistError, RootlistPage, RootlistSource, RootlistWire, fetch_all,
        parse_rootlist,
    };

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
        let mut list = ListItems::new();
        list.pos = Some(0);
        list.truncated = Some(false);
        list.items = items;
        let mut content = SelectedListContent::new();
        content.length = length;
        content.contents = MessageField::some(list);
        content.write_to_bytes().expect("serialize fixture")
    }

    fn page_with(count: usize, total: Option<usize>) -> RootlistPage {
        RootlistPage {
            playlists: (0..count)
                .map(|index| format!("spotify:playlist:{index:022}"))
                .collect(),
            scanned: count,
            total,
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
        let bytes = binary(vec![item(PLAYLIST_A), item(FOLDER)], Some(2));
        let content = SelectedListContent::parse_from_bytes(&bytes).expect("fixture");
        let json = protobuf_json_mapping::print_to_string(&content).expect("serialize json");
        let page = parse_rootlist(json.as_bytes(), RootlistWire::Json).expect("valid json");
        assert_eq!(page.playlists, vec![PLAYLIST_A.to_owned()]);
        assert_eq!(page.scanned, 2);
        assert_eq!(page.total, Some(2));
    }

    #[tokio::test]
    async fn paging_stops_at_total() {
        let source = Scripted::new(vec![page_with(2, Some(2)), page_with(5, None)]);
        let result = fetch_all(&source).await.expect("walk");
        assert_eq!(result.len(), 2);
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
        assert_eq!(result.len(), total);
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
        }]);
        fetch_all(&folder_heavy).await.expect("walk");
        assert_eq!(folder_heavy.froms(), vec![0, 2]);
    }

    #[tokio::test]
    async fn empty_page_stops_without_total() {
        let source = Scripted::new(vec![page_with(0, None)]);
        let result = fetch_all(&source).await.expect("walk");
        assert!(result.is_empty());
        assert_eq!(source.calls(), 1);
    }

    #[tokio::test]
    async fn server_that_ignores_from_hits_the_cap() {
        let source = Scripted::repeating(page_with(PAGE_SIZE, None));
        let error = fetch_all(&source).await.expect_err("cap");
        assert!(matches!(error, RootlistError::TooLarge));
        assert_eq!(source.calls(), MAX_ITEMS / PAGE_SIZE + 1);
    }

    #[tokio::test]
    async fn source_error_propagates() {
        let source = Scripted::failing();
        let error = fetch_all(&source).await.expect_err("source failure");
        assert!(matches!(error, RootlistError::Source(_)));
    }
}
