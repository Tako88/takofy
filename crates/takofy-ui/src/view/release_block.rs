//! One release block in an artist's discography.
//!
//! The reference draws a release as a head — its art, its name, its facts, and
//! its own play, like and more — followed by the release's tracklist with no
//! column head and its rows numbered from one. The block is expanded in place
//! rather than opened as a page.
//!
//! The tracklist's rows are labelled only while the block is on screen, so the
//! head draws from the release metadata alone and the body is a reserved block of
//! [`extent::ROW_H`] until the labels arrive. That keeps the scrollbar honest
//! while the rows fill in.

use egui::{FontId, Id, Rect, Sense, Ui, Vec2, pos2, vec2};
use takofy_model::Release;

use crate::availability::Control;
use crate::cover::Cover;
use crate::format::{format_position, release_meta, total_duration_ms};
use crate::icon::Icon;
use crate::layout::{Table, table as table_layout};
use crate::player::Transport;
use crate::releases::Labels;
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, Weight, extent, fonts, radius, space, stroke, text, tracking,
};

use super::carry::{self, Ghost};
use super::track_row::{self, Click, Row, TitleCell, state_of};
use super::{art, controls, text as recipe};

/// What one block draws.
pub struct Block<'a> {
    pub release: &'a Release,
    /// The block's label state, as the release store reports it.
    pub labels: Labels<'a>,
    /// The release's cover, as the art pipeline resolves it.
    pub cover: Cover,
    /// Whether the block intersects the viewport. An off-screen block draws its
    /// head, reserves its body, and asks for no labels.
    pub on_screen: bool,
    /// Whether the release's own play can act: its rows must be labelled before
    /// there is anything to start.
    pub playable: bool,
}

/// The height a block occupies: its hairline, the head, the gap under it, one
/// row per track, and the discography's trailing gap.
///
/// The body is reserved for every track whether or not its label has arrived, so
/// a frame that fills rows in does not move the scrollbar. [`body`] draws into
/// exactly this rect, so the height the page culls with is the height drawn.
pub fn height(count: usize) -> f32 {
    stroke::HAIRLINE
        + space::S5
        + extent::RELEASE_ART
        + space::S4
        + count as f32 * extent::ROW_H
        + space::S6
}

/// What a click inside the block asked for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Clicked {
    /// The block's own play control: the release from its first track.
    pub played: bool,
    /// A row's double click, as an index into the release's tracks.
    pub row: Option<usize>,
}

/// Draws one release block and reports what its controls asked for.
pub fn show(
    ui: &mut Ui,
    table: Table,
    block: &Block<'_>,
    transport: &Transport,
    tokens: &Tokens,
) -> Clicked {
    // Each block is its own id scope: a play tile, a like and a more appear once
    // per release, so without the scope every block would share one hover, one
    // focus node and one widget id.
    let mut clicked = Clicked::default();
    ui.scope_builder(
        egui::UiBuilder::new().id_salt(("release-block", block.release.uri.as_str())),
        |ui| {
            clicked = body(ui, table, block, transport, tokens);
        },
    );
    clicked
}

fn body(
    ui: &mut Ui,
    table: Table,
    block: &Block<'_>,
    transport: &Transport,
    tokens: &Tokens,
) -> Clicked {
    let release = block.release;
    let width = ui.available_width();
    let count = release.tracks.len();
    let mut clicked = Clicked::default();

    // One allocation for the whole block: the page culls with `height`, and what
    // it culls is exactly what is drawn, so a block never blanks early.
    let (rect, _) = ui.allocate_exact_size(vec2(width, height(count)), Sense::hover());

    let line = Rect::from_min_size(rect.min, vec2(width, stroke::HAIRLINE));
    ui.painter().rect_filled(line, 0.0, tokens.palette.border);

    let head = Rect::from_min_size(
        pos2(rect.left(), line.bottom() + space::S5),
        vec2(width, extent::RELEASE_ART),
    );
    let art = Rect::from_min_size(head.min, Vec2::splat(extent::RELEASE_ART));
    art::tile(
        ui.painter(),
        art,
        radius::SM,
        tokens.palette.border,
        block.cover,
        tokens,
    );

    // The controls hug the right edge: more, like, then the release's play.
    let centre = head.center().y;
    let mut right = head.right();
    for (control, icon, label) in [
        (Control::ReleaseMore, Icon::More, "More options"),
        (Control::ReleaseLike, Icon::Heart, "Save to your library"),
    ] {
        let rect = Rect::from_min_size(
            pos2(right - extent::BUTTON_LG, centre - extent::BUTTON_LG / 2.0),
            Vec2::splat(extent::BUTTON_LG),
        );
        controls::icon_button_keyed(
            ui,
            tokens,
            control,
            release.uri.as_str(),
            rect,
            icon,
            Size::MD,
            label,
            false,
        );
        right = rect.left() - space::S2;
    }
    let play = Rect::from_min_size(
        pos2(
            right - extent::RELEASE_PLAY,
            centre - extent::RELEASE_PLAY / 2.0,
        ),
        Vec2::splat(extent::RELEASE_PLAY),
    );
    clicked.played = controls::play_tile(
        ui,
        tokens,
        Control::ReleasePlay,
        play,
        &format!("Play {}", release.name),
        block.playable,
    )
    .response
    .clicked();
    right = play.left() - space::S4;

    // The name over its facts, centred as one block in the head.
    let total = match block.labels {
        Labels::Ready(tracks) => total_duration_ms(tracks),
        Labels::Idle | Labels::Loading | Labels::Failed(_) => 0,
    };
    let facts = release_meta(release.kind, release.year, (release.tracks.len(), total)).join(" · ");
    let left = art.right() + space::S5;
    let text_width = (right - left).max(0.0);
    if text_width > 0.0 {
        let name = recipe::line(
            ui,
            &release.name,
            FontId::new(text::XL, fonts::sans(Weight::Black)),
            tokens.palette.text,
            recipe::tracking_pts(text::XL, tracking::TIGHT),
            text_width,
        );
        let facts = recipe::spaced(
            ui,
            &facts.to_uppercase(),
            FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
            tokens.palette.text_subtle,
            recipe::tracking_pts(text::XXS, tracking::WIDE),
        );
        let name_height = name.size().y;
        let height = name_height + space::S2 + facts.size().y;
        let top = head.center().y - height / 2.0;
        ui.painter()
            .galley(pos2(left, top), name, tokens.palette.text);
        ui.painter().galley(
            pos2(left, top + name_height + space::S2),
            facts,
            tokens.palette.text_subtle,
        );
    }

    // The body: no column head, the whole list reserved so the scrollbar does
    // not move as rows fill in, and rows only while the block is on screen and
    // its labels have landed.
    let list = Rect::from_min_size(
        pos2(rect.left(), head.bottom() + space::S4),
        vec2(width, extent::ROW_H * count as f32),
    );
    if !block.on_screen || count == 0 {
        return clicked;
    }

    match block.labels {
        Labels::Idle | Labels::Loading => {}
        Labels::Failed(failure) => {
            let copy = recipe::line(
                ui,
                crate::signin::failure_message(failure),
                recipe::track_secondary(),
                tokens.palette.text_muted,
                0.0,
                list.width(),
            );
            ui.painter()
                .galley(list.min, copy, tokens.palette.text_muted);
        }
        Labels::Ready(tracks) => {
            let columns = table_layout::columns(width, table.columns().for_album());
            let visible = ui.clip_rect();
            let mut selection = carry::List::open(
                ui.ctx(),
                Id::unique(("release-tracks", &release.uri)),
                tracks.len(),
            );
            for (index, entry) in tracks.iter().enumerate() {
                let rect = table_layout::row_rect(list, index, extent::ROW_H);
                if !visible.intersects(rect) {
                    continue;
                }
                let duration = entry.duration_ms.map(format_position);
                let row = Row {
                    uri: &entry.uri,
                    number: index + 1,
                    title: &entry.title,
                    title_cell: TitleCell::NameOverArtist,
                    // The block's head already names the artist; a slim row is
                    // the name alone.
                    artist: "",
                    artist_uri: None,
                    album: None,
                    album_uri: None,
                    added: None,
                    duration: duration.as_deref(),
                    state: state_of(&entry.uri, transport),
                    // The block's head already holds the release's one picture.
                    cover: Cover::Missing,
                    look: selection.look(index),
                    // This block's rows open no menu yet.
                    menu: false,
                };
                match track_row::show(ui, rect, columns, &row, tokens) {
                    Some(Click::Play) => clicked.row = Some(index),
                    Some(Click::Select(press)) => selection.press(index, press),
                    Some(Click::Lift) => {
                        let ghost = Ghost::Table(row.lifted(columns));
                        selection.lift(index, rect.size(), ghost, |row| tracks.get(row).cloned());
                    }
                    Some(Click::Menu(_)) | Some(Click::Open(_)) | None => {}
                }
            }
            selection.finish(ui.ctx());
        }
    }

    clicked
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Id, Pos2, RawInput, Rect, Vec2};
    use takofy_model::{Release, ReleaseKind, TrackEntry};

    use super::*;
    use crate::cover::Cover;
    use crate::releases::Labels;
    use crate::theme::{Tokens, fonts, style};
    use crate::view::test_support::click_events;

    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn release(count: usize) -> Release {
        Release {
            uri: "spotify:album:r1".to_owned(),
            name: "One".to_owned(),
            kind: ReleaseKind::Album,
            year: Some(2024),
            cover: None,
            tracks: (0..count)
                .map(|index| format!("spotify:track:{index}"))
                .collect(),
        }
    }

    /// The cursor's advance across one block, less the gap the parent adds
    /// between its widgets: the block's own extent, measured at `width`.
    fn advance(width: f32, count: usize) -> f32 {
        let ctx = context();
        let tokens = Tokens::DARK;
        let transport = Transport::default();
        let release = release(count);
        let block = Block {
            release: &release,
            labels: Labels::Idle,
            cover: Cover::Missing,
            on_screen: false,
            playable: false,
        };

        let mut measured = None;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 2000.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            ui.set_max_width(width);
            let top = ui.cursor().min.y;
            let _ = show(ui, Table::Full, &block, &transport, &tokens);
            measured = Some(ui.cursor().min.y - top - ui.spacing().item_spacing.y);
        });
        output.textures_delta.clear();
        measured.expect("the block lays out")
    }

    /// The page culls with [`height`] and then draws at the cursor, so the two
    /// numbers must be the same one or a block blanks before it leaves the top.
    #[test]
    fn the_cursor_advance_is_the_height_the_culling_uses() {
        for width in [420.0, 900.0] {
            assert_eq!(advance(width, 3), height(3), "three rows at {width} px");
            assert_eq!(advance(width, 0), height(0), "no rows at {width} px");
        }
    }

    fn labelled(uri: &str, title: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: title.to_owned(),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    /// One frame of a labelled block, and what its rows reported.
    fn painted(
        ctx: &Context,
        release: &Release,
        tracks: &[TrackEntry],
        events: Vec<Event>,
    ) -> (Clicked, egui::FullOutput) {
        let tokens = Tokens::DARK;
        let transport = Transport::default();
        let block = Block {
            release,
            labels: Labels::Ready(tracks),
            cover: Cover::Missing,
            on_screen: true,
            playable: true,
        };
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 800.0))),
            events,
            ..Default::default()
        };
        let mut clicked = Clicked::default();
        let mut output = ctx.run_ui(input, |ui| {
            clicked = show(ui, Table::Full, &block, &transport, &tokens);
        });
        output.textures_delta.clear();
        (clicked, output)
    }

    /// The rect a painted label occupies, read back from the frame.
    fn text_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
    }

    #[test]
    fn a_single_click_selects_and_a_double_click_plays_it() {
        let ctx = context();
        let release = release(2);
        let tracks = vec![
            labelled("spotify:track:1", "First"),
            labelled("spotify:track:2", "Second"),
        ];
        let (_, output) = painted(&ctx, &release, &tracks, Vec::new());
        let row = text_rect(&output, "Second").expect("the row's title");
        let id = Id::unique(("release-tracks", &release.uri));

        let mut clicked = Clicked::default();
        for event in click_events(row.center()) {
            clicked = painted(&ctx, &release, &tracks, vec![event]).0;
        }

        assert_eq!(clicked.row, None, "a single click plays nothing");
        assert!(
            crate::view::carry::List::open(&ctx, id, 2).look(1).selected,
            "the row under the click is picked"
        );
        assert!(
            !crate::view::carry::List::open(&ctx, id, 2).look(0).selected,
            "and only that row"
        );

        let mut clicked = Clicked::default();
        // A fresh block for the double click: a third click inside the window is
        // a triple click, and that plays nothing.
        let ctx = context();
        for _ in 0..2 {
            for event in click_events(row.center()) {
                clicked = painted(&ctx, &release, &tracks, vec![event]).0;
            }
        }

        assert_eq!(clicked.row, Some(1), "a double click plays the row");
    }
}
