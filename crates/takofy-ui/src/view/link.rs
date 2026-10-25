//! The shared link: painted words that open a page.
//!
//! A surface lays its line out once and hands the galley here; this module
//! decides what the pointer can click, what the cursor says, what assistive
//! technology announces, and how the hover ink eases. A link with no page to
//! open is painted as plain text and takes nothing.

use std::sync::Arc;

use egui::{Color32, FontId, Galley, Pos2, Rect, Sense, Ui};

use crate::route::{Page, credit_is_show};
use crate::theme::{Tokens, ease, motion};
use crate::view::text as recipe;

/// The page a link opens.
#[derive(Clone, Copy)]
pub(super) enum Target<'a> {
    Album(&'a str),
    Credit(&'a str),
}

impl Target<'_> {
    /// The page the target names.
    fn page(self) -> Page {
        match self {
            Self::Album(uri) => Page::Album(uri.to_owned()),
            Self::Credit(uri) => Page::credit(uri),
        }
    }

    /// The page kind's name, spelled as the link's label says it.
    fn kind(self) -> &'static str {
        match self {
            Self::Album(_) => "album",
            Self::Credit(uri) if credit_is_show(uri) => "show",
            Self::Credit(_) => "artist",
        }
    }
}

/// The type a link is drawn in.
pub(super) struct Face {
    pub font: FontId,
    pub tracking: f32,
}

/// Lays `text` out as one elided line in `face`.
///
/// The layout ink is a placeholder: the galley is painted through
/// [`Placed::paint`], and layout does not depend on colour.
pub(super) fn lay(ui: &Ui, text: &str, face: &Face, width: f32) -> Arc<Galley> {
    recipe::line(
        ui,
        text,
        face.font.clone(),
        Color32::PLACEHOLDER,
        face.tracking,
        width,
    )
}

/// A line laid out at its place on screen, ready to be painted.
pub(super) struct Placed {
    pub pos: Pos2,
    pub galley: Arc<Galley>,
}

impl Placed {
    /// The box the line occupies on screen.
    pub(super) fn rect(&self) -> Rect {
        Rect::from_min_size(self.pos, self.galley.size())
    }

    /// Paints the line in `color`, which must be a real ink.
    ///
    /// Never [`Color32::PLACEHOLDER`]: the tessellator asserts that it never
    /// receives one. The placeholder is only the argument to the layout call
    /// that measures a line, and the ink is chosen before painting.
    pub(super) fn paint(self, ui: &Ui, color: Color32) {
        ui.painter().galley(self.pos, self.galley, color);
    }

    /// Paints the line as a link, or as plain text when there is no page.
    ///
    /// The hit area is the painted, elided words inside `clip`, never the whole
    /// cell. The interaction is created before the label is painted so the
    /// painter can read its hover. The sense is `Sense::CLICK` rather than
    /// `Sense::click()`: a link must not add a focus stop to a long list.
    pub(super) fn link(
        self,
        ui: &Ui,
        id: egui::Id,
        clip: Rect,
        target: Option<Target<'_>>,
        rest: Color32,
        tokens: &Tokens,
    ) -> Shown {
        let rect = self.rect();
        let hit = rect.intersect(clip);
        let Some(target) = target.filter(|_| hit.width() > 0.0 && hit.height() > 0.0) else {
            self.paint(ui, rest);
            return Shown {
                rect,
                hovered: false,
                page: None,
            };
        };
        let response = ui.interact(hit, id, Sense::CLICK);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Link,
                true,
                format!("Open {} {}", target.kind(), self.galley.text()),
            )
        });
        let hovered = response.hovered();
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let reveal = ui.ctx().animate_bool_with_time_and_easing(
            id.with("hover"),
            hovered,
            motion::FAST,
            ease::out,
        );
        self.paint(ui, rest.lerp_to_gamma(tokens.palette.text, reveal));
        Shown {
            rect,
            hovered,
            page: response.clicked().then(|| target.page()),
        }
    }
}

/// What one link drew and reported.
pub(super) struct Shown {
    /// The box the painted words occupy.
    #[allow(dead_code, reason = "a caller places a plain fact beside the words")]
    pub rect: Rect,
    /// Whether the pointer is on the words.
    pub hovered: bool,
    /// The page a click asked for.
    pub page: Option<Page>,
}

#[cfg(test)]
mod tests {
    use egui::{Event, Pos2, Rect, vec2};

    use super::*;
    use crate::view::test_support::click_events;

    const TEXT: &str = "Slow Tide";
    const ID: &str = "link";

    fn context() -> egui::Context {
        let tokens = Tokens::DARK;
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::theme::fonts::definitions());
        ctx.set_global_style(crate::theme::style(&tokens));
        ctx
    }

    fn input(events: Vec<Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 800.0))),
            events,
            ..Default::default()
        }
    }

    fn face() -> Face {
        Face {
            font: recipe::track_secondary(),
            tracking: 0.0,
        }
    }

    /// A cell wide enough for [`TEXT`] to fit unelided.
    fn cell() -> Rect {
        Rect::from_min_size(Pos2::new(100.0, 100.0), vec2(300.0, 40.0))
    }

    /// Draws the words into `place` and hit-tests them against `clip`.
    fn draw(ui: &Ui, place: Rect, clip: Rect, target: Option<Target<'_>>, width: f32) -> Shown {
        let tokens = Tokens::DARK;
        let face = face();
        let galley = lay(ui, TEXT, &face, width);
        let pos = Pos2::new(place.left(), place.center().y - galley.size().y / 2.0);
        Placed { pos, galley }.link(
            ui,
            egui::Id::unique(ID),
            clip,
            target,
            tokens.palette.text_muted,
            &tokens,
        )
    }

    /// One pass, with the box the link's interaction registered.
    fn frame(
        ctx: &egui::Context,
        place: Rect,
        clip: Rect,
        target: Option<Target<'_>>,
        width: f32,
        events: Vec<Event>,
    ) -> (egui::FullOutput, Shown, Option<Rect>) {
        let mut shown = None;
        let mut hit = None;
        let mut output = ctx.run_ui(input(events), |ui| {
            shown = Some(draw(ui, place, clip, target, width));
            hit = ui
                .ctx()
                .read_response(egui::Id::unique(ID))
                .map(|response| response.rect);
        });
        output.textures_delta.clear();
        (output, shown.expect("the pass draws the link"), hit)
    }

    /// One click, returning the release pass's link and output.
    fn click(
        ctx: &egui::Context,
        place: Rect,
        clip: Rect,
        target: Option<Target<'_>>,
        width: f32,
        at: Pos2,
    ) -> (egui::FullOutput, Shown) {
        let _ = frame(ctx, place, clip, target, width, Vec::new());
        let mut last = None;
        for event in click_events(at) {
            let (output, shown, _) = frame(ctx, place, clip, target, width, vec![event]);
            last = Some((output, shown));
        }
        last.expect("the click runs")
    }

    /// The ink and box of the painted words.
    fn painted(output: &egui::FullOutput, label: &str) -> Option<(Color32, Rect)> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text() == label => Some((
                    text.fallback_color,
                    Rect::from_min_size(text.pos, text.galley.size()),
                )),
                _ => None,
            })
    }

    /// Every label a pass gave a link node.
    fn link_labels(output: &egui::FullOutput) -> Vec<String> {
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        (node.role() == egui::accesskit::Role::Link)
                            .then(|| node.label().map(str::to_owned))
                            .flatten()
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn only_the_painted_words_take_the_click() {
        let target = Some(Target::Credit("spotify:artist:b"));
        let ctx = context();
        let (_, shown, _) = frame(&ctx, cell(), cell(), target, cell().width(), Vec::new());
        let (_, on) = click(
            &ctx,
            cell(),
            cell(),
            target,
            cell().width(),
            shown.rect.center(),
        );
        assert_eq!(on.page, Some(Page::credit("spotify:artist:b")));
        assert!(on.hovered);

        let ctx = context();
        let (_, shown, _) = frame(&ctx, cell(), cell(), target, cell().width(), Vec::new());
        let past = Pos2::new(shown.rect.right() + 10.0, shown.rect.center().y);
        assert!(cell().contains(past), "the probe must stay inside the cell");
        let (_, off) = click(&ctx, cell(), cell(), target, cell().width(), past);
        assert_eq!(off.page, None, "a click past the words opened the page");
        assert!(!off.hovered);
    }

    #[test]
    fn no_target_paints_plain_and_adds_no_node() {
        let tokens = Tokens::DARK;
        let ctx = context();
        ctx.enable_accesskit();
        let at = cell().center();
        let (output, shown, _) = frame(
            &ctx,
            cell(),
            cell(),
            None,
            cell().width(),
            vec![Event::PointerMoved(at)],
        );
        assert_eq!(shown.page, None);
        assert!(!shown.hovered);
        assert!(
            link_labels(&output).is_empty(),
            "plain text registered a link node"
        );
        let (ink, _) = painted(&output, TEXT).expect("the words are painted");
        assert_eq!(ink, tokens.palette.text_muted, "plain text stays at rest");
        assert_ne!(
            output.platform_output.cursor_icon,
            egui::CursorIcon::PointingHand,
            "plain text points the hand"
        );
    }

    #[test]
    fn hover_points_the_hand_and_brightens() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let target = Some(Target::Credit("spotify:artist:b"));
        let (_, shown, _) = frame(&ctx, cell(), cell(), target, cell().width(), Vec::new());
        let at = shown.rect.center();
        let mut events = vec![Event::PointerMoved(at)];
        // The ink eases in, so the hover is read once the animation has settled.
        let mut settled = None;
        for _ in 0..60 {
            let (output, on, _) = frame(&ctx, cell(), cell(), target, cell().width(), events);
            assert!(on.hovered, "the pointer rests on the words");
            settled = Some(output);
            events = Vec::new();
        }
        let output = settled.expect("the pass runs");
        assert_eq!(
            output.platform_output.cursor_icon,
            egui::CursorIcon::PointingHand,
            "a link points the hand"
        );
        let (ink, _) = painted(&output, TEXT).expect("the words are painted");
        assert_eq!(
            ink, tokens.palette.text,
            "the hover ink settles at full ink"
        );
    }

    #[test]
    fn elided_text_hit_stays_inside_clip() {
        let ctx = context();
        let width = 48.0;
        let (output, shown, hit) = frame(
            &ctx,
            cell(),
            cell(),
            Some(Target::Album("spotify:album:a")),
            width,
            Vec::new(),
        );
        let (_, words) = painted(&output, TEXT).expect("the words are painted");
        assert!(words.width() <= width + 0.01, "the words elide to {width}");
        assert!(
            shown.rect.right() <= cell().right() + 0.01,
            "the placed line ends at {} inside a cell ending at {}",
            shown.rect.right(),
            cell().right()
        );
        let hit = hit.expect("an elided link is still interacted");
        assert!(
            hit.left() >= cell().left() - 0.01 && hit.right() <= cell().right() + 0.01,
            "the hit is {hit:?} outside the cell {:?}",
            cell()
        );
    }

    #[test]
    fn a_disjoint_clip_adds_no_node() {
        let ctx = context();
        ctx.enable_accesskit();
        let place = cell();
        let clip = Rect::from_min_size(Pos2::new(500.0, 500.0), cell().size());
        let (output, shown, hit) = frame(
            &ctx,
            place,
            clip,
            Some(Target::Credit("spotify:artist:b")),
            cell().width(),
            Vec::new(),
        );
        assert!(
            link_labels(&output).is_empty(),
            "a disjoint clip made a node"
        );
        assert_eq!(shown.page, None);
        assert!(!shown.hovered);
        assert!(hit.is_none(), "a disjoint clip was interacted");
        let (ink, _) = painted(&output, TEXT).expect("the words are painted");
        assert_eq!(ink, Tokens::DARK.palette.text_muted);
    }

    #[test]
    fn zero_width_clip_does_not_panic_or_interact() {
        let ctx = context();
        ctx.enable_accesskit();
        let place = cell();
        let clip = Rect::from_min_size(place.min, vec2(0.0, place.height()));
        let (output, shown, hit) = frame(
            &ctx,
            place,
            clip,
            Some(Target::Album("spotify:album:a")),
            place.width(),
            Vec::new(),
        );
        assert_eq!(shown.page, None);
        assert!(!shown.hovered);
        assert!(hit.is_none(), "a zero-width clip was interacted");
        assert!(link_labels(&output).is_empty());
    }

    #[test]
    fn link_node_is_named_by_kind() {
        let cases = [
            (Target::Album("spotify:album:a"), "Open album Slow Tide"),
            (Target::Credit("spotify:artist:a"), "Open artist Slow Tide"),
            (Target::Credit("spotify:show:a"), "Open show Slow Tide"),
        ];
        for (target, want) in cases {
            let ctx = context();
            ctx.enable_accesskit();
            let (output, _, _) = frame(
                &ctx,
                cell(),
                cell(),
                Some(target),
                cell().width(),
                Vec::new(),
            );
            assert_eq!(link_labels(&output), vec![want.to_owned()], "{want}");
        }
    }
}
