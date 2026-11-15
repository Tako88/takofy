//! The shared head of a row: the seed, what it came from, and what the row
//! reports.
//!
//! A station and a discovery row name their own seed the same way, so they draw
//! the same head: the seed and its provenance on the left, and on the right a
//! ledger, the re-roll, and the play. The geometry lives in
//! [`crate::layout::row_head`]; this module measures the words and wires the
//! controls.

use std::fmt::Debug;
use std::hash::Hash;

use egui::{Pos2, Rect, Ui, pos2, vec2};

use crate::availability::Control;
use crate::icon::Icon;
use crate::layout::row_head::{self, About, Cluster};
use crate::route::Page;
use crate::theme::icon::Size;
use crate::theme::{MonoWeight, Tokens, extent, fonts, space, text, tracking};

use super::link::{self, Face, Placed, Target};
use super::{controls, text as recipe};

/// One trailing control of a head.
pub struct Button<K> {
    pub control: Control,
    pub key: K,
    pub live: bool,
}

/// What one head draws.
pub struct Head<'a, K> {
    pub seed: &'a str,
    /// The seed's URI, which keys the seed link: two rows can share a name.
    pub key: &'a str,
    /// The page the seed opens, when the row can name one.
    pub seed_page: Option<Page>,
    /// The line under the seed: what it was built from.
    pub provenance: &'a str,
    /// The row's arithmetic, when it has one to report.
    pub ledger: Option<&'a str>,
    pub reroll: Option<Button<K>>,
    /// How much of the re-roll's wait has run, when it is cooling.
    pub waited: Option<f32>,
    pub play: Option<Button<K>>,
}

/// What a head's presses asked for.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Opened {
    pub seed: Option<Page>,
    pub reroll: bool,
    pub play: bool,
}

/// The head's height: the seed and its provenance, or the controls beside them.
pub fn height(ui: &Ui, tokens: &Tokens) -> f32 {
    let lines = line_height(ui, recipe::station_seed(), tokens)
        + space::S1
        + line_height(ui, recipe::station_source(), tokens);
    row_head::height(lines, extent::BUTTON_LG)
}

/// Draws one head at `rect`'s edges and reports the presses it took.
///
/// The seed is a link only when the row names a page; otherwise it is plain
/// text. A control whose `live` is false is drawn present and inert.
pub fn show<K: Hash + Debug>(
    ui: &mut Ui,
    rect: Rect,
    head: Head<'_, K>,
    tokens: &Tokens,
) -> Opened {
    let seed_h = line_height(ui, recipe::station_seed(), tokens);
    let source_h = line_height(ui, recipe::station_source(), tokens);
    let block_h = seed_h + space::S1 + source_h;

    let ledger = head
        .ledger
        .map(|label| ledger_galley(ui, label, tokens).size());
    let cluster = Cluster {
        ledger,
        reroll: head
            .reroll
            .as_ref()
            .map(|_| vec2(extent::BUTTON_LG, extent::BUTTON_LG)),
        play: head
            .play
            .as_ref()
            .map(|_| vec2(extent::BUTTON_LG, extent::BUTTON_LG)),
    };
    let placed = row_head::place(
        rect,
        About {
            width: rect.width(),
            height: block_h,
            seed: seed_h,
        },
        cluster,
        space::S2,
    );

    let mut opened = Opened::default();

    let seed = Placed {
        pos: placed.about.min,
        galley: link::lay(
            ui,
            head.seed,
            &Face {
                font: recipe::station_seed(),
                tracking: recipe::tracking_pts(text::MD, tracking::TIGHT),
            },
            placed.about.width(),
        ),
    };
    let shown = seed.link(
        ui,
        ui.scope_id().with(("row-head-seed", head.key)),
        rect,
        head.seed_page.as_ref().and_then(target_of),
        tokens.palette.text,
        tokens,
    );
    opened.seed = shown.page;

    let source = recipe::line(
        ui,
        head.provenance,
        recipe::station_source(),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::WIDE),
        placed.about.width(),
    );
    ui.painter().galley(
        Pos2::new(placed.about.left(), placed.about.top() + seed_h + space::S1),
        source,
        tokens.palette.text_subtle,
    );

    if let (Some(label), Some(slot)) = (head.ledger, placed.ledger) {
        let galley = ledger_galley(ui, label, tokens);
        ui.painter().galley(
            pos2(slot.left(), slot.center().y - galley.size().y / 2.0),
            galley,
            tokens.palette.text_subtle,
        );
    }

    if let (Some(button), Some(slot)) = (head.reroll, placed.reroll) {
        let control = controls::head_action_keyed(
            ui,
            tokens,
            button.control,
            button.key,
            slot,
            Icon::Again,
            Size::SM,
            &format!("Draw another {}", head.seed),
            controls::HeadTone::Quiet,
            head.waited,
            button.live,
        );
        opened.reroll = control.response.clicked();
    }

    if let (Some(button), Some(slot)) = (head.play, placed.play) {
        let control = controls::head_action_keyed(
            ui,
            tokens,
            button.control,
            button.key,
            slot,
            Icon::Play,
            Size::SM,
            &format!("Play {}", head.seed),
            controls::HeadTone::Strong,
            None,
            button.live,
        );
        opened.play = control.response.clicked();
    }

    opened
}

/// The link target a seed's page names, when the shared link can draw it.
fn target_of(page: &Page) -> Option<Target<'_>> {
    match page {
        Page::Playlist(uri) => Some(Target::Playlist(uri)),
        Page::Album(uri) => Some(Target::Album(uri)),
        Page::Artist(uri) => Some(Target::Credit(uri)),
        Page::Home | Page::Show(_) | Page::Search(_) => None,
    }
}

/// The one line of type a head's words are measured in.
fn line_height(ui: &Ui, font: egui::FontId, tokens: &Tokens) -> f32 {
    ui.painter()
        .layout_no_wrap(" ".to_owned(), font, tokens.palette.text)
        .size()
        .y
}

/// The ledger's galley: the reference's tracked mono capitals.
fn ledger_galley(ui: &Ui, label: &str, tokens: &Tokens) -> std::sync::Arc<egui::Galley> {
    recipe::spaced(
        ui,
        &label.to_uppercase(),
        egui::FontId::new(text::XS, fonts::mono(MonoWeight::Regular)),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XS, tracking::WIDE),
    )
}

#[cfg(test)]
mod tests;
