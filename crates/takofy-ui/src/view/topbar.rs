//! The topbar: the drawer toggle, history, the search field and the trail.
//!
//! The trail, the history pair and the drawer toggle do something: they say
//! where you are and how to leave. The search field waits for a view that does
//! not exist yet, so it is drawn disabled and does not pretend.

use egui::{FontId, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};

use crate::availability::Control;
use crate::icon::{self, Icon};
use crate::layout::{self, Sidebar};
use crate::library::Library;
use crate::theme::icon::Size;
use crate::theme::{MonoWeight, Tokens, extent, fonts, radius, space, stroke, text, tracking};

use super::controls;
use super::shell::{row_centred, rule};
use super::text as recipe;

/// What the topbar did this frame.
pub struct Topbar {
    /// The move the trail or the history pair asked for. It is a request, not a
    /// move: the shell applies it to the library, because the crumbs borrow the
    /// library they were built from.
    pub nav: Option<Nav>,
    /// The drawer toggle's response, so the shell can read a click from it and
    /// hand focus back to it when the drawer closes.
    pub toggle: Option<egui::Response>,
}

/// A move the topbar can ask the shell for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    /// Back to the previous page.
    Back,
    /// Forward to the page Back left behind.
    Forward,
    /// Back to the playlists grid.
    Playlists,
}

/// Draws the topbar inside `rect`.
pub fn show(
    ui: &mut Ui,
    library: &mut Library,
    form: Sidebar,
    rect: Rect,
    tokens: &Tokens,
) -> Topbar {
    let crumbs = crate::trail::trail(library);
    let natural = measure(ui, &crumbs, tokens);
    let cols = layout::topbar_for(rect, form, natural);
    let places = cols.columns(rect);
    let (can_back, can_forward) = (library.can_back(), library.can_forward());
    let mut nav = None;

    let toggle = places.toggle.map(|rect| {
        controls::round_button(
            ui,
            tokens,
            Control::DrawerToggle,
            rect,
            Icon::Library,
            Size::LG,
            "Open your library",
            true,
        )
        .response
    });

    row_centred(ui, "history", places.history, |ui| {
        for (control, glyph, label, requested, live) in [
            (
                Control::HistoryBack,
                Icon::ArrowLeft,
                "Go back",
                Nav::Back,
                can_back,
            ),
            (
                Control::HistoryForward,
                Icon::ArrowRight,
                "Go forward",
                Nav::Forward,
                can_forward,
            ),
        ] {
            let rect = Rect::from_min_size(ui.cursor().min, Vec2::splat(extent::BUTTON));
            let slot =
                controls::round_button(ui, tokens, control, rect, glyph, Size::BASE, label, live);
            if slot.response.clicked() {
                nav = Some(requested);
            }
            ui.advance_cursor_after_rect(Rect::from_min_size(
                rect.min,
                Vec2::new(extent::BUTTON + space::S1, extent::BUTTON),
            ));
        }
    });

    search(ui, places.search, tokens);

    if trail(ui, &crumbs, places.trail, tokens) {
        nav = Some(Nav::Playlists);
    }

    rule(
        ui,
        Rect::from_min_size(
            Pos2::new(rect.left() + stroke::HAIRLINE, rect.bottom()),
            Vec2::new(rect.width() - 2.0 * stroke::HAIRLINE, stroke::HAIRLINE),
        ),
        tokens.palette.border,
    );

    Topbar { nav, toggle }
}

/// The natural width of the trail's text, which the solver needs before it can
/// decide how much of it fits.
fn measure(ui: &Ui, crumbs: &[crate::trail::Crumb<'_>], tokens: &Tokens) -> f32 {
    let joined = crumbs
        .iter()
        .map(|crumb| crumb.label)
        .collect::<Vec<_>>()
        .join(" / ");
    let galley = recipe::spaced(
        ui,
        &joined,
        FontId::new(text::XXS, fonts::mono(MonoWeight::Medium)),
        tokens.palette.text_subtle,
        recipe::tracking_pts(text::XXS, tracking::WIDE),
    );
    // Separators and the rule in front of the current crumb are not in the text.
    galley.size().x + space::S2 + strokes(crumbs)
}

fn strokes(crumbs: &[crate::trail::Crumb<'_>]) -> f32 {
    if crumbs.iter().any(|crumb| crumb.current) {
        space::S2
    } else {
        0.0
    }
}

/// The path you came by, flush against the topbar's right padding.
fn trail(ui: &mut Ui, crumbs: &[crate::trail::Crumb<'_>], rect: Rect, tokens: &Tokens) -> bool {
    let font = FontId::new(text::XXS, fonts::mono(MonoWeight::Medium));
    let spacing = recipe::tracking_pts(text::XXS, tracking::WIDE);
    let mut x = rect.left();
    let mut clicked = false;

    for (index, crumb) in crumbs.iter().enumerate() {
        if index > 0 {
            let separator =
                recipe::spaced(ui, "/", font.clone(), tokens.palette.text_subtle, spacing);
            let at = Pos2::new(x + space::S2, rect.center().y - separator.size().y / 2.0);
            ui.painter()
                .galley(at, separator.clone(), tokens.palette.text_subtle);
            x = at.x + separator.size().x + space::S2;
        }

        let base = if crumb.current {
            tokens.palette.text
        } else {
            tokens.palette.text_muted
        };
        let galley = recipe::spaced(ui, crumb.label, font.clone(), base, spacing);
        let width = galley.size().x.min((rect.right() - x).max(0.0));
        let place = Rect::from_min_size(
            Pos2::new(x, rect.center().y - galley.size().y / 2.0),
            Vec2::new(width, galley.size().y),
        );

        if crumb.current {
            ui.painter().rect_filled(
                Rect::from_min_size(
                    Pos2::new(
                        place.left() - space::S2,
                        rect.center().y - Size::MD.side / 2.0,
                    ),
                    Vec2::new(stroke::RULE, Size::MD.side),
                ),
                radius::SM,
                tokens.palette.text,
            );
            ui.painter().galley(place.min, galley, base);
        } else if crumb.link {
            // One link crumb can exist at a time, which is why this is safe to
            // name with a single control: two of them would share a widget id.
            let slot = controls::slot(
                ui,
                Control::TrailPlaylists,
                egui::Role::Button,
                place,
                crumb.label,
                tokens,
                true,
            );
            // A link brightens to full bone under the pointer, as the reference's
            // does. The width does not depend on the colour, so the galley is laid
            // out again only once the hover is known.
            let color = base.lerp_to_gamma(tokens.palette.text, slot.hover);
            let galley = if color == base {
                galley
            } else {
                recipe::spaced(ui, crumb.label, font.clone(), color, spacing)
            };
            ui.painter().galley(place.min, galley, color);
            clicked |= slot.response.clicked();
        } else {
            // Home has no view to go back to, so it is text, not a control.
            ui.painter().galley(place.min, galley, base);
        }
        x = place.right() + space::S2;
    }

    clicked
}

/// The search field. It waits, so it never takes focus and never becomes a text
/// editor that would fight the drawer for Escape.
fn search(ui: &mut Ui, rect: Rect, tokens: &Tokens) {
    if rect.width() <= 0.0 {
        return;
    }

    ui.painter().rect(
        rect,
        radius::SM,
        tokens.palette.surface_raised,
        Stroke::new(stroke::HAIRLINE, tokens.palette.border),
        StrokeKind::Inside,
    );

    let glyph = Rect::from_min_size(
        Pos2::new(
            rect.left() + space::S3,
            rect.center().y - Size::MD.side / 2.0,
        ),
        Vec2::splat(Size::MD.side),
    );
    icon::paint(
        ui.painter(),
        Icon::Search,
        glyph,
        Size::MD,
        tokens.palette.text_subtle,
    );

    let placeholder = recipe::spaced(
        ui,
        "Search",
        FontId::new(text::SM, fonts::sans(crate::theme::Weight::Regular)),
        tokens.palette.text_subtle,
        0.0,
    );
    let at = Pos2::new(
        glyph.right() + space::S2,
        rect.center().y - placeholder.size().y / 2.0,
    );
    let room = (rect.right() - space::S3 - at.x).max(0.0);
    if room >= placeholder.size().x {
        ui.painter()
            .galley(at, placeholder, tokens.palette.text_subtle);
    }

    controls::slot(
        ui,
        Control::SearchField,
        egui::Role::SearchInput,
        rect,
        "Search",
        tokens,
        true,
    );
}
