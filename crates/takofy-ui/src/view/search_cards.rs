//! One results block of cards: the albums, or the artists, a query answered.
//!
//! The block is the reference's `.card-grid`: the page's column widened by one
//! card padding each side, so the cover art lines up with the headings above it
//! while the card's own hover surface still bleeds past the gutter. The cards are
//! `media::tile`'s, the same card the library draws, so an artist is a circle and
//! an album a rounded square without this file deciding anything.
//!
//! Both blocks are drawn from the whole answer's index, never from the rows a
//! neighbouring block shows, and only the cards the viewport can see are looked
//! up: a face is fetched by looking at it, and a card nobody can see must not
//! ask for one.

use std::time::Instant;

use egui::{Rect, Sense, Ui, pos2, vec2};

use crate::cover::{Art, want_px};
use crate::layout::{cards, table as table_layout};
use crate::search::Search;
use crate::search_index::{Band, Index, Kind, PAGE, tracks_phrase};
use crate::sidebar_kind::SidebarKind;
use crate::theme::{Tokens, extent, space};

use super::art;
use super::hero;
use super::list_more;
use super::media;
use super::section;
use super::skeleton;

/// What one card block's controls asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Act {
    /// The card at this index was pressed.
    Open(usize),
    /// The reveal row was taken. `focus` is the card it reveals first, so the
    /// keyboard continues where the pointer went.
    More { focus: egui::Id },
}

/// Draws `band`'s head, cards and reveal row.
pub(crate) fn show(
    ui: &mut Ui,
    band: &Band,
    index: &Index,
    search: &Search,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Option<Act> {
    section::head_spoken(ui, band.heading(), &band.share(), &band.spoken(), tokens);
    block(ui, band, index, search, art, now, tokens).0
}

/// The cards and the reveal row, under a head the caller has already drawn.
///
/// Answers what the block asked for, and the grid it solved: the page ignores
/// the grid, and a test measures the reservation with it.
fn block(
    ui: &mut Ui,
    band: &Band,
    index: &Index,
    search: &Search,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> (Option<Act>, cards::Grid) {
    let grid = solved(ui);
    let reserved = Rect::from_min_size(
        ui.cursor().min,
        vec2(ui.available_width(), grid.height(band.shown)),
    );
    ui.allocate_rect(reserved, Sense::hover());
    let visible = ui.clip_rect();
    let want = want_px(grid.card_w, ui.pixels_per_point());
    let row_want = want_px(crate::theme::extent::LIBRARY_ART, ui.pixels_per_point());

    let mut act = None;
    for position in 0..band.shown {
        let rect = grid.card_rect(position);
        // A card out of sight is not looked up, so a long block does not fetch
        // faces nobody is going to see.
        if !table_layout::on_screen(visible, rect) {
            continue;
        }
        let pressed = match band.kind {
            Kind::Albums => {
                let Some(album) = index.albums.get(position) else {
                    break;
                };
                let cover = art.look(album.cover.as_ref(), want, now);
                let item = media::Media {
                    uri: &album.uri,
                    name: &album.name,
                    kind: "Album",
                    sub: &album.artist,
                    size: "",
                    round: false,
                    shelf: SidebarKind::Albums,
                    playable: false,
                    cover,
                    picture: hero::Picture::Cover,
                };
                media::tile(ui, rect, &item, tokens)
            }
            Kind::Artists => {
                let Some(artist) = index.artists.get(position) else {
                    break;
                };
                let cover = art::artist_cover(
                    search.portrait(&artist.uri),
                    artist.record_cover.as_ref(),
                    art,
                    row_want,
                    now,
                );
                let sub = tracks_phrase(artist.tracks);
                let item = media::Media {
                    uri: &artist.uri,
                    name: &artist.name,
                    kind: "Artist",
                    sub: &sub,
                    size: "",
                    round: true,
                    shelf: SidebarKind::Artists,
                    playable: false,
                    cover,
                    picture: hero::Picture::Cover,
                };
                media::tile(ui, rect, &item, tokens)
            }
            // A track's card is drawn from the queue, so it is no tile here.
            Kind::Tracks => None,
        };
        if pressed == Some(media::Press::Open) {
            act = Some(Act::Open(position));
        }
    }

    let label = band.more();
    if list_more::show(ui, reserved, label.as_deref(), space::S2, tokens)
        && let Some(focus) = focus_of(ui, band, index)
    {
        act = Some(Act::More { focus });
    }
    (act, grid)
}

/// The block a loading page reserves for `kind`: the grid its cards will land
/// in, drawn as placeholders.
///
/// The height is the same [`cards::Grid::height`] the answered block reserves, so
/// nothing on the page moves when the answer arrives.
pub(crate) fn loading(ui: &mut Ui, kind: Kind, tokens: &Tokens) {
    let grid = solved(ui);
    let count = match kind {
        Kind::Tracks => 0,
        Kind::Albums => PAGE.albums,
        Kind::Artists => PAGE.artists,
    };
    let block = Rect::from_min_size(
        ui.cursor().min,
        vec2(ui.available_width(), grid.height(count)),
    );
    ui.allocate_rect(block, Sense::hover());
    skeleton::tiles(ui, &grid, count, tokens);
}

/// The area the cards are solved in: the page's column, one card padding wider
/// on each side, so the art lines up with the headings.
fn area(ui: &Ui) -> Rect {
    let cursor = ui.cursor().min;
    Rect::from_min_size(
        pos2(cursor.x - space::S2, cursor.y),
        vec2(ui.available_width() + 2.0 * space::S2, 0.0),
    )
}

/// The grid the cards are laid out in at the current cursor.
fn solved(ui: &Ui) -> cards::Grid {
    cards::grid(
        area(ui),
        extent::CARD_MIN_W,
        space::S3,
        media::extra_height(ui),
    )
}

/// The id of the `band.shown`-th card: the first one a reveal would show.
///
/// It is the id `media::tile` interacts the card under, computed here on the
/// frame of the press so the next frame's card is the one that takes the
/// keyboard.
fn focus_of(ui: &Ui, band: &Band, index: &Index) -> Option<egui::Id> {
    let uri = match band.kind {
        Kind::Albums => &index.albums.get(band.shown)?.uri,
        Kind::Artists => &index.artists.get(band.shown)?.uri,
        Kind::Tracks => return None,
    };
    Some(ui.scope_id().with(("library-media", uri.as_str())))
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, FullOutput, Pos2, RawInput, Rect, Shape, Vec2};

    use super::*;
    use crate::library::Library;
    use crate::route::Page;
    use crate::search_index::{Album, Artist};
    use crate::theme::style;

    const WIDTH: f32 = 800.0;

    fn context() -> Context {
        let ctx = Context::default();
        ctx.set_fonts(crate::theme::fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        ctx
    }

    fn input(events: Vec<Event>) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WIDTH, 900.0))),
            events,
            ..Default::default()
        }
    }

    fn cover_url(index: usize) -> takofy_model::CoverUrl {
        takofy_model::CoverUrl::from_wire(&format!("https://i.scdn.co/image/{index}"))
            .expect("a cover url")
    }

    fn cover(index: usize) -> takofy_model::CoverSet {
        takofy_model::CoverSet::single(cover_url(index))
    }

    /// `count` albums, each on a cover of its own so a fetch names its card.
    fn albums(count: usize) -> Index {
        Index {
            tracks: count,
            albums: (0..count)
                .map(|index| Album {
                    uri: format!("spotify:album:{index}"),
                    name: format!("Album {index}"),
                    artist: format!("Artist {index}"),
                    cover: Some(cover(index)),
                })
                .collect(),
            artists: Vec::new(),
        }
    }

    /// `count` artists, each credited with one more track than the last.
    fn artists(count: usize) -> Index {
        Index {
            tracks: count,
            albums: Vec::new(),
            artists: (0..count)
                .map(|index| Artist {
                    uri: format!("spotify:artist:{index}"),
                    name: format!("Artist {index}"),
                    tracks: index + 1,
                    record_cover: Some(cover(index)),
                })
                .collect(),
        }
    }

    fn band(kind: Kind, shown: usize, total: usize) -> Band {
        Band { kind, shown, total }
    }

    /// What one pass reported, painted, solved and moved.
    struct Drawn {
        act: Option<Act>,
        output: FullOutput,
        advance: f32,
        grid: cards::Grid,
        /// Every shown card's rect, in card order.
        rects: Vec<Rect>,
    }

    /// One pass of the block — its head, then its cards — with the clip cut to
    /// `clip` when given.
    fn pass(
        ctx: &Context,
        band: &Band,
        index: &Index,
        clip: Option<Rect>,
        events: Vec<Event>,
    ) -> Drawn {
        let tokens = Tokens::DARK;
        let search = Search::default();
        let mut art = Art::default();
        art.begin_frame();
        let mut act = None;
        let mut advance = 0.0;
        let mut grid = None;
        let mut rects = Vec::new();
        let mut output = ctx.run_ui(input(events), |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            if let Some(clip) = clip {
                ui.set_clip_rect(clip);
            }
            let top = ui.cursor().top();
            section::head(ui, band.heading(), &band.share(), &tokens);
            let (asked, solved) =
                block(ui, band, index, &search, &mut art, Instant::now(), &tokens);
            act = asked;
            grid = Some(solved);
            rects = (0..band.shown).map(|i| solved.card_rect(i)).collect();
            advance = ui.cursor().top() - top;
        });
        output.textures_delta.clear();
        Drawn {
            act,
            output,
            advance,
            grid: grid.expect("one pass"),
            rects,
        }
    }

    /// The fetches a pass asked for, in the order the cards were looked at.
    fn fetches(
        ctx: &Context,
        band: &Band,
        index: &Index,
        clip: Option<Rect>,
    ) -> Vec<takofy_model::CoverUrl> {
        let tokens = Tokens::DARK;
        let search = Search::default();
        let mut art = Art::default();
        let now = Instant::now();
        art.begin_frame();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            if let Some(clip) = clip {
                ui.set_clip_rect(clip);
            }
            section::head(ui, band.heading(), &band.share(), &tokens);
            let _ = block(ui, band, index, &search, &mut art, now, &tokens);
        });
        output.textures_delta.clear();
        art.requests(now)
            .into_iter()
            .filter_map(|command| match command {
                crate::message::Command::Fetch { url } => Some(url),
                _ => None,
            })
            .collect()
    }

    /// Every line the frame painted, as it reads.
    fn painted(output: &FullOutput) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Text(text) => text
                    .galley
                    .rows
                    .first()
                    .map(|row| row.row.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    /// The height one bare section head takes, for the reservation's math.
    fn head_advance(ctx: &Context) -> f32 {
        let tokens = Tokens::DARK;
        let mut advance = 0.0;
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let top = ui.cursor().top();
            section::head(ui, "Albums", "3", &tokens);
            advance = ui.cursor().top() - top;
        });
        output.textures_delta.clear();
        advance
    }

    #[test]
    fn the_block_reserves_the_grids_height() {
        let ctx = context();
        let index = albums(10);
        let drawn = pass(&ctx, &band(Kind::Albums, 10, 10), &index, None, Vec::new());
        let expected = drawn.grid.height(10);
        let reserved = drawn.advance - head_advance(&ctx);
        assert!(
            (reserved - expected).abs() < 0.5,
            "the block reserved {reserved}, not the grid's {expected}"
        );
    }

    #[test]
    fn the_grid_is_widened_by_one_card_padding_each_side() {
        let ctx = context();
        let drawn = pass(
            &ctx,
            &band(Kind::Albums, 10, 10),
            &albums(10),
            None,
            Vec::new(),
        );
        let grid = drawn.grid;
        assert_eq!(
            grid.card_rect(0).left(),
            -space::S2,
            "the grid starts a padding left of the page's column"
        );
        assert!(
            (grid.card_rect(grid.columns - 1).right() - (WIDTH + space::S2)).abs() < 0.5,
            "and ends a padding past it"
        );
    }

    #[test]
    fn a_card_out_of_sight_is_not_looked_up() {
        let ctx = context();
        let index = albums(10);
        let drawn = pass(&ctx, &band(Kind::Albums, 10, 10), &index, None, Vec::new());
        let columns = drawn.grid.columns;
        // The clip ends just above the second row, so only the first row is in
        // sight.
        let cut_at = drawn.rects[columns].top() - 0.5;
        let clip = Rect::from_min_max(Pos2::ZERO, Pos2::new(WIDTH, cut_at));
        let cut = fetches(&ctx, &band(Kind::Albums, 10, 10), &index, Some(clip));
        let row: Vec<_> = (0..columns).map(cover_url).collect();
        assert_eq!(cut, row, "only the cards in sight were looked at");

        let tall = fetches(&ctx, &band(Kind::Albums, 10, 10), &index, None);
        assert!(
            tall.contains(&cover_url(columns)),
            "a card in the second row is looked at when it is in sight: {tall:?}"
        );
        assert!(
            !cut.contains(&cover_url(columns)),
            "and never when it is not: {cut:?}"
        );
    }

    #[test]
    fn a_card_click_reports_its_index() {
        let ctx = context();
        let index = albums(10);
        let drawn = pass(&ctx, &band(Kind::Albums, 10, 10), &index, None, Vec::new());
        let at = drawn.rects[2].center();
        let mut act = None;
        for event in crate::view::test_support::click_events(at) {
            act = pass(&ctx, &band(Kind::Albums, 10, 10), &index, None, vec![event]).act;
        }
        assert_eq!(act, Some(Act::Open(2)), "the third card reports itself");
    }

    #[test]
    fn the_reveal_row_is_drawn_only_while_something_is_hidden() {
        let ctx = context();
        let index = albums(12);
        let whole = pass(&ctx, &band(Kind::Albums, 12, 12), &index, None, Vec::new());
        assert!(
            !painted(&whole.output)
                .iter()
                .any(|line| line.starts_with("Show ")),
            "nothing is hidden, so there is no row: {:?}",
            painted(&whole.output)
        );

        let hidden = pass(&ctx, &band(Kind::Albums, 10, 12), &index, None, Vec::new());
        assert!(
            painted(&hidden.output).contains(&"Show 2 more albums".to_owned()),
            "the row says how much is left: {:?}",
            painted(&hidden.output)
        );
    }

    #[test]
    fn taking_the_reveal_reports_the_card_it_will_show_first() {
        let ctx = context();
        let index = albums(12);
        let hidden = pass(&ctx, &band(Kind::Albums, 10, 12), &index, None, Vec::new());
        // The row hangs under the block's own allocated width.
        let block_rect = Rect::from_min_size(
            Pos2::new(0.0, hidden.rects[0].top()),
            vec2(WIDTH, hidden.grid.height(10)),
        );
        let row = list_more::rect(block_rect);
        let mut act = None;
        for event in crate::view::test_support::click_events(row.center()) {
            act = pass(&ctx, &band(Kind::Albums, 10, 12), &index, None, vec![event]).act;
        }
        let Some(Act::More { focus }) = act else {
            panic!("the reveal was taken: {act:?}");
        };
        // The block the take grows draws the eleventh card, and the id the row
        // reported is that card's.
        let grown = pass(&ctx, &band(Kind::Albums, 12, 12), &index, None, Vec::new());
        let first = ctx
            .read_response(focus)
            .expect("the card it reveals draws on the next frame");
        assert_eq!(
            first.rect, grown.rects[10],
            "focus lands on the eleventh card"
        );
    }

    /// An artist card is `media::tile`'s round card, with the artist's track
    /// count under the name, and an unread face asks for nothing.
    #[test]
    fn an_artist_card_wears_its_track_count_and_no_face_until_asked() {
        let ctx = context();
        let index = artists(3);
        let tokens = Tokens::DARK;
        let search = Search::default();
        let mut art = Art::default();
        art.begin_frame();
        let now = Instant::now();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let _ = show(
                ui,
                &band(Kind::Artists, 3, 3),
                &index,
                &search,
                &mut art,
                now,
                &tokens,
            );
        });
        output.textures_delta.clear();
        // An unread face is the raised slot: it asks for nothing at all.
        assert!(
            art.requests(now).is_empty(),
            "an unread portrait fetched something"
        );
        let names = painted(&output);
        assert!(names.contains(&"Artist 0".to_owned()), "{names:?}");
        assert!(names.contains(&"1 track".to_owned()), "{names:?}");
    }

    #[test]
    fn the_skeleton_reserves_the_answered_grid_and_paints_every_placeholder() {
        for kind in [Kind::Albums, Kind::Artists] {
            let ctx = context();
            let tokens = Tokens::DARK;
            let mut advance = 0.0;
            let mut height = 0.0;
            let mut output = ctx.run_ui(input(Vec::new()), |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let top = ui.cursor().top();
                height = solved(ui).height(PAGE.albums);
                loading(ui, kind, &tokens);
                advance = ui.cursor().top() - top;
            });
            output.textures_delta.clear();
            assert!(
                (advance - height).abs() < 0.5,
                "{kind:?}: the skeleton reserved {advance}, not the grid's {height}"
            );
            let boxes = output
                .shapes
                .iter()
                .filter(|clipped| {
                    matches!(&clipped.shape, Shape::Rect(rect)
                        if rect.fill == tokens.palette.skeleton)
                })
                .count();
            assert_eq!(boxes, PAGE.albums, "{kind:?}: one placeholder per card");
        }
    }

    #[test]
    fn an_artist_card_draws_the_portrait_the_page_holds() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut library = Library::default();
        let _ = library.open(Page::Search("q".to_owned()));
        library.apply(&crate::message::Update::Search {
            query: "q".to_owned(),
            tracks: Vec::new(),
        });
        // The frame that draws the page is also the one that records it as the
        // surface a portrait reply is judged against.
        let _ = library.search.request_portraits(Some("q"));
        let portrait = cover(7);
        library.apply(&crate::message::Update::Portraits {
            query: "q".to_owned(),
            portraits: vec![takofy_model::ArtistPortrait {
                artist: "spotify:artist:0".to_owned(),
                portrait: Some(portrait.clone()),
            }],
        });
        let index = artists(3);
        let now = Instant::now();
        let mut art = Art::default();
        art.begin_frame();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let _ = show(
                ui,
                &band(Kind::Artists, 3, 3),
                &index,
                &library.search,
                &mut art,
                now,
                &tokens,
            );
        });
        output.textures_delta.clear();
        assert!(
            art.requests(now).contains(&crate::message::Command::Fetch {
                url: portrait.variants()[0].url.clone()
            }),
            "a known portrait is what the card fetches"
        );
    }
}
