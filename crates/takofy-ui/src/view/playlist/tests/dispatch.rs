use super::super::*;

#[test]
fn main_page_maps_show_to_the_show_view() {
    let mut library = Library::default();
    assert_eq!(main_page(&library), MainPage::Root);
    let _ = library.open(Page::Playlist("spotify:playlist:a".to_owned()));
    assert_eq!(main_page(&library), MainPage::Playlist);
    let _ = library.open(Page::Album("spotify:album:a".to_owned()));
    assert_eq!(main_page(&library), MainPage::Album);
    let _ = library.open(Page::Artist("spotify:artist:a".to_owned()));
    assert_eq!(main_page(&library), MainPage::Artist);
    let _ = library.open(Page::Show("spotify:show:a".to_owned()));
    assert_eq!(
        main_page(&library),
        MainPage::Show,
        "a show page has its own body now"
    );
    let _ = library.open(Page::Search("never gonna".to_owned()));
    assert_eq!(main_page(&library), MainPage::Search);
}

/// A query can spell a playlist's URI. The pane must still draw the results
/// page and not that playlist's tracklist: the page variant decides.
#[test]
fn a_search_query_that_spells_a_playlist_uri_draws_the_results_page() {
    let mut library = Library::default();
    let _ = library.open(Page::Search("spotify:playlist:a".to_owned()));

    assert_eq!(main_page(&library), MainPage::Search);
}
