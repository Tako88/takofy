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
use takofy_model::TrackEntry;

use crate::availability::Control;
use crate::cover::{Art, want_px};
use crate::icon::Icon;
use crate::layout::panel::{self, Stack};
use crate::likes::Likes;
use crate::player::Transport;
use crate::queue::Queue;
use crate::theme::icon::Size;
use crate::theme::{Tokens, extent, radius, space, text, tracking};

use crate::lyrics::{LyricsPanel, Tab};
use crate::menu::{Anchor, Device, Door, Listing};
use crate::route::Page;
use crate::verbs;

use super::art;
use super::context_menu;
use super::controls;
use super::link::{self, Face, Target};
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
    /// The page a click on the title or the artist asked for.
    pub open: Option<Page>,
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
    /// The entry the pane draws, handed in by the shell: the one the head's
    /// more button acts on, or nothing when the drawn track is no entry here.
    pub current: Option<&'a TrackEntry>,
    pub tokens: &'a Tokens,
}

/// Draws the panel's contents inside its own rect.
pub fn show(
    ui: &mut Ui,
    transport: &Transport,
    queue: &Queue,
    lyrics: &LyricsPanel,
    likes: &Likes,
    position_ms: u32,
    frame: &mut Frame<'_>,
) -> Reply {
    let tokens = frame.tokens;
    let band = ui.available_rect_before_wrap();
    let meta_h = meta_height(ui);
    let tabs_h = tabs_height(ui, tokens);
    let stack = panel::stack_for(band, meta_h, tabs_h);
    // The panel asks for the size its own art box takes. While the playbar is
    // also on screen one track is fetched twice, which the cache's byte budget
    // pays for.
    let cover = frame.art.look(
        transport
            .now_playing
            .as_ref()
            .and_then(|now| now.cover.as_ref()),
        want_px(stack.art.width(), ui.pixels_per_point()),
        frame.now,
    );

    let menu = head(ui, stack, frame.current, likes, tokens);
    // Idle is the transport, never the cover: a track with no cover of its own
    // still has a box to frame.
    if transport.now_playing.is_none() {
        art::hole(ui.painter(), stack.art, radius::SM, tokens);
    } else {
        art::tile(
            ui.painter(),
            stack.art,
            radius::SM,
            tokens.palette.border_strong,
            cover,
            tokens,
        );
    }
    let open = meta(ui, stack.meta, transport, tokens);
    let tab = tabs(ui, stack.tabs, tokens, lyrics.showing());

    let showing = tab.unwrap_or(lyrics.showing());
    let retry = match showing {
        Tab::Queue => {
            return Reply {
                queue: queue_pane::show(ui, stack.pane, queue, frame.art, frame.now, tokens),
                open,
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
        open,
        tab,
        retry,
        menu,
    }
}

/// The head: the panel's caption and the drawn entry's more button.
///
/// `current` is the entry the pane draws, handed in by the shell: this window's
/// current row, or the holder's track while the queue holds it. The button is
/// live only when that entry has something to do — a holder whose track is no
/// entry here leaves it waiting — and the menu it opens acts on that entry, so
/// it can never reach a track the pane is not drawing.
fn head(
    ui: &mut Ui,
    stack: Stack,
    current: Option<&TrackEntry>,
    likes: &Likes,
    tokens: &Tokens,
) -> Option<Door> {
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
    let live = current.is_some_and(|entry| {
        !verbs::track_items(
            std::slice::from_ref(entry),
            None,
            &Listing::Current,
            likes.standing(&[entry.uri.as_str()]),
        )
        .is_empty()
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
///
/// The idle line sits where a title would, at the top of the reserved rect: the
/// same block with nothing in it. The title opens the album and the artist their
/// page, when the track names one; the block's heights do not change with them.
fn meta(ui: &Ui, rect: Rect, transport: &Transport, tokens: &Tokens) -> Option<Page> {
    if rect.width() <= 0.0 {
        return None;
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
            Pos2::new(rect.left(), rect.top()),
            line,
            tokens.palette.text_muted,
        );
        return None;
    };

    let title = link::Placed {
        pos: Pos2::new(rect.left(), rect.top()),
        galley: link::lay(
            ui,
            &now.title,
            &Face {
                font: recipe::panel_title(),
                tracking: recipe::tracking_pts(text::MD, tracking::TIGHT),
            },
            rect.width(),
        ),
    };
    let height = title.rect().height();
    let title = title.link(
        ui,
        ui.scope_id().with("now-playing-title"),
        rect,
        now.album_uri.as_deref().map(Target::Album),
        tokens.palette.text,
        tokens,
    );
    let mut top = rect.top() + height;
    let mut open = title.page;

    if !now.artists.is_empty() {
        let artist = link::Placed {
            pos: Pos2::new(rect.left(), top),
            galley: link::lay(
                ui,
                &now.artists,
                &Face {
                    font: recipe::track_secondary(),
                    tracking: 0.0,
                },
                rect.width(),
            ),
        };
        let shown = artist.link(
            ui,
            ui.scope_id().with("now-playing-artist"),
            rect,
            now.artist_uri.as_deref().map(Target::Credit),
            tokens.palette.text_muted,
            tokens,
        );
        open = open.or(shown.page);
    }
    // The artist line is reserved whether or not it holds an artist.
    top += line_height(ui, &recipe::track_secondary());

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
    open
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

/// The reservation, from the pane's own line heights: every state names all three
/// lines, so the tabs and the art box are a function of the pane alone.
fn meta_height(ui: &Ui) -> f32 {
    panel::meta_reserved(
        line_height(ui, &recipe::panel_title()),
        line_height(ui, &recipe::track_secondary()),
        line_height(ui, &recipe::track_mono(text::XXS)),
    )
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
    use takofy_model::{CoverImage, CoverSet, CoverUrl, TrackEntry};

    use super::*;
    use crate::cover::{Art, Cover, want_px};
    use crate::message::{TransportUpdate, Update};
    use crate::player::NowPlaying;
    use crate::route::Page;
    use crate::selection::Press;
    use crate::theme::{fonts, style};
    use crate::view::test_support::tab;

    const PROBE: Vec2 = Vec2::new(360.0, 700.0);

    fn context() -> Context {
        let tokens = Tokens::DARK;
        let ctx = Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&tokens));
        ctx
    }

    fn input(events: Vec<Event>) -> egui::RawInput {
        input_in(PROBE, events)
    }

    fn input_in(size: Vec2, events: Vec<Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
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

    /// Whether two rects are the same box, allowing for the pixel grid.
    fn same_box(rect: Rect, other: Rect) -> bool {
        (rect.min - other.min).length() < 0.01 && (rect.max - other.max).length() < 0.01
    }

    /// The rects the frame painted inside `area`.
    fn rects_in(output: &egui::FullOutput, area: Rect) -> Vec<egui::epaint::RectShape> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Rect(rect) if rect.rect.intersects(area) => Some(rect.clone()),
                _ => None,
            })
            .collect()
    }

    fn track(index: usize) -> TrackEntry {
        TrackEntry {
            uri: format!("t{index}"),
            title: format!("t{index}"),
            artists: format!("a{index}"),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    /// A transport on a named track, ready to draw.
    fn playing(title: &str, artists: &str) -> Transport {
        let mut transport = Transport::default();
        transport.play(NowPlaying::named("spotify:track:x", title, artists));
        transport
    }

    const ALBUM: &str = "spotify:album:a";
    const ARTIST: &str = "spotify:artist:b";

    /// A transport whose track names the album and the credit it links to.
    fn linked(title: &str, artists: &str) -> Transport {
        let mut transport = Transport::default();
        transport.play(NowPlaying {
            album_uri: Some(ALBUM.to_owned()),
            artist_uri: Some(ARTIST.to_owned()),
            ..NowPlaying::named("spotify:track:x", title, artists)
        });
        transport
    }

    /// The panel's own frame, with a queue and an art store behind it.
    struct Panel {
        transport: Transport,
        queue: Queue,
        art: Art,
        now: Instant,
        lyrics: crate::lyrics::LyricsPanel,
        likes: crate::likes::Likes,
        /// The entry the pane is handed, when a test wants it to differ from
        /// the queue's current one.
        drawn: Option<TrackEntry>,
    }

    impl Panel {
        fn new(transport: Transport) -> Self {
            Self {
                transport,
                queue: Queue::default(),
                art: Art::default(),
                now: Instant::now(),
                lyrics: crate::lyrics::LyricsPanel::default(),
                likes: crate::likes::Likes::default(),
                drawn: None,
            }
        }

        fn with_queue(mut self, count: usize, current: usize) -> Self {
            let tracks: Vec<TrackEntry> = (0..count).map(track).collect();
            self.queue
                .start(&tracks, current, None, &mut self.transport);
            self
        }

        /// Draws one frame and returns the queue reply it reported.
        fn frame(&mut self, ctx: &Context, events: Vec<Event>) -> queue_pane::Reply {
            self.reply(ctx, events).queue
        }

        /// Draws one frame and returns the whole reply it reported.
        fn reply(&mut self, ctx: &Context, events: Vec<Event>) -> Reply {
            let tokens = Tokens::DARK;
            self.art.begin_frame();
            let now = self.now;
            let mut reply = Reply::default();
            let Self {
                transport,
                queue,
                art,
                lyrics,
                likes,
                drawn,
                ..
            } = self;
            let position_ms = transport.position_ms;
            let current = drawn.as_ref().or_else(|| queue.current());
            run(ctx, input(events), |ui| {
                reply = show(
                    ui,
                    transport,
                    queue,
                    lyrics,
                    likes,
                    position_ms,
                    &mut Frame {
                        art,
                        now,
                        current,
                        tokens: &tokens,
                    },
                );
            });
            reply
        }

        /// Draws one frame and returns the text it painted.
        fn texts(&mut self, ctx: &Context) -> Vec<String> {
            let tokens = Tokens::DARK;
            let mut output = ctx.run_ui(input(Vec::new()), |ui| {
                let _ = show(
                    ui,
                    &self.transport,
                    &self.queue,
                    &self.lyrics,
                    &self.likes,
                    self.transport.position_ms,
                    &mut Frame {
                        art: &mut self.art,
                        now: self.now,
                        current: self.queue.current(),
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
            let mut output = ctx.run_ui(input(Vec::new()), |ui| {
                let _ = show(
                    ui,
                    &self.transport,
                    &self.queue,
                    &self.lyrics,
                    &self.likes,
                    self.transport.position_ms,
                    &mut Frame {
                        art: &mut self.art,
                        now: self.now,
                        current: self.queue.current(),
                        tokens: &tokens,
                    },
                );
            });
            output.textures_delta.clear();
            output
        }
    }

    /// The stack the panel solves for a `size` window, the cover it drew and the
    /// frame it painted.
    fn solve(panel: &mut Panel, ctx: &Context, size: Vec2) -> (Stack, Cover, egui::FullOutput) {
        let tokens = Tokens::DARK;
        panel.art.begin_frame();
        let now = panel.now;
        let mut stack = None;
        let mut output = ctx.run_ui(input_in(size, Vec::new()), |ui| {
            let band = ui.available_rect_before_wrap();
            stack = Some(panel::stack_for(
                band,
                meta_height(ui),
                tabs_height(ui, &tokens),
            ));
            let _ = show(
                ui,
                &panel.transport,
                &panel.queue,
                &panel.lyrics,
                &panel.likes,
                panel.transport.position_ms,
                &mut Frame {
                    art: &mut panel.art,
                    now,
                    current: panel.queue.current(),
                    tokens: &tokens,
                },
            );
        });
        output.textures_delta.clear();
        let stack = stack.expect("the panel solved its stack while drawing");
        let cover = panel.art.look(
            panel
                .transport
                .now_playing
                .as_ref()
                .and_then(|now| now.cover.as_ref()),
            want_px(stack.art.width(), ctx.pixels_per_point()),
            panel.now,
        );
        (stack, cover, output)
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

    /// Clicks `at` over the passes egui's hit test needs, returning the page the
    /// release asked for.
    fn click_open(panel: &mut Panel, ctx: &Context, at: Pos2) -> Option<Page> {
        let _ = panel.reply(ctx, Vec::new());
        let mut found = None;
        for event in crate::view::test_support::click_events(at) {
            if let Some(page) = panel.reply(ctx, vec![event]).open {
                found = Some(page);
            }
        }
        found
    }

    #[test]
    fn the_title_is_a_link_to_the_album() {
        let ctx = context();
        let mut panel = Panel::new(linked("A Title", "A Band"));
        panel.lyrics.select(Tab::Lyrics);
        let frame = panel.output(&ctx);
        let at = painted_at(&frame, "A Title").expect("the title is painted");
        assert_eq!(
            click_open(&mut panel, &ctx, at + Vec2::new(4.0, 4.0)),
            Some(Page::Album(ALBUM.to_owned())),
            "a click on the title must open the album"
        );
    }

    #[test]
    fn the_artist_is_a_link_to_the_artist() {
        let ctx = context();
        let mut panel = Panel::new(linked("A Title", "A Band"));
        panel.lyrics.select(Tab::Lyrics);
        let frame = panel.output(&ctx);
        let at = painted_at(&frame, "A Band").expect("the artist is painted");
        assert_eq!(
            click_open(&mut panel, &ctx, at + Vec2::new(4.0, 4.0)),
            Some(Page::Artist(ARTIST.to_owned())),
            "a click on the artist must open the artist"
        );
    }

    #[test]
    fn no_uris_means_no_links() {
        let ctx = context();
        ctx.enable_accesskit();
        let mut panel = Panel::new(playing("A Title", "A Band"));
        panel.lyrics.select(Tab::Lyrics);
        let frame = panel.output(&ctx);
        let labels: Vec<String> = frame
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
            .unwrap_or_default();
        assert!(
            labels.is_empty(),
            "a track with no uris drew link nodes: {labels:?}"
        );
        let at = painted_at(&frame, "A Title").expect("the title is painted");
        assert_eq!(
            click_open(&mut panel, &ctx, at + Vec2::new(4.0, 4.0)),
            None,
            "a title with no album opened a page"
        );
    }

    #[test]
    fn the_queue_tab_early_return_still_carries_open() {
        let ctx = context();
        let mut panel = Panel::new(linked("A Title", "A Band"));
        let frame = panel.output(&ctx);
        assert_eq!(
            panel.lyrics.showing(),
            Tab::Queue,
            "the early return is the queue tab's"
        );
        assert!(
            drawn_texts(&frame).contains(&queue_pane::EMPTY),
            "the queue pane must be the one drawn"
        );
        let at = painted_at(&frame, "A Title").expect("the title is painted");
        assert_eq!(
            click_open(&mut panel, &ctx, at + Vec2::new(4.0, 4.0)),
            Some(Page::Album(ALBUM.to_owned())),
            "the queue tab's reply dropped the open"
        );
    }

    #[test]
    fn a_cover_arriving_after_play_is_ready_on_the_next_frame() {
        let tokens = Tokens::DARK;
        let ctx = context();
        let url = CoverUrl::from_wire("https://i.scdn.co/image/abc").expect("a cover url");
        let mut transport = Transport::default();
        transport.play(NowPlaying {
            cover: Some(CoverSet::single(url.clone())),
            ..NowPlaying::named("spotify:track:x", "A Title", "A Band")
        });
        let before = transport.now_playing.clone();

        let mut art = Art::default();
        let now = Instant::now();
        art.begin_frame();
        assert_eq!(
            art.look(Some(&CoverSet::single(url.clone())), 256, now),
            Cover::Loading
        );
        let _ = art.requests(now);
        art.apply(
            &Update::Cover {
                url: url.clone(),
                image: CoverImage::new(1, 1, vec![0; 4]).expect("a 1x1 image"),
            },
            now,
        );
        art.upload(&ctx, crate::theme::TintParams::DARK.alpha_floor);
        art.begin_frame();
        let cover = art.look(Some(&CoverSet::single(url.clone())), 256, now);
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
                &crate::likes::Likes::default(),
                transport.position_ms,
                &mut Frame {
                    art: &mut art,
                    now,
                    current: queue.current(),
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
    fn the_state_line_stays_put_when_the_track_has_no_artist() {
        let ctx = context();
        let mut none = Panel::new(playing("A Title", ""));
        let without = none.output(&ctx);
        let mut with = Panel::new(playing("A Title", "A Band"));
        let held = with.output(&ctx);

        // The block holds the artist line whether or not there is an artist, so
        // the state under it does not move.
        let title = painted_at(&without, "A Title").expect("the title is painted");
        let state = painted_at(&without, "Loading").expect("the state is painted");
        let held_title = painted_at(&held, "A Title").expect("the title is painted");
        let held_state = painted_at(&held, "Loading").expect("the state is painted");
        assert_eq!(title, held_title);
        assert!(
            (state.y - held_state.y).abs() < 0.01,
            "an absent artist must reserve its line: {state:?} vs {held_state:?}"
        );

        // The reservation is the pane's own sum of three lines.
        let mut measured = None;
        run(&ctx, input(Vec::new()), |ui| {
            measured = Some((
                meta_height(ui),
                panel::meta_reserved(
                    line_height(ui, &recipe::panel_title()),
                    line_height(ui, &recipe::track_secondary()),
                    line_height(ui, &recipe::track_mono(text::XXS)),
                ),
            ));
        });
        let (reserved, summed) = measured.expect("the pane measures its reservation");
        assert!(
            (reserved - summed).abs() < 0.01,
            "the reservation is three lines: {reserved} vs {summed}"
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
    fn an_idle_pane_draws_its_art_box_as_a_hole_with_no_hairline() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut panel = Panel::new(Transport::default());
        let (stack, _, output) = solve(&mut panel, &ctx, PROBE);

        let painted = rects_in(&output, stack.art);
        let hole = painted.iter().find(|rect| {
            rect.fill == tokens.palette.surface_sunken && same_box(rect.rect, stack.art)
        });
        let hole =
            hole.unwrap_or_else(|| panic!("the idle art box is not the sunken well: {painted:?}"));
        assert!(hole.stroke.is_empty(), "the idle art box draws no hairline");

        // Scoped to the art box: the Queue and Lyrics chips keep their strokes,
        // so this is not a claim that the pane draws no edges at all.
        for rect in painted {
            assert!(
                rect.stroke.is_empty(),
                "a hairline crosses the idle art box: {:?}",
                rect.rect
            );
        }
        let chips = rects_in(&output, stack.tabs);
        assert!(
            chips.iter().any(|rect| !rect.stroke.is_empty()),
            "the tab chips keep their strokes: {chips:?}"
        );
    }

    #[test]
    fn a_named_track_with_no_cover_keeps_its_tile_and_hairline() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let mut panel = Panel::new(playing("T", "A"));
        let (stack, _, output) = solve(&mut panel, &ctx, PROBE);

        let edged = rects_in(&output, stack.art)
            .into_iter()
            .find(|rect| !rect.stroke.is_empty() && same_box(rect.rect, stack.art))
            .expect("a named track's art box keeps its hairline");
        assert_eq!(edged.stroke.color, tokens.palette.border_strong);
        assert_eq!(
            edged.fill, tokens.palette.surface_sunken,
            "no cover is still the sunken well, inside its frame"
        );
    }

    #[test]
    fn a_named_track_with_a_cover_paints_what_art_tile_paints() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let url = CoverUrl::from_wire("https://i.scdn.co/image/tile").expect("a cover url");
        let mut transport = Transport::default();
        transport.play(NowPlaying {
            cover: Some(CoverSet::single(url.clone())),
            ..NowPlaying::named("spotify:track:x", "T", "A")
        });
        let mut panel = Panel::new(transport);
        // One frame to want the cover and send the fetch, then the answer
        // arrives, is uploaded, and the next frame draws it.
        panel.art.begin_frame();
        assert_eq!(
            panel
                .art
                .look(Some(&CoverSet::single(url.clone())), 256, panel.now),
            Cover::Loading
        );
        assert!(
            !panel.art.requests(panel.now).is_empty(),
            "the cover is fetched"
        );
        let image = CoverImage::new(1, 1, vec![0; 4]).expect("a 1x1 image");
        panel.art.apply(&Update::Cover { url, image }, panel.now);
        panel
            .art
            .upload(&ctx, crate::theme::TintParams::DARK.alpha_floor);
        panel.art.begin_frame();

        let (stack, cover, output) = solve(&mut panel, &ctx, PROBE);
        assert!(matches!(cover, Cover::Ready(_)), "the cover is held");
        let ours = rects_in(&output, stack.art);
        assert!(
            ours.iter().any(|rect| rect.brush.is_some()),
            "the held cover is drawn, not a slot: {ours:?}"
        );

        let mut tile = ctx.run_ui(input(Vec::new()), |ui| {
            art::tile(
                ui.painter(),
                stack.art,
                radius::SM,
                tokens.palette.border_strong,
                cover,
                &tokens,
            );
        });
        tile.textures_delta.clear();
        assert_eq!(
            ours,
            rects_in(&tile, stack.art),
            "the pane draws the tile's own box"
        );
    }

    #[test]
    fn the_tabs_and_art_do_not_move_when_playback_starts() {
        let ctx = context();
        let labels = ["QUEUE", "LYRICS"];
        let mut idle = Panel::new(Transport::default());
        let (idle_stack, _, idle_frame) = solve(&mut idle, &ctx, PROBE);
        let idle_tabs = labels.map(|label| {
            painted_at(&idle_frame, label).unwrap_or_else(|| panic!("the idle pane draws {label}"))
        });

        for (title, artists) in [("T", "A"), ("T", "")] {
            let mut panel = Panel::new(playing(title, artists));
            let (stack, _, frame) = solve(&mut panel, &ctx, PROBE);
            assert!(
                same_box(stack.art, idle_stack.art),
                "the art box moved when {title:?} with {artists:?} started: {:?} was {:?}",
                stack.art,
                idle_stack.art
            );
            for (label, at) in labels.iter().zip(idle_tabs) {
                assert_eq!(
                    painted_at(&frame, label),
                    Some(at),
                    "{label} moved when {title:?} with {artists:?} started"
                );
            }
        }
    }

    #[test]
    fn the_art_box_is_the_same_size_idle_and_playing_in_a_short_pane() {
        // A window short enough that the reservation decides how much room the
        // art gets, so a track that reserved fewer lines would show it here.
        let short = Vec2::new(360.0, 360.0);
        let ctx = context();
        let mut idle = Panel::new(Transport::default());
        let (idle_stack, _, _) = solve(&mut idle, &ctx, short);
        assert!(
            idle_stack.art.width() < extent::NOWPLAYING_ART_MAX,
            "the pane must be short enough to squeeze the art: {}",
            idle_stack.art.width()
        );

        for (title, artists) in [("T", "A"), ("T", "")] {
            let mut panel = Panel::new(playing(title, artists));
            let (stack, _, _) = solve(&mut panel, &ctx, short);
            assert!(
                (stack.art.width() - idle_stack.art.width()).abs() < 0.01,
                "{title:?} with {artists:?} gives the art {} against the idle {}",
                stack.art.width(),
                idle_stack.art.width()
            );
        }
    }

    #[test]
    fn the_idle_state_is_the_default_transport() {
        let ctx = context();
        let tokens = Tokens::DARK;
        let transport = Transport::default();
        assert!(transport.now_playing.is_none(), "a fresh transport is idle");

        let mut panel = Panel::new(transport);
        let (stack, _, output) = solve(&mut panel, &ctx, PROBE);
        assert!(
            rects_in(&output, stack.art)
                .iter()
                .any(|rect| rect.fill == tokens.palette.surface_sunken
                    && rect.stroke.is_empty()
                    && same_box(rect.rect, stack.art)),
            "the default transport takes the hole path"
        );
    }

    #[test]
    fn the_idle_line_sits_at_the_top_of_the_reserved_meta() {
        let ctx = context();
        let mut panel = Panel::new(Transport::default());
        let (stack, _, output) = solve(&mut panel, &ctx, PROBE);
        let at = painted_at(&output, "Nothing playing").expect("the idle line is painted");
        assert!(
            (at.y - stack.meta.top()).abs() < 0.01,
            "the idle line sits at {} in a meta block starting at {}",
            at.y,
            stack.meta.top()
        );
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
        let [tab, _] = tab();
        let lyrics_at = at + Vec2::new(2.0, 2.0);

        let mut reached_the_panel = false;
        for _ in 0..8 {
            let mut output = ctx.run_ui(input(vec![tab.clone()]), |ui| {
                let _ = show(
                    ui,
                    &panel.transport,
                    &panel.queue,
                    &panel.lyrics,
                    &panel.likes,
                    panel.transport.position_ms,
                    &mut Frame {
                        art: &mut panel.art,
                        now: panel.now,
                        current: panel.queue.current(),
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
            artist_names: vec!["b".to_owned()],
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
        panel.queue.start(&[named], 0, None, &mut panel.transport);

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
            queue.start(&[track(0)], 0, None, &mut Transport::default());
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

    #[test]
    fn the_more_button_acts_on_the_entry_the_pane_was_handed() {
        let ctx = context();
        // The queue holds a row with nothing to open, and the pane is handed a
        // different entry: the button follows the entry it draws.
        let mut panel = Panel::new(playing("A Title", "A Band"));
        panel
            .queue
            .start(&[track(0)], 0, None, &mut Transport::default());
        panel.drawn = Some(named_track());

        let door = click_more(&mut panel, &ctx)
            .menu
            .expect("the handed entry's menu did not open");
        assert_eq!(door.device, crate::menu::Device::Pointer);
    }
}
