//! The sidebar: the brand, the navigation and the library.
//!
//! The rail and the full sidebar are one function, because they share everything
//! but the label and the padding, and which one is on screen comes from the
//! region model rather than from a mode of its own.

use egui::{Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};

use crate::availability::Control;
use crate::icon::{self, Icon};
use crate::layout::Sidebar;
use crate::library::{Library, Load};
use crate::message::Command;
use crate::signin::failure_message;
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, Weight, extent, fonts, radius, space, stroke, text, tracking,
};

use super::controls;
use super::shell::rule;
use super::text as recipe;

/// A library row: the art tile plus the padding above and below it.
pub(super) const ROW_H: f32 = extent::LIBRARY_ART + 2.0 * space::S2;

/// Draws the sidebar, returning a command if one of its controls asked for one.
pub fn show(ui: &mut Ui, form: Sidebar, library: &mut Library, tokens: &Tokens) -> Option<Command> {
    let band = ui.available_rect_before_wrap();
    let y = brand(ui, form, tokens, band, band.top());
    let y = nav(ui, form, tokens, band, y);

    match form {
        Sidebar::Rail => None,
        Sidebar::Full => library_block(ui, library, tokens, band, y),
    }
}

/// The monogram, and beside it the wordmark the rail has no room for.
fn brand(ui: &mut Ui, form: Sidebar, tokens: &Tokens, band: Rect, top: f32) -> f32 {
    let side = Size::LG.side;
    let row = Rect::from_min_size(
        Pos2::new(band.left(), top + space::S4),
        Vec2::new(band.width(), side),
    );
    let logo = match form {
        Sidebar::Rail => Rect::from_center_size(row.center(), Vec2::splat(side)),
        Sidebar::Full => Rect::from_min_size(
            Pos2::new(row.left() + space::S4, row.top()),
            Vec2::splat(side),
        ),
    };
    icon::paint(
        ui.painter(),
        Icon::Logo,
        logo,
        Size::LG,
        tokens.palette.text,
    );

    if form == Sidebar::Full {
        let mark = Rect::from_min_size(
            Pos2::new(logo.right() + space::S3, row.top()),
            Vec2::new(row.right() - logo.right() - space::S4, side),
        );
        ui.put(mark, egui::Label::new(recipe::wordmark("Takofy", tokens)));
    }

    row.bottom() + space::S3
}

/// Home and Search. Neither view exists yet, so both are drawn waiting.
fn nav(ui: &mut Ui, form: Sidebar, tokens: &Tokens, band: Rect, top: f32) -> f32 {
    let height = Size::LG.side + 2.0 * space::S2;
    let inset = match form {
        Sidebar::Rail => 0.0,
        Sidebar::Full => space::S1,
    };
    let mut y = top;

    for (control, glyph, label) in [
        (Control::NavHome, Icon::Home, "Home"),
        (Control::NavSearch, Icon::Search, "Search"),
    ] {
        let rect = Rect::from_min_size(
            Pos2::new(band.left() + inset, y),
            Vec2::new(band.width() - 2.0 * inset, height),
        );
        controls::nav_item(ui, tokens, control, rect, form, glyph, label);
        y = rect.bottom() + space::PX;
    }

    y
}

/// The library: the rule, the caption, the filter chips and the list.
fn library_block(
    ui: &mut Ui,
    library: &mut Library,
    tokens: &Tokens,
    band: Rect,
    top: f32,
) -> Option<Command> {
    let inset = space::S1;
    let rule_rect = Rect::from_min_size(
        Pos2::new(band.left() + inset, top + space::S4),
        Vec2::new(band.width() - 2.0 * inset, stroke::HAIRLINE),
    );
    rule(ui, rule_rect, tokens.palette.border);
    let mut y = rule_rect.bottom() + space::S3;

    // The caption, with the reference's two buttons to the right. Creating a
    // playlist and expanding the library both wait for folders.
    let row = Rect::from_min_size(
        Pos2::new(band.left() + space::S4, y),
        Vec2::new(band.width() - 2.0 * space::S4, extent::BUTTON),
    );
    let caption = recipe::spaced(
        ui,
        "YOUR LIBRARY",
        egui::FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::CAPS),
    );
    ui.painter().galley(
        Pos2::new(row.left(), row.center().y - caption.size().y / 2.0),
        caption,
        tokens.palette.text_subtle,
    );
    let expand = Rect::from_min_size(
        Pos2::new(
            row.right() - extent::BUTTON,
            row.center().y - extent::BUTTON / 2.0,
        ),
        Vec2::splat(extent::BUTTON),
    );
    controls::icon_button(
        ui,
        tokens,
        Control::LibraryExpand,
        expand,
        Icon::ArrowRight,
        Size::MD,
        "Expand your library",
    );
    let create = Rect::from_min_size(
        Pos2::new(expand.left() - extent::BUTTON - space::S1, expand.top()),
        Vec2::splat(extent::BUTTON),
    );
    controls::icon_button(
        ui,
        tokens,
        Control::LibraryCreate,
        create,
        Icon::Plus,
        Size::MD,
        "Create playlist or folder",
    );
    y = row.bottom() + space::S2;

    // Playlists is the only kind the list holds, so it is drawn selected rather
    // than as a chip that could be toggled to no effect. The rest wait.
    let chips_left = band.left() + space::S4;
    let chips_width = band.width() - 2.0 * space::S4;
    let mut left = chips_left;
    for (control, label, selected) in [
        (Control::FilterPlaylists, "Playlists", true),
        (Control::FilterArtists, "Artists", false),
        (Control::FilterAlbums, "Albums", false),
        (Control::FilterPodcasts, "Podcasts", false),
    ] {
        let size = controls::chip_size(ui, tokens, label);
        if left > chips_left && left + size.x > chips_left + chips_width {
            left = chips_left;
            y += extent::BUTTON + space::S1;
        }
        let rect = Rect::from_min_size(Pos2::new(left, y), size);
        controls::chip_in(ui, tokens, control, rect, label, selected);
        left = rect.right() + space::S1;
    }
    y += extent::BUTTON + space::S3;

    let list = Rect::from_min_size(
        Pos2::new(band.left() + space::S2, y),
        Vec2::new(band.width() - 2.0 * space::S2, band.bottom() - y),
    );
    rows(ui, list, library, tokens)
}

/// The playlist rows, or the state of the load that fetches them.
fn rows(ui: &mut Ui, rect: Rect, library: &mut Library, tokens: &Tokens) -> Option<Command> {
    if rect.height() < ROW_H {
        return None;
    }

    let mut picked: Option<String> = None;
    let mut retry = false;

    match &library.playlists {
        Load::Idle => {}
        Load::Loading => state(ui, rect, recipe::muted("Loading your playlists…", tokens)),
        Load::Ready(playlists) if playlists.is_empty() => {
            state(ui, rect, recipe::muted("No playlists yet.", tokens));
        }
        Load::Ready(playlists) => {
            let current = library.selected().map(str::to_owned);
            let entries = playlists.clone();
            ui.scope_builder(
                UiBuilder::new()
                    .id_salt("library-list")
                    .max_rect(rect)
                    .layout(Layout::top_down(egui::Align::Min)),
                |ui| {
                    ui.spacing_mut().item_spacing.y = space::PX;
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show_rows(ui, ROW_H, entries.len(), |ui, range| {
                            for index in range {
                                let Some(entry) = entries.get(index) else {
                                    continue;
                                };
                                let (_, row) =
                                    ui.allocate_space(Vec2::new(ui.available_width(), ROW_H));
                                let selected = current.as_deref() == Some(entry.uri.as_str());
                                if one(ui, row, &entry.name, selected, tokens) {
                                    picked = Some(entry.uri.clone());
                                }
                            }
                        });
                },
            );
        }
        Load::Failed(failure) => {
            state(ui, rect, recipe::muted(failure_message(*failure), tokens));
            retry = true;
        }
    }

    if let Some(uri) = picked {
        return Some(library.select(&uri));
    }
    if retry { Some(library.reload()) } else { None }
}

/// One row: a placeholder tile, the name and the kind.
///
/// The tile stands in until covers are fetched. A selected row borrows the
/// reference's language for the current navigation item — a raised fill and a
/// bone rule at the left edge — because the reference defines no selected row.
fn one(ui: &mut Ui, rect: Rect, name: &str, selected: bool, tokens: &Tokens) -> bool {
    let response = ui.interact(
        rect,
        ui.scope_id().with(("library-row", name)),
        Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, name));

    if selected {
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.surface_raised);
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height())),
            radius::SM,
            tokens.palette.text,
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, radius::SM, tokens.palette.surface_hover);
    }

    let tile = Rect::from_min_size(
        Pos2::new(
            rect.left() + space::S2,
            rect.center().y - extent::LIBRARY_ART / 2.0,
        ),
        Vec2::splat(extent::LIBRARY_ART),
    );
    ui.painter().rect(
        tile,
        radius::SM,
        tokens.palette.surface_raised,
        Stroke::new(stroke::HAIRLINE, tokens.palette.border),
        StrokeKind::Inside,
    );

    let name_x = tile.right() + space::S3;
    let name_width = (rect.right() - name_x - space::S2).max(0.0);
    let name_line = recipe::line(
        ui,
        name,
        egui::FontId::new(text::SM, fonts::sans(Weight::Medium)),
        tokens.palette.text,
        0.0,
        name_width,
    );
    let kind_line = recipe::line(
        ui,
        "Playlist",
        egui::FontId::new(text::XXS, fonts::mono(MonoWeight::Regular)),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::SLIGHT),
        name_width,
    );
    let top = rect.center().y - (name_line.size().y + kind_line.size().y) / 2.0;
    ui.painter().galley(
        Pos2::new(name_x, top),
        name_line.clone(),
        tokens.palette.text,
    );
    ui.painter().galley(
        Pos2::new(name_x, top + name_line.size().y),
        kind_line,
        tokens.palette.text_subtle,
    );

    response.clicked()
}

/// A line of text where a list would be, for the empty and failed states.
fn state(ui: &mut Ui, rect: Rect, text: RichText) {
    ui.scope_builder(
        UiBuilder::new()
            .id_salt("library-state")
            .max_rect(rect)
            .layout(Layout::top_down(egui::Align::Center)),
        |ui| {
            ui.add_space(rect.height() / 4.0);
            ui.label(text);
        },
    );
}
