//! The topbar: the drawer toggle, history, the search field and the trail.
//!
//! The trail, the history pair and the drawer toggle say where you are and how
//! to leave. The search field is a real text edit: it owns the query, the clear
//! control and the keyboard path its drop-down reads (R1–R5, R17, R18). Its
//! states are border-only, because the menu hangs off the field's own width and
//! a field that resizes while you type moves the thing you are aiming at.

use egui::{
    Align, FontId, Key, Layout, Modifiers, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, Vec2,
};

use crate::availability::Control;
use crate::icon::{self, Icon};
use crate::layout::{self, Sidebar};
use crate::library::Library;
use crate::search::{Act, KeyInput, MenuView};
use crate::theme::icon::Size;
use crate::theme::{
    MonoWeight, Tokens, Weight, extent, fonts, radius, space, stroke, text, tracking,
};

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
    /// The search field as this frame drew it, or `None` when the layout gave it
    /// no room. The shell anchors the menu to `rect`, keeps the menu open on
    /// `has_focus`, and applies `act` when no other command took the frame.
    pub search_field: Option<Field>,
}

/// The search field as one frame drew it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Field {
    /// The painted box: what the menu hangs from and what counts as the field
    /// for the pointer.
    pub rect: Rect,
    /// The text edit's widget id. The shell surrenders the field's focus through
    /// it when it applies `OpenResults`, because only an applied action takes
    /// the keyboard with the menu it closes.
    pub id: egui::Id,
    /// Whether the text edit has the keyboard after this frame.
    pub has_focus: bool,
    /// What this frame's keys asked the shell to do.
    pub act: Act,
}

/// A move the topbar can ask the shell for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    /// Back to the previous page.
    Back,
    /// Forward to the page Back left behind.
    Forward,
    /// Back to the library root, which is Home.
    Home,
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

    // The layout hands the toggle a whole column; the button in it is the same
    // square as every other topbar button.
    let toggle = places.toggle.map(|column| {
        let rect = Rect::from_center_size(column.center(), Vec2::splat(extent::BUTTON));
        controls::round_button(
            ui,
            tokens,
            Control::DrawerToggle,
            rect,
            Icon::Panel,
            Size::MD,
            "Open your library",
            true,
            false,
        )
        .response
    });

    // Home sits in the cluster now that the sidebar's navigation is gone, and it
    // doubles as the position marker when the library root is on screen.
    let home = library.page().is_none();
    row_centred(ui, "history", places.history, |ui| {
        // The cluster spaces itself, so egui must not add its own gap on top of
        // each advance or the third button is pushed past the row's clip.
        ui.spacing_mut().item_spacing.x = 0.0;
        let cluster: [(Control, Icon, &str, Nav, bool, bool); 3] = [
            (Control::NavHome, Icon::Home, "Home", Nav::Home, true, home),
            (
                Control::HistoryBack,
                Icon::ArrowLeft,
                "Go back",
                Nav::Back,
                can_back,
                false,
            ),
            (
                Control::HistoryForward,
                Icon::ArrowRight,
                "Go forward",
                Nav::Forward,
                can_forward,
                false,
            ),
        ];
        let side = extent::BUTTON;
        let top = crate::view::shell::row_centre(ui) - side / 2.0;
        for (index, (control, glyph, label, requested, live, current)) in
            cluster.into_iter().enumerate()
        {
            let rect = Rect::from_min_size(Pos2::new(ui.cursor().min.x, top), Vec2::splat(side));
            let slot = controls::round_button(
                ui,
                tokens,
                control,
                rect,
                glyph,
                Size::BASE,
                label,
                live,
                current,
            );
            if slot.response.clicked() {
                nav = Some(requested);
            }
            // The cluster is exactly its buttons and the gaps between them, so
            // the last one gets no trailing gap to overflow the row by.
            let advance = if index + 1 == cluster.len() {
                extent::BUTTON
            } else {
                extent::BUTTON + space::S1
            };
            ui.advance_cursor_after_rect(Rect::from_min_size(rect.min, Vec2::new(advance, side)));
        }
    });

    // The trail draws before the field, because the crumbs borrow the library
    // the field has to be edited through: once `trail` returns, the borrow is
    // done and the search state is free.
    if trail(ui, &crumbs, places.trail, tokens) {
        nav = Some(Nav::Home);
    }
    let search_field = search(ui, places.search, &mut library.search, tokens);

    rule(
        ui,
        Rect::from_min_size(
            Pos2::new(rect.left() + stroke::HAIRLINE, rect.bottom()),
            Vec2::new(rect.width() - 2.0 * stroke::HAIRLINE, stroke::HAIRLINE),
        ),
        tokens.palette.border,
    );

    Topbar {
        nav,
        toggle,
        search_field,
    }
}

/// The natural width of the trail's text, which the solver needs before it can
/// decide how much of it fits.
fn measure(ui: &Ui, crumbs: &[crate::trail::Crumb<'_>], tokens: &Tokens) -> f32 {
    let joined = crumbs
        .iter()
        .map(|crumb| crumb.label.to_uppercase())
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
        let label = crumb.label.to_uppercase();
        let galley = recipe::spaced(ui, &label, font.clone(), base, spacing);
        // The current crumb leads with the reference's accent rule, inside its
        // own box: a rule, a gap, then the name. Drawing it in the separator's
        // gap would read as a second slash.
        let lead = if crumb.current { space::S2 } else { 0.0 };
        let text_left = x + lead;
        let width = galley.size().x.min((rect.right() - text_left).max(0.0));
        let place = Rect::from_min_size(
            Pos2::new(text_left, rect.center().y - galley.size().y / 2.0),
            Vec2::new(width, galley.size().y),
        );
        if crumb.current {
            ui.painter().rect_filled(
                Rect::from_min_size(
                    Pos2::new(x, place.top()),
                    Vec2::new(stroke::RULE, galley.size().y),
                ),
                radius::NONE,
                tokens.palette.accent,
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
                recipe::spaced(ui, &label, font.clone(), color, spacing)
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

/// The placeholder, in every topbar (R17).
const PLACEHOLDER: &str = "Track names, artists, albums";

/// The text edit's own key filter: egui's defaults, plus Escape.
///
/// egui's memory surrenders focus on Escape before the topbar sees it, so
/// without this the field would blur before R3's first step could act. The two
/// arrow flags are the text edit's defaults and must stay: they keep arrow
/// presses out of spatial focus movement.
///
/// It is one constant because focus is taken two ways — the text edit's own
/// `show`, and the clear control's hand-back — and the filter stored with the
/// focused widget must be the same in both.
const FIELD_FILTER: egui::EventFilter = egui::EventFilter {
    tab: false,
    horizontal_arrows: true,
    vertical_arrows: true,
    escape: true,
};

/// The search field: a real text edit inside the painted box.
///
/// The keys the field owns are taken before `TextEdit::show`, because a
/// singleline edit surrenders focus on Enter and would eat the key that plays a
/// marked row. Escape is taken only when the search state has something to undo,
/// so a drawer under an empty, closed search keeps its own Escape.
///
/// The right inset is reserved whether or not the clear control is drawn, so the
/// text column — and therefore the field's width — never changes (R4/R5).
fn search(
    ui: &mut Ui,
    column: Rect,
    search: &mut crate::search::Search,
    tokens: &Tokens,
) -> Option<Field> {
    if column.width() <= 0.0 {
        return None;
    }
    // The column is the topbar's full height; the field inside it is the
    // reference's 2rem, centred, so it does not read as a filled band.
    let rect = Rect::from_center_size(column.center(), Vec2::new(column.width(), extent::SEARCH_H));

    // The fill first, the border last: TextEdit paints its own text in between,
    // and the border is the only thing that changes with focus (R4).
    ui.painter()
        .rect_filled(rect, radius::SM, tokens.palette.surface_raised);

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

    // The clear control's box: the field's height, the field's right padding and
    // the 12px glyph. Its width is reserved even when the glyph is not drawn.
    let clear = Rect::from_min_max(
        Pos2::new(
            rect.right() - space::S3 - Size::SM.side - space::S1,
            rect.top(),
        ),
        Pos2::new(rect.right() - space::S1, rect.bottom()),
    );
    // The text edit is the whole painted box and insets its text instead of
    // being a narrower widget: a click on the magnifier, either gap or the
    // padding is a click on the field (R2), not "elsewhere" that blurs it,
    // closes the menu and never reaches `clicked_field`. The clear control is
    // interacted after this, in the same layer, so it still wins on its own rect.
    let inset = egui::Margin {
        left: (glyph.right() + space::S2 - rect.left()).round() as i8,
        right: (rect.right() - (clear.left() - space::S2)).round() as i8,
        top: 0,
        bottom: 0,
    };

    // The field's id is the control's, so the drawer's focus hand-back and the
    // shell's reads name the same widget the text edit stores its state under.
    let id = ui.scope_id().with((Control::SearchField, ()));
    let focused_before = ui.memory(|memory| memory.has_focus(id));
    // Read every key through the same unmodified match it is consumed with: a
    // read that ignored the modifiers while the consume matched them let
    // Cmd+Down mark a row and move the caret at once. Arrow repeats are fine;
    // Enter and Escape act once per press, so a held key cannot do two things.
    let (down, up, enter, enter_once, escape) = if focused_before {
        ui.input(|input| {
            (
                pressed(input, Key::ArrowDown),
                pressed(input, Key::ArrowUp),
                pressed(input, Key::Enter),
                pressed_once(input, Key::Enter),
                pressed_once(input, Key::Escape),
            )
        })
    } else {
        (false, false, false, false, false)
    };
    if focused_before {
        ui.input_mut(|input| {
            // Read first, consume second: `consume_key` removes the event, so a
            // read after it would see nothing.
            if down {
                input.consume_key(Modifiers::NONE, Key::ArrowDown);
            }
            if up {
                input.consume_key(Modifiers::NONE, Key::ArrowUp);
            }
            // Every unmodified Enter is the field's while it has the keyboard: a
            // held one would otherwise reach the text edit, which surrenders
            // focus on any Enter. Only the first press acts.
            if enter {
                input.consume_key(Modifiers::NONE, Key::Enter);
            }
            if escape && search.escapes() {
                input.consume_key(Modifiers::NONE, Key::Escape);
            }
        });
    }

    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt("search-field-text")
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_clip_rect(rect);
    // The hint is the reference's subtle tier; egui's own hint colour is the
    // muted one, a step too bright for a placeholder. The caret is the text
    // token rather than egui's default blue, because the field's only states are
    // the palette's.
    child.visuals_mut().weak_text_color = Some(tokens.palette.text_subtle);
    child.visuals_mut().text_cursor.stroke = Stroke::new(stroke::HAIRLINE, tokens.palette.text);
    // While a row carries the rule, the field must not also blink a caret: the
    // rule is what says where Enter lands (Q2). A zero-width stroke is empty, so
    // epaint's line tessellator draws nothing for it.
    if search.mark().is_some() {
        child.visuals_mut().text_cursor.stroke.width = 0.0;
    }

    let output = egui::TextEdit::singleline(&mut search.text)
        .id(id)
        // The inset has to ride on the frame: `TextEdit::margin` is applied only
        // when no frame is given, so a frameless edit would ignore it and paint
        // its text over the magnifier. A `Frame::NONE` with a margin draws
        // nothing and still insets.
        .frame(egui::Frame::NONE.inner_margin(inset))
        .min_size(rect.size())
        .desired_width(rect.width())
        // egui's text-edit style is top-aligned; the field's 2rem box wants the
        // line on its centre, the way the reference's flex box centres it.
        .align(egui::Align2::LEFT_CENTER)
        .font(FontId::new(text::SM, fonts::sans(Weight::Regular)))
        .text_color(tokens.palette.text)
        .event_filter(FIELD_FILTER)
        .hint_text(RichText::new(PLACEHOLDER))
        .show(&mut child);
    let response = output.response;
    let typed = response.changed();
    let clicked_field = response.clicked();
    let focused = response.has_focus();
    // The text edit's own `WidgetInfo` reports the generic text-input role.
    // Refining the node directly — rather than through a second `widget_info` —
    // sets the search role and the name without emitting a duplicate
    // value-changed event for every keystroke (Q3).
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::Role::SearchInput);
        node.set_label("Search");
    });

    if !search.text.is_empty() {
        let hit = ui.interact(clear, id.with("clear"), Sense::click());
        if hit.clicked() {
            search.text.clear();
            // Only the text edit reports `changed`; a programmatic clear has to
            // tell the state the key moved itself, and close the menu the query
            // was keeping open (R3 step two, without the keyboard).
            search.text_changed();
            search.close();
            ui.memory_mut(|memory| {
                memory.request_focus(id);
                // `request_focus` stores a default `EventFilter`, so without this
                // the first Escape or arrow after clearing would behave as if the
                // field were a plain widget.
                memory.set_focus_lock_filter(id, FIELD_FILTER);
            });
        }
        if hit.hovered() {
            ui.painter()
                .rect_filled(clear, radius::XS, tokens.palette.surface_hover);
        }
        let ink = if hit.hovered() {
            tokens.palette.text
        } else {
            tokens.palette.text_subtle
        };
        let mark = Rect::from_center_size(clear.center(), Vec2::splat(Size::SM.side));
        icon::paint(ui.painter(), Icon::Close, mark, Size::SM, ink);
        hit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Clear search"));
    }

    // Focus is read after the text edit so the border is this frame's truth. It
    // is border-only: no glow, no icon swap, no width change (R4).
    let border = if focused {
        tokens.palette.text
    } else {
        tokens.palette.border
    };
    ui.painter().rect_stroke(
        rect,
        radius::SM,
        Stroke::new(stroke::HAIRLINE, border),
        StrokeKind::Inside,
    );

    let (shown, floor) = match search.menu_view() {
        MenuView::Rows { tracks, floor, .. } => (tracks.len(), floor),
        // There is no row to mark when the menu says nothing true (R14).
        MenuView::Empty | MenuView::Failed(_) | MenuView::Nothing => (0, false),
    };
    let act = search.step(
        KeyInput {
            down,
            up,
            enter: enter_once,
            escape,
            typed,
            clicked_field,
            focused,
        },
        shown,
        floor,
    );
    // `OpenResults` is applied by the shell, and only when no other command took
    // the frame; the shell is the one that surrenders the field's focus, so a
    // dropped action leaves the menu open with the keyboard still in the field.

    Some(Field {
        rect,
        id,
        // The clear control may have asked for the field back after `show`, so
        // the answer is the memory's, not the response's.
        has_focus: ui.memory(|memory| memory.has_focus(id)),
        act,
    })
}

/// Any press of `key` with no modifiers, auto-repeat included.
///
/// The field reads and consumes with the same rule: `consume_key` matches a key
/// whose modifiers match logically, which would eat an Alt+Down the read never
/// saw.
fn pressed(input: &egui::InputState, key: Key) -> bool {
    input.events.iter().any(|event| {
        matches!(
            event,
            egui::Event::Key {
                key: pressed,
                pressed: true,
                modifiers,
                ..
            } if *pressed == key && !modifiers.any()
        )
    })
}

/// A first press of `key` with no modifiers, never an auto-repeat.
///
/// A held Enter must not replay the marked row, and a held Escape must not step
/// through both of R3's steps: one press, one action.
fn pressed_once(input: &egui::InputState, key: Key) -> bool {
    input.events.iter().any(|event| {
        matches!(
            event,
            egui::Event::Key {
                key: pressed,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } if *pressed == key && !modifiers.any()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Q3: a later `widget_info` on the text edit's own id refines the role
    /// egui's builder gave it. Read inside the pass, because that is when an
    /// AccessKit node exists.
    #[test]
    fn the_field_announces_itself_as_a_search_input() {
        let tokens = Tokens::DARK;
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(crate::theme::style(&tokens));
        ctx.enable_accesskit();

        let mut library = Library::default();
        let content = Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0));
        let regions = layout::regions_for(content.width());
        let shell = layout::shell_for(&regions);
        let bar = layout::grid_for(content, &shell).topbar;

        let mut role = None;
        let input = egui::RawInput {
            screen_rect: Some(content),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let topbar = show(ui, &mut library, regions.sidebar, bar, &tokens);
            assert!(topbar.search_field.is_some(), "the field is drawn");
            let id = ui.scope_id().with((Control::SearchField, ()));
            role = ui.ctx().accesskit_node_builder(id, |node| node.role());
        });
        output.textures_delta.clear();
        assert_eq!(role, Some(egui::Role::SearchInput));
    }

    /// The magnifier sits at the field's left edge, so the text has to start
    /// after it. `TextEdit::margin` is ignored when a frame is given, which is
    /// how the text came to be painted over the glyph — this reads the painted
    /// galley rather than the numbers that build it.
    #[test]
    fn the_text_starts_after_the_magnifier() {
        let tokens = Tokens::DARK;
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(crate::theme::style(&tokens));

        let mut library = Library::default();
        library.search.text = "harbour".to_owned();
        let content = Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0));
        let regions = layout::regions_for(content.width());
        let shell = layout::shell_for(&regions);
        let bar = layout::grid_for(content, &shell).topbar;
        let input = egui::RawInput {
            screen_rect: Some(content),
            ..Default::default()
        };
        // Two passes: the first sizes the edit, the second paints the text.
        let mut field = None;
        let mut first = ctx.run_ui(input.clone(), |ui| {
            let topbar = show(ui, &mut library, regions.sidebar, bar, &tokens);
            field = topbar.search_field.map(|drawn| drawn.rect);
        });
        first.textures_delta.clear();
        let mut output = ctx.run_ui(input, |ui| {
            let topbar = show(ui, &mut library, regions.sidebar, bar, &tokens);
            field = topbar.search_field.map(|drawn| drawn.rect);
        });
        output.textures_delta.clear();
        let field = field.expect("the field is drawn");

        fn text_at(shapes: &[egui::epaint::ClippedShape], wanted: &str) -> Option<f32> {
            fn walk(shape: &egui::Shape, wanted: &str, found: &mut Option<f32>) {
                match shape {
                    egui::Shape::Text(text) => {
                        if text.galley.job.text == wanted {
                            *found = Some(text.pos.x);
                        }
                    }
                    egui::Shape::Vec(shapes) => {
                        for shape in shapes {
                            walk(shape, wanted, found);
                        }
                    }
                    _ => {}
                }
            }
            let mut found = None;
            for clipped in shapes {
                walk(&clipped.shape, wanted, &mut found);
            }
            found
        }

        let x = text_at(&output.shapes, "harbour").expect("the typed text is painted");
        let after_glyph = field.left() + space::S3 + Size::MD.side + space::S2;
        assert!(
            x >= after_glyph - 0.5,
            "the text starts at {x}, inside the magnifier (glyph ends at {after_glyph})"
        );
        assert!(
            x <= after_glyph + space::S5,
            "the text starts at {x}, far past the glyph"
        );
    }
}
