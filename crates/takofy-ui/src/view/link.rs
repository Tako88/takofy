//! The shared link: painted words that open a page.
//!
//! A surface lays its line out once and hands the galley here; this module
//! decides what the pointer can click, what the cursor says, what assistive
//! technology announces, and how the hover ink eases. A link with no page to
//! open is painted as plain text and takes nothing.

use std::sync::Arc;

use egui::emath::GuiRounding as _;
use egui::{Color32, FontId, Galley, Pos2, Rect, Sense, Ui, vec2};

use crate::route::{Page, credit_is_show};
use crate::theme::{Tokens, ease, extent, motion, radius, stroke};
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
    ///
    /// The hover eases the words from `rest` to full ink and fades a hairline
    /// underline in under them on the same animation, so a link whose rest ink
    /// is already full ink still answers the pointer.
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
        let ink = rest.lerp_to_gamma(tokens.palette.text, reveal);
        self.paint(ui, ink);
        if reveal > 0.0 {
            // Snap y and height only: a hairline must land on whole pixels, but
            // the rule stays exactly as wide as the words.
            let scale = ui.pixels_per_point();
            let underline = Rect::from_min_size(
                Pos2::new(
                    rect.left(),
                    (rect.bottom() - extent::LINK_UNDERLINE_INSET).round_to_pixels(scale),
                ),
                vec2(rect.width(), stroke::HAIRLINE.round_to_pixels(scale)),
            );
            // The rule fades in by alpha; the ink itself is already the hovered
            // ink, so a mid-fade difference in opacity is the fade at work.
            ui.painter()
                .rect_filled(underline, radius::NONE, ink.gamma_multiply(reveal));
        }
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
    /// The rest ink every link in these tests is drawn in, unless it says so.
    const REST: Color32 = Tokens::DARK.palette.text_muted;

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

    /// Draws the words into `place` in `rest` ink and hit-tests them against
    /// `clip`.
    fn draw(
        ui: &Ui,
        place: Rect,
        clip: Rect,
        target: Option<Target<'_>>,
        rest: Color32,
        width: f32,
    ) -> Shown {
        let tokens = Tokens::DARK;
        let face = face();
        let galley = lay(ui, TEXT, &face, width);
        let pos = Pos2::new(place.left(), place.center().y - galley.size().y / 2.0);
        Placed { pos, galley }.link(ui, egui::Id::unique(ID), clip, target, rest, &tokens)
    }

    /// One pass, with the box the link's interaction registered.
    fn frame(
        ctx: &egui::Context,
        place: Rect,
        clip: Rect,
        target: Option<Target<'_>>,
        rest: Color32,
        width: f32,
        events: Vec<Event>,
    ) -> (egui::FullOutput, Shown, Option<Rect>) {
        let mut shown = None;
        let mut hit = None;
        let mut output = ctx.run_ui(input(events), |ui| {
            shown = Some(draw(ui, place, clip, target, rest, width));
            hit = ui
                .ctx()
                .read_response(egui::Id::unique(ID))
                .map(|response| response.rect);
        });
        output.textures_delta.clear();
        (output, shown.expect("the pass draws the link"), hit)
    }

    /// The pass the hover settles in, with the pointer resting on the words.
    fn settle(
        ctx: &egui::Context,
        place: Rect,
        clip: Rect,
        target: Option<Target<'_>>,
        rest: Color32,
        width: f32,
    ) -> egui::FullOutput {
        let (_, shown, _) = frame(ctx, place, clip, target, rest, width, Vec::new());
        let mut events = vec![Event::PointerMoved(shown.rect.center())];
        let mut settled = None;
        // The ink eases in, so the hover is read once the animation has settled.
        for _ in 0..60 {
            let (output, on, _) = frame(ctx, place, clip, target, rest, width, events);
            assert!(on.hovered, "the pointer rests on the words");
            settled = Some(output);
            events = Vec::new();
        }
        settled.expect("the pass runs")
    }

    /// One click, returning the release pass's link and output.
    fn click(
        ctx: &egui::Context,
        place: Rect,
        clip: Rect,
        target: Option<Target<'_>>,
        rest: Color32,
        width: f32,
        at: Pos2,
    ) -> (egui::FullOutput, Shown) {
        let _ = frame(ctx, place, clip, target, rest, width, Vec::new());
        let mut last = None;
        for event in click_events(at) {
            let (output, shown, _) = frame(ctx, place, clip, target, rest, width, vec![event]);
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

    /// Every filled rect a pass painted, as its box and its fill.
    fn fills(output: &egui::FullOutput) -> Vec<(Rect, Color32)> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Rect(rect) => Some((rect.rect, rect.fill)),
                _ => None,
            })
            .collect()
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
        let (_, shown, _) = frame(
            &ctx,
            cell(),
            cell(),
            target,
            REST,
            cell().width(),
            Vec::new(),
        );
        let (_, on) = click(
            &ctx,
            cell(),
            cell(),
            target,
            REST,
            cell().width(),
            shown.rect.center(),
        );
        assert_eq!(on.page, Some(Page::credit("spotify:artist:b")));
        assert!(on.hovered);

        let ctx = context();
        let (_, shown, _) = frame(
            &ctx,
            cell(),
            cell(),
            target,
            REST,
            cell().width(),
            Vec::new(),
        );
        let past = Pos2::new(shown.rect.right() + 10.0, shown.rect.center().y);
        assert!(cell().contains(past), "the probe must stay inside the cell");
        let (_, off) = click(&ctx, cell(), cell(), target, REST, cell().width(), past);
        assert_eq!(off.page, None, "a click past the words opened the page");
        assert!(!off.hovered);
    }

    #[test]
    fn at_rest_no_underline_is_painted() {
        let ctx = context();
        let target = Some(Target::Credit("spotify:artist:b"));
        let (output, shown, _) = frame(
            &ctx,
            cell(),
            cell(),
            target,
            REST,
            cell().width(),
            Vec::new(),
        );
        assert!(!shown.hovered, "the pointer is off the words");
        assert!(
            fills(&output).is_empty(),
            "a link at rest painted {:?}",
            fills(&output)
        );
    }

    #[test]
    fn hover_paints_an_underline_under_the_words() {
        let ctx = context();
        let target = Some(Target::Credit("spotify:artist:b"));
        let output = settle(&ctx, cell(), cell(), target, REST, cell().width());
        let (ink, words) = painted(&output, TEXT).expect("the words are painted");
        let rules: Vec<(Rect, Color32)> = fills(&output)
            .into_iter()
            .filter(|(rect, _)| (rect.height() - stroke::HAIRLINE).abs() < 0.01)
            .collect();
        assert_eq!(rules.len(), 1, "the hover painted {rules:?}");
        let (rule, rule_ink) = rules[0];
        assert!(
            (rule.width() - words.width()).abs() < 0.01,
            "the underline spans {} under words {} wide",
            rule.width(),
            words.width()
        );
        assert!(
            rule.left() >= words.left() - 0.01 && rule.right() <= words.right() + 0.01,
            "the underline {rule:?} runs past the words {words:?}"
        );
        assert!(
            rule.top() >= words.top() && rule.bottom() <= words.bottom(),
            "the underline {rule:?} leaves the line box {words:?}"
        );
        assert!(
            rule.center().y > words.center().y,
            "the underline sits above the words' centre"
        );
        let snap = 0.5 / output.pixels_per_point;
        let want_top = words.bottom() - extent::LINK_UNDERLINE_INSET;
        assert!(
            (rule.top() - want_top).abs() <= snap,
            "the underline top {} sits {} above the words' bottom {}",
            rule.top(),
            words.bottom() - rule.top(),
            words.bottom()
        );
        assert!(
            (rule.height() - stroke::HAIRLINE).abs() < 0.01,
            "the underline is {} tall, not a hairline",
            rule.height()
        );
        assert_eq!(rule_ink, ink, "the underline is drawn in another ink");
    }

    #[test]
    fn an_elided_link_underlines_only_its_visible_run() {
        let ctx = context();
        let target = Some(Target::Album("spotify:album:a"));
        let width = 48.0;
        let output = settle(&ctx, cell(), cell(), target, REST, width);
        let (_, words) = painted(&output, TEXT).expect("the words are painted");
        let rules: Vec<Rect> = fills(&output)
            .into_iter()
            .filter(|(rect, _)| (rect.height() - stroke::HAIRLINE).abs() < 0.01)
            .map(|(rect, _)| rect)
            .collect();
        assert_eq!(rules.len(), 1, "the hover painted {rules:?}");
        let rule = rules[0];
        assert!(
            rule.width() <= words.width() + 0.01,
            "the underline spans {} under {} wide elided words",
            rule.width(),
            words.width()
        );
        assert!(
            rule.right() <= words.right() + 0.01,
            "the underline {rule:?} runs past the elided words {words:?}"
        );
        assert!(
            rule.left() >= words.left() - 0.01,
            "the underline {rule:?} starts before the elided words {words:?}"
        );
    }

    #[test]
    fn hovering_adds_no_hit_area() {
        let ctx = context();
        let target = Some(Target::Credit("spotify:artist:b"));
        let (_, shown, rest) = frame(
            &ctx,
            cell(),
            cell(),
            target,
            REST,
            cell().width(),
            Vec::new(),
        );
        let rest = rest.expect("the link is interacted at rest");
        let mut events = vec![Event::PointerMoved(shown.rect.center())];
        for _ in 0..60 {
            let (_, on, hit) = frame(&ctx, cell(), cell(), target, REST, cell().width(), events);
            assert!(on.hovered, "the pointer rests on the words");
            assert_eq!(
                hit.expect("the link stays interacted"),
                rest,
                "the hover changed the hit area"
            );
            events = Vec::new();
        }
    }

    #[test]
    fn hover_eases_the_rest_ink_toward_text() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let target = Some(Target::Credit("spotify:artist:b"));
        let (_, shown, _) = frame(
            &ctx,
            cell(),
            cell(),
            target,
            REST,
            cell().width(),
            Vec::new(),
        );
        let mut events = vec![Event::PointerMoved(shown.rect.center())];
        let mut seen = Vec::new();
        for _ in 0..60 {
            let (output, _, _) = frame(&ctx, cell(), cell(), target, REST, cell().width(), events);
            seen.push(painted(&output, TEXT).expect("the words are painted").0);
            events = Vec::new();
        }
        let between = seen
            .iter()
            .position(|ink| *ink != REST)
            .expect("the ink never left its rest colour");
        assert_ne!(seen[between], tokens.palette.text, "the ink jumped to full");
        for pair in seen.windows(2) {
            assert!(
                pair[0].r() <= pair[1].r()
                    && pair[0].g() <= pair[1].g()
                    && pair[0].b() <= pair[1].b(),
                "the ink stepped back from {:?} to {:?}",
                pair[0],
                pair[1]
            );
        }
        assert_eq!(
            seen.last(),
            Some(&tokens.palette.text),
            "the hover ink settles at full ink"
        );
    }

    #[test]
    fn a_text_ink_link_still_shows_the_underline() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let target = Some(Target::Credit("spotify:artist:b"));
        let output = settle(
            &ctx,
            cell(),
            cell(),
            target,
            tokens.palette.text,
            cell().width(),
        );
        let (ink, words) = painted(&output, TEXT).expect("the words are painted");
        assert_eq!(ink, tokens.palette.text, "the words keep their rest ink");
        assert!(
            fills(&output)
                .iter()
                .any(|(rect, _)| (rect.height() - stroke::HAIRLINE).abs() < 0.01
                    && rect.center().y > words.center().y),
            "a link already at full ink painted no underline"
        );
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
            REST,
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
        let output = settle(&ctx, cell(), cell(), target, REST, cell().width());
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
            REST,
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
            REST,
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
            REST,
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
                REST,
                cell().width(),
                Vec::new(),
            );
            assert_eq!(link_labels(&output), vec![want.to_owned()], "{want}");
        }
    }
}
