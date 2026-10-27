//! The search drop-down: the bands an answer derives, the floor row, and the
//! line that says there is nothing to show.
//!
//! The box is the app's one menu box ([`super::menu_box`]), so it is the same
//! raised card, hairline and shadow as a track row's context menu. This view
//! keeps only its own row grammar: the cover, the title over the owner, the
//! action glyph and the floor row.
//!
//! The menu is a foreground `Area` anchored to the field's own box: the same
//! width and `extent::MENU_GAP` below it. It is not clipped by the topbar
//! and it draws over the view, the way the drawer's `Area` does.
//!
//! Two tiers, never three, and the last device wins. The keyboard owns the 2px
//! rule, the `row_selected` wash and the action glyph; the pointer owns a
//! `row_hover` wash and a 1rem tick, and never borrows the rule or the glyph.
//! The playing row keeps the accent rule and the equaliser, and identity beats
//! focus: if the playing row is also marked, the accent rule stays and the glyph
//! becomes pause.
//!
//! The washes are the app-wide row tokens (`row_hover` for the pointer,
//! `row_selected` for the mark), not the reference's literal values: its 4% wash
//! predates the row-hover token.

use std::time::Instant;

use egui::text::LayoutJob;
use egui::{FontId, Order, Rect, Sense, TextFormat, Ui, pos2, vec2};
use takofy_model::TrackEntry;

use crate::cover::{Art, Cover, want_px};
use crate::icon::{self, Icon};
use crate::layout::search_menu as geometry;
use crate::menu::{Device, Tier};
use crate::player::Transport;
use crate::search::{Act, MenuView, Search};
use crate::search_index::Band;
use crate::theme::icon::Size;
use crate::theme::{MonoWeight, Tokens, Weight, fonts, radius, space, stroke, text, tracking};

use super::art;
use super::menu_box::{self, Card, clicked_on, paint_highlight, paint_marker, row_corners};
use super::text as recipe;
use super::track_row::{self, State, state_of};

/// What the menu asked the shell to do this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Menu {
    /// The action a row click asked for. Closing is the shell's decision, not
    /// this view's.
    pub click: Option<Act>,
    /// The frame the menu was drawn in, so the shell can tell whether a pointer
    /// is on it. `None` when there was nothing true to draw.
    pub rect: Option<Rect>,
}

/// One row's drawing decisions, so the row function takes a spec and not a row
/// type's worth of arguments.
struct RowSpec<'a> {
    id: egui::Id,
    rect: Rect,
    last: bool,
    marked: bool,
    current: bool,
    held: bool,
    state: Option<State>,
    cover: Cover,
    art_radius: egui::CornerRadius,
    title: &'a str,
    subtitle: String,
    spoken: String,
    glyph: Icon,
    action: Act,
}

/// The state and paint a row needs, shared by every band.
struct Drawer<'a> {
    search: &'a Search,
    art: &'a mut Art,
    want: u32,
    now: Instant,
    tokens: &'a Tokens,
}

/// Draws the menu under `field` and returns what its rows asked for.
///
/// The answer is read from the search state, guarded by the query it was asked
/// for: an answer to the query the user has typed past is never drawn.
pub fn show(
    ui: &mut Ui,
    field: Rect,
    search: &mut Search,
    transport: &Transport,
    art: &mut Art,
    now: Instant,
    tokens: &Tokens,
) -> Menu {
    let view = search.menu_view();
    let height = match &view {
        MenuView::Bands(listing) => {
            geometry::bands_height(&listing.shape.rows_per_band(), listing.shape.floor)
        }
        MenuView::Empty | MenuView::Failed(_) => geometry::line_height(),
        // No skeleton, no empty box: nothing true to say.
        MenuView::Nothing => return Menu::default(),
    };
    let menu = geometry::menu_rect(field, ui.ctx().content_rect(), height);

    // Every state of the menu is one surface, and every row is keyed under one
    // salt, so a test or a screen reader can name what was drawn.
    let base = ui.make_persistent_id("search-menu-row");

    // A pointer that is merely resting over the menu is not the last device; a
    // pointer that moves over it is.
    let pointer_moved = ui.input(|input| {
        input.pointer.is_moving()
            && input
                .pointer
                .hover_pos()
                .is_some_and(|pos| menu.contains(pos))
    });

    let mut click = None;
    let area = egui::Area::new(ui.scope_id().with("search-menu"))
        .order(Order::Foreground)
        .fixed_pos(menu.min)
        .constrain(false)
        .fade_in(false);

    area.show(ui.ctx(), |ui| {
        let card = Card {
            rect: menu,
            id: base.with("surface"),
            // The rows are the only Tab stops a menu of results needs, and it has
            // none: the keyboard path is arrows only.
            sense: Sense::CLICK,
            tokens,
        };
        menu_box::card(ui, card, |ui| {
            let mut drawer = Drawer {
                search,
                art,
                want: want_px(
                    crate::theme::extent::MENU_COVER,
                    ui.ctx().pixels_per_point(),
                ),
                now,
                tokens,
            };
            match &view {
                MenuView::Bands(listing) => {
                    let placement = geometry::place(
                        menu,
                        &listing.shape.rows_per_band(),
                        listing.shape.floor,
                        ui.ctx().pixels_per_point(),
                    );
                    for (band_index, (band, rects)) in
                        listing.shape.bands.iter().zip(&placement.bands).enumerate()
                    {
                        if let Some(rule) = rects.rule {
                            ui.painter()
                                .rect_filled(rule, radius::NONE, tokens.palette.border);
                        }
                        drawer.heading(ui, band, rects.head, base);
                        for (row_index, row) in rects.rows.iter().enumerate() {
                            let last = !listing.shape.floor
                                && band_index + 1 == listing.shape.bands.len()
                                && row_index + 1 == band.shown;
                            let spec = match band.kind {
                                crate::search_index::Kind::Tracks => {
                                    let entry = &listing.answer[row_index];
                                    track_row_spec(
                                        *row,
                                        last,
                                        entry,
                                        transport,
                                        &mut drawer,
                                        base,
                                        row_index,
                                    )
                                }
                                crate::search_index::Kind::Albums => {
                                    let album = &listing.index.albums[row_index];
                                    RowSpec {
                                        id: base.with(("album", row_index)),
                                        rect: *row,
                                        last,
                                        marked: drawer.search.marked()
                                            == Some(crate::search_index::Row::Album(row_index)),
                                        current: false,
                                        held: false,
                                        state: None,
                                        cover: drawer.art.look(
                                            album.cover.as_ref(),
                                            drawer.want,
                                            now,
                                        ),
                                        art_radius: radius::XS,
                                        title: &album.name,
                                        subtitle: album.artist.clone(),
                                        spoken: format!(
                                            "{}, album by {}",
                                            album.name, album.artist
                                        ),
                                        glyph: Icon::ChevronRight,
                                        action: Act::OpenAlbum(row_index),
                                    }
                                }
                                crate::search_index::Kind::Artists => {
                                    let artist = &listing.index.artists[row_index];
                                    let cover = art::artist_cover(
                                        drawer.search.portrait(&artist.uri),
                                        artist.record_cover.as_ref(),
                                        drawer.art,
                                        drawer.want,
                                        now,
                                    );
                                    RowSpec {
                                        id: base.with(("artist", row_index)),
                                        rect: *row,
                                        last,
                                        marked: drawer.search.marked()
                                            == Some(crate::search_index::Row::Artist(row_index)),
                                        current: false,
                                        held: false,
                                        state: None,
                                        cover,
                                        art_radius: radius::PILL,
                                        title: &artist.name,
                                        subtitle: crate::search_index::tracks_phrase(artist.tracks),
                                        spoken: format!(
                                            "{}, artist, {}",
                                            artist.name,
                                            crate::search_index::tracks_phrase(artist.tracks)
                                        ),
                                        glyph: Icon::ChevronRight,
                                        action: Act::OpenArtist(row_index),
                                    }
                                }
                            };
                            if let Some(act) = drawer.row(ui, spec) {
                                click = Some(act);
                            }
                        }
                    }
                    if let Some(floor_rect) = placement.floor
                        && let Some(act) = drawer.floor(ui, floor_rect, base)
                    {
                        click = Some(act);
                    }
                }
                MenuView::Empty => {
                    let query = drawer.search.key().unwrap_or_default();
                    line(ui, tokens, menu, &format!("No matches for “{query}”"), base);
                }
                MenuView::Failed(failure) => {
                    line(
                        ui,
                        tokens,
                        menu,
                        crate::signin::failure_message(*failure),
                        base,
                    );
                }
                MenuView::Nothing => {}
            }
        });
    });

    // The pointer moved over the menu, so the pointer owns the highlight.
    if pointer_moved {
        search.pointer_moved();
    }

    Menu {
        click,
        rect: Some(menu),
    }
}

/// One track row's spec: its own cover, credits and transport state.
fn track_row_spec<'a>(
    rect: Rect,
    last: bool,
    entry: &'a TrackEntry,
    transport: &Transport,
    drawer: &mut Drawer<'_>,
    base: egui::Id,
    index: usize,
) -> RowSpec<'a> {
    let spoken = if entry.artists.is_empty() {
        entry.title.clone()
    } else {
        format!("{} by {}", entry.title, entry.artists)
    };
    let hold = transport.hold(&entry.uri);
    let state = state_of(&entry.uri, transport);
    RowSpec {
        id: base.with(("track", index)),
        rect,
        last,
        marked: drawer.search.marked() == Some(crate::search_index::Row::Track(index)),
        current: !matches!(state, State::Resting),
        held: hold.held,
        state: Some(state),
        cover: drawer
            .art
            .look(entry.cover.as_ref(), drawer.want, drawer.now),
        art_radius: radius::XS,
        title: &entry.title,
        subtitle: subtitle(entry),
        spoken,
        glyph: Icon::Play,
        action: Act::Play(index),
    }
}

impl Drawer<'_> {
    /// Draws one band's heading: its label at the left, its share at the right.
    fn heading(&mut self, ui: &mut Ui, band: &Band, rect: Rect, base: egui::Id) {
        let spoken = band.spoken();
        let hit = ui.interact(rect, base.with(("head", band.kind)), Sense::hover());
        hit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, spoken.clone()));

        let inner = Rect::from_min_max(
            pos2(rect.left() + space::S3, rect.top() + space::S2),
            pos2(rect.right() - space::S3, rect.bottom() - space::S1),
        );
        let color = self.tokens.palette.text_subtle;
        let font = FontId::new(text::XXS, fonts::mono(MonoWeight::Medium));
        let spacing = recipe::tracking_pts(text::XXS, tracking::CAPS);
        let share = recipe::line(
            ui,
            &band.share().to_uppercase(),
            font.clone(),
            color,
            spacing,
            inner.width(),
        );
        let label_width = (inner.width() - share.size().x - space::S2).max(0.0);
        let label = recipe::line(
            ui,
            &band.heading().to_uppercase(),
            font,
            color,
            spacing,
            label_width,
        );
        let y = inner.center().y;
        ui.painter()
            .galley(pos2(inner.left(), y - label.size().y / 2.0), label, color);
        ui.painter().galley(
            pos2(inner.right() - share.size().x, y - share.size().y / 2.0),
            share,
            color,
        );
    }

    /// Draws one row and reports the click it took.
    fn row(&mut self, ui: &mut Ui, spec: RowSpec<'_>) -> Option<Act> {
        let marked = spec.marked;
        let hit = ui.interact(spec.rect, spec.id, Sense::CLICK);
        let spoken = spec.spoken.clone();
        hit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, spoken.clone()));
        let hovered = hit.hovered() && !marked && self.search.device() == Device::Pointer;
        let corners = row_corners(false, spec.last);
        let tier = if marked {
            Tier::Keyboard
        } else if hovered {
            Tier::Pointer
        } else {
            Tier::None
        };
        paint_highlight(ui, self.tokens, spec.rect, corners, tier);

        // Identity beats focus: the playing row keeps the accent rule even while
        // it is marked.
        if spec.current || marked {
            paint_marker(
                ui,
                spec.rect,
                corners,
                if spec.current {
                    self.tokens.palette.accent
                } else {
                    self.tokens.palette.text
                },
            );
        }

        let parts = geometry::row_parts(spec.rect);
        // A row too narrow to keep the cover does not ask the art store for one.
        if parts.art.width() > 0.0 {
            art::tile(
                ui.painter(),
                parts.art,
                spec.art_radius,
                self.tokens.palette.border,
                spec.cover,
                self.tokens,
            );
        }
        paint_text(
            ui,
            parts.text,
            spec.title,
            &spec.subtitle,
            marked || hovered,
            self.tokens,
        );

        if spec.held && marked {
            icon::paint(
                ui.painter(),
                Icon::Pause,
                parts.glyph,
                Size::SM,
                self.tokens.palette.text,
            );
        } else if spec.held {
            if let Some(state) = spec.state {
                track_row::equaliser(ui, parts.glyph, state, self.tokens);
            }
        } else if marked {
            icon::paint(
                ui.painter(),
                spec.glyph,
                parts.glyph,
                Size::SM,
                self.tokens.palette.text,
            );
        }

        if hit.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        clicked_on(ui, &hit, spec.rect).then_some(spec.action)
    }

    /// Draws the floor row: `See more results for “q”`, with the chevron and the
    /// same highlight a result row takes. It is drawn exactly when a band hid
    /// something.
    fn floor(&mut self, ui: &mut Ui, row: Rect, base: egui::Id) -> Option<Act> {
        let marked = self.search.marked() == Some(crate::search_index::Row::Floor);
        let hit = ui.interact(row, base.with("floor"), Sense::CLICK);
        let hovered = hit.hovered() && !marked && self.search.device() == Device::Pointer;
        let corners = row_corners(false, true);
        let tier = if marked {
            Tier::Keyboard
        } else if hovered {
            Tier::Pointer
        } else {
            Tier::None
        };

        paint_highlight(ui, self.tokens, row, corners, tier);
        // The floor is an action, not a result: one hairline separates it from
        // the rows above. It sits over the wash, where the reference's border
        // does.
        ui.painter().rect_filled(
            Rect::from_min_size(row.min, vec2(row.width(), stroke::HAIRLINE)),
            radius::NONE,
            self.tokens.palette.border,
        );
        if marked {
            paint_marker(ui, row, corners, self.tokens.palette.text);
        }

        let inner = Rect::from_min_max(
            pos2(row.left() + space::S3, row.top() + space::S1),
            pos2(row.right() - space::S3, row.bottom() - space::S1),
        );
        let glyph = Rect::from_center_size(
            pos2(inner.right() - Size::SM.side / 2.0, inner.center().y),
            vec2(Size::SM.side, Size::SM.side),
        );
        let color = if marked {
            self.tokens.palette.text
        } else {
            self.tokens.palette.text_muted
        };
        let query = self.search.key().unwrap_or_default();
        let label = format!("See more results for “{query}”");
        hit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label.clone()));
        let width = (glyph.left() - space::S3 - inner.left()).max(0.0);
        let mut job = LayoutJob::default();
        job.wrap.max_width = width;
        job.wrap.max_rows = 1;
        job.wrap.overflow_character = Some('…');
        job.append(
            "See more results for ",
            0.0,
            TextFormat {
                font_id: FontId::new(text::SM, fonts::sans(Weight::Regular)),
                color: self.tokens.palette.text_muted,
                ..Default::default()
            },
        );
        job.append(
            &format!("“{query}”"),
            0.0,
            TextFormat {
                font_id: FontId::new(text::SM, fonts::sans(Weight::Medium)),
                color: self.tokens.palette.text,
                ..Default::default()
            },
        );
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            pos2(inner.left(), inner.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
        icon::paint(ui.painter(), Icon::ChevronRight, glyph, Size::SM, color);

        if hit.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        clicked_on(ui, &hit, row).then_some(Act::OpenResults)
    }
}

/// The single muted line for no matches or a failed read: nothing to press, no
/// spinner.
fn line(ui: &mut Ui, tokens: &Tokens, menu: Rect, message: &str, base: egui::Id) {
    let row = geometry::row_rect(menu, 0);
    // The line has no action, but it is still one node in the accessibility
    // tree: an unlabelled card is an empty region to a screen reader.
    let hit = ui.interact(row, base.with("line"), Sense::hover());
    hit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, message));
    let inner = Rect::from_min_max(
        pos2(row.left() + space::S3, row.top() + space::S1),
        pos2(row.right() - space::S3, row.bottom() - space::S1),
    );
    let color = tokens.palette.text_subtle;
    let galley = recipe::line(
        ui,
        message,
        FontId::new(text::XS, fonts::sans(Weight::Regular)),
        color,
        0.0,
        inner.width(),
    );
    ui.painter().galley(
        pos2(inner.left(), inner.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// The title over `artist · album`, both truncated to the text column.
fn paint_text(ui: &Ui, rect: Rect, title_text: &str, subtitle: &str, lit: bool, tokens: &Tokens) {
    if rect.width() <= 0.0 {
        return;
    }
    let title_color = tokens.palette.text;
    let sub_color = if lit {
        tokens.palette.text_muted
    } else {
        tokens.palette.text_subtle
    };
    let title = recipe::line(
        ui,
        title_text,
        FontId::new(text::SM, fonts::sans(Weight::Medium)),
        title_color,
        0.0,
        rect.width(),
    );
    let sub = (!subtitle.is_empty()).then(|| {
        recipe::line(
            ui,
            subtitle,
            FontId::new(text::XS, fonts::sans(Weight::Regular)),
            sub_color,
            0.0,
            rect.width(),
        )
    });

    let title_height = title.size().y;
    let height = title_height + sub.as_ref().map_or(0.0, |line| line.size().y);
    let top = rect.center().y - height / 2.0;
    ui.painter()
        .galley(pos2(rect.left(), top), title, title_color);
    if let Some(sub) = sub {
        ui.painter()
            .galley(pos2(rect.left(), top + title_height), sub, sub_color);
    }
}

/// `artist · album`, or whichever half exists.
fn subtitle(entry: &TrackEntry) -> String {
    match (entry.artists.is_empty(), entry.album.as_deref()) {
        (false, Some(album)) => format!("{} · {album}", entry.artists),
        (false, None) => entry.artists.clone(),
        (true, Some(album)) => album.to_owned(),
        (true, None) => String::new(),
    }
}

#[cfg(test)]
mod tests;
