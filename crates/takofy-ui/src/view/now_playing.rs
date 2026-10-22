//! The Now Playing panel: what the transport is on, and what is queued after it.
//!
//! The panel is handed everything it draws — the transport, the queue, the art
//! store and the frame's clock — and reports what its queue pane asked for. It
//! never reaches for any of them itself, and applying a jump, a press or a lift
//! is the shell's job, once every region has drawn.
//!
//! The tabs are the reference's pair and both are live: the queue is state the
//! window holds, and the lyrics pane asks for what it draws.

use std::time::Instant;

use egui::{FontId, Pos2, Rect, Ui, Vec2};

use crate::availability::Control;
use crate::cover::{Art, Cover};
use crate::icon::Icon;
use crate::layout::panel::{self, Stack};
use crate::player::Transport;
use crate::queue::Queue;
use crate::theme::icon::Size;
use crate::theme::{Tokens, extent, radius, space, text, tracking};

use crate::lyrics::{LyricsPanel, Tab};
use crate::menu::{Anchor, Device, Door, Listing};
use crate::verbs;

use super::art;
use super::context_menu;
use super::controls;
use super::lyrics_pane;
use super::queue_pane;
use super::text as recipe;

/// The tabs the reference draws, in its order.
const TABS: [(&str, Control); 2] = [
    ("Queue", Control::NowPlayingQueueTab),
    ("Lyrics", Control::NowPlayingLyricsTab),
];

/// What a frame of the panel asks the window to do.
///
/// Every one of these is the caller's to apply, after every region has drawn: a
/// frame that already carries a command must not also move the queue, or the
/// queue would name a track the transport was never asked to play.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    /// What the queue pane asked for.
    pub queue: queue_pane::Reply,
    /// The tab a press asked for.
    pub tab: Option<Tab>,
    /// Whether the lyrics pane's Retry was pressed.
    pub retry: bool,
    /// The door the head's more button opened.
    pub menu: Option<Door>,
}

/// The frame's own facts, so the panel's signature stays readable.
pub struct Frame<'a> {
    pub art: &'a mut Art,
    pub now: Instant,
    pub cover: Cover,
    pub tokens: &'a Tokens,
}

/// Draws the panel's contents inside its own rect.
pub fn show(
    ui: &mut Ui,
    transport: &Transport,
    queue: &Queue,
    lyrics: &LyricsPanel,
    position_ms: u32,
    frame: &mut Frame<'_>,
) -> Reply {
    let tokens = frame.tokens;
    let band = ui.available_rect_before_wrap();
    let meta_h = meta_height(ui, transport);
    let tabs_h = tabs_height(ui, tokens);
    let stack = panel::stack_for(band, meta_h, tabs_h);

    let menu = head(ui, stack, queue, tokens);
    art::tile(
        ui.painter(),
        stack.art,
        radius::SM,
        tokens.palette.border_strong,
        frame.cover,
        tokens,
    );
    meta(ui, stack.meta, transport, tokens);
    let tab = tabs(ui, stack.tabs, tokens, lyrics.showing());

    let showing = tab.unwrap_or(lyrics.showing());
    let retry = match showing {
        Tab::Queue => {
            return Reply {
                queue: queue_pane::show(ui, stack.pane, queue, frame.art, frame.now, tokens),
                tab,
                retry: false,
                menu,
            };
        }
        Tab::Lyrics => lyrics_pane::show(
            ui,
            stack.pane,
            lyrics,
            position_ms,
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            tokens,
        ),
    };
    Reply {
        queue: queue_pane::Reply::default(),
        tab,
        retry,
        menu,
    }
}

/// The head: the panel's caption and the current entry's more button.
///
/// The button is live only when there is a current entry with something to do:
/// a queue with no current row, a local file and an entry with nothing to open
/// all leave it waiting.
fn head(ui: &mut Ui, stack: Stack, queue: &Queue, tokens: &Tokens) -> Option<Door> {
    let caption = recipe::caption(ui, "Now playing", tokens);
    ui.painter().galley(
        Pos2::new(
            stack.head.left(),
            stack.head.center().y - caption.size().y / 2.0,
        ),
        caption,
        tokens.palette.text_subtle,
    );

    let more = Rect::from_min_size(
        Pos2::new(
            stack.head.right() - extent::BUTTON,
            stack.head.center().y - extent::BUTTON / 2.0,
        ),
        Vec2::splat(extent::BUTTON),
    );
    let live = queue.current().is_some_and(|entry| {
        !verbs::track_items(std::slice::from_ref(entry), None, &Listing::Current).is_empty()
    });
    let id = controls::slot_id(ui, Control::NowPlayingMore, ());
    let open = context_menu::open_opener(ui.ctx()) == Some(id);
    let slot = controls::icon_button_open(
        ui,
        tokens,
        Control::NowPlayingMore,
        (),
        more,
        Icon::More,
        Size::MD,
        "More",
        live,
        open,
    );
    if let Some(door) = context_menu::button_door(ui, &slot, more, id) {
        return Some(door);
    }
    // The keyboard's door, taken only while the button holds it, so the key is
    // left alone everywhere else.
    if !slot.response.has_focus() {
        return None;
    }
    context_menu::menu_key_pressed(ui).then_some(Door {
        anchor: Anchor::Button(more),
        opener: id,
        device: Device::Keyboard,
    })
}

/// Title, artist and the transport's state, or the one line that says nothing is
/// playing.
fn meta(ui: &Ui, rect: Rect, transport: &Transport, tokens: &Tokens) {
    if rect.width() <= 0.0 {
        return;
    }
    let Some(now) = &transport.now_playing else {
        let line = recipe::line(
            ui,
            "Nothing playing",
            recipe::track_secondary(),
            tokens.palette.text_muted,
            recipe::tracking_pts(text::SM, tracking::SLIGHT),
            rect.width(),
        );
        ui.painter().galley(
            Pos2::new(rect.left(), rect.center().y - line.size().y / 2.0),
            line,
            tokens.palette.text_muted,
        );
        return;
    };

    let title = recipe::line(
        ui,
        &now.title,
        recipe::panel_title(),
        tokens.palette.text,
        recipe::tracking_pts(text::MD, tracking::TIGHT),
        rect.width(),
    );
    let mut top = rect.top();
    let height = title.size().y;
    ui.painter()
        .galley(Pos2::new(rect.left(), top), title, tokens.palette.text);
    top += height;

    if !now.artists.is_empty() {
        let artist = recipe::line(
            ui,
            &now.artists,
            recipe::track_secondary(),
            tokens.palette.text_muted,
            0.0,
            rect.width(),
        );
        let height = artist.size().y;
        ui.painter().galley(
            Pos2::new(rect.left(), top),
            artist,
            tokens.palette.text_muted,
        );
        top += height;
    }

    let state = transport.state.label();
    if !state.is_empty() {
        let line = recipe::line(
            ui,
            state,
            recipe::track_mono(text::XXS),
            tokens.palette.text_subtle,
            recipe::tracking_pts(text::XXS, tracking::SLIGHT),
            rect.width(),
        );
        ui.painter().galley(
            Pos2::new(rect.left(), top + space::S1),
            line,
            tokens.palette.text_subtle,
        );
    }
}

/// The tab pair, reporting the tab a press asked for.
fn tabs(ui: &mut Ui, rect: Rect, tokens: &Tokens, chosen: Tab) -> Option<Tab> {
    if rect.width() <= 0.0 {
        return None;
    }
    let mut pressed = None;
    let mut left = rect.left();
    for (index, (label, control)) in TABS.into_iter().enumerate() {
        let size = controls::chip_size(ui, tokens, label);
        let chip = Rect::from_min_size(
            Pos2::new(left, rect.top() + (rect.height() - size.y) / 2.0),
            size,
        );
        let tab = if index == 0 { Tab::Queue } else { Tab::Lyrics };
        let slot = controls::chip_in(ui, tokens, control, chip, label, chosen == tab);
        if slot.response.clicked() {
            pressed = Some(tab);
        }
        left += size.x + space::PX;
    }
    pressed
}

/// The height the three meta lines take, or the one line nothing playing takes.
fn meta_height(ui: &Ui, transport: &Transport) -> f32 {
    let Some(now) = &transport.now_playing else {
        return line_height(ui, &recipe::track_secondary());
    };
    let mut height = line_height(ui, &recipe::panel_title());
    if !now.artists.is_empty() {
        height += line_height(ui, &recipe::track_secondary());
    }
    if !transport.state.label().is_empty() {
        height += space::S1 + line_height(ui, &recipe::track_mono(text::XXS));
    }
    height
}

/// The height of the taller of the two chips.
fn tabs_height(ui: &Ui, tokens: &Tokens) -> f32 {
    TABS.into_iter()
        .map(|(label, _)| controls::chip_size(ui, tokens, label).y)
        .fold(0.0, f32::max)
}

fn line_height(ui: &Ui, font: &FontId) -> f32 {
    ui.ctx().fonts_mut(|fonts| fonts.row_height(font))
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Shape, Ui, Vec2};
    use takofy_model::{CoverImage, CoverUrl, TrackEntry};

    use super::*;
    use crate::cover::Art;
    use crate::message::{TransportUpdate, Update};
    use crate::selection::Press;
    use crate::theme::{fonts, style};

    const PROBE: Vec2 = Vec2::new(360.0, 700.0);

    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn input(events: Vec<Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, PROBE)),
            events,
            ..Default::default()
        }
    }

    fn run(ctx: &Context, input: egui::RawInput, draw: impl FnMut(&mut Ui)) {
        let mut output = ctx.run_ui(input, draw);
        output.textures_delta.clear();
    }

    fn drawn_texts(output: &egui::FullOutput) -> Vec<&str> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect()
    }

    fn painted_at(output: &egui::FullOutput, label: &str) -> Option<Pos2> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                Shape::Text(text) if text.galley.text() == label => Some(text.pos),
                _ => None,
            })
    }

    fn track(index: usize) -> TrackEntry {
        TrackEntry {
            uri: format!("t{index}"),
            title: format!("t{index}"),
            artists: format!("a{index}"),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    /// A transport on a named track, ready to draw.
    fn playing(title: &str, artists: &str) -> Transport {
        let mut transport = Transport::default();
        transport.play("spotify:track:x", title, artists, None);
        transport
    }

    /// The panel's own frame, with a queue and an art store behind it.
    struct Panel {
        transport: Transport,
        queue: Queue,
        art: Art,
        now: Instant,
        lyrics: crate::lyrics::LyricsPanel,
    }

    impl Panel {
        fn new(transport: Transport) -> Self {
            Self {
                transport,
                queue: Queue::default(),
                art: Art::default(),
                now: Instant::now(),
                lyrics: crate::lyrics::LyricsPanel::default(),
            }
        }

        fn with_queue(mut self, count: usize, current: usize) -> Self {
            let tracks: Vec<TrackEntry> = (0..count).map(track).collect();
            self.queue.start(&tracks, current, &mut self.transport);
            self
        }

        /// Draws one frame and returns the queue reply it reported.
        fn frame(&mut self, ctx: &Context, events: Vec<Event>) -> queue_pane::Reply {
            self.reply(ctx, events).queue
        }

        /// Draws one frame and returns the whole reply it reported.
        fn reply(&mut self, ctx: &Context, events: Vec<Event>) -> Reply {
            let tokens = Tokens::DARK;
            let cover = self.art.look(
                self.transport
                    .now_playing
                    .as_ref()
                    .and_then(|now| now.cover.as_ref()),
                self.now,
            );
            self.art.begin_frame();
            let now = self.now;
            let mut reply = Reply::default();
            let Self {
                transport,
                queue,
                art,
                lyrics,
                ..
            } = self;
            let position_ms = transport.position_ms;
            run(ctx, input(events), |ui| {
                reply = show(
                    ui,
                    transport,
                    queue,
                    lyrics,
                    position_ms,
                    &mut Frame {
                        art,
                        now,
                        cover,
                        tokens: &tokens,
                    },
                );
            });
            reply
        }

        /// Draws one frame and returns the text it painted.
        fn texts(&mut self, ctx: &Context) -> Vec<String> {
            let tokens = Tokens::DARK;
            let cover = self.art.look(None, self.now);
            let mut output = ctx.run_ui(input(Vec::new()), |ui| {
                let _ = show(
                    ui,
                    &self.transport,
                    &self.queue,
                    &self.lyrics,
                    self.transport.position_ms,
                    &mut Frame {
                        art: &mut self.art,
                        now: self.now,
                        cover,
                        tokens: &tokens,
                    },
                );
            });
            output.textures_delta.clear();
            drawn_texts(&output)
                .into_iter()
                .map(str::to_owned)
                .collect()
        }

        /// The frame output, so a test can find where something was painted.
        fn output(&mut self, ctx: &Context) -> egui::FullOutput {
            let tokens = Tokens::DARK;
            let cover = self.art.look(None, self.now);
            let mut output = ctx.run_ui(input(Vec::new()), |ui| {
                let _ = show(
                    ui,
                    &self.transport,
                    &self.queue,
                    &self.lyrics,
                    self.transport.position_ms,
                    &mut Frame {
                        art: &mut self.art,
                        now: self.now,
                        cover,
                        tokens: &tokens,
                    },
                );
            });
            output.textures_delta.clear();
            output
        }
    }

    /// A click at `at` over the three passes egui's hit test needs, returning the
    /// queue reply that asked for something.
    fn click(panel: &mut Panel, ctx: &Context, at: Pos2) -> queue_pane::Reply {
        // Warm up so hit-testing has a previous pass to read.
        panel.frame(ctx, Vec::new());
        let mut reply = queue_pane::Reply::default();
        for event in crate::view::test_support::click_events(at) {
            let next = panel.frame(ctx, vec![event]);
            if next.play.is_some() || next.press.is_some() || next.lift.is_some() {
                reply = next;
            }
        }
        reply
    }

    #[test]
    fn a_cover_arriving_after_play_is_ready_on_the_next_frame() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
        let mut transport = Transport::default();
        transport.play("spotify:track:x", "A Title", "A Band", Some(url.clone()));
        let before = transport.now_playing.clone();

        let mut art = Art::default();
        let now = Instant::now();
        art.begin_frame();
        assert_eq!(art.look(Some(&url), now), Cover::Loading);
        let _ = art.requests(now);
        art.apply(
            &Update::Cover {
                url: url.clone(),
                image: CoverImage::new(1, 1, vec![0; 4]).expect("a 1x1 image"),
            },
            now,
        );
        art.upload(&ctx);
        art.begin_frame();
        let cover = art.look(Some(&url), now);
        assert!(matches!(cover, Cover::Ready(_)));

        // The panel reads the cover each frame and stores no pixels, so the
        // transport is untouched by the arrival.
        assert_eq!(transport.now_playing, before);

        let queue = Queue::default();
        let mut output = ctx.run_ui(input(Vec::new()), |ui| {
            let _ = show(
                ui,
                &transport,
                &queue,
                &crate::lyrics::LyricsPanel::default(),
                transport.position_ms,
                &mut Frame {
                    art: &mut art,
                    now,
                    cover,
                    tokens: &tokens,
                },
            );
        });
        output.textures_delta.clear();
        let texts: Vec<String> = drawn_texts(&output)
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert!(
            texts.iter().any(|text| text == "A Title"),
            "the panel must name the track: {texts:?}"
        );

        // The art store's texture is what the panel draws, not a placeholder.
        let Cover::Ready(texture) = cover else {
            panic!("a ready cover");
        };
        assert!(
            output.shapes.iter().any(|clipped| match &clipped.shape {
                Shape::Rect(rect) => rect
                    .brush
                    .as_ref()
                    .is_some_and(|brush| brush.fill_texture_id == texture),
                _ => false,
            }),
            "the panel must draw the cover it was handed"
        );
    }

    #[test]
    fn the_panel_names_the_artist() {
        let ctx = context();
        let mut panel = Panel::new(playing("A Title", "A Band"));
        let texts = panel.texts(&ctx);
        assert!(
            texts.iter().any(|text| text == "A Band"),
            "the panel must name the artist: {texts:?}"
        );
    }

    #[test]
    fn no_artist_line_is_drawn_when_the_track_has_none() {
        let ctx = context();
        let mut none = Panel::new(playing("A Title", ""));
        let without = none.output(&ctx);
        let mut with = Panel::new(playing("A Title", "A Band"));
        let held = with.output(&ctx);

        // The title stays where it was; the state label moves up when there is
        // no artist line to reserve.
        let title = painted_at(&without, "A Title").expect("the title is painted");
        let state = painted_at(&without, "Loading").expect("the state is painted");
        let held_title = painted_at(&held, "A Title").expect("the title is painted");
        let held_state = painted_at(&held, "Loading").expect("the state is painted");
        assert_eq!(title, held_title);
        assert!(
            state.y < held_state.y,
            "an absent artist must not reserve its line: {state:?} vs {held_state:?}"
        );
    }

    #[test]
    fn nothing_playing_shows_its_line_the_empty_queue_and_both_tabs() {
        let ctx = context();
        let mut panel = Panel::new(Transport::default());
        let texts = panel.texts(&ctx);

        for label in [
            "Nothing playing",
            "NEXT IN QUEUE",
            queue_pane::EMPTY,
            "QUEUE",
            "LYRICS",
        ] {
            assert!(
                texts.iter().any(|text| text == label),
                "the panel must draw {label:?}: {texts:?}"
            );
        }
    }

    #[test]
    fn a_click_on_a_row_reports_its_index_and_moves_nothing() {
        let ctx = context();
        let mut panel = Panel::new(playing("t0", "a0")).with_queue(5, 0);
        let frame = panel.output(&ctx);
        let at = painted_at(&frame, "t3").expect("row t3 is painted");

        let reply = click(&mut panel, &ctx, at + Vec2::new(6.0, 6.0));
        assert_eq!(
            reply.press,
            Some((3, Press::Replace)),
            "the click reports the queue's own index, not the row's position"
        );
        assert_eq!(reply.play, None, "a single click does not play");
        assert_eq!(
            panel
                .transport
                .now_playing
                .as_ref()
                .map(|now| now.uri.as_str()),
            Some("t0"),
            "the panel only reports the click; applying the jump is the shell's"
        );
        assert_eq!(
            panel.queue.upcoming().next().map(|(index, _)| index),
            Some(1),
            "drawing must not move the queue either"
        );
    }

    #[test]
    fn a_failed_track_at_the_skip_cap_still_lists_the_rest() {
        // The failure stops the auto-advance; the rows stay, and a click is how
        // the listener gets out of it. Capped mid-list, where the claim holds.
        let ctx = context();
        let mut panel = Panel::new(Transport::default()).with_queue(30, 0);
        for _ in 0..=crate::queue::MAX_CONSECUTIVE_SKIPS {
            let update = Update::Transport(TransportUpdate::Loading);
            panel.transport.apply(&update);
            let _ = panel.queue.apply(&update, &mut panel.transport);
            let update = Update::Transport(TransportUpdate::Unavailable);
            panel.transport.apply(&update);
            let _ = panel.queue.apply(&update, &mut panel.transport);
        }
        assert!(
            matches!(
                panel.transport.state,
                crate::player::TransportState::Unavailable
            ),
            "the run must have stopped on a failure: {:?}",
            panel.transport.state
        );

        let upcoming: Vec<String> = panel
            .queue
            .upcoming()
            .map(|(_, entry)| entry.title.clone())
            .collect();
        assert!(
            !upcoming.is_empty(),
            "the rest of the queue must still follow"
        );

        let texts = panel.texts(&ctx);
        let listed = upcoming
            .iter()
            .filter(|title| texts.iter().any(|text| text == *title))
            .count();
        assert!(
            listed > 0,
            "the rows must stay listed at the cap: {texts:?}"
        );

        let frame = panel.output(&ctx);
        let at = painted_at(&frame, &upcoming[0]).expect("the first remaining row is painted");
        assert_eq!(
            click(&mut panel, &ctx, at + Vec2::new(6.0, 6.0)).press,
            Some((
                crate::queue::MAX_CONSECUTIVE_SKIPS as usize + 1,
                Press::Replace
            )),
            "a click must still reach the entry after the cap stopped the run"
        );
    }

    #[test]
    fn a_paused_queue_still_lists_and_its_rows_still_answer_a_click() {
        let ctx = context();
        let mut panel = Panel::new(playing("t0", "a0")).with_queue(4, 0);
        let paused = Update::Transport(TransportUpdate::Position {
            position_ms: 1_000,
            paused: true,
        });
        panel.transport.apply(&paused);
        let _ = panel.queue.apply(&paused, &mut panel.transport);

        let texts = panel.texts(&ctx);
        assert!(texts.iter().any(|text| text == "Paused"), "{texts:?}");
        assert!(texts.iter().any(|text| text == "t2"), "{texts:?}");

        let frame = panel.output(&ctx);
        let at = painted_at(&frame, "t2").expect("row t2 is painted");
        assert_eq!(
            click(&mut panel, &ctx, at + Vec2::new(6.0, 6.0)).press,
            Some((2, Press::Replace)),
            "a paused queue's rows must still report their index"
        );
    }

    #[test]
    fn the_lyrics_tab_is_live() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let mut panel = Panel::new(playing("t0", "a0")).with_queue(3, 0);
        let frame = panel.output(&ctx);
        let at = painted_at(&frame, "LYRICS").expect("the lyrics tab is drawn");

        // The tab is live now, so a press reaches the panel; what a press means is
        // the shell's business, and a tab is not a queue row.
        let reply = click(&mut panel, &ctx, at + Vec2::new(2.0, 2.0));
        assert_eq!(
            (reply.press, reply.play),
            (None, None),
            "a tab press is not a queue row"
        );

        // The keyboard reaches a live tab and it wears the ring.
        let ring_around = |output: &egui::FullOutput, centre: Pos2| {
            output.shapes.iter().any(|clipped| match &clipped.shape {
                Shape::Rect(rect) => {
                    rect.stroke.color == tokens.palette.accent
                        && rect.rect.expand(extent::FOCUS_OFFSET).contains(centre)
                }
                _ => false,
            })
        };
        let tab = Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let lyrics_at = at + Vec2::new(2.0, 2.0);

        let mut reached_the_panel = false;
        for _ in 0..8 {
            let cover = panel.art.look(None, panel.now);
            let mut output = ctx.run_ui(input(vec![tab.clone()]), |ui| {
                let _ = show(
                    ui,
                    &panel.transport,
                    &panel.queue,
                    &panel.lyrics,
                    panel.transport.position_ms,
                    &mut Frame {
                        art: &mut panel.art,
                        now: panel.now,
                        cover,
                        tokens: &tokens,
                    },
                );
            });
            output.textures_delta.clear();
            reached_the_panel |= ring_around(&output, lyrics_at);
        }
        assert!(
            reached_the_panel,
            "the keyboard reaches the live Lyrics tab"
        );
    }
    /// The head's more button, solved the way the panel solves it.
    fn more_at() -> Pos2 {
        let head = Rect::from_min_size(
            Pos2::new(space::S3, space::S3),
            Vec2::new(PROBE.x - 2.0 * space::S3, extent::BUTTON),
        );
        Pos2::new(head.right() - extent::BUTTON / 2.0, head.center().y)
    }

    /// A queue entry the menu has verbs for.
    fn named_track() -> TrackEntry {
        TrackEntry {
            uri: "spotify:track:a".to_owned(),
            album_uri: Some("spotify:album:a".to_owned()),
            artist_uris: vec!["spotify:artist:b".to_owned()],
            ..track(0)
        }
    }

    /// Clicks the more button and returns the reply that opened a door.
    fn click_more(panel: &mut Panel, ctx: &Context) -> Reply {
        let _ = panel.reply(ctx, Vec::new());
        let at = more_at();
        let mut found = Reply::default();
        for event in crate::view::test_support::click_events(at) {
            let next = panel.reply(ctx, vec![event]);
            if next.menu.is_some() {
                found = next;
            }
        }
        found
    }

    #[test]
    fn the_more_button_opens_a_current_door() {
        let ctx = context();
        let mut panel = Panel::new(playing("A Title", "A Band"));
        let named = named_track();
        panel.queue.start(&[named], 0, &mut panel.transport);

        let reply = click_more(&mut panel, &ctx);

        let door = reply.menu.expect("the more button opened no door");
        assert_eq!(door.device, crate::menu::Device::Pointer);
        assert!(
            matches!(door.anchor, crate::menu::Anchor::Button(_)),
            "the more's menu did not anchor on the button"
        );
    }

    #[test]
    fn the_more_button_waits_without_a_current_entry_that_has_verbs() {
        for queue in [Queue::default(), {
            let mut queue = Queue::default();
            queue.start(&[track(0)], 0, &mut Transport::default());
            queue
        }] {
            let ctx = context();
            let mut panel = Panel::new(playing("A Title", "A Band"));
            panel.queue = queue;

            assert!(
                click_more(&mut panel, &ctx).menu.is_none(),
                "a waiting more button opened a menu"
            );
        }
    }
}
