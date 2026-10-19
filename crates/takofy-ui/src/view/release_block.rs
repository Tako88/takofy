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

use egui::{FontId, Rect, Sense, Ui, Vec2, pos2, vec2};
use takofy_model::Release;

use crate::availability::Control;
use crate::cover::Cover;
use crate::icon::Icon;
use crate::format::{format_position, release_meta, total_duration_ms};
use crate::layout::{Table, table as table_layout};
use crate::player::Transport;
use crate::releases::Labels;
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, Weight, extent, fonts, radius, space, stroke, text, tracking,
};

use super::track_list::list_rect;
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

/// The height a block occupies: its hairline, the head, the gap under it, and
/// one row per track.
///
/// The body is reserved for every track whether or not its label has arrived, so
/// a frame that fills rows in does not move the scrollbar.
pub fn height(count: usize) -> f32 {
    stroke::HAIRLINE
        + space::S5
        + extent::RELEASE_ART
        + space::S4
        + count as f32 * extent::ROW_H
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
    let mut clicked = Clicked::default();

    let (line, _) = ui.allocate_exact_size(vec2(width, stroke::HAIRLINE), Sense::hover());
    ui.painter()
        .rect_filled(line, 0.0, tokens.palette.border);
    ui.add_space(space::S5);

    let (head, _) = ui.allocate_exact_size(vec2(width, extent::RELEASE_ART), Sense::hover());
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
    ui.add_space(space::S4);

    // The body: no column head, the whole list reserved so the scrollbar does
    // not move as rows fill in, and rows only while the block is on screen and
    // its labels have landed.
    let count = release.tracks.len();
    let list = list_rect(ui, count);
    ui.allocate_rect(list, Sense::hover());
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
                };
                match track_row::show(ui, rect, columns, &row, tokens) {
                    Some(Click::Play) => clicked.row = Some(index),
                    Some(Click::Open(_)) | None => {}
                }
            }
        }
    }

    clicked
}
