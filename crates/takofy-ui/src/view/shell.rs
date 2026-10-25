//! The signed-in shell: the panel grid and the regions it holds.
//!
//! `egui::Panel` is deliberately not used. Its frame margins do not collapse, so
//! a gap between two panels would be the sum of two margins, and its separator
//! line reserves stroke width inside the size that was asked for. The grid is
//! solved as rects instead, which is what the reference's CSS grid does.

use egui::{
    Align, Color32, DragAndDrop, Layout, Order, PointerButton, Pos2, Rect, Stroke, StrokeKind, Ui,
    UiBuilder, Vec2,
};

use takofy_model::TrackEntry;

use crate::cover::want_px;
use crate::devices::Source as DeviceSource;
use crate::drawer::{Drawer, Input};
use crate::landing::{self, Area, Landing, Target};
use crate::layout::{self, Grid, NowPlaying, Playbar, Sidebar};
use crate::library::Library;
use crate::menu::{Cut, Gate, LibraryItem, Listing, Verb};
use crate::message::Command;
use crate::player::Transport;
use crate::queue::{Queue, Step};
use crate::reorder;
use crate::route::Page;
use crate::search::Act;
use crate::theme::{Tokens, radius, stroke};
use crate::verbs;

use super::Screen;
use super::carry::{self, Carry, ColumnOffer, Offer, Source};
use super::{
    context_menu, device_list, ghost, heart, now_playing, playbar, playlist, queue_pane,
    search_menu, sidebar, topbar,
};

/// The signed-in shell: three columns over a playbar, each panel a hairline box
/// on the window background.
pub fn show(ui: &mut Ui, screen: &mut Screen<'_>, tokens: &Tokens) -> Option<Command> {
    // Before any key is taken out of the input: whether focus is visible is this
    // frame's. `Window::frame` calls here without `view::show`, so both do it.
    super::focus::begin_frame(ui.ctx());
    // Before any region draws: see `take_space`.
    let space = take_space(ui);
    // A pinned row's height is this frame's fact, told by the page that pins it.
    // Forgetting it here is what keeps a page with no pinned row from inheriting
    // one, and its rows where egui puts them.
    super::pin::clear(ui.ctx());

    // A page change forgets every list's selection, so a row picked before the
    // nav cannot be picked again on a page that never drew it.
    carry::follow_page(ui.ctx(), screen.library.page());
    // A refocus set by the frame before this one has had its rows drawn by now;
    // one set during this frame has not, and waits for the next.
    let pending = screen.queue.refocus();
    let content = ui.ctx().content_rect();
    // Beside the window's size, because a fullscreen change the window manager
    // made is read here on the next frame; nothing caches a request.
    let fullscreen = ui.input(|input| input.viewport().fullscreen);
    let regions = layout::regions_with(content.width(), *screen.pane);
    // A row's rename field is drawn by the column. The rail draws the same
    // sidebar only in the drawer, so a closed drawer leaves the field with no
    // row to sit in and no row to hand the keyboard back to.
    if regions.sidebar != Sidebar::Full {
        screen.library.dismiss_row_rename();
    }
    let shell = layout::shell_for(&regions);
    let grid = layout::grid_for(content, &shell);

    // Chrome first, so each region draws on top of its own panel. The playbar
    // has none: it sits straight on the window.
    panel(ui, grid.sidebar, tokens.palette.surface, tokens);
    panel(ui, main_panel(&grid), tokens.palette.surface, tokens);
    if let Some(now_playing) = grid.now_playing {
        panel(ui, now_playing, tokens.palette.panel, tokens);
    }

    let inset = stroke::HAIRLINE;
    let mut command = match regions.sidebar {
        Sidebar::Full => region(
            ui,
            "sidebar",
            grid.sidebar,
            grid.sidebar.shrink(inset),
            |ui| sidebar::column(ui, screen.library, screen.art, screen.now, tokens),
        ),
        // Below 900 the sidebar is not a column: the layout gives it no width
        // and the drawer toggle puts the same sidebar over the main pane.
        Sidebar::Rail => None,
    };

    // The topbar reports a nav as a request, because the crumbs borrow the
    // library they were built from. It is applied here, before the view, and
    // only if no region has asked for a command yet: a nav can move the page and
    // start a load, and one whose command was then dropped would leave that load
    // marked in flight with nothing fetching it. Side buttons and browser keys
    // ride the same request; a click wins.
    let bar = topbar::show(ui, screen.library, regions.sidebar, grid.topbar, tokens);
    command = apply_nav(
        screen.library,
        bar.nav.or_else(|| nav_from_input(ui)),
        command,
    );

    // Hazard, deferred: if an earlier region already reported a command, this
    // `or` drops whatever the view asked for — including a `LoadAlbum` the
    // library has already marked `Loading`. `settle` sees the want and the
    // in-flight load and refuses to re-issue it, so the album page would spin
    // for a reply nobody asked for. The playlist's `select` has had the same
    // hazard all along; the album page has just newly reached it. Fixing it means
    // restructuring this merge, which is not this slice's work.
    command = command.or(region(
        ui,
        "view",
        main_panel(&grid),
        grid.view.shrink(inset),
        |ui| {
            playlist::tracks_view(
                ui,
                regions.table,
                screen.library,
                screen.transport,
                screen.queue,
                screen.art,
                screen.now,
                screen.now_ms,
                tokens,
            )
        },
    ));

    // The view region is the only one that draws an action row. A panel below
    // has rows of its own to reveal, and those rows are not under the page's
    // bar: they must land where egui puts them, so the seam is forgotten for
    // them. Only a frame that draws the panel forgets it here: a frame without
    // one leaves the page's own inset for the next frame's early clear.
    let panel_drawn = grid.now_playing.is_some() && regions.now_playing != NowPlaying::Hidden;
    if panel_drawn {
        super::pin::clear(ui.ctx());
    }

    // One source per frame: the panel, the bar and the cover all draw the
    // device that holds the sound, or this window's own player.
    let source = screen.devices.source(screen.transport);
    // The holder's transport is display only, so it carries this window's own
    // level: volume and mute are inert while another device has the sound.
    screen.devices.display().volume = screen.transport.volume;
    // A preview belongs to the holder's bar, under the pointer. A local play
    // takes that bar away, and the release that would end the drag is then
    // never the holder's to see, so the preview goes with the bar.
    if source == DeviceSource::Local {
        let display = screen.devices.display();
        display.scrub = None;
        display.dragging = false;
    }
    let position_ms = match source {
        DeviceSource::Local => screen.position_ms,
        DeviceSource::Remote => screen.devices.shown_position(screen.now),
    };
    let caption = screen.devices.caption(screen.now);
    let steerable = screen.devices.steerable();

    // The panel and the playbar share one tile: the track the source names,
    // looked up each frame so a cover that arrives after `play` still reaches
    // them.
    let playing = match source {
        DeviceSource::Local => screen.transport.now_playing.as_ref(),
        DeviceSource::Remote => screen.devices.display().now_playing.as_ref(),
    };
    let now_playing_cover = screen.art.look(
        playing.and_then(|now| now.cover.as_ref()),
        want_px(
            crate::theme::extent::NOWPLAYING_ART_MAX,
            ui.pixels_per_point(),
        ),
        screen.now,
    );
    let like = playing.and_then(|now| screen.library.likes.state(&now.uri));

    // The panel's lyrics follow the transport: a different track drops the load
    // and keeps the tab the reader chose.
    if panel_drawn {
        screen.library.lyrics.sync(playing);
    }

    // The entry the pane draws, so its More button acts on that track: this
    // window's current row, or the holder's track while this queue holds it.
    let current = match source {
        DeviceSource::Local => screen.queue.current(),
        DeviceSource::Remote => {
            let uri = screen
                .devices
                .display()
                .now_playing
                .as_ref()
                .map(|now| now.uri.clone());
            uri.as_deref()
                .and_then(|uri| queue_entry(screen.queue, uri))
        }
    };

    let mut reply = now_playing::Reply::default();
    if let Some(panel) = grid.now_playing
        && panel_drawn
    {
        let display: &mut Transport = match source {
            DeviceSource::Local => screen.transport,
            DeviceSource::Remote => screen.devices.display(),
        };
        reply = region(ui, "now-playing", panel, panel.shrink(inset), |ui| {
            now_playing::show(
                ui,
                display,
                screen.queue,
                &screen.library.lyrics,
                &screen.library.likes,
                position_ms,
                &mut now_playing::Frame {
                    art: screen.art,
                    now: screen.now,
                    cover: now_playing_cover,
                    current,
                    tokens,
                },
            )
        });
    }
    if let Some(tab) = reply.tab {
        screen.library.lyrics.select(tab);
    }
    if reply.retry {
        screen.library.lyrics.retry();
    }
    if let Some(door) = reply.menu {
        let gate = door_gate(ui.ctx(), screen, &Listing::Current);
        carry::current_menu(
            current,
            ui.ctx(),
            door,
            gate,
            &screen.library.likes,
            screen.library.page(),
        );
    }

    // The pane reports what the pointer and the keyboard asked of the queue; it
    // never moves the queue itself, because its row loop is still reading it.
    let mut queue_reply = std::mem::take(&mut reply.queue);
    // Every page a link click named is collected here and applied once, below
    // and after the queue's own jump; a region that draws later adds its own open
    // before that block.
    let open = take_open(&mut reply.open, &mut queue_reply.open);
    apply_queue_reply(ui, screen, &mut queue_reply);

    // Resolved once, before the playbar draws: the queue button fills from the
    // same answer `settle_carry` applies, and every lift this frame has happened
    // (a page list sets its payload in `List::finish`, inside the view region).
    let bar_rect = grid.playbar.shrink(inset);
    let carry = carry::carried(ui.ctx());
    // Read every frame, carried or not, so a surface that did not draw offers
    // nothing.
    let offer = carry::take_offer(ui.ctx());
    let column = carry::take_column_offer(ui.ctx());
    let pointer = ui.ctx().pointer_latest_pos();
    let target = carry.as_deref().and_then(|carry| match carry.source {
        // The column is a write of its own: a playlist never reaches the queue.
        Source::Playlist => rootlist_target(carry, column.as_ref(), screen.library, pointer),
        Source::List(_) | Source::Queue => {
            // A row lifted from a page list lands in that list's own rows, so
            // that page's zone wins there; the sidebar's rows take an add; and
            // everywhere else the queue's zones decide.
            reorder_target(carry, offer.as_ref(), pointer)
                .or_else(|| add_target(carry, column.as_ref(), screen.library, pointer))
                .or_else(|| {
                    let area = queue_area(regions.now_playing, regions.playbar, bar_rect, carry);
                    landing::target(pointer, queue_reply.zone.as_ref(), area.as_ref())
                })
        }
    });

    let bar_state = playbar::Bar {
        queue_claimed: matches!(target, Some(Target::End)),
        like,
        pane: layout::pane_button(content.width(), *screen.pane),
        fullscreen,
        devices: screen.devices.button(),
    };
    // Another device's line replaces the track's artist, and only a holder that
    // lets remote control draws its transport live.
    let remote = (source == DeviceSource::Remote).then_some(playbar::Remote {
        line: &caption,
        steerable,
    });
    let mut playbar_reply = region(ui, "playbar", grid.playbar, bar_rect, |ui| {
        let display: &mut Transport = match source {
            DeviceSource::Local => screen.transport,
            DeviceSource::Remote => screen.devices.display(),
        };
        playbar::show(
            ui,
            display,
            screen.queue,
            position_ms,
            regions.playbar,
            bar_state,
            remote,
            bar_rect,
            now_playing_cover,
            tokens,
        )
    });
    command = command.or(playbar_reply.command);
    // A press opens the card above the button, and a second press of the same
    // button closes it. Opening asks for one refresh: a reply is the only way to
    // learn about a device that signed in after launch.
    if let Some(pressed) = playbar_reply.devices
        && let Some(anchor) = playbar::devices_button(bar_rect, regions.playbar)
        && screen
            .list
            .press(crate::menu::Anchor::Button(anchor), pressed)
    {
        screen.devices.list_opened();
    }
    // A steer is the holder's own transport: it never reaches the local queue,
    // and a frame whose one command is taken sends none. A seek it drops is not
    // sent, so the preview the drag left is cleared rather than standing until a
    // push that will never confirm it.
    if let Some(steer) = playbar_reply.steer {
        let sent = if command.is_none() {
            command = screen.devices.steer(steer, screen.now);
            command.is_some()
        } else {
            false
        };
        if !sent {
            screen.devices.display().scrub = None;
        }
    }
    // A pane press is local and drops no command, so it is applied whatever the
    // frame's command did. The regions drew from the preference this frame began
    // with, so the click needs a frame to show.
    if playbar_reply.pane {
        *screen.pane = screen.pane.press(content.width());
        ui.ctx().request_repaint();
    }
    // Never optimistic: the command carries what the window is not, and the
    // button lights only once the window reports the new state back.
    if playbar_reply.fullscreen
        && let Some(current) = fullscreen
    {
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Fullscreen(!current));
    }

    // The playbar draws after the panel and the queue, so its link click joins
    // theirs here; the one place that applies an open stays below.
    let open = open.or(playbar_reply.open.take());

    // The panel only reports the row it was asked to play. Applying the jump
    // here, and only when no other region asked for a command, keeps a frame
    // from moving the queue past a click whose `Play` would then be dropped.
    // A carried block holds the queue still: a jump moves `first_upcoming`
    // under the zone this frame's drop was resolved against.
    if command.is_none() && !carry::carrying(ui.ctx()) {
        command = queue_reply
            .play
            .and_then(|index| screen.queue.jump(index, screen.transport));
    }
    // A link click asks for a page and never for a play, so it is applied here,
    // after the jump that a play would have taken the frame for, and only while
    // the frame has no command of its own and nothing is carried. A dropped open
    // changes nothing: the words are still there to click again.
    if command.is_none()
        && !carry::carrying(ui.ctx())
        && let Some(page) = open
    {
        command = screen.library.open(page);
    }
    // The lyrics fetch is the open tab's, and it is asked for only once the
    // frame's own command is settled: a load marked `Loading` for a command that
    // was then dropped would wait forever. The track is the source's own, the
    // one the pane drew, or a remote load would be reset every frame.
    if command.is_none() {
        let track = match source {
            DeviceSource::Local => screen.transport.now_playing.as_ref(),
            DeviceSource::Remote => screen.devices.display().now_playing.as_ref(),
        };
        command = screen.library.lyrics.request(track);
    }

    // The menu draws after every region and before the drawer overlay: it hangs
    // over the view, and a drawer opened on top of it must stay on top. It is
    // anchored to the field rect the topbar reported, so it is the field's own
    // width and never the layout's guess (R7).
    let mut menu = search_menu::Menu::default();
    if screen.library.search.is_open()
        && let Some(field) = bar.search_field
    {
        menu = search_menu::show(
            ui,
            field.rect,
            &mut screen.library.search,
            screen.transport,
            screen.art,
            screen.now,
            tokens,
        );
    }

    // A requested action is applied only when no region took the frame's one
    // command. `queue.start` and `library.open` mutate before they return, so
    // applying one whose command would then be dropped leaves state changed for
    // a command nobody sends. A dropped action changes nothing, and the menu
    // stays open so the user can click again.
    let act = bar
        .search_field
        .map(|field| field.act)
        .filter(|act| *act != Act::None)
        .or(menu.click);
    if command.is_none()
        && let Some(act) = act
    {
        command = match act {
            Act::Play(index) => match screen.library.search.answer() {
                Some(tracks) => {
                    // R10: the loaded row's glyph is pause, and its action is the
                    // transport's toggle rather than a second start of the same
                    // track. A stopped transport has nothing to pause, so it
                    // starts the answer at this row like any other. The one
                    // decision is the transport's, so the glyph the row drew and
                    // the action here cannot disagree.
                    let held = tracks
                        .get(index)
                        .is_some_and(|entry| screen.transport.hold(&entry.uri).held);
                    if held {
                        screen.queue.toggle(screen.transport)
                    } else {
                        // R16: the queue becomes the whole answer, entered at the
                        // hit, so next and previous walk the results. The answer is
                        // merged from every kind, so it names no context.
                        screen.queue.start(tracks, index, None, screen.transport)
                    }
                }
                None => None,
            },
            Act::OpenAlbum(index) => {
                // Resolve before `open` mutably borrows the library.
                let uri = screen.library.search.album_uri(index);
                if let Some(uri) = uri {
                    if let Some(field) = bar.search_field {
                        ui.memory_mut(|memory| memory.surrender_focus(field.id));
                    }
                    screen.library.open(Page::Album(uri))
                } else {
                    None
                }
            }
            Act::OpenArtist(index) => {
                let uri = screen.library.search.artist_uri(index);
                if let Some(uri) = uri {
                    if let Some(field) = bar.search_field {
                        ui.memory_mut(|memory| memory.surrender_focus(field.id));
                    }
                    screen.library.open(Page::Artist(uri))
                } else {
                    None
                }
            }
            Act::OpenResults => {
                // The page takes the keyboard with the menu it closes. Only an
                // applied action does: a dropped `OpenResults` leaves the menu
                // open with the field still focused.
                if let Some(field) = bar.search_field {
                    ui.memory_mut(|memory| memory.surrender_focus(field.id));
                }
                screen
                    .library
                    .search
                    .key()
                    .and_then(|query| screen.library.open(Page::Search(query)))
            }
            Act::None | Act::Close => None,
        };
        screen.library.search.close();
    }

    // The shell is the only place that closes the menu on blur, once per frame,
    // after the field and the menu have both been drawn. The press that starts a
    // row click may surrender the field's focus a frame before the release
    // reports the click, so the pointer's geometry — not egui's surrender timing
    // — is what keeps the menu alive across that frame.
    let engaged = {
        let (position, active) = ui.input(|input| {
            (
                input
                    .pointer
                    .hover_pos()
                    .or_else(|| input.pointer.interact_pos()),
                input.pointer.any_pressed()
                    || input.pointer.any_down()
                    || input.pointer.any_released(),
            )
        });
        let on_field = position.is_some_and(|pos| {
            bar.search_field
                .is_some_and(|field| field.rect.contains(pos))
        });
        let on_menu = position.is_some_and(|pos| menu.rect.is_some_and(|rect| rect.contains(pos)));
        active && (on_field || on_menu)
    };
    let focused = bar.search_field.is_some_and(|field| field.has_focus);
    screen.library.search.settle(focused, engaged);
    // A press and release that ended on the menu without leaving it engaged the
    // menu across the frame that blurred the field, and nothing else asks for
    // the frame that closes it. Ask here, or the menu stays drawn over the view
    // until the next input event.
    if screen.library.search.is_open() && !focused && engaged {
        ui.ctx().request_repaint();
    }

    // The context menu draws after the search drop-down, so it hangs over it and
    // over every region. A page posts at most one request per frame, and a page
    // that did not draw posts nothing.
    let request = context_menu::take_request(ui.ctx());
    let page = screen.library.page();
    let _ = screen.menu.follow_page(page);
    if let Some(mut request) = request {
        request.allow_add(screen.library.add_targets());
        request.allow_create(regions.sidebar == Sidebar::Full && screen.library.creatable());
        let gate = door_gate(ui.ctx(), screen, &request.listing);
        let _ = screen.menu.open(request, gate);
    }
    let frame = context_menu::Frame {
        menu: &mut *screen.menu,
        // A choice is applied only when no region took the frame's command:
        // `library.open` mutates before it returns, so applying one whose
        // command would then be dropped leaves state changed for a command
        // nobody sends. A dropped choice is spent with its press, not retried.
        free: command.is_none(),
        tokens,
    };
    if let Some(closed) = context_menu::show(ui, frame)
        && let Some(verb) = closed.chose
    {
        command = apply_verb(ui, screen, verb);
    }

    // A list's `Del` posts its cut; the shell is the only place one is applied.
    // It is taken whether or not it can be, so a frame that took the command
    // leaves nothing behind for the next.
    let cut = context_menu::take_cut(ui.ctx());
    if command.is_none()
        && let Some(cut) = cut
    {
        command = apply_cut(ui, screen, cut);
    }

    // The head's submit posts an intent too. Applying it here is what keeps a
    // dropped submit from leaving the sidebar waiting on a command nobody sent:
    // the field keeps its text and the press is spent.
    let submit = sidebar::take_submit(ui.ctx());
    if command.is_none() && submit && !carry::carrying(ui.ctx()) {
        command = screen.library.create_playlist();
    }
    let rename_submit = crate::view::name_field::take_submit(ui.ctx());
    if command.is_none() && rename_submit && !carry::carrying(ui.ctx()) {
        command = screen.library.rename_playlist();
    }

    // A heart posts its one-frame intent. It is taken whether or not it can be
    // applied, so a dropped click leaves nothing behind for the next frame.
    let ask = heart::take(ui.ctx());
    if command.is_none()
        && let Some(ask) = ask
    {
        command = apply_like(ui, screen, ask.tracks, ask.liked);
    }

    // Settled before the drawer, so the Escape that cancels a drag does not also
    // close it. egui takes that Escape itself, before any region draws.
    let free = command.is_none();
    command = command.or(settle_carry(
        ui,
        screen,
        target,
        offer.as_ref(),
        free,
        tokens,
    ));

    // The card hangs over every region and under the drawer overlay, so a
    // drawer opened on top of it stays on top. Escape closes it before the
    // drawer reads the same key.
    let card = device_list::show(
        ui,
        device_list::Frame {
            list: screen.list,
            devices: screen.devices,
            local: screen.transport,
            has_row: screen.queue.current().is_some(),
            now: screen.now,
            tokens,
        },
    );
    if let Some(opener) = card.refocus {
        ui.memory_mut(|memory| memory.request_focus(opener));
    }
    // A picked row asks for the sound to move. A frame that already holds a
    // command keeps the pick for `logic` rather than dropping it.
    if let Some(device) = card.pick {
        let free = command.is_none();
        command = command.or(screen.devices.pick(&device, free));
    }

    // The drawer is stepped after the regions, so a playlist picked from the rail
    // closes it in the same frame it is picked. Its pick is the frame's command
    // when no earlier region took one, and the drawer refuses the pick when one
    // did: a row that armed a load whose command was then dropped would leave the
    // page waiting for a request nobody sent.
    let open = *screen.drawer == Drawer::Open;
    let (clicked_outside, asked) = if open {
        drawer_overlay(ui, grid, screen, tokens, command.is_none())
    } else {
        (false, sidebar::Asked::default())
    };
    let selected = asked.picked;
    command = command.or(asked.command);

    let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
    let next = screen.drawer.step(Input {
        rail: regions.sidebar == Sidebar::Rail,
        toggle: bar.toggle.as_ref().is_some_and(egui::Response::clicked),
        escape,
        clicked_outside,
        selected,
    });
    // Closing with Escape hands focus back to the control that opened the drawer,
    // so the keyboard does not end up nowhere.
    if open
        && next == Drawer::Closed
        && escape
        && let Some(toggle) = &bar.toggle
    {
        toggle.request_focus();
    }
    *screen.drawer = next;

    // The space toggles the transport the source names; a row starts a track on
    // Enter or a double click. It comes after the drawer step so that pausing
    // does not also close a drawer nobody picked from. Another device answers
    // to its own state, so the local queue's index and armed flag stay put.
    if command.is_none() && space {
        command = match source {
            DeviceSource::Remote => screen
                .devices
                .toggle()
                .and_then(|steer| screen.devices.steer(steer, screen.now)),
            DeviceSource::Local
                if screen.transport.now_playing.is_some() || screen.queue.armed() =>
            {
                screen.queue.toggle(screen.transport)
            }
            DeviceSource::Local => None,
        };
    }

    // Rows that landed for a shelf play start here, once nothing else took the
    // frame: a start mutates the queue before it returns, so applying one whose
    // command would then be dropped leaves the queue moved for nobody.
    if command.is_none()
        && !carry::carrying(ui.ctx())
        && let Some(start) = screen.library.take_play()
    {
        command = screen.queue.start(
            &start.tracks,
            0,
            Some(start.origin.as_str()),
            screen.transport,
        );
    }
    // A held start must not wait for the next input event: the frame that kept
    // the rows may be the last one egui runs for a while, and that frame is
    // exactly the one that took a command of its own.
    if screen.library.has_landed_play() {
        ui.ctx().request_repaint();
    }

    // After every edit: `logic` runs before `ui`, so a read this frame armed —
    // the last keystroke, Back onto a dropped page, a Retry — would otherwise
    // wait for the next input event. `app.rs` asks the same question after the
    // draw; asking here too keeps the contract inside the frame that arms it.
    let page = screen.library.page_search_query().map(str::to_owned);
    if screen.library.search.wants(page.as_deref()) {
        ui.ctx().request_repaint();
    }

    // A field the user closed takes the sentence its failed write left with it:
    // the notice named a field that is no longer on screen, so nothing is left
    // asking for the retry it described.
    screen.library.settle_rename();

    // Only a refocus this frame's rows drew is settled: one set after they drew —
    // a held Alt+↓ sets one every frame — waits for the frame that draws it.
    if screen.queue.refocus() == pending {
        screen.queue.settle_refocus();
    }

    command
}

/// Applies the verb a context menu closed on.
///
/// The snapshot the menu took is what the verb acts on, so a list that reloaded
/// under the menu cannot change what it does.
fn apply_verb(ui: &Ui, screen: &mut Screen<'_>, verb: Verb) -> Option<Command> {
    // A library menu's rows act on the one item it named, and it carries no
    // targets: the listing is what decides, before the pick is read.
    if let Some(item) = screen.menu.library_item().cloned()
        && let Some(outcome) = verbs::library_outcome(verb, &item)
    {
        return match outcome {
            verbs::Outcome::Copy(text) => {
                ui.ctx().copy_text(text);
                None
            }
            verbs::Outcome::Unlink(item) => apply_remove_from_library(ui, screen, &item),
            verbs::Outcome::Save(item) => screen.library.save_to_library(&item),
            verbs::Outcome::Follow { artist, followed } => apply_follow(screen, &artist, followed),
            verbs::Outcome::Rename(item) => {
                let surface = match screen.menu.listing() {
                    Listing::Band(_) => crate::naming::Surface::Band,
                    _ => crate::naming::Surface::Row,
                };
                apply_rename(ui, screen, &item, surface)
            }
            _ => None,
        };
    }
    let targets = screen.menu.targets().to_vec();
    match verbs::outcome(verb, &targets)? {
        verbs::Outcome::Queue(entries) => {
            // An add has no position to choose, so the entries go after the last.
            let _ = screen.queue.append(entries);
            None
        }
        verbs::Outcome::Copy(text) => {
            // One URL per line, through egui's own platform output, so the
            // window crate needs no clipboard dependency.
            ui.ctx().copy_text(text);
            None
        }
        verbs::Outcome::Open(page) => screen.library.open(page),
        verbs::Outcome::Remove => {
            let cut = screen.menu.cut();
            cut.and_then(|cut| apply_cut(ui, screen, cut))
        }
        verbs::Outcome::Like { tracks, liked } => apply_like(ui, screen, tracks, liked),
        verbs::Outcome::NewPlaylist { pick } => apply_new_playlist(ui, screen, pick),
        verbs::Outcome::AddToPlaylist { tracks } => {
            let uri = screen.menu.chosen_target()?.uri.clone();
            screen.library.add_pick(&uri, &tracks)
        }
        // A library row's verbs are the listing branch above; a track pick has
        // no such verb.
        verbs::Outcome::Unlink(_)
        | verbs::Outcome::Save(_)
        | verbs::Outcome::Follow { .. }
        | verbs::Outcome::Rename(_) => None,
    }
}

/// Opens the sidebar's name field for the menu's new-playlist row.
///
/// The field is the sidebar's, so the drawer and a carried block both drop the
/// choice: a field nobody can see, or one opened over a list about to move, is
/// worse than nothing.
fn apply_new_playlist(ui: &Ui, screen: &mut Screen<'_>, pick: Vec<String>) -> Option<Command> {
    if *screen.drawer == Drawer::Open || carry::carrying(ui.ctx()) {
        return None;
    }
    screen.library.naming.open_with(pick);
    None
}

/// Opens the rename field the menu's Rename row chose.
///
/// The field lives in a surface the drawer or a carried block can hide, so both
/// drop the choice; a typed field takes the keys instead.
fn apply_rename(
    ui: &Ui,
    screen: &mut Screen<'_>,
    item: &LibraryItem,
    surface: crate::naming::Surface,
) -> Option<Command> {
    if *screen.drawer == Drawer::Open || carry::carrying(ui.ctx()) || ui.ctx().text_edit_focused() {
        return None;
    }
    screen.library.begin_rename(&item.uri, surface);
    None
}

/// Applies a save or a remove-from-your-library the menu chose.
///
/// The frame's gates are checked again here, as a removal's are: an intent made
/// while a field is typing, the drawer is open, a block is carried or another
/// region took the command is dropped rather than applied.
fn apply_like(
    ui: &Ui,
    screen: &mut Screen<'_>,
    tracks: Vec<String>,
    liked: bool,
) -> Option<Command> {
    if *screen.drawer == Drawer::Open || carry::carrying(ui.ctx()) || ui.ctx().text_edit_focused() {
        return None;
    }
    screen.library.set_liked(tracks, liked)
}

/// Applies the removal a sidebar row's menu chose.
///
/// The frame's gates are checked again here, as a page removal's are: an intent
/// made while a field is typing, the drawer is open, a block is carried or a
/// write is already in flight is dropped rather than applied.
fn apply_remove_from_library(
    ui: &Ui,
    screen: &mut Screen<'_>,
    item: &LibraryItem,
) -> Option<Command> {
    let gate = door_gate(ui.ctx(), screen, &Listing::Library(item.clone()));
    if !gate.allows() || *screen.drawer == Drawer::Open || ui.ctx().text_edit_focused() {
        return None;
    }
    screen.library.remove_from_library(item)
}

/// The gate a menu on `listing` opens under.
///
/// The queue's entry at `uri`, if the queue holds it.
///
/// The pane draws the current row and the ones after it, so those are the
/// entries it can claim: a track in the history is no row on screen.
fn queue_entry<'q>(queue: &'q Queue, uri: &str) -> Option<&'q TrackEntry> {
    queue
        .current()
        .into_iter()
        .chain(queue.upcoming().map(|(_, entry)| entry))
        .find(|entry| entry.uri == uri)
}

/// A queue or Now Playing menu touches no revision, so a playlist write in
/// flight must not hold it shut; every other list still waits for it.
fn door_gate(ctx: &egui::Context, screen: &Screen<'_>, listing: &Listing) -> Gate {
    Gate {
        carrying: carry::carrying(ctx),
        writing: !matches!(listing, Listing::Queue | Listing::Current)
            && screen.library.write_in_flight(),
    }
}

/// Applies a removal a menu chose, or a list's `Del` posted.
///
/// The frame's gates are checked again here: an intent made while a field is
/// typing, the drawer is open, a block is carried, a write the list waits on is
/// in flight or another region took the command is dropped rather than applied.
fn apply_cut(ui: &Ui, screen: &mut Screen<'_>, cut: Cut) -> Option<Command> {
    let gate = door_gate(ui.ctx(), screen, &cut.listing);
    if !gate.allows()
        || screen.menu.is_open()
        || *screen.drawer == Drawer::Open
        || ui.ctx().text_edit_focused()
    {
        return None;
    }
    match cut.listing {
        Listing::Playlist { uri, list } => {
            let command = screen
                .library
                .remove_tracks(&uri, &cut.rows, &cut.entries)?;
            carry::forget_rows(ui.ctx(), list, &cut.rows);
            Some(command)
        }
        Listing::Queue => {
            // A refusal is silent: the removal touched no wire, so there is
            // nothing to report and nothing to retry.
            let _ = screen.queue.remove_rows(&cut.rows, &cut.entries);
            None
        }
        // A library row's removal is applied by the menu's own verb, because it
        // names no rows to cut.
        Listing::Page | Listing::Current | Listing::Library(_) | Listing::Band(_) => None,
    }
}

/// Follows or unfollows the artist a band's menu named.
///
/// A page that is on it also owns the label, so the page's own state is what the
/// write is asked through; any other artist is a plain write.
fn apply_follow(screen: &mut Screen<'_>, artist: &str, followed: bool) -> Option<Command> {
    // A write already in flight for this artist is not doubled: two would land in
    // order and leave the label and the row apart.
    if screen.library.follow.artist() == Some(artist) && screen.library.follow.pending() {
        return None;
    }
    Some(Command::SetFollow {
        artist: artist.to_owned(),
        followed,
    })
}

/// The page this frame's link clicks named, taken from the fields the panel and
/// the queue report one in: the panel's own first, then the queue's.
///
/// A frame opens at most one page, so both opens are spent with the one taken;
/// the shell applies the answer once. A region that draws later joins this after
/// its own reply.
fn take_open(panel: &mut Option<Page>, queue: &mut Option<Page>) -> Option<Page> {
    panel.take().or(queue.take())
}

/// Applies what the queue pane reported: the picked row, the lifted block, and
/// the Alt keys that move rows with no pointer at all.
fn apply_queue_reply(ui: &Ui, screen: &mut Screen<'_>, reply: &mut queue_pane::Reply) {
    if let Some((index, press)) = reply.press {
        screen.queue.press(index, press);
    }
    if let Some(lift) = reply.lift.take()
        && let Some(carry) = carry::from_queue(screen.queue, lift)
    {
        carry::lift(ui.ctx(), carry);
    }
    let gate = door_gate(ui.ctx(), screen, &Listing::Queue);
    if let Some((row, door)) = reply.menu.take() {
        carry::queue_menu(
            screen.queue,
            ui.ctx(),
            door,
            row,
            gate,
            &screen.library.likes,
            screen.library.page(),
        );
    }
    if let Some(row) = reply.remove.take() {
        carry::queue_cut(screen.queue, ui.ctx(), row, gate);
    }
    // A row just lifted is on its way somewhere and must not also step.
    if !carry::carrying(ui.ctx()) {
        alt_step(ui, screen, reply.focused);
    }
}

/// Moves the queue's picked rows, or the focused row, one place with Alt+↑/↓.
///
/// Alt+Shift matches too, because a pattern without shift admits one; Ctrl+Alt
/// does not, because a pattern without ctrl or command refuses both.
fn alt_step(ui: &Ui, screen: &mut Screen<'_>, focused: Option<usize>) {
    let Some(focused) = focused else {
        return;
    };
    let step = ui.input_mut(|input| {
        if input.consume_key(egui::Modifiers::ALT, egui::Key::ArrowUp) {
            Some(Step::Up)
        } else if input.consume_key(egui::Modifiers::ALT, egui::Key::ArrowDown) {
            Some(Step::Down)
        } else {
            None
        }
    });
    let Some(step) = step else {
        return;
    };
    let rows = screen.queue.acting_rows(Some(focused));
    let Some(moved) = screen.queue.step_rows(&rows, step) else {
        return;
    };
    if let Some(landed) = moved.iter().map(|(_, to)| *to).min() {
        screen.queue.refocus_at(landed);
    }
    ui.ctx().request_repaint();
}

/// Where a carried page row lands in the list it came from, when that list
/// offered its rows this frame.
fn reorder_target(carry: &Carry, offer: Option<&Offer>, pointer: Option<Pos2>) -> Option<Target> {
    let offer = offer.filter(|offer| carry.source == Source::List(offer.list))?;
    let at = landing::resolve(pointer, Some(&offer.zone))?;
    // The marker is drawn where a move would land, so a blocked drop draws none.
    reorder::gather(offer.zone.count, &carry.rows, at.boundary)?;
    Some(Target::Reorder(at))
}

/// Where a carried playlist lands in the sidebar column, when it moved at all.
///
/// A drop on the carried row's own two gaps is no move: it would name the place
/// the row already holds. A column whose order is not the wire's lands nothing,
/// so no rule is drawn on a list that would refuse the move.
fn rootlist_target(
    carry: &Carry,
    column: Option<&ColumnOffer>,
    library: &Library,
    pointer: Option<Pos2>,
) -> Option<Target> {
    let column = column?;
    // A narrowed column draws some of its rows: the boundary the pointer names
    // is a drawn slot's, and moving the playlist that slot stands for would move
    // the neighbours the query hid with it.
    if column.rows.is_some() || !library.rootlist_movable() {
        return None;
    }
    let at = landing::resolve(pointer, Some(&column.zone))?;
    if let [row] = carry.rows[..]
        && (at.boundary == row || at.boundary == row + 1)
    {
        return None;
    }
    Some(Target::Rootlist(at))
}

/// Where a carried block adds itself: the playlist row under the pointer, when
/// that row may take the whole pick.
fn add_target(
    carry: &Carry,
    column: Option<&ColumnOffer>,
    library: &Library,
    pointer: Option<Pos2>,
) -> Option<Target> {
    let column = column?;
    if !carry.addable() {
        return None;
    }
    let slot = landing::slot_at(pointer, Some(&column.zone), column.drawn)?;
    // The drawn slot is not the playlist's index in a narrowed column, so every
    // write the drop makes names the true row the slot stands for.
    let row = column.source_row(slot.row)?;
    library
        .addable(row)
        .then_some(Target::Add(landing::Slot { row, ..slot }))
}

/// Moves a dropped playlist in the sidebar, and asks for one rootlist write.
fn rearrange(screen: &mut Screen<'_>, carry: &Carry, at: Landing, free: bool) -> Option<Command> {
    if !free {
        return None;
    }
    let [row] = carry.rows[..] else {
        return None;
    };
    screen
        .library
        .move_playlist(row, at.boundary, carry.playlist.as_deref()?)
}

/// Adds the carried tracks to the row they were dropped on, and asks for one write.
fn add(screen: &mut Screen<'_>, carry: &Carry, slot: landing::Slot, free: bool) -> Option<Command> {
    if !free {
        return None;
    }
    screen.library.add_tracks(slot.row, &carry.pick()?)
}

/// Moves the dropped rows in their own list and picks where they landed.
///
/// A page that is no longer free to write lands nothing: the move is the page's
/// to accept, and a block it refuses is refused whole.
fn reorder(
    ctx: &egui::Context,
    screen: &mut Screen<'_>,
    offer: Option<&Offer>,
    carry: &Carry,
    at: Landing,
    free: bool,
) -> Option<Command> {
    // A refused reorder is dropped, not retried, even though the drag marked it.
    if !free {
        return None;
    }
    let offer = offer?;
    let (command, landed) =
        screen
            .library
            .move_tracks(&offer.playlist, &carry.rows, at.boundary, &carry.entries)?;
    carry::select_rows(ctx, offer.list, landed);
    Some(command)
}

/// Looks at the carried block once every region has drawn: lands it, drops it, or
/// paints its ghost.
fn settle_carry(
    ui: &mut Ui,
    screen: &mut Screen<'_>,
    target: Option<Target>,
    offer: Option<&Offer>,
    free: bool,
    tokens: &Tokens,
) -> Option<Command> {
    let ctx = ui.ctx().clone();
    let carry = carry::carried(&ctx)?;
    let pointer = ctx.pointer_latest_pos();

    if ctx.input(|input| input.pointer.button_released(PointerButton::Primary)) {
        let command = match target {
            Some(Target::Reorder(at)) => reorder(&ctx, screen, offer, &carry, at, free),
            Some(Target::Rootlist(at)) => rearrange(screen, &carry, at, free),
            Some(Target::Add(slot)) => add(screen, &carry, slot, free),
            Some(target) => {
                land(screen.queue, &carry, target);
                None
            }
            None => None,
        };
        DragAndDrop::clear_payload(&ctx);
        ctx.request_repaint();
        return command;
    }
    // Defensive: egui keeps a button down until it sees the release, so this
    // covers only a release the window never delivered.
    if !ctx.input(|input| input.pointer.primary_down()) {
        DragAndDrop::clear_payload(&ctx);
        return None;
    }
    if let Some(pointer) = pointer {
        ghost::paint(
            ui,
            ghost::Paint {
                carry: &carry,
                target,
                pointer,
                tokens,
            },
        );
    }
    None
}

/// The queue button's drop area, when it is one: only while the pane is hidden,
/// and only for page lists, because the button adds and never moves.
fn queue_area(now_playing: NowPlaying, form: Playbar, bar: Rect, carry: &Carry) -> Option<Area> {
    if now_playing != NowPlaying::Hidden || !matches!(carry.source, Source::List(_)) {
        return None;
    }
    Some(Area::around(playbar::queue_button(bar, form)))
}

/// Applies the drop a release made, and asks for the row it landed on.
fn land(queue: &mut Queue, carry: &Carry, target: Target) {
    let range = match (target, carry.source) {
        // A queue that restarted under the drag holds other entries at these rows,
        // and moving them would move tracks the drag never picked.
        (Target::At(at), Source::Queue) if queue.entries_at(&carry.rows) == carry.entries => {
            queue.move_rows(&carry.rows, at.boundary)
        }
        // The button adds; it never moves a queue row.
        (Target::At(_) | Target::End, Source::Queue) => None,
        (Target::At(at), Source::List(_)) => queue.insert_at(at.boundary, carry.entries.clone()),
        // A row dropped in its own list is moved by `reorder`, not by the queue.
        (Target::Reorder(_), _) => None,
        // The sidebar's two meanings are writes of their own, never a queue move.
        (Target::Rootlist(_) | Target::Add(_), _) => None,
        // A playlist carry has no queue zone and no queue button, so it can only
        // ever mean the sidebar.
        (Target::At(_) | Target::End, Source::Playlist) => None,
        // An add asks for no row: the pane that would show it is not on screen.
        (Target::End, Source::List(_)) => {
            let _ = queue.append(carry.entries.clone());
            None
        }
    };
    if let Some(range) = range {
        queue.refocus_at(range.start);
    }
}

/// The rail's overflow panel: the full sidebar, over everything, above the
/// playbar so the transport stays reachable.
///
/// Returns whether a click landed outside the drawer, and what the drawer asked
/// the shell for.
fn drawer_overlay(
    ui: &mut Ui,
    grid: Grid,
    screen: &mut Screen<'_>,
    tokens: &Tokens,
    free: bool,
) -> (bool, sidebar::Asked) {
    let shadow = tokens.palette.shadow_lg.as_shape(grid.drawer, radius::LG);
    ui.painter().add(shadow);

    let area = egui::Area::new(ui.scope_id().with("drawer"))
        .order(Order::Foreground)
        .fixed_pos(grid.drawer.min)
        .constrain(false)
        .fade_in(false);

    let picked = area
        .show(ui.ctx(), |ui| {
            ui.set_clip_rect(grid.drawer);
            // The drawer paints at explicit rects and draws its rows in a child,
            // so nothing would allocate here and the area would be zero-sized.
            // A zero-sized area never holds the pointer: the page behind would
            // stay the top layer, take the wheel and leave the drawer unscrollable.
            ui.set_min_size(grid.drawer.size());
            panel(ui, grid.drawer, tokens.palette.surface, tokens);
            let mut child = ui.new_child(
                UiBuilder::new()
                    .id_salt("drawer-sidebar")
                    .max_rect(grid.drawer)
                    .layout(Layout::top_down(Align::Min)),
            );
            child.set_clip_rect(grid.drawer);
            sidebar::show(
                &mut child,
                screen.library,
                screen.art,
                screen.now,
                tokens,
                free,
            )
        })
        .inner;

    // The drawer's own field draws inside this box, so the area is given its
    // size above: asking the area whether a click was elsewhere would answer yes
    // for a click on the field. The box the drawer covers is what decides.
    let outside = ui.input(|input| {
        input.pointer.any_click()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|pos| !grid.drawer.contains(pos))
    });
    (outside, picked)
}

/// Applies the topbar's nav request, unless another region already asked for a
/// command this frame.
///
/// A nav can move the page and start a load: Back onto a playlist settles it,
/// marking the tracks wanted and loading. The shell keeps the first command of
/// the frame, so a nav that lost to an earlier one would have changed the
/// library for a command nobody sends, and the page would spin for a reply that
/// was never asked for. Yielding instead keeps the page and the load in step.
fn apply_nav(
    library: &mut Library,
    nav: Option<topbar::Nav>,
    command: Option<Command>,
) -> Option<Command> {
    if command.is_some() {
        return command;
    }
    match nav {
        None => None,
        Some(topbar::Nav::Back) => library.back(),
        Some(topbar::Nav::Forward) => library.forward(),
        // The root is local state and never needs a fetch, so it adds no command
        // of its own.
        Some(topbar::Nav::Home) => {
            library.to_playlists();
            None
        }
        // A list crumb asks for the root showing its own chip.
        Some(topbar::Nav::Root(kind)) => {
            library.to_playlists();
            library.root_filter.choose(kind);
            None
        }
        Some(topbar::Nav::Page(page)) => library.open(page),
    }
}

/// The side buttons and browser keys ask for the same nav as the topbar
/// buttons and go through `apply_nav`.
///
/// Auto-repeat is deliberate: `key_pressed` also counts repeat events, so a held
/// browser Back key steps back one page per repeat, the way a browser does.
fn nav_from_input(ui: &Ui) -> Option<topbar::Nav> {
    ui.input(|input| {
        if input.key_pressed(egui::Key::BrowserBack)
            || input.pointer.button_pressed(egui::PointerButton::Extra1)
        {
            Some(topbar::Nav::Back)
        } else if input.key_pressed(egui::Key::BrowserForward)
            || input.pointer.button_pressed(egui::PointerButton::Extra2)
        {
            Some(topbar::Nav::Forward)
        } else {
            None
        }
    })
}

/// Takes the frame's space for the transport, if there is one to take.
///
/// It is taken before anything draws, because egui turns a space into a click for
/// whatever holds focus: a focused row would start its own track in the same
/// frame the transport paused. The press and the auto-repeats behind it are all
/// taken away for that reason, and only a first press toggles anything.
///
/// Returns whether this frame held that first press. A space with a modifier, or
/// one pressed while a field is typing, is left alone.
fn take_space(ui: &Ui) -> bool {
    if ui.ctx().text_edit_focused() {
        return false;
    }

    let mut owned = false;
    let mut first = false;
    ui.input(|input| {
        for event in &input.events {
            if let Some(repeat) = transport_space(event) {
                owned = true;
                first |= !repeat;
            }
        }
    });
    if !owned {
        return false;
    }

    ui.input_mut(|input| {
        input
            .events
            .retain(|event| transport_space(event).is_none())
    });
    first
}

/// `Some(repeat)` when this event is a space the transport owns.
fn transport_space(event: &egui::Event) -> Option<bool> {
    match event {
        egui::Event::Key {
            key: egui::Key::Space,
            pressed: true,
            repeat,
            modifiers,
            ..
        } if modifiers.is_none() => Some(*repeat),
        _ => None,
    }
}

/// The topbar and the view share one panel, so they share one frame.
fn main_panel(grid: &Grid) -> Rect {
    Rect::from_min_max(grid.topbar.min, grid.view.max)
}

/// Paints one panel: a hairline box on the window background.
fn panel(ui: &Ui, rect: Rect, fill: Color32, tokens: &Tokens) {
    ui.painter().rect(
        rect,
        radius::LG,
        fill,
        Stroke::new(stroke::HAIRLINE, tokens.palette.border),
        StrokeKind::Inside,
    );
}

/// Draws one region into its content rect, clipped to the panel it sits on.
fn region<R>(
    ui: &mut Ui,
    id: &str,
    panel: Rect,
    content: Rect,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.scope_builder(
        UiBuilder::new()
            .id_salt(id)
            .max_rect(content)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            // A child inherits its parent's clip rect, so without this a panel's
            // contents would spill into the channels beside it.
            ui.set_clip_rect(panel);
            add_contents(ui)
        },
    )
    .inner
}

/// Runs `add` inside a left-to-right child at `rect`, so a control that
/// allocates its own space still lands where the design wants it.
pub(super) fn row_of<R>(ui: &mut Ui, id: &str, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(
        UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.set_clip_rect(rect);
            add(ui)
        },
    )
    .inner
}

/// A row's vertical centre: a region is the whole column's height, so a row that
/// draws its own rects has to centre them itself rather than trust the cursor.
pub(super) fn row_centre(ui: &Ui) -> f32 {
    ui.max_rect().center().y
}

/// The same, named for the rows that hold a run of controls.
pub(super) fn row_centred<R>(
    ui: &mut Ui,
    id: &str,
    rect: Rect,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    row_of(ui, id, rect, add)
}

/// Paints a hairline rule inside `rect`.
pub(super) fn rule(ui: &Ui, rect: Rect, color: Color32) {
    ui.painter().rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(rect.width(), stroke::HAIRLINE)),
        0.0,
        color,
    );
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use takofy_model::{
        ArtistDetail, CoverSet, CoverUrl, Device, DeviceKind, FollowedArtist, FollowedShow, Move,
        PlaylistAccess, PlaylistEntry, Release, ReleaseKind, RemotePlayback, RemoteState, Removal,
        Revision, SavedAlbum, TrackEntry,
    };

    use super::*;
    use crate::cover::Art;
    use crate::device_list::DeviceList;
    use crate::devices::Devices;
    use crate::library::{Library, Load, MediaLayout};
    use crate::library_filter::RootKind;
    use crate::menu::LibraryItem;
    use crate::message::{DeviceUpdate, Failure, Steer, Update};
    use crate::player::{NowPlaying, Transport, TransportState};
    use crate::queue::Queue;
    use crate::route::Page;
    use crate::search::Device as SearchDevice;
    use crate::selection::Press;
    use crate::sidebar_kind::SidebarKind;
    use crate::signin::SignIn;
    use crate::theme::{extent, fonts, icon, space, style};

    fn cover_url(tail: &str) -> CoverUrl {
        CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
    }

    fn cover(tail: &str) -> CoverSet {
        CoverSet::single(cover_url(tail))
    }

    /// A cover offering 64, 300 and 640 squares, with their URLs.
    fn sized(tail: &str) -> (CoverSet, CoverUrl, CoverUrl, CoverUrl) {
        let small = CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}-64"))
            .expect("a cover url");
        let middle = CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}-300"))
            .expect("a cover url");
        let large = CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}-640"))
            .expect("a cover url");
        let set = CoverSet::from_sizes([
            (Some((64, 64)), small.clone()),
            (Some((300, 300)), middle.clone()),
            (Some((640, 640)), large.clone()),
        ])
        .expect("a cover set");
        (set, small, middle, large)
    }

    fn playing(cover: CoverSet) -> Transport {
        Transport {
            now_playing: Some(NowPlaying {
                uri: "spotify:track:x".to_owned(),
                title: "A Title".to_owned(),
                artists: "A Band".to_owned(),
                cover: Some(cover),
                ..Default::default()
            }),
            ..Transport::default()
        }
    }

    fn track(index: usize, cover: Option<CoverSet>) -> TrackEntry {
        TrackEntry {
            uri: format!("t{index}"),
            title: format!("t{index}"),
            artists: String::new(),
            album: None,
            album_uri: None,
            artist_uris: Vec::new(),
            artist_names: Vec::new(),
            added_at: None,
            duration_ms: None,
            named: true,
            cover,
        }
    }

    /// The window's own frame, kept across passes because egui's hit test reads
    /// the previous one.
    struct Window {
        ctx: egui::Context,
        size: Vec2,
        state: SignIn,
        library: Library,
        devices: Devices,
        list: DeviceList,
        drawer: Drawer,
        menu: crate::menu::Menu,
        art: Art,
        now: Instant,
        /// The modifiers every frame reports as held. egui has no field for them
        /// on `RawInput`: a frame is told through `Event::ModifiersChanged`.
        held: egui::Modifiers,
        /// Whether the frame reports the window as holding the OS keyboard.
        unfocused: bool,
        /// What the user asked of the Now Playing column.
        pane: layout::PanePref,
        /// What the window reports about itself, or `None` while it cannot say.
        fullscreen: Option<bool>,
    }

    impl Window {
        fn new(size: Vec2) -> Self {
            let tokens = Tokens::DARK;
            let ctx = egui::Context::default();
            ctx.set_fonts(fonts::definitions());
            ctx.set_global_style(style(&tokens));
            Self {
                ctx,
                size,
                state: SignIn::SignedIn,
                library: Library::default(),
                devices: Devices::default(),
                list: DeviceList::default(),
                drawer: Drawer::default(),
                menu: crate::menu::Menu::default(),
                art: Art::default(),
                now: Instant::now(),
                held: egui::Modifiers::NONE,
                unfocused: false,
                pane: layout::PanePref::default(),
                fullscreen: Some(false),
            }
        }

        /// Draws one frame, returning the command the shell reported and the
        /// frame's output.
        fn frame(
            &mut self,
            transport: &mut Transport,
            queue: &mut Queue,
            events: Vec<egui::Event>,
        ) -> (Option<Command>, egui::FullOutput) {
            let tokens = Tokens::DARK;
            let mut events = events;
            events.insert(0, egui::Event::ModifiersChanged(self.held));
            let mut viewports = egui::ViewportIdMap::default();
            viewports.insert(
                egui::ViewportId::ROOT,
                egui::ViewportInfo {
                    fullscreen: self.fullscreen,
                    ..Default::default()
                },
            );
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                viewports,
                events,
                focused: !self.unfocused,
                ..Default::default()
            };
            self.art.begin_frame();
            let mut command = None;
            let mut screen = Screen {
                state: &mut self.state,
                library: &mut self.library,
                transport,
                queue,
                devices: &mut self.devices,
                list: &mut self.list,
                drawer: &mut self.drawer,
                menu: &mut self.menu,
                position_ms: 0,
                art: &mut self.art,
                now: self.now,
                now_ms: 0,
                pane: &mut self.pane,
            };
            let mut output = self.ctx.run_ui(input, |ui| {
                command = show(ui, &mut screen, &tokens);
            });
            output.textures_delta.clear();
            (command, output)
        }

        /// The covers the last frame's draw asked for.
        fn requests(&mut self) -> Vec<CoverUrl> {
            self.art
                .requests(self.now)
                .into_iter()
                .filter_map(|command| match command {
                    Command::Fetch { url } => Some(url),
                    _ => None,
                })
                .collect()
        }

        /// Double clicks `at` over the passes egui's hit test needs, returning
        /// the command the shell reported. A track row plays on a double click,
        /// the way every other table's rows do.
        fn double_click(
            &mut self,
            transport: &mut Transport,
            queue: &mut Queue,
            at: egui::Pos2,
        ) -> Option<Command> {
            let _ = self.frame(transport, queue, Vec::new());
            let mut command = None;
            for _ in 0..2 {
                for event in crate::view::test_support::click_events(at) {
                    if let Some(next) = self.frame(transport, queue, vec![event]).0 {
                        command = Some(next);
                    }
                }
            }
            command
        }

        /// Clicks `at` over the passes egui's hit test needs, returning the
        /// viewport commands the passes sent.
        fn click_viewport(
            &mut self,
            transport: &mut Transport,
            queue: &mut Queue,
            at: egui::Pos2,
        ) -> Vec<egui::ViewportCommand> {
            let _ = self.frame(transport, queue, Vec::new());
            let mut commands = Vec::new();
            for event in crate::view::test_support::click_events(at) {
                let (_, frame) = self.frame(transport, queue, vec![event]);
                commands.extend(viewport_commands(&frame));
            }
            commands
        }

        /// Clicks `at` over the passes egui's hit test needs, returning the
        /// command the shell reported.
        fn click(
            &mut self,
            transport: &mut Transport,
            queue: &mut Queue,
            at: egui::Pos2,
        ) -> Option<Command> {
            let _ = self.frame(transport, queue, Vec::new());
            let mut command = None;
            for event in crate::view::test_support::click_events(at) {
                if let Some(next) = self.frame(transport, queue, vec![event]).0 {
                    command = Some(next);
                }
            }
            command
        }

        /// Drags from `from` to `to`, release included, returning the command the
        /// shell reported.
        fn drag(
            &mut self,
            transport: &mut Transport,
            queue: &mut Queue,
            from: egui::Pos2,
            to: egui::Pos2,
        ) -> Option<Command> {
            let last = self.hold_drag(transport, queue, from, to);
            self.frame(transport, queue, vec![self.release(to)])
                .0
                .or(last.0)
        }

        /// Drags from `from` to `to` and stops there with the button still down,
        /// returning the last frame the pointer moved in.
        fn hold_drag(
            &mut self,
            transport: &mut Transport,
            queue: &mut Queue,
            from: egui::Pos2,
            to: egui::Pos2,
        ) -> (Option<Command>, egui::FullOutput) {
            let mut events = crate::view::test_support::drag_events(from, to);
            events.pop();
            let _ = self.frame(transport, queue, Vec::new());
            let mut last = None;
            let mut command = None;
            for event in events {
                let (next, output) = self.frame(transport, queue, vec![event]);
                command = command.or(next);
                last = Some(output);
            }
            (
                command,
                last.expect("a drag moves the pointer at least once"),
            )
        }

        /// The release event for a drag that ends at `to`, carrying the held
        /// modifiers the way a real release does.
        fn release(&self, to: egui::Pos2) -> egui::Event {
            egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: self.held,
            }
        }
    }

    /// Where a label was painted, so a click lands on a row the shell drew
    /// rather than on a coordinate a test guessed.
    fn painted_at(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == label => Some(text.pos),
                _ => None,
            })
    }

    /// The seek track the bar painted inside `column`.
    fn painted_seek_track(output: &egui::FullOutput, column: Rect) -> Rect {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if (shape.rect.height() - extent::SLIDER_TRACK_H).abs() < 0.01
                        && shape.fill == Tokens::DARK.palette.border_strong
                        && column.contains(shape.rect.center()) =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .expect("the bar paints its seek track")
    }

    /// The rect the first text shape with `label` inside `area` was painted in.
    fn text_in(output: &egui::FullOutput, label: &str, area: Rect) -> Option<Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text)
                    if text.galley.text() == label && area.contains(text.pos) =>
                {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
    }

    /// The topbar's rect, solved the same way the shell solves it.
    /// The bar spans the main column, which the pane widens; `sidebar_rect` is width-only.
    fn topbar_rect(window: &Window) -> Rect {
        let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
        let regions = layout::regions_with(content.width(), window.pane);
        let shell = layout::shell_for(&regions);
        layout::grid_for(content, &shell).topbar
    }

    /// The drawer's box, solved the same way the shell solves it.
    fn drawer_rect(window: &Window) -> Rect {
        let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
        let regions = layout::regions_with(content.width(), window.pane);
        let shell = layout::shell_for(&regions);
        layout::grid_for(content, &shell).drawer
    }

    /// The view's rect, solved the same way the shell solves it. A same-frame
    /// destination is read from the text the view paints here, never from the
    /// sidebar, which draws before the nav is applied.
    fn view_rect(window: &Window) -> Rect {
        let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
        let regions = layout::regions_with(content.width(), window.pane);
        let shell = layout::shell_for(&regions);
        layout::grid_for(content, &shell).view
    }

    /// The bottom rule the topbar paints, which spells out the width it drew at.
    fn topbar_rule(frame: &egui::FullOutput, bar: Rect) -> Rect {
        frame
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.stroke.width == 0.0
                        && (shape.rect.top() - bar.bottom()).abs() < 0.01 =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .expect("the topbar paints its bottom rule")
    }

    /// One press or release of `key` with no modifiers.
    fn key(key: egui::Key, pressed: bool) -> egui::Event {
        key_with_repeat(key, pressed, false)
    }

    /// One press of `key` as a held key's auto-repeat, never a release.
    fn key_repeat(key: egui::Key) -> egui::Event {
        key_with_repeat(key, true, true)
    }

    /// `event` as one that says it holds `modifiers`.
    fn holding(mut event: egui::Event, modifiers: egui::Modifiers) -> egui::Event {
        if let egui::Event::Key {
            modifiers: held, ..
        } = &mut event
        {
            *held = modifiers;
        }
        event
    }

    /// One press of `key` holding `modifiers`, then its release.
    ///
    /// `consume_key` reads the event's own modifiers, not the frame's, so a chord
    /// has to say what it holds.
    fn chord(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        pressed: egui::Key,
        modifiers: egui::Modifiers,
    ) -> Option<Command> {
        window.held = modifiers;
        let press = holding(key(pressed, true), modifiers);
        let (command, _) = window.frame(transport, queue, vec![press]);
        let (_, _) = window.frame(transport, queue, vec![key(pressed, false)]);
        window.held = egui::Modifiers::NONE;
        command
    }

    fn key_with_repeat(key: egui::Key, pressed: bool, repeat: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// One press or release of `button` at `pos`.
    ///
    /// The side buttons are read globally, so most tests pass `Pos2::ZERO`: a
    /// `Primary` press there only arms a click on whatever sits top-left, which
    /// is benign without a release. A frame that also clicks passes the click's
    /// own position, because that is where the press would arrive on real
    /// hardware. `test_support::button` is `Primary`-only, so it is not used
    /// here.
    fn side(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// The topbar's round buttons, left to right: Home, back, forward. Found from
    /// the buttons' painted frames rather than from coordinates a test guessed.
    fn topbar_buttons(output: &egui::FullOutput) -> Vec<Rect> {
        // Only the topbar's band: the library's density toggle draws the same
        // 28-point squares further down the window.
        let band = extent::TOPBAR_H;
        let mut rects = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.stroke.width > 0.0
                        && (shape.rect.width() - extent::BUTTON).abs() < 0.01
                        && (shape.rect.height() - extent::BUTTON).abs() < 0.01
                        && shape.rect.bottom() <= band =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        rects.sort_by(|left, right| left.left().total_cmp(&right.left()));
        rects
    }

    /// The search field's painted box, found by its height rather than by a
    /// coordinate a test guessed.
    fn search_field(output: &egui::FullOutput) -> Option<Rect> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if (shape.rect.height() - extent::SEARCH_H).abs() < 0.01
                        && shape.rect.width() > 100.0 =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
    }

    /// The sidebar's highlighted library rows, which wear a bone rule at the
    /// left edge.
    fn selected_rows(output: &egui::FullOutput) -> usize {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) => {
                    shape.fill == tokens.palette.text
                        && (shape.rect.width() - stroke::RULE).abs() < 0.01
                        && shape.rect.height() > extent::ROW_H
                        && shape.rect.left() < extent::SIDEBAR_W
                }
                _ => false,
            })
            .count()
    }

    const PLAYLIST: &str = "spotify:playlist:a";
    const ARTIST: &str = "spotify:artist:a";
    const ALBUM: &str = "spotify:album:b";
    const ALBUM_B: &str = "spotify:album:c";
    const QUEUE_ALBUM: &str = "spotify:album:q";
    const QUEUE_ARTIST: &str = "spotify:artist:q";

    /// Puts one named, ready playlist on screen.
    fn ready_playlist(window: &mut Window) {
        window.library.apply(&Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
            uri: PLAYLIST.to_owned(),
            name: "Mix".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }]));
        window.library.select(PLAYLIST);
        window.library.apply(&Update::Tracks {
            playlist: PLAYLIST.to_owned(),
            tracks: Vec::new(),
            cover: None,
            access: None,
        });
    }

    /// The middle of a three-page history: album A on screen, the playlist
    /// behind it and album B ahead.
    fn history_of_three(window: &mut Window) {
        ready_playlist(window);
        window.library.open(Page::Album(ALBUM.to_owned()));
        window.library.open(Page::Album(ALBUM_B.to_owned()));
        window.library.back();
    }

    /// In drawer mode the layout hands the toggle a whole column; the button in
    /// it is still a button, the same square as its neighbours.
    #[test]
    fn the_drawer_toggle_is_a_button_not_a_column() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(800.0, 900.0));
        ready_playlist(&mut window);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let bar = topbar_rect(&window);
        let buttons = topbar_buttons(&frame);
        assert_eq!(buttons.len(), 4, "the toggle, Home, back and forward");
        for button in &buttons {
            assert!(
                (button.height() - extent::BUTTON).abs() < 0.01,
                "a topbar button {button:?} is not one button tall"
            );
        }

        // Nothing drawn as a frame in the bar is taller than a button: a frame
        // stretched to the region came from placing a control on the column
        // rather than in it. The search field is the one box with its own height.
        for clipped in &frame.shapes {
            if let egui::Shape::Rect(shape) = &clipped.shape
                && shape.stroke.width > 0.0
                && bar.contains_rect(shape.rect)
                && (shape.rect.height() - extent::SEARCH_H).abs() > 0.01
            {
                assert!(
                    shape.rect.height() <= extent::BUTTON + 0.01,
                    "the topbar draws a {:?} frame, taller than a button",
                    shape.rect
                );
            }
        }
    }

    #[test]
    fn every_topbar_item_shares_one_baseline() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        ready_playlist(&mut window);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let bar = topbar_rect(&window).center().y;
        let buttons = topbar_buttons(&frame);
        assert_eq!(buttons.len(), 3, "Home, back and forward");
        for button in &buttons {
            assert!(
                (button.center().y - bar).abs() < 0.01,
                "a topbar button at {:?} is not on the bar's centre {bar}",
                button.center()
            );
        }
        let field = search_field(&frame).expect("the topbar paints the search field");
        assert!(
            (field.center().y - bar).abs() < 0.01,
            "the search field at {:?} is not on the bar's centre {bar}",
            field.center()
        );
    }

    #[test]
    fn the_topbar_widens_when_the_pane_is_hidden() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        ready_playlist(&mut window);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let shown = topbar_rect(&window);
        let shown_rule = topbar_rule(&frame, shown);
        assert!(
            (shown_rule.width() - (shown.width() - 2.0 * stroke::HAIRLINE)).abs() < 0.01,
            "the topbar drew {shown_rule:?}, not the region it solved {shown:?}"
        );

        window.pane = layout::PanePref::Hidden;
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let hidden = topbar_rect(&window);
        let hidden_rule = topbar_rule(&frame, hidden);

        assert!(
            hidden_rule.width() > shown_rule.width(),
            "the hidden pane left the topbar at the width it had beside it: \
             {hidden_rule:?} against {shown_rule:?}"
        );
        assert!(
            (hidden_rule.width() - (hidden.width() - 2.0 * stroke::HAIRLINE)).abs() < 0.01,
            "the topbar region {hidden:?} disagrees with the bar it drew {hidden_rule:?}"
        );
    }

    #[test]
    fn the_history_buttons_move_between_pages() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        ready_playlist(&mut window);
        window.library.open(Page::Album(ALBUM.to_owned()));

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let buttons = topbar_buttons(&frame);
        assert_eq!(buttons.len(), 3, "the topbar paints Home, back and forward");

        assert_eq!(
            window.click(&mut transport, &mut queue, buttons[1].center()),
            None,
            "back into a ready playlist needs no load"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned()))
        );

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let buttons = topbar_buttons(&frame);
        assert_eq!(
            window.click(&mut transport, &mut queue, buttons[2].center()),
            None
        );
        assert_eq!(window.library.page(), Some(&Page::Album(ALBUM.to_owned())));
    }

    #[test]
    fn the_history_buttons_do_nothing_with_no_history() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let buttons = topbar_buttons(&frame);
        assert_eq!(buttons.len(), 3, "the topbar paints Home, back and forward");

        for button in buttons {
            assert_eq!(
                window.click(&mut transport, &mut queue, button.center()),
                None
            );
            assert_eq!(window.library.page(), None);
        }
    }

    #[test]
    fn the_back_side_button_goes_back() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);

        let (command, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra1, true)],
        );
        assert_eq!(command, None, "back onto a ready playlist needs no load");
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned()))
        );
        assert!(
            text_in(&frame, "Mix", view_rect(&window)).is_some(),
            "the view paints the playlist in the frame the nav applies"
        );

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false)],
        );
    }

    #[test]
    fn the_forward_side_button_goes_forward() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);
        window.library.back();
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned()))
        );

        // The control frame: the predicate the next frame asserts on is live.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

        let (command, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra2, true)],
        );
        assert_eq!(command, None);
        assert_eq!(window.library.page(), Some(&Page::Album(ALBUM.to_owned())));
        // The album page draws a spinner until its load settles, and a band and
        // a table after that. Its load has only just started here, so the
        // same-frame proof that the view moved is that the playlist band is no
        // longer painted in the view's own rect. The sidebar and topbar are
        // excluded: both draw before the nav is applied.
        assert!(
            text_in(&frame, "Mix", view_rect(&window)).is_none(),
            "the view leaves the playlist in the frame the nav applies"
        );

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra2, false)],
        );
    }

    #[test]
    fn browser_back_and_forward_keys_do_the_same() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);

        let (command, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::BrowserBack, true)],
        );
        assert_eq!(command, None, "back onto a ready playlist needs no load");
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned()))
        );
        assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

        // A fresh window, so the two cases share no input state.
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);
        window.library.back();

        // The control frame: the predicate the next frame asserts on is live.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

        let (command, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::BrowserForward, true)],
        );
        assert_eq!(command, None);
        assert_eq!(window.library.page(), Some(&Page::Album(ALBUM.to_owned())));
        assert!(text_in(&frame, "Mix", view_rect(&window)).is_none());
    }

    /// Auto-repeat is intended, the way a browser behaves: a held Back key
    /// steps back once per repeat, so each frame moves exactly one page rather
    /// than draining the history in one go.
    #[test]
    fn a_held_browser_key_steps_back_once_per_frame() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        ready_playlist(&mut window);
        window.library.open(Page::Album(ALBUM.to_owned()));
        window.library.open(Page::Album(ALBUM_B.to_owned()));

        let (command, _frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![key_repeat(egui::Key::BrowserBack)],
        );
        assert_eq!(
            command,
            Some(Command::LoadAlbum {
                album: ALBUM.to_owned()
            }),
            "album B's want superseded album A's, so Back asks for A again"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Album(ALBUM.to_owned())),
            "one repeat takes one step back, not two"
        );

        let (command, _frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![key_repeat(egui::Key::BrowserBack)],
        );
        assert_eq!(command, None, "back onto a ready playlist needs no load");
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned())),
            "the next repeat takes the next step, one page per frame"
        );
    }

    #[test]
    fn side_button_back_into_a_stale_playlist_loads_it() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        stale_playlist_behind_an_album(&mut window.library);

        let (command, _frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra1, true)],
        );
        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: PLAYLIST.to_owned()
            })
        );
        assert_eq!(window.library.tracks, Load::Loading);

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false)],
        );
    }

    #[test]
    fn back_beats_forward_in_one_frame() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);

        let (command, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![
                side(egui::Pos2::ZERO, egui::PointerButton::Extra2, true),
                side(egui::Pos2::ZERO, egui::PointerButton::Extra1, true),
            ],
        );
        assert_eq!(command, None);
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned())),
            "Back is checked first, so one frame gives one action"
        );
        assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![
                side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false),
                side(egui::Pos2::ZERO, egui::PointerButton::Extra2, false),
            ],
        );
    }

    #[test]
    fn side_buttons_do_nothing_with_no_history() {
        let cases = [
            (
                side(egui::Pos2::ZERO, egui::PointerButton::Extra1, true),
                Some(side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false)),
            ),
            (
                side(egui::Pos2::ZERO, egui::PointerButton::Extra2, true),
                Some(side(egui::Pos2::ZERO, egui::PointerButton::Extra2, false)),
            ),
            (key(egui::Key::BrowserBack, true), None),
            (key(egui::Key::BrowserForward, true), None),
        ];

        for (input, release) in cases {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));

            let (command, _frame) = window.frame(&mut transport, &mut queue, vec![input]);
            assert_eq!(command, None, "there is nowhere to go");
            assert_eq!(window.library.page(), None);
            assert_eq!(window.library.tracks, Load::Idle);

            if let Some(release) = release {
                let _ = window.frame(&mut transport, &mut queue, vec![release]);
            }
        }
    }

    #[test]
    fn releases_and_other_buttons_are_ignored() {
        let cases = [
            side(egui::Pos2::ZERO, egui::PointerButton::Extra1, false),
            side(egui::Pos2::ZERO, egui::PointerButton::Primary, true),
            side(egui::Pos2::ZERO, egui::PointerButton::Secondary, true),
            side(egui::Pos2::ZERO, egui::PointerButton::Middle, true),
            key(egui::Key::Escape, true),
            key(egui::Key::ArrowLeft, true),
            key(egui::Key::BrowserBack, false),
        ];

        for input in cases {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            history_of_three(&mut window);

            let (command, _frame) = window.frame(&mut transport, &mut queue, vec![input]);
            assert_eq!(command, None);
            assert_eq!(
                window.library.page(),
                Some(&Page::Album(ALBUM.to_owned())),
                "only a press of a nav input moves the page"
            );
        }
    }

    /// The end-to-end pin of click-wins: one frame carries a primary press, a
    /// primary release and an `Extra2` press, and the click's Back must win over
    /// the side button's Forward.
    ///
    /// The `Extra2` press lands at the click's own position, which is where it
    /// would arrive on real hardware, so the frame is shaped like a real one. The
    /// second window is the control: the same events with the click on the inert
    /// search field, so only the `Extra2` press can act, and it must move
    /// Forward. That proves the side press is live in the frame where Back won,
    /// so the test cannot pass with the side buttons unwired.
    #[test]
    fn a_topbar_click_beats_a_side_button() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let back = topbar_buttons(&frame)[1];
        assert!(window.library.can_back(), "history exists, so Back is live");

        // The hover frame, so frame 3's hit test sees the button there.
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(back.center())],
        );

        let (command, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![
                crate::view::test_support::button(back.center(), true),
                crate::view::test_support::button(back.center(), false),
                side(back.center(), egui::PointerButton::Extra2, true),
            ],
        );
        assert_eq!(command, None);
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned())),
            "the click's Back wins over the side button's Forward"
        );
        assert!(text_in(&frame, "Mix", view_rect(&window)).is_some());

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![side(back.center(), egui::PointerButton::Extra2, false)],
        );

        // The liveness control: no click to win, so the same-frame `Extra2`
        // press must move Forward. The click lands on the topbar's right edge,
        // where the search field paints no widget.
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        history_of_three(&mut window);
        let inert = topbar_rect(&window).right_center() - Vec2::new(40.0, 0.0);

        let (command, _frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![
                crate::view::test_support::button(inert, true),
                crate::view::test_support::button(inert, false),
                side(inert, egui::PointerButton::Extra2, true),
            ],
        );
        assert_eq!(
            command,
            Some(Command::LoadAlbum {
                album: ALBUM_B.to_owned()
            }),
            "album A's want superseded album B's, so Forward asks for B again"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Album(ALBUM_B.to_owned())),
            "with no click to win, the same-frame side press moves Forward"
        );
    }

    #[test]
    fn the_playlists_crumb_returns_to_the_root_and_back_loads_the_playlist() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        ready_playlist(&mut window);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let crumb = text_in(&frame, "PLAYLISTS", topbar_rect(&window))
            .expect("the trail's Playlists crumb is painted");
        assert_eq!(
            window.click(&mut transport, &mut queue, crumb.center()),
            None
        );
        assert_eq!(window.library.page(), None, "the crumb returns to the root");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let back = topbar_buttons(&frame)[1];
        assert_eq!(
            window.click(&mut transport, &mut queue, back.center()),
            Some(Command::LoadTracks {
                playlist: PLAYLIST.to_owned()
            }),
            "the root cleared the wanted load, so Back reloads it"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist(PLAYLIST.to_owned()))
        );
    }

    #[test]
    fn back_into_a_playlist_whose_tracks_were_cleared_loads_it() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        window.library.apply(&Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
            uri: PLAYLIST.to_owned(),
            name: "Mix".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }]));
        window.library.select(PLAYLIST);
        window.library.to_playlists();
        assert_eq!(window.library.tracks, Load::Idle);
        assert_eq!(window.library.page(), None);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let back = topbar_buttons(&frame)[1];
        assert_eq!(
            window.click(&mut transport, &mut queue, back.center()),
            Some(Command::LoadTracks {
                playlist: PLAYLIST.to_owned()
            })
        );
    }

    #[test]
    fn the_sidebar_highlight_clears_on_an_album_page() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        ready_playlist(&mut window);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            selected_rows(&frame),
            1,
            "the current playlist is highlighted"
        );

        window.library.open(Page::Album(ALBUM.to_owned()));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            selected_rows(&frame),
            0,
            "no playlist is current on an album page"
        );
    }

    /// A history that holds a stale playlist behind an album, so Back has to
    /// start the playlist's load again.
    fn stale_playlist_behind_an_album(library: &mut Library) {
        library.apply(&Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
            uri: PLAYLIST.to_owned(),
            name: "Mix".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }]));
        library.select(PLAYLIST);
        library.apply(&Update::TracksFailed {
            playlist: PLAYLIST.to_owned(),
            failure: crate::message::Failure::Load,
        });
        library.open(Page::Album(ALBUM.to_owned()));
    }

    /// A nav can settle a page: Back onto a playlist marks its load wanted and
    /// starts it. Keeping an earlier command of the frame and dropping the nav's
    /// would leave that load marked in flight with nothing fetching it, so the
    /// page would spin for a reply nobody asked for. A losing nav must therefore
    /// not touch the library at all.
    ///
    /// Two activations in one frame need a keyboard and a pointer together, which
    /// the harness above cannot stage: egui resolves one click position per
    /// frame. So the collision is driven through the shell's nav step, which is
    /// where the fix lives.
    ///
    /// A nav from a side button or a browser key merges into the same call, so
    /// this guard covers it too. The sidebar-command case is not duplicated with
    /// a second shell frame: the shell keeps the region's command before the nav
    /// is ever looked at.
    #[test]
    fn a_nav_that_loses_the_frame_leaves_the_library_alone() {
        let mut library = Library::default();
        stale_playlist_behind_an_album(&mut library);
        let page = library.page().cloned();
        let tracks = library.tracks.clone();

        let won = Command::LoadTracks {
            playlist: ALBUM.to_owned(),
        };
        let command = apply_nav(&mut library, Some(topbar::Nav::Back), Some(won.clone()));

        assert_eq!(command, Some(won), "the earlier command keeps the frame");
        assert_eq!(
            library.page(),
            page.as_ref(),
            "the losing nav must not move the page"
        );
        assert_eq!(
            library.tracks, tracks,
            "and must not mark a load that nothing would fetch"
        );
        assert_eq!(library.page(), Some(&Page::Album(ALBUM.to_owned())));
    }

    /// The same Back, with no other command in the frame, still settles the page
    /// and reports the load it started.
    #[test]
    fn a_nav_applies_when_no_other_region_asked() {
        let mut library = Library::default();
        stale_playlist_behind_an_album(&mut library);

        assert_eq!(
            apply_nav(&mut library, Some(topbar::Nav::Back), None),
            Some(Command::LoadTracks {
                playlist: PLAYLIST.to_owned()
            })
        );
        assert_eq!(library.page(), Some(&Page::Playlist(PLAYLIST.to_owned())));
        assert_eq!(library.tracks, Load::Loading);
    }

    /// A list crumb asks for the root showing its own chip, which is the whole
    /// reason a crumb names a list rather than just "back".
    #[test]
    fn a_list_crumb_returns_to_the_root_on_its_own_chip() {
        let mut library = Library::default();
        library.select(PLAYLIST);
        library.root_filter.choose(RootKind::Albums);

        assert_eq!(
            apply_nav(
                &mut library,
                Some(topbar::Nav::Root(RootKind::Playlists)),
                None
            ),
            None
        );
        assert_eq!(library.page(), None, "the root is on screen");
        assert_eq!(library.root_filter.chosen(), RootKind::Playlists);
    }

    /// A crumb that names a page opens it.
    #[test]
    fn a_page_crumb_opens_the_page() {
        let mut library = Library::default();

        assert_eq!(
            apply_nav(
                &mut library,
                Some(topbar::Nav::Page(Page::Artist(ARTIST.to_owned()))),
                None,
            ),
            Some(Command::LoadArtist {
                artist: ARTIST.to_owned()
            })
        );
        assert_eq!(library.page(), Some(&Page::Artist(ARTIST.to_owned())));
    }

    /// Draws one frame of the whole shell at `screen` and returns the covers it
    /// asked for.
    fn requested(screen: Vec2, transport: &mut Transport, queue: &mut Queue) -> Vec<CoverUrl> {
        let mut window = Window::new(screen);
        let _ = window.frame(transport, queue, Vec::new());
        window.requests()
    }

    /// Queues two tracks, so the transport has something to pause.
    fn two_tracks(transport: &mut Transport) -> Queue {
        let tracks = vec![track(0, None), track(1, None)];
        let mut queue = Queue::default();
        queue.start(&tracks, 0, None, transport);
        queue
    }

    /// One press and its release, so the next press reads as a new one: egui
    /// marks a press with no release behind it as auto-repeat.
    fn tap(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        event: egui::Event,
    ) -> (Option<Command>, egui::FullOutput) {
        let frame = window.frame(transport, queue, vec![event]);
        let _ = window.frame(transport, queue, vec![key(egui::Key::Space, false)]);
        frame
    }

    #[test]
    fn space_pauses_and_resumes_the_transport() {
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        for (state, expected) in [
            (TransportState::Playing, Command::Pause),
            (TransportState::Paused, Command::Resume),
            (TransportState::Loading, Command::Pause),
        ] {
            transport.state = state;
            let (command, _) = tap(
                &mut window,
                &mut transport,
                &mut queue,
                key(egui::Key::Space, true),
            );
            assert_eq!(command, Some(expected), "a space on {state:?}");
        }
    }

    #[test]
    fn space_with_nothing_queued_asks_for_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        let (command, _) = tap(
            &mut window,
            &mut transport,
            &mut queue,
            key(egui::Key::Space, true),
        );
        assert_eq!(command, None, "there is no track to start");
    }

    #[test]
    fn a_held_space_does_not_thrash_the_transport() {
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        transport.state = TransportState::Playing;

        let (first, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Space, true)],
        );
        assert_eq!(first, Some(Command::Pause));

        // The same key pressed again with no release: what a held key looks like.
        let (held, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Space, true)],
        );
        assert_eq!(held, None, "a held space must not thrash the transport");
    }

    #[test]
    fn a_clicked_row_keeps_the_keyboard_and_lends_the_space_to_the_transport() {
        // A click leaves the keyboard where the pointer went, so Tab carries on
        // from the row that was picked. The space is still the transport's: the
        // row's own keys are Enter and a double click.
        let mut transport = playing(cover("abc"));
        let tracks = vec![
            track(0, Some(cover("abc"))),
            track(1, Some(cover("row1"))),
            track(2, Some(cover("row2"))),
        ];
        let mut queue = Queue::default();
        queue.start(&tracks, 0, None, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = painted_at(&frame, "t2").expect("row t2 is painted");
        let _ = window.click(&mut transport, &mut queue, at + Vec2::new(6.0, 6.0));
        assert!(
            window.ctx.memory(|memory| memory.focused().is_some()),
            "the clicked row holds the keyboard"
        );

        transport.state = TransportState::Playing;
        let (command, _) = tap(
            &mut window,
            &mut transport,
            &mut queue,
            key(egui::Key::Space, true),
        );
        assert_eq!(
            command,
            Some(Command::Pause),
            "the space is the transport's after a click on a row"
        );
    }

    #[test]
    fn a_held_space_never_reaches_a_focused_row() {
        // A row that holds focus would take egui's space click on every repeat of
        // a held key, so the shell takes the repeats away too: one pause, and
        // then nothing at all — not to the transport, and not as a row activation.
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        // A library with something to reach: a sidebar row that Tab can focus and
        // that a space would open, so a leaked repeat would show up as a command.
        ready_playlist(&mut window);
        window.library.to_playlists();
        transport.state = TransportState::Playing;

        let _ = window.frame(&mut transport, &mut queue, vec![key(egui::Key::Tab, true)]);
        assert!(
            window.ctx.memory(|memory| memory.focused().is_some()),
            "Tab has to land somewhere for this to mean anything"
        );

        let (first, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Space, true)],
        );
        assert_eq!(first, Some(Command::Pause));
        let page = window.library.page().cloned();

        for _ in 0..3 {
            let (held, _) = window.frame(
                &mut transport,
                &mut queue,
                vec![key_repeat(egui::Key::Space)],
            );
            assert_eq!(held, None, "a repeat is nobody's press");
        }
        assert_eq!(
            window.library.page().cloned(),
            page,
            "a held space must not open what it holds focus on"
        );
    }

    #[test]
    fn a_space_with_no_loaded_track_asks_for_nothing() {
        // The queue can hold entries without one of them being the transport's,
        // and the space must not pick one: it only ever toggles what is loaded.
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        transport.now_playing = None;
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        let (command, _) = tap(
            &mut window,
            &mut transport,
            &mut queue,
            key(egui::Key::Space, true),
        );
        assert_eq!(command, None, "there is nothing loaded to pause or start");
    }

    #[test]
    fn a_modified_space_is_somebody_elses_shortcut() {
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        transport.state = TransportState::Playing;

        let mut event = key(egui::Key::Space, true);
        if let egui::Event::Key { modifiers, .. } = &mut event {
            *modifiers = egui::Modifiers::COMMAND;
        }
        let (command, _) = tap(&mut window, &mut transport, &mut queue, event);
        assert_eq!(command, None, "a modifier is a shortcut of somebody else's");
    }

    #[test]
    fn a_focused_row_leaves_the_space_to_the_transport() {
        // egui turns a space into a click for whatever holds focus, so the shell
        // has to take it away before anything draws: otherwise Tab onto a row and
        // a space would start that row's track while the transport paused.
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        transport.state = TransportState::Playing;

        let _ = window.frame(&mut transport, &mut queue, vec![key(egui::Key::Tab, true)]);
        assert!(
            window.ctx.memory(|memory| memory.focused().is_some()),
            "Tab has to land somewhere for this to mean anything"
        );

        let (command, _) = tap(
            &mut window,
            &mut transport,
            &mut queue,
            key(egui::Key::Space, true),
        );
        assert_eq!(
            command,
            Some(Command::Pause),
            "the space is the transport's even on a focused row"
        );
    }

    #[test]
    fn the_now_playing_lookup_asks_for_the_large_size_at_two_x() {
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        window.ctx.set_pixels_per_point(2.0);
        let (set, small, _, large) = sized("panel");
        let mut transport = playing(set);
        let mut queue = Queue::default();

        let (_, _) = window.frame(&mut transport, &mut queue, Vec::new());
        let asked = window.requests();

        assert!(
            asked.contains(&large),
            "the panel's 512px box takes the 640: {asked:?}"
        );
        assert!(
            !asked.contains(&small),
            "the panel must not fetch the 64: {asked:?}"
        );
    }

    #[test]
    fn the_playbar_tile_is_requested_even_with_the_panel_hidden() {
        // Narrower than the panel breakpoint, so the Now Playing panel is not
        // drawn and only the playbar can want a cover — the queue's rows too.
        let mut transport = playing(cover("abc"));
        let tracks = vec![
            track(0, Some(cover("abc"))),
            track(1, Some(cover("row1"))),
            track(2, Some(cover("row2"))),
        ];
        let mut queue = Queue::default();
        queue.start(&tracks, 0, None, &mut transport);

        assert_eq!(
            requested(Vec2::new(900.0, 600.0), &mut transport, &mut queue),
            vec![cover_url("abc")],
            "a hidden panel must not ask for its rows' covers"
        );
    }

    #[test]
    fn a_visible_panel_requests_the_visible_rows_covers() {
        let mut transport = playing(cover("abc"));
        let tracks = vec![
            track(0, Some(cover("abc"))),
            track(1, Some(cover("row1"))),
            track(2, Some(cover("row2"))),
        ];
        let mut queue = Queue::default();
        queue.start(&tracks, 0, None, &mut transport);

        let mut asked = requested(Vec2::new(1400.0, 900.0), &mut transport, &mut queue);
        asked.sort_by(|left, right| left.as_str().cmp(right.as_str()));

        let mut expected = vec![cover_url("abc"), cover_url("row1"), cover_url("row2")];
        expected.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        assert_eq!(
            asked, expected,
            "the panel must ask for the rows it draws, and the playbar for its tile"
        );
    }

    /// The Lyrics tab asks for the playing track's lyrics once, and the shell is
    /// what sends it.
    #[test]
    fn opening_the_lyrics_tab_asks_for_the_lyrics_once() {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = painted_at(&frame, "LYRICS").expect("the lyrics tab is painted");
        let command = window.click(&mut transport, &mut queue, at + Vec2::new(2.0, 2.0));

        assert_eq!(
            command,
            Some(Command::LoadLyrics {
                track: "spotify:track:x".to_owned()
            }),
            "opening the tab asks for the track the transport names"
        );

        // The next frame does not ask again: the load is in flight.
        let (_, _) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            window.frame(&mut transport, &mut queue, Vec::new()).0,
            None,
            "one request per track"
        );
    }

    #[test]
    fn a_single_click_on_a_queue_row_does_not_play() {
        // A single click is a press; the shell applies presses with the drag
        // wiring, so nothing may move the transport here.
        let mut transport = playing(cover("abc"));
        let tracks = vec![
            track(0, Some(cover("abc"))),
            track(1, Some(cover("row1"))),
            track(2, Some(cover("row2"))),
        ];
        let mut queue = Queue::default();
        queue.start(&tracks, 0, None, &mut transport);

        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = painted_at(&frame, "t2").expect("row t2 is painted");

        assert_eq!(
            window.click(&mut transport, &mut queue, at + Vec2::new(6.0, 6.0)),
            None
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t0"),
            "a single click must not move the transport"
        );
    }

    #[test]
    fn a_double_click_on_a_queue_row_plays_that_entry() {
        // The panel reports the row and the shell is what moves the queue and
        // the transport, so nothing can name a track the transport never got.
        let mut transport = playing(cover("abc"));
        let tracks = vec![
            track(0, Some(cover("abc"))),
            track(1, Some(cover("row1"))),
            track(2, Some(cover("row2"))),
        ];
        let mut queue = Queue::default();
        queue.start(&tracks, 0, None, &mut transport);

        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = painted_at(&frame, "t2").expect("row t2 is painted");

        assert_eq!(
            window.double_click(&mut transport, &mut queue, at + Vec2::new(6.0, 6.0)),
            Some(Command::Play {
                uri: "t2".to_owned()
            })
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t2"),
            "the shell's jump must reach the transport, not only the queue"
        );
    }

    /// Puts a ready search page on screen, answered with `count` labelled
    /// tracks. The page is opened through `library.open`, which is the only way
    /// in while the field cannot navigate yet.
    fn answered(window: &mut Window, query: &str, count: usize) {
        let _ = window.library.open(Page::Search(query.to_owned()));
        window.library.apply(&Update::Search {
            query: query.to_owned(),
            tracks: (0..count).map(|index| track(index, None)).collect(),
        });
    }

    /// Takes the reveal row labelled `label`, which the page applies itself.
    fn reveal(window: &mut Window, transport: &mut Transport, queue: &mut Queue, label: &str) {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let at = painted_at(&frame, label).unwrap_or_else(|| panic!("{label:?} is painted"))
            + Vec2::new(4.0, 4.0);
        assert_eq!(
            window.click(transport, queue, at),
            None,
            "a reveal asks for nothing"
        );
    }

    /// Asks the search state for the read a frame wants, the way `logic` does.
    fn search_read(window: &mut Window) -> Option<Command> {
        let page = window.library.page_search_query().map(str::to_owned);
        window.library.search.request(page.as_deref())
    }

    /// The skeleton bars painted inside `area`.
    fn skeleton_rects(output: &egui::FullOutput, area: Rect) -> usize {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) => {
                    shape.fill == tokens.palette.skeleton_rule && area.contains_rect(shape.rect)
                }
                _ => false,
            })
            .count()
    }

    #[test]
    fn the_search_page_renders_its_heading_and_count() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        answered(&mut window, "never gonna", 20);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(
            painted_at(&frame, "Results for “never gonna”").is_some(),
            "the heading names the query"
        );
        assert!(
            painted_at(&frame, "TOP 20 TRACKS").is_some(),
            "the page's note states the cap once"
        );
        assert!(
            painted_at(&frame, "10 OF 20").is_some(),
            "the track block's head says how much of it is shown"
        );
        assert!(
            painted_at(&frame, "t0").is_some(),
            "the answer's tracks are the page's table"
        );
    }

    /// The page holds its shape while the read is out, says so when the answer
    /// is empty, and offers a Retry when the read failed — the three states the
    /// table has nothing to draw in.
    #[test]
    fn the_search_page_holds_skeleton_empty_and_failure_states() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = window.library.open(Page::Search("never gonna".to_owned()));

        // Idle: nothing true to draw yet, so the page holds rows open.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(painted_at(&frame, "Results for “never gonna”").is_some());
        assert!(
            skeleton_rects(&frame, view_rect(&window)) > 0,
            "a pending page holds its rows open"
        );
        assert!(
            painted_at(&frame, "t0").is_none(),
            "and draws no answer it does not have"
        );

        // Failed: the failure's own words and a Retry that asks once.
        window.library.apply(&Update::SearchFailed {
            query: "never gonna".to_owned(),
            failure: Failure::Search,
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            painted_at(&frame, crate::signin::failure_message(Failure::Search)).is_some(),
            "the failure is one muted line, not an error box"
        );
        let retry = painted_at(&frame, "Retry").expect("the failure offers a Retry");
        assert_eq!(
            window.click(&mut transport, &mut queue, retry + Vec2::new(4.0, 4.0)),
            Some(Command::Search {
                query: "never gonna".to_owned()
            }),
            "Retry is the same read, asked again"
        );
        assert_eq!(
            window.library.search.page_answer("never gonna"),
            Some(&Load::Loading)
        );

        // Empty: the page head with nothing to count, then the two-line empty
        // state, nothing to press, and no rows held open any more.
        window.library.apply(&Update::Search {
            query: "never gonna".to_owned(),
            tracks: Vec::new(),
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            painted_at(&frame, "0 TRACKS").is_some(),
            "an empty answer still says how many tracks it found"
        );
        assert!(
            painted_at(&frame, "Nothing in the catalogue matches that").is_some(),
            "an empty answer says so"
        );
        assert!(
            painted_at(&frame, "Retry").is_none(),
            "an empty answer offers nothing to retry"
        );
        assert_eq!(
            skeleton_rects(&frame, view_rect(&window)),
            0,
            "an answered page holds nothing open"
        );
    }

    #[test]
    fn a_click_on_a_result_row_queues_the_whole_answer_at_that_row() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        answered(&mut window, "never gonna", 20);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = painted_at(&frame, "t3").expect("row t3 is painted");

        assert_eq!(
            window.double_click(&mut transport, &mut queue, at + Vec2::new(6.0, 6.0)),
            Some(Command::Play {
                uri: "t3".to_owned()
            }),
            "a row plays itself"
        );
        assert_eq!(
            queue.upcoming().len(),
            16,
            "the queue is the whole answer, entered at the row"
        );
        assert_eq!(
            queue.next(&mut transport),
            Some(Command::Play {
                uri: "t4".to_owned()
            }),
            "next walks the answer, not just the row that was clicked"
        );
    }

    #[test]
    fn back_and_forward_walk_the_search_pages() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        answered(&mut window, "a", 3);
        answered(&mut window, "b", 3);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(painted_at(&frame, "Results for “b”").is_some());
        let buttons = topbar_buttons(&frame);

        // Back onto a's page: its answer was dropped when b replaced it, so the
        // page asks again — and never paints b's rows under a's heading.
        assert_eq!(
            window.click(&mut transport, &mut queue, buttons[1].center()),
            None,
            "a search page never settles with a command"
        );
        assert_eq!(
            window.library.page_search_query(),
            Some("a"),
            "Back walks the history"
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(painted_at(&frame, "Results for “a”").is_some());
        assert!(
            painted_at(&frame, "t0").is_none(),
            "another query's answer must not be drawn"
        );
        assert_eq!(
            search_read(&mut window),
            Some(Command::Search {
                query: "a".to_owned()
            })
        );
        window.library.apply(&Update::Search {
            query: "a".to_owned(),
            tracks: vec![track(0, None), track(1, None), track(2, None)],
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            painted_at(&frame, "t0").is_some(),
            "the re-asked answer is the page's table"
        );

        // Forward onto b's page, which is asked for the same way.
        let buttons = topbar_buttons(&frame);
        window.click(&mut transport, &mut queue, buttons[2].center());
        assert_eq!(window.library.page_search_query(), Some("b"));
        assert_eq!(
            search_read(&mut window),
            Some(Command::Search {
                query: "b".to_owned()
            })
        );
        window.library.apply(&Update::Search {
            query: "b".to_owned(),
            tracks: vec![track(0, None), track(1, None), track(2, None)],
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(painted_at(&frame, "Results for “b”").is_some());
    }

    /// The scroll area is salted with the query, so a new query starts at the
    /// top rather than where the last one was left.
    ///
    /// Ten rows fit the window, so a page only scrolls once its reveal is taken
    /// — which is also the press that grows it.
    #[test]
    fn a_new_query_starts_at_the_top_of_the_results() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        answered(&mut window, "a", 40);
        reveal(
            &mut window,
            &mut transport,
            &mut queue,
            "Show 30 more tracks",
        );

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let first = painted_at(&frame, "t0").expect("row t0 is painted").y;
        let top = painted_at(&frame, "t5").expect("row t5 is painted").y;
        // The wheel only reaches the area the pointer is over.
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(view_rect(&window).center())],
        );

        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -120.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        };
        // A wheel notch is smoothed over a few passes, and the page lays out
        // with the offset it was left on, so the movement shows a pass later.
        for _ in 0..4 {
            let _ = window.frame(&mut transport, &mut queue, vec![wheel.clone()]);
        }
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let scrolled = painted_at(&frame, "t5").expect("row t5 is still painted").y;
        assert!(
            scrolled < top - 50.0,
            "the wheel moves this query's page: {scrolled} against {top}"
        );

        // The offset is stored under this query's own id: another query's page
        // is a different area, so it starts at the top.
        answered(&mut window, "b", 40);
        reveal(
            &mut window,
            &mut transport,
            &mut queue,
            "Show 30 more tracks",
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let fresh = painted_at(&frame, "t5").expect("row t5 is painted").y;
        assert!(
            (fresh - top).abs() < 0.01,
            "a new query starts at the top: {fresh} against {top}"
        );

        // Back onto a, whose answer b dropped: the page is answered again and
        // its blocks start at ten, because the read the reveal belonged to is
        // gone. It is a fresh page, so it starts at the top too — not where a
        // was left, and not at the shortened page's bottom.
        assert_eq!(window.library.back(), None, "a search page never settles");
        assert_eq!(window.library.page_search_query(), Some("a"));
        window.library.apply(&Update::Search {
            query: "a".to_owned(),
            tracks: (0..40).map(|index| track(index, None)).collect(),
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(painted_at(&frame, "Results for “a”").is_some());
        assert!(
            painted_at(&frame, "Results for “b”").is_none(),
            "b's page is gone"
        );
        assert!(
            painted_at(&frame, "Show 30 more tracks").is_some(),
            "a's blocks start at ten again"
        );
        assert!(
            painted_at(&frame, "t39").is_none(),
            "so only ten of a's rows are drawn"
        );
        let back = painted_at(&frame, "t0")
            .expect("a's first row is painted")
            .y;
        assert!(
            (back - first).abs() < 0.01,
            "a page that starts fresh starts at the top: {back} against {first}"
        );
    }

    // -----------------------------------------------------------------------
    // The field, the menu and the frame order (R1–R18).
    // -----------------------------------------------------------------------

    /// The field's painted box this frame.
    fn field_box(window: &mut Window, transport: &mut Transport, queue: &mut Queue) -> Rect {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        search_field(&frame).expect("the topbar paints the search field")
    }

    /// The text edit's own rect, read from egui's widget memory. The id comes
    /// from the focused widget, so the field must have the keyboard: a painted
    /// box is where the view draws, but the text edit's own response rect is
    /// what the width contract and the pointer both read.
    fn edit_box(window: &Window) -> Rect {
        let id = window
            .ctx
            .memory(|memory| memory.focused())
            .expect("the field has the keyboard");
        window
            .ctx
            .read_response(id)
            .expect("the text edit was laid out")
            .rect
    }

    /// Clicks the field and types `query` into it, leaving the read unsent so a
    /// test can assert what it asks for.
    fn type_query(window: &mut Window, transport: &mut Transport, queue: &mut Queue, query: &str) {
        let at = field_box(window, transport, queue).center();
        let _ = window.click(transport, queue, at);
        let _ = window.frame(transport, queue, vec![egui::Event::Text(query.to_owned())]);
    }

    /// The menu's frame for a given number of rows, solved the way the view
    /// solves it.
    fn menu_box(window: &Window, field: Rect, height: f32) -> Rect {
        let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
        layout::search_menu::menu_rect(field, content, height)
    }

    /// Types `query`, sends and answers its read with `count` tracks, and leaves
    /// the menu drawn. Returns the menu's frame and that frame's output.
    fn answered_menu(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        query: &str,
        count: usize,
    ) -> (Rect, egui::FullOutput) {
        type_query(window, transport, queue, query);
        let key = crate::search::collapse(query).expect("a key");
        assert_eq!(
            search_read(window),
            Some(Command::Search { query: key.clone() }),
            "typing arms one read"
        );
        window.library.apply(&Update::Search {
            query: key,
            tracks: (0..count).map(|index| track(index, None)).collect(),
        });
        // The first frame an `Area` is shown is its sizing pass: egui draws
        // nothing and marks it invisible, then asks for the next frame. The
        // second frame is the menu the reader sees.
        let _ = window.frame(transport, queue, Vec::new());
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let field = search_field(&frame).expect("the topbar paints the field");
        let (rows, floor) = window
            .library
            .search
            .listing()
            .map(|listing| (listing.shape.rows_per_band(), listing.shape.floor))
            .expect("the answered menu has a shape");
        let height = layout::search_menu::bands_height(&rows, floor);
        (menu_box(window, field, height), frame)
    }

    /// A track on its own album, by its own artist.
    fn band_track(index: usize) -> TrackEntry {
        let mut entry = track(index, None);
        entry.artists = format!("Artist{index}");
        entry.album = Some(format!("Album{index}"));
        entry.album_uri = Some(format!("spotify:album:{index}"));
        entry.artist_uris = vec![format!("spotify:artist:{index}")];
        entry.artist_names = vec![format!("Artist{index}")];
        entry
    }

    /// Types `a`, answers it with three bands, and returns the drawn menu.
    fn banded_menu(window: &mut Window, transport: &mut Transport, queue: &mut Queue) -> Rect {
        type_query(window, transport, queue, "a");
        window.library.apply(&Update::Search {
            query: "a".to_owned(),
            tracks: (0..8).map(band_track).collect(),
        });
        let _ = window.frame(transport, queue, Vec::new());
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let field = search_field(&frame).expect("the topbar paints the field");
        let (rows, floor) = window
            .library
            .search
            .listing()
            .map(|listing| (listing.shape.rows_per_band(), listing.shape.floor))
            .expect("the answered menu has a shape");
        let height = layout::search_menu::bands_height(&rows, floor);
        menu_box(window, field, height)
    }

    fn band_row(window: &Window, menu: Rect, band: usize, row: usize) -> Rect {
        let (rows, floor) = window
            .library
            .search
            .listing()
            .map(|listing| (listing.shape.rows_per_band(), listing.shape.floor))
            .expect("the answered menu has a shape");
        layout::search_menu::place(menu, &rows, floor, window.ctx.pixels_per_point()).bands[band]
            .rows[row]
    }

    /// One keyboard press, with a release behind it so the next press is a new
    /// one rather than an auto-repeat.
    fn press(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        code: egui::Key,
    ) -> Option<Command> {
        let command = window.frame(transport, queue, vec![key(code, true)]).0;
        let _ = window.frame(transport, queue, vec![key(code, false)]);
        command
    }

    /// Q2's executor check: zeroing the caret's stroke width removes its pixels.
    /// epaint treats a zero-width stroke as empty and paints nothing, so the
    /// marked row's rule is the only thing saying where Enter lands.
    #[test]
    fn a_marked_row_hides_the_caret() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        // The caret blinks; a paused cursor is what a single frame can assert.
        window
            .ctx
            .all_styles_mut(|style| style.visuals.text_cursor.blink = false);
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

        let caret = |frame: &egui::FullOutput| {
            frame
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::LineSegment { stroke, .. } => Some(stroke.width),
                    _ => None,
                })
        };
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let visible = caret(&frame).expect("a focused field paints its caret");
        assert!(visible > 0.0, "an unmarked field shows the caret");

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            window.ctx.text_edit_focused(),
            "the field still has the keyboard while a row is marked"
        );
        assert_eq!(
            caret(&frame),
            Some(0.0),
            "a marked row hides the caret, so the rule is the only target"
        );
    }

    #[test]
    fn typing_focuses_the_field_and_arms_one_read() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        type_query(&mut window, &mut transport, &mut queue, "a");
        assert!(
            window.ctx.text_edit_focused(),
            "typing reaches the field only when the field has the keyboard"
        );
        assert_eq!(window.library.search.text, "a");
        assert_eq!(
            search_read(&mut window),
            Some(Command::Search {
                query: "a".to_owned()
            }),
            "the first character is a read (R6)"
        );

        // Focus alone is not a query: the click opens nothing and asks nothing.
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let at = field_box(&mut window, &mut transport, &mut queue).center();
        let _ = window.click(&mut transport, &mut queue, at);
        assert!(window.ctx.text_edit_focused());
        assert!(
            !window.library.search.is_open(),
            "focus alone opens nothing"
        );
        assert_eq!(search_read(&mut window), None);
    }

    #[test]
    fn an_answer_draws_its_capped_track_rows_with_no_mark() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

        assert_eq!(
            window.library.search.mark(),
            None,
            "the menu opens with its first row unmarked (R1)"
        );
        for index in 0..3 {
            assert!(
                text_in(&frame, &format!("t{index}"), menu).is_some(),
                "row {index} is drawn in the menu"
            );
        }
        assert!(
            text_in(&frame, "t3", menu).is_none(),
            "the tracks band shows three of the answer"
        );
    }

    #[test]
    fn down_and_enter_play_the_marked_row_and_queue_the_whole_answer() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

        // A non-zero row, so a zeroed index cannot pass for the marked one.
        for _ in 0..2 {
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::ArrowDown, true)],
            );
        }
        assert_eq!(window.library.search.mark(), Some(1));

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
            Some(Command::Play {
                uri: "t1".to_owned()
            })
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t1")
        );
        assert_eq!(
            queue.upcoming().len(),
            6,
            "the queue is the whole answer, entered at the hit (R16)"
        );
        assert_eq!(
            queue.next(&mut transport),
            Some(Command::Play {
                uri: "t2".to_owned()
            }),
            "next walks the answer from the marked row"
        );
        assert!(
            !window.library.search.is_open(),
            "an applied action closes it"
        );
    }

    #[test]
    fn up_from_the_field_marks_the_last_drawn_row() {
        for (count, expected) in [(3, 2), (4, 3)] {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", count);
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::ArrowUp, true)],
            );
            assert_eq!(
                window.library.search.mark(),
                Some(expected),
                "with {count} results the last row is {expected}"
            );
        }
    }

    #[test]
    fn typing_over_a_marked_row_returns_to_the_field() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        assert_eq!(window.library.search.mark(), Some(0));

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::Text("b".to_owned())],
        );
        assert_eq!(window.library.search.text, "ab");
        assert_eq!(window.library.search.mark(), None, "R2 puts the rule back");
        assert!(window.library.search.is_open());
    }

    #[test]
    fn escape_is_two_steps_and_the_second_one_is_the_searchs() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        assert_eq!(window.library.search.mark(), Some(0));

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
            None
        );
        assert_eq!(window.library.search.text, "a", "the query is kept (R3)");
        assert!(window.library.search.is_open(), "and the menu stays up");
        assert_eq!(window.library.search.mark(), None);

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
            None
        );
        assert_eq!(window.library.search.text, "");
        assert!(!window.library.search.is_open());
    }

    #[test]
    fn the_drawers_ring_never_crosses_the_rails_edge() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(800.0, 900.0));
        window.library.apply(&Update::Playlists(vec![
            PlaylistEntry {
                composed: Vec::new(),
                uri: "spotify:playlist:a".to_owned(),
                name: "A".to_owned(),
                named: true,
                cover: None,
                description: None,
                owner: None,
                tracks: None,
                access: None,
            },
            PlaylistEntry {
                composed: Vec::new(),
                uri: "spotify:playlist:b".to_owned(),
                name: "B".to_owned(),
                named: true,
                cover: None,
                description: None,
                owner: None,
                tracks: None,
                access: None,
            },
        ]));

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let toggle = topbar_buttons(&frame)[0];
        let _ = window.click(&mut transport, &mut queue, toggle.center());
        assert_eq!(window.drawer, Drawer::Open);

        let drawer = drawer_rect(&window);
        let mut row = None;
        for _ in 0..32 {
            let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Tab);
            let Some(focused) = window.ctx.memory(|memory| memory.focused()) else {
                continue;
            };
            let Some(rect) = window.ctx.read_response(focused).map(|drawn| drawn.rect) else {
                continue;
            };
            if drawer.contains_rect(rect) && (rect.height() - sidebar::ROW_H).abs() < 0.6 {
                row = Some(rect);
                break;
            }
        }
        let row = row.expect("Tab reached a rail row in the drawer");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let rings = crate::view::test_support::rings(&frame, &Tokens::DARK);
        assert!(!rings.is_empty(), "the tabbed rail row wears the ring");
        for ring in &rings {
            assert!(
                drawer.contains_rect(*ring),
                "a ring crossed the drawer's edge: {ring:?} in {drawer:?}"
            );
        }
        assert!(drawer.contains_rect(row), "the ringed row left the drawer");
    }

    #[test]
    fn escape_belongs_to_the_drawer_only_when_search_has_nothing_to_undo() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(800.0, 900.0));

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let toggle = topbar_buttons(&frame)[0];
        let _ = window.click(&mut transport, &mut queue, toggle.center());
        assert_eq!(window.drawer, Drawer::Open);
        assert!(
            !window.ctx.text_edit_focused(),
            "the toggle, not the field, has the keyboard"
        );

        // A held key is a repeat in egui, so every press below is released: the
        // field ignores a repeat's Escape, the way it ignores a held Enter.
        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
            None
        );
        assert_eq!(
            window.drawer,
            Drawer::Closed,
            "an empty, closed search leaves Escape to the drawer"
        );

        // The other way round: an open drawer and an active search at once,
        // reached by the keyboard so no click closes the drawer on the way.
        let _ = window.click(&mut transport, &mut queue, toggle.center());
        assert_eq!(window.drawer, Drawer::Open);
        for _ in 0..24 {
            if window.ctx.text_edit_focused() {
                break;
            }
            let _ = window.frame(&mut transport, &mut queue, vec![key(egui::Key::Tab, true)]);
        }
        assert!(
            window.ctx.text_edit_focused(),
            "Tab reaches the search field"
        );
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::Text("a".to_owned())],
        );
        assert!(window.library.search.is_open());

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
            None
        );
        assert_eq!(window.library.search.text, "");
        assert!(!window.library.search.is_open());
        assert_eq!(
            window.drawer,
            Drawer::Open,
            "search consumed the Escape it acted on"
        );

        // With nothing left to undo, the next Escape is the drawer's again.
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
        assert_eq!(window.drawer, Drawer::Closed);
    }

    #[test]
    fn a_space_typed_in_the_field_is_not_the_transports() {
        let mut transport = Transport::default();
        let mut queue = two_tracks(&mut transport);
        transport.state = TransportState::Playing;
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        let at = field_box(&mut window, &mut transport, &mut queue).center();
        let _ = window.click(&mut transport, &mut queue, at);
        assert!(window.ctx.text_edit_focused());

        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![
                key(egui::Key::Space, true),
                egui::Event::Text(" ".to_owned()),
            ],
        );
        assert_eq!(command, None, "a space in the field is a character");
        assert_eq!(
            transport.state,
            TransportState::Playing,
            "and it does not pause the transport"
        );
        assert_eq!(window.library.search.text, " ");
    }

    #[test]
    fn the_floor_appears_only_when_the_answer_is_longer_than_the_menu() {
        for (count, floor) in [(3, false), (4, true)] {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", count);
            let label = "See more results for “a”";
            assert_eq!(
                text_in(&frame, label, menu).is_some(),
                floor,
                "an answer of {count} draws the floor: {floor}"
            );
            let (rows, floor) = window
                .library
                .search
                .listing()
                .map(|listing| (listing.shape.rows_per_band(), listing.shape.floor))
                .expect("the answered menu has a shape");
            assert_eq!(
                menu.height(),
                layout::search_menu::bands_height(&rows, floor),
                "and the frame is exactly its bands"
            );
        }
    }

    /// Press a row, then release it: the menu must survive the frame in which
    /// the press surrenders the field's focus (Q4). Both outcomes are staged:
    /// the natural one, and the opposite one forced between press and release.
    #[test]
    fn a_row_click_plays_under_both_focus_outcomes() {
        for keep_focus in [false, true] {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
            let at = text_in(&frame, "t1", menu)
                .expect("row t1 is drawn")
                .center();
            let field = window.ctx.memory(|memory| memory.focused());

            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![egui::Event::PointerMoved(at)],
            );
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![crate::view::test_support::button(at, true)],
            );

            match (keep_focus, field) {
                (true, Some(id)) => window.ctx.memory_mut(|memory| memory.request_focus(id)),
                (false, _) => {
                    if let Some(id) = window.ctx.memory(|memory| memory.focused()) {
                        window.ctx.memory_mut(|memory| memory.surrender_focus(id));
                    }
                }
                (true, None) => panic!("the field had the keyboard before the press"),
            }
            assert!(window.library.search.is_open(), "the press keeps it open");

            let (command, _) = window.frame(
                &mut transport,
                &mut queue,
                vec![crate::view::test_support::button(at, false)],
            );
            assert_eq!(
                command,
                Some(Command::Play {
                    uri: "t1".to_owned()
                }),
                "a click plays the row with keep_focus = {keep_focus}"
            );
            assert_eq!(
                queue.upcoming().len(),
                6,
                "the queue is the whole answer, entered at the clicked row ({keep_focus})"
            );
            assert_eq!(
                queue.next(&mut transport),
                Some(Command::Play {
                    uri: "t2".to_owned()
                }),
                "next walks the answer from the clicked row ({keep_focus})"
            );
        }
    }

    #[test]
    fn a_press_on_one_row_and_a_release_on_another_play_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let one = text_in(&frame, "t1", menu).expect("row t1").center();
        let other = text_in(&frame, "t2", menu).expect("row t2").center();

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(one)],
        );
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![crate::view::test_support::button(one, true)],
        );
        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![crate::view::test_support::button(other, false)],
        );
        assert_eq!(command, None, "two rows are not one click");
        assert!(transport.now_playing.is_none());
        assert!(
            window.library.search.is_open(),
            "the release is still engaged"
        );

        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            !window.library.search.is_open(),
            "the next idle frame closes it"
        );
    }

    #[test]
    fn a_click_outside_both_closes_the_menu() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        assert!(window.library.search.is_open());

        let _ = window.click(&mut transport, &mut queue, view_rect(&window).center());
        assert!(!window.library.search.is_open());
    }

    #[test]
    fn tabbing_out_of_the_field_closes_the_menu() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        assert!(window.ctx.text_edit_focused());

        let _ = window.frame(&mut transport, &mut queue, vec![key(egui::Key::Tab, true)]);
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(!window.ctx.text_edit_focused(), "Tab leaves the text edit");
        assert!(!window.library.search.is_open());
    }

    #[test]
    fn enter_on_the_field_opens_the_results_page() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
            None,
            "a search page never settles with a command"
        );
        assert_eq!(window.library.page(), Some(&Page::Search("a".to_owned())));
        assert!(!window.library.search.is_open());
        assert_eq!(
            window.library.search.page_answer("a"),
            Some(&Load::Ready(
                (0..8).map(|index| track(index, None)).collect()
            )),
            "a ready menu seeds the page without a second read"
        );
        assert_eq!(search_read(&mut window), None);
    }

    #[test]
    fn enter_after_a_failed_menu_asks_exactly_once() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        type_query(&mut window, &mut transport, &mut queue, "a");
        assert_eq!(
            search_read(&mut window),
            Some(Command::Search {
                query: "a".to_owned()
            })
        );
        window.library.apply(&Update::SearchFailed {
            query: "a".to_owned(),
            failure: Failure::Search,
        });
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
        assert_eq!(window.library.page(), Some(&Page::Search("a".to_owned())));
        assert_eq!(
            search_read(&mut window),
            Some(Command::Search {
                query: "a".to_owned()
            }),
            "Enter on a failed menu is the retry"
        );
        assert_eq!(search_read(&mut window), None, "asked exactly once");
    }

    #[test]
    fn a_dropped_action_leaves_the_menu_open_and_the_queue_untouched() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        stale_playlist_behind_an_album(&mut window.library);
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        assert_eq!(window.library.search.mark(), Some(0));

        // Enter plays the marked row, but a browser Back in the same frame
        // takes the frame's one command: the action must be dropped whole.
        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![
                key(egui::Key::Enter, true),
                key(egui::Key::BrowserBack, true),
            ],
        );
        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: PLAYLIST.to_owned()
            }),
            "the nav kept the frame"
        );
        assert!(transport.now_playing.is_none(), "the queue was not touched");
        assert_eq!(queue.upcoming().len(), 0);
        assert!(window.library.search.is_open(), "the menu stays open");
    }

    #[test]
    fn a_held_enter_plays_once() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );

        let (first, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Enter, true)],
        );
        assert_eq!(
            first,
            Some(Command::Play {
                uri: "t0".to_owned()
            })
        );
        let (held, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key_repeat(egui::Key::Enter)],
        );
        assert_eq!(held, None, "an auto-repeat is nobody's press");
        // If the repeat were read as a press it would be the field's Enter, which
        // opens the page: the two assertions below are what the inversion breaks.
        assert_eq!(
            window.library.page(),
            None,
            "a held Enter plays once and opens no page"
        );
        assert!(
            window.ctx.text_edit_focused(),
            "and it must not blur the field it was pressed in"
        );
    }

    #[test]
    fn the_fields_width_never_moves() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));

        // The text edit's own rect, not the painted box: the box is drawn at a
        // width nothing can change, so it could never fail this.
        let at = field_box(&mut window, &mut transport, &mut queue).center();
        let _ = window.click(&mut transport, &mut queue, at);
        let empty = edit_box(&window);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::Text("a".to_owned())],
        );
        let typed = edit_box(&window);

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
        let cleared = edit_box(&window);

        assert_eq!(empty, typed, "typing does not resize the field (R4)");
        assert_eq!(empty, cleared, "clearing does not resize it either (R5)");
    }

    #[test]
    fn home_and_back_leave_no_stale_open_menu() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        ready_playlist(&mut window);
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        assert!(window.library.search.is_open());

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let buttons = topbar_buttons(&frame);
        let _ = window.click(&mut transport, &mut queue, buttons[0].center());
        assert!(!window.library.search.is_open(), "Home leaves the field");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let buttons = topbar_buttons(&frame);
        let _ = window.click(&mut transport, &mut queue, buttons[1].center());
        assert!(
            !window.library.search.is_open(),
            "and Back does not reopen it"
        );
        assert_eq!(window.library.search.text, "a");
    }

    #[test]
    fn the_clear_control_empties_the_query_without_moving_the_field() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let field = search_field(&frame).expect("the field is painted");
        // Inside the padded hit rect, which spans the field's height at its right
        // padding (R5).
        let at = egui::Pos2::new(field.right() - 16.0, field.center().y);
        // The text edit's own rect, which the clear control must not move.
        let before = edit_box(&window);

        let _ = window.click(&mut transport, &mut queue, at);
        assert_eq!(window.library.search.text, "");
        assert!(
            !window.library.search.is_open(),
            "clearing closes the menu the query kept open"
        );
        assert!(
            window.ctx.text_edit_focused(),
            "the keyboard goes back to the field"
        );
        assert_eq!(
            edit_box(&window),
            before,
            "clearing never changes the field's width (R5)"
        );
    }

    #[test]
    fn the_menu_says_no_matches_and_a_failure_in_one_muted_line() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        type_query(&mut window, &mut transport, &mut queue, "a");
        assert_eq!(
            search_read(&mut window),
            Some(Command::Search {
                query: "a".to_owned()
            })
        );
        window.library.apply(&Update::Search {
            query: "a".to_owned(),
            tracks: Vec::new(),
        });
        // The first frame an `Area` is shown sizes it; the second draws it.
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let field = search_field(&frame).expect("the field is painted");
        let menu = menu_box(&window, field, layout::search_menu::line_height());
        assert!(
            text_in(&frame, "No matches for “a”", menu).is_some(),
            "an empty answer is one line naming the query (R13)"
        );

        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        type_query(&mut window, &mut transport, &mut queue, "a");
        let _ = search_read(&mut window);
        window.library.apply(&Update::SearchFailed {
            query: "a".to_owned(),
            failure: Failure::Search,
        });
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let field = search_field(&frame).expect("the field is painted");
        let menu = menu_box(&window, field, layout::search_menu::line_height());
        assert!(
            text_in(
                &frame,
                crate::signin::failure_message(Failure::Search),
                menu
            )
            .is_some(),
            "a failed read is one line with its own words (R13)"
        );
    }

    #[test]
    fn enter_on_the_loaded_row_pauses_instead_of_starting_it_again() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

        // Play row 1, then reopen the menu on the same query and mark it again.
        for _ in 0..2 {
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::ArrowDown, true)],
            );
        }
        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
            Some(Command::Play {
                uri: "t1".to_owned()
            })
        );
        let queued = queue.upcoming().len();

        // The arrows re-open the closed menu and mark the rows it draws.
        for _ in 0..2 {
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::ArrowDown, true)],
            );
        }
        assert!(window.library.search.is_open(), "an arrow re-opens it (R1)");
        assert_eq!(window.library.search.mark(), Some(1));
        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
            Some(Command::Pause),
            "the loaded row's action is pause, not a second start (R10)"
        );
        assert_eq!(queue.upcoming().len(), queued, "pausing keeps the queue");
        assert_eq!(window.library.search.text, "a", "and keeps the query");
    }

    #[test]
    fn an_arrow_reopens_a_closed_menu_on_the_row_it_would_have_marked() {
        // Down: the first row.
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
        assert!(!window.library.search.is_open());
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        assert!(window.library.search.is_open());
        assert_eq!(window.library.search.mark(), Some(0));

        // Up: the last drawn row, which is the floor when it is drawn.
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
        assert!(!window.library.search.is_open());
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowUp, true)],
        );
        assert!(window.library.search.is_open());
        assert_eq!(window.library.search.mark(), Some(3), "the floor is last");
    }

    /// R2, for the whole painted box: the text edit is the field, so a click on
    /// the magnifier or the padding beside it focuses the field and re-opens the
    /// menu the query still holds. Before the text edit covered the box, those
    /// clicks were "elsewhere": the field blurred, `settle` closed the menu, and
    /// `clicked_field` never fired.
    #[test]
    fn a_click_on_the_magnifier_or_the_left_padding_is_a_click_on_the_field() {
        let pad = space::S3 + icon::Size::MD.side / 2.0;
        let cases = [("the magnifier", pad), ("the left padding", space::S1)];
        for (name, dx) in cases {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            let (_, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
            let field = search_field(&frame).expect("the field is painted");

            // Enter opens the page: the menu closes, the query stays, and the
            // keyboard leaves the field with the menu.
            let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
            assert_eq!(window.library.page(), Some(&Page::Search("a".to_owned())));
            assert!(!window.library.search.is_open());
            assert!(!window.ctx.text_edit_focused());

            let at = egui::Pos2::new(field.left() + dx, field.center().y);
            let _ = window.click(&mut transport, &mut queue, at);
            assert!(
                window.ctx.text_edit_focused(),
                "a click on {name} focuses the field"
            );
            assert!(
                window.library.search.is_open(),
                "and a click on {name} re-opens the menu the query holds (R2)"
            );

            // The reason those points are the field's: the text edit is the
            // painted box, not a narrower widget inside it.
            let edit = edit_box(&window);
            assert_eq!(
                edit.left(),
                field.left(),
                "the text edit starts at the edge"
            );
            assert_eq!(edit.right(), field.right(), "and ends at the other one");
        }
    }

    /// R9: the pointer's tick is a hint on a row that is not the mark. Moving
    /// the pointer makes it the last device; it never takes the keyboard's rule
    /// with it.
    #[test]
    fn the_pointer_tick_skips_the_marked_row() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (menu, _frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let tokens = Tokens::DARK;
        let ticks = |frame: &egui::FullOutput| {
            frame
                .shapes
                .iter()
                .filter(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape) => {
                        shape.fill == tokens.palette.text_muted
                            && (shape.rect.width() - stroke::RULE).abs() < 0.01
                            && (shape.rect.height() - space::S4).abs() < 0.01
                            && menu.contains_rect(shape.rect)
                    }
                    _ => false,
                })
                .count()
        };

        // The pointer arrives over the floor: it owns the highlight, so the
        // floor takes the row hover wash and the tick.
        let third = layout::search_menu::row_rect(menu, 3).center();
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(third)],
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(window.library.search.device(), SearchDevice::Pointer);
        assert_eq!(ticks(&frame), 1, "the pointed-at row takes the tick");

        // The keyboard marks the row the pointer is on: the mark's rule is the
        // answer now, so the pointer stops drawing its tick there.
        for _ in 0..4 {
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::ArrowDown, true)],
            );
        }
        assert_eq!(window.library.search.mark(), Some(3));
        assert_eq!(window.library.search.device(), SearchDevice::Keyboard);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            ticks(&frame),
            0,
            "the marked row keeps the rule and takes no tick"
        );

        // The pointer moves on: the device switches back and the new row takes
        // the tick beside the mark, which does not move.
        let second = layout::search_menu::row_rect(menu, 2).center();
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(second)],
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(window.library.search.device(), SearchDevice::Pointer);
        assert_eq!(
            window.library.search.mark(),
            Some(3),
            "the pointer moves no mark"
        );
        assert_eq!(
            ticks(&frame),
            1,
            "a row that is not the mark takes the tick"
        );
    }

    #[test]
    fn a_click_on_an_album_row_opens_the_album_page() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let menu = banded_menu(&mut window, &mut transport, &mut queue);
        let album = band_row(&window, menu, 1, 0).center();
        let _ = window.click(&mut transport, &mut queue, album);
        assert_eq!(
            window.library.page(),
            Some(&Page::Album("spotify:album:0".to_owned()))
        );
        assert!(
            !window.library.search.is_open(),
            "the album closes the menu"
        );
    }

    #[test]
    fn a_click_on_an_artist_row_opens_the_artist_page() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let menu = banded_menu(&mut window, &mut transport, &mut queue);
        let artist = band_row(&window, menu, 2, 0).center();
        let _ = window.click(&mut transport, &mut queue, artist);
        assert_eq!(
            window.library.page(),
            Some(&Page::Artist("spotify:artist:0".to_owned()))
        );
        assert!(
            !window.library.search.is_open(),
            "the artist closes the menu"
        );
    }

    #[test]
    fn enter_on_an_album_and_an_artist_opens_the_same_pages() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = banded_menu(&mut window, &mut transport, &mut queue);
        for _ in 0..4 {
            let _ = press(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::ArrowDown,
            );
        }
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
        assert_eq!(
            window.library.page(),
            Some(&Page::Album("spotify:album:0".to_owned()))
        );

        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = banded_menu(&mut window, &mut transport, &mut queue);
        for _ in 0..6 {
            let _ = press(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::ArrowDown,
            );
        }
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
        assert_eq!(
            window.library.page(),
            Some(&Page::Artist("spotify:artist:0".to_owned()))
        );
    }

    /// R12: the floor row is an action, and a click on it opens the page — the
    /// same page Enter on the floor opens.
    #[test]
    fn a_click_on_the_floor_opens_the_results_page() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (menu, frame) = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        let (rows, capped) = window
            .library
            .search
            .listing()
            .map(|listing| (listing.shape.rows_per_band(), listing.shape.floor))
            .expect("the answered menu has a shape");
        let floor = layout::search_menu::place(menu, &rows, capped, window.ctx.pixels_per_point())
            .floor
            .expect("the answer is long enough to draw the floor");
        assert!(
            text_in(&frame, "See more results for “a”", menu).is_some(),
            "the answer is long enough to draw the floor"
        );

        assert_eq!(
            window.click(&mut transport, &mut queue, floor.center()),
            None,
            "a search page never settles with a command"
        );
        assert_eq!(window.library.page(), Some(&Page::Search("a".to_owned())));
        assert!(
            !window.library.search.is_open(),
            "the floor closes the menu"
        );
    }

    /// Finding 6: a nav that takes the frame's command drops `OpenResults`, and
    /// a dropped action leaves the menu open *and* the keyboard in the field.
    #[test]
    fn a_dropped_open_results_leaves_the_field_focused() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        stale_playlist_behind_an_album(&mut window.library);
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);
        // Enter with the field marked, not on a row: it asks for the page.
        assert!(window.ctx.text_edit_focused());

        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![
                key(egui::Key::Enter, true),
                key(egui::Key::BrowserBack, true),
            ],
        );
        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: PLAYLIST.to_owned()
            }),
            "the nav kept the frame"
        );
        assert_ne!(
            window.library.page(),
            Some(&Page::Search("a".to_owned())),
            "the page was not opened"
        );
        assert!(window.library.search.is_open(), "the menu stays open");
        assert!(
            window.ctx.text_edit_focused(),
            "and a dropped OpenResults does not surrender the field"
        );
    }

    /// R10 with the shared hold decision: the loaded row's action follows the
    /// transport's state, exactly as its glyph does. A paused track resumes; an
    /// ended one is restarted, because pausing it would do nothing.
    #[test]
    fn enter_resumes_a_paused_row_and_restarts_a_stopped_one() {
        for (state, expected) in [
            (TransportState::Paused, Command::Resume),
            (
                TransportState::Stopped,
                Command::Play {
                    uri: "t1".to_owned(),
                },
            ),
        ] {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 8);

            // Load row 1, then put the transport in the state under test and
            // mark the loaded row again from the closed menu.
            for _ in 0..2 {
                let _ = window.frame(
                    &mut transport,
                    &mut queue,
                    vec![key(egui::Key::ArrowDown, true)],
                );
            }
            let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
            assert_eq!(
                transport.now_playing.as_ref().map(|now| now.uri.as_str()),
                Some("t1")
            );
            transport.state = state;
            for _ in 0..2 {
                let _ = window.frame(
                    &mut transport,
                    &mut queue,
                    vec![key(egui::Key::ArrowDown, true)],
                );
            }
            assert_eq!(window.library.search.mark(), Some(1));

            assert_eq!(
                press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
                Some(expected.clone()),
                "{state:?}"
            );
            if state == TransportState::Stopped {
                assert_eq!(
                    queue.upcoming().len(),
                    6,
                    "a restart enters the whole answer at the row"
                );
                assert_eq!(
                    queue.next(&mut transport),
                    Some(Command::Play {
                        uri: "t2".to_owned()
                    }),
                    "and next walks the answer from there"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // The queue's selection, its drags, the ghost, and the Alt keys.
    // -----------------------------------------------------------------------

    /// One queue row's pitch.
    const QUEUE_ROW_H: f32 = crate::layout::queue::ROW_H;

    /// A queue of `count` tracks, started on the first.
    fn queue_of(count: usize, transport: &mut Transport) -> Queue {
        let tracks: Vec<TrackEntry> = (0..count).map(|index| track(index, None)).collect();
        let mut queue = Queue::default();
        queue.start(&tracks, 0, None, transport);
        queue
    }

    /// The sidebar column's content box: the panel less its hairline.
    fn sidebar_rect(window: &Window) -> Rect {
        let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
        let regions = layout::regions_for(content.width());
        let shell = layout::shell_for(&regions);
        layout::grid_for(content, &shell)
            .sidebar
            .shrink(stroke::HAIRLINE)
    }

    /// The rows the sidebar holds, in the order it holds them.
    fn sidebar_order(window: &Window) -> Vec<String> {
        match &window.library.playlists {
            Load::Ready(entries) => entries.iter().map(|entry| entry.name.clone()).collect(),
            Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
        }
    }

    /// Puts `rows` in the sidebar, each read at `access`.
    ///
    /// The list is written directly so a page already on screen survives it: a
    /// `Playlists` update prunes a page whose playlist is not in it.
    fn sidebar_playlists(
        window: &mut Window,
        rows: &[(&str, Option<usize>)],
        access: Option<PlaylistAccess>,
    ) {
        window.library.playlists = Load::Ready(
            rows.iter()
                .map(|(name, tracks)| PlaylistEntry {
                    composed: Vec::new(),
                    uri: format!("playlist:{name}"),
                    name: (*name).to_owned(),
                    named: true,
                    cover: None,
                    description: None,
                    owner: None,
                    tracks: *tracks,
                    access: access.clone(),
                })
                .collect(),
        );
        if let Some(access) = access {
            window.library.apply(&Update::RootlistAccess(access));
        }
    }

    fn rootlist_access(editable: bool) -> PlaylistAccess {
        PlaylistAccess {
            revision: revision("root-1"),
            editable,
        }
    }

    /// A page whose rows are real track URIs, so one can be added to a playlist.
    fn page_of_tracks(window: &mut Window, uris: &[&str]) {
        window.library.apply(&Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
            uri: PLAYLIST.to_owned(),
            name: "Mix".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }]));
        window.library.select(PLAYLIST);
        window.library.apply(&Update::Tracks {
            playlist: PLAYLIST.to_owned(),
            tracks: uris
                .iter()
                .map(|uri| TrackEntry {
                    uri: (*uri).to_owned(),
                    title: (*uri).to_owned(),
                    ..track(0, None)
                })
                .collect(),
            cover: None,
            access: page_access(true),
        });
    }

    const TRACK_A: &str = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";
    const TRACK_B: &str = "spotify:track:7ouMYWpwJ422jRcDASZB7P";

    /// The top of the sidebar row painted as `title`, from the two lines it
    /// draws. A row centres its name and kind in itself, and that offset is the
    /// same for every row, so one row's lines measure all of them.
    fn sidebar_row_top(output: &egui::FullOutput, area: Rect, title: &str, kind: &str) -> f32 {
        let name = text_in(output, title, area)
            .unwrap_or_else(|| panic!("{title} is painted in the sidebar"));
        let kind = text_in(output, kind, area)
            .unwrap_or_else(|| panic!("{kind} is painted in the sidebar"));
        let inset = sidebar::ROW_H / 2.0 - (name.height() + kind.height()) / 2.0;
        name.top() - inset
    }

    /// A point at the gap `at` of the sidebar's rows: 0 is above the first row,
    /// `titles.len()` below the last.
    fn sidebar_boundary(
        window: &Window,
        output: &egui::FullOutput,
        titles: &[&str],
        kind: &str,
        at: usize,
    ) -> egui::Pos2 {
        let area = sidebar_rect(window);
        let pitch = sidebar::ROW_H + space::PX;
        let last = titles.len() - 1;
        let y = if at > last {
            sidebar_row_top(output, area, titles[last], kind) + pitch
        } else {
            sidebar_row_top(output, area, titles[at], kind)
        };
        egui::pos2(area.left() + 100.0, y)
    }

    /// The rail filter's box, read from the hairline under it inside `area`.
    fn rail_filter_rect(output: &egui::FullOutput, area: Rect) -> Rect {
        let tokens = Tokens::DARK;
        // The lowest match: the drawer is drawn over the topbar, and the
        // topbar's search field can fall inside the drawer's own box.
        let (hairline, _) = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                        && shape.rect.width() > 80.0
                        && shape.rect.width() < 200.0
                        && area.contains_rect(shape.rect)
                        && (shape.fill == tokens.palette.border
                            || shape.fill == tokens.palette.text) =>
                {
                    Some((shape.rect, shape.fill))
                }
                _ => None,
            })
            .max_by(|left, right| left.0.bottom().total_cmp(&right.0.bottom()))
            .expect("the rail filter paints its hairline");
        Rect::from_min_max(
            egui::pos2(hairline.left(), hairline.bottom() - extent::BUTTON),
            egui::pos2(hairline.right(), hairline.bottom()),
        )
    }

    /// The rect the widget holding the keyboard was laid out in.
    fn focused_stop(window: &Window) -> Option<Rect> {
        let focused = window.ctx.memory(|memory| memory.focused())?;
        window.ctx.read_response(focused).map(|drawn| drawn.rect)
    }

    /// Points the keyboard at `field`, settling the frame egui needs before the
    /// field owns a key: it stores a widget's key filter only for a widget that
    /// had focus last frame, so the Escape immediately after a click is not yet
    /// the field's.
    fn focus_field(window: &mut Window, transport: &mut Transport, queue: &mut Queue, field: Rect) {
        let _ = window.click(transport, queue, field.center());
        let _ = window.frame(transport, queue, Vec::new());
        assert!(
            window.ctx.text_edit_focused(),
            "the click focused a filter field"
        );
    }

    /// The two fields' boxes, read from one frame at a width where both are
    /// drawn.
    fn field_rects(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> (Rect, Rect) {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let rail = rail_filter_rect(&frame, sidebar_rect(window));
        // `page_filter_field` answers the field's bottom hairline; the field is
        // one button tall above it.
        let hairline = page_filter_field(&frame, view_rect(window))
            .expect("the page's filter field is painted");
        let page = Rect::from_min_max(
            egui::pos2(hairline.left(), hairline.bottom() - extent::BUTTON),
            egui::pos2(hairline.right(), hairline.bottom()),
        );
        (rail, page)
    }

    /// A playlist page open with rows to filter, and playlists in the rail, at a
    /// width where both fields are drawn. The returned rects are the two fields.
    fn both_fields(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> (Rect, Rect) {
        playlist_page(window, &["keep0", "p1"]);
        sidebar_playlists(
            window,
            &[
                ("Midnight City", None),
                ("Outro", None),
                ("Midnight Sun", None),
            ],
            // The create button is a Tab stop only while it is live, and it is
            // live only when the rootlist allows a write.
            Some(rootlist_access(true)),
        );
        field_rects(window, transport, queue)
    }

    /// The same two fields in a library that cannot create a playlist: with no
    /// rootlist revision the head's `+` is inert, so it is no Tab stop.
    fn both_fields_without_create(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> (Rect, Rect) {
        playlist_page(window, &["keep0", "p1"]);
        sidebar_playlists(
            window,
            &[
                ("Midnight City", None),
                ("Outro", None),
                ("Midnight Sun", None),
            ],
            None,
        );
        field_rects(window, transport, queue)
    }

    /// Every Tab stop from the first press until the page's field takes the
    /// keyboard, named by the part of the shell it sits in.
    fn tab_stops_until_page_field(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        rail: Rect,
        page: Rect,
    ) -> Vec<&'static str> {
        let rail_area = sidebar_rect(window);
        let topbar = topbar_rect(window);
        let mut stops: Vec<&'static str> = Vec::new();
        for _ in 0..48 {
            let _ = press(window, transport, queue, egui::Key::Tab);
            let Some(stop) = focused_stop(window) else {
                continue;
            };
            let kind = if rail.contains(stop.center()) {
                "rail-field"
            } else if page.contains(stop.center()) {
                "page-field"
            } else if topbar.contains_rect(stop) {
                "topbar"
            } else if rail_area.contains_rect(stop) {
                if stop.center().y < rail.top() {
                    "rail-chips"
                } else if stop.center().y <= rail.bottom() {
                    "rail-head"
                } else {
                    "rail-row"
                }
            } else {
                "other"
            };
            stops.push(kind);
            if kind == "page-field" {
                break;
            }
        }
        stops
    }

    /// Opens the drawer with the topbar's toggle and settles its first frame.
    fn open_drawer(window: &mut Window, transport: &mut Transport, queue: &mut Queue) {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let toggle = topbar_buttons(&frame)[0];
        let _ = window.click(transport, queue, toggle.center());
        assert_eq!(window.drawer, Drawer::Open, "the toggle opened no drawer");
        let _ = window.frame(transport, queue, Vec::new());
    }

    /// The drawer's copy of the rail field, read from the hairline under it.
    fn drawer_field_rect(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> Rect {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        rail_filter_rect(&frame, drawer_rect(window))
    }

    /// The box a sidebar row's fill covers, from the row's own painted line.
    fn sidebar_row_rect(
        window: &Window,
        output: &egui::FullOutput,
        title: &str,
        kind: &str,
    ) -> Rect {
        let area = sidebar_rect(window);
        let top = sidebar_row_top(output, area, title, kind);
        Rect::from_min_size(
            egui::pos2(area.left() + space::S2, top),
            Vec2::new(area.width() - 2.0 * space::S2, sidebar::ROW_H),
        )
    }

    /// The colours a frame painted inside `rect` in glyph shapes. The wash, the
    /// fills and the focus ring are `Shape::Rect`, so the backgrounds are
    /// excluded by kind.
    fn glyph_colours_in(output: &egui::FullOutput, rect: Rect) -> Vec<Color32> {
        let mut colours = Vec::new();
        for clipped in &output.shapes {
            if !rect.contains(clipped.shape.visual_bounding_rect().center()) {
                continue;
            }
            match &clipped.shape {
                egui::Shape::Path(path) => {
                    if path.fill != Color32::TRANSPARENT {
                        colours.push(path.fill);
                    }
                    if let egui::epaint::ColorMode::Solid(color) = &path.stroke.color
                        && path.stroke.width > 0.0
                    {
                        colours.push(*color);
                    }
                }
                egui::Shape::Circle(circle) => colours.push(circle.fill),
                _ => {}
            }
        }
        colours
    }

    /// Every entry in the queue, in order, wherever the index has got to.
    fn entries(queue: &Queue) -> Vec<String> {
        let count = queue.first_upcoming() + queue.upcoming().len();
        queue
            .entries_at(&(0..count).collect::<Vec<_>>())
            .into_iter()
            .map(|entry| entry.uri)
            .collect()
    }

    /// The Now Playing column's content box, for a window whose pane is shown.
    /// The queue pane takes its width and ends at its bottom edge; its top is
    /// the panel's head, well above the pane.
    fn pane_rect_when_shown(window: &Window) -> Rect {
        let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
        let regions = layout::regions_with(content.width(), window.pane);
        let shell = layout::shell_for(&regions);
        layout::grid_for(content, &shell)
            .now_playing
            .expect("the Now Playing column is on screen")
            .shrink(stroke::HAIRLINE)
    }

    /// The playbar's queue button, solved the way the shell solves it. A test
    /// aims the pointer with it; what the button draws is read from the frame.
    fn queue_button_at(window: &Window) -> Rect {
        let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
        let regions = layout::regions_with(content.width(), window.pane);
        let shell = layout::shell_for(&regions);
        let bar = layout::grid_for(content, &shell)
            .playbar
            .shrink(stroke::HAIRLINE);
        playbar::queue_button(bar, regions.playbar)
    }

    /// The playbar's fullscreen button: the last of the extras, which the
    /// columns and the run tables both end with.
    fn fullscreen_button_at(window: &Window) -> Rect {
        let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
        let regions = layout::regions_with(content.width(), window.pane);
        let shell = layout::shell_for(&regions);
        let bar = layout::grid_for(content, &shell)
            .playbar
            .shrink(stroke::HAIRLINE);
        let [_, _, extras] = layout::playbar_for(bar, regions.playbar).columns(bar);
        Rect::from_min_size(
            egui::pos2(
                extras.right() - extent::BUTTON,
                extras.center().y - extent::BUTTON / 2.0,
            ),
            Vec2::splat(extent::BUTTON),
        )
    }

    /// The viewport commands a frame sent the window.
    fn viewport_commands(output: &egui::FullOutput) -> Vec<egui::ViewportCommand> {
        output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|viewport| viewport.commands.clone())
            .unwrap_or_default()
    }

    /// The rect the queue button's claim filled, if it drew one. The whole
    /// playbar is scanned, and it holds no view row that could wear the same
    /// fill.
    fn claimed_fill(output: &egui::FullOutput, window: &Window) -> Option<Rect> {
        let tokens = Tokens::DARK;
        let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
        let regions = layout::regions_with(content.width(), window.pane);
        let shell = layout::shell_for(&regions);
        let bar = layout::grid_for(content, &shell).playbar;
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.fill == tokens.palette.row_selected
                        && bar.contains_rect(shape.rect) =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
    }

    /// The passes a page list's lift takes: the pointer arrives, presses, and
    /// jumps past egui's threshold in one move.
    fn carried_frame_events(from: egui::Pos2, to: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(from),
            crate::view::test_support::button(from, true),
            egui::Event::PointerMoved(to),
        ]
    }

    /// A point inside the row painted as `title` in `area`, on its own line.
    fn row_line(output: &egui::FullOutput, area: Rect, title: &str) -> egui::Pos2 {
        let at = text_in(output, title, area)
            .unwrap_or_else(|| panic!("{title} is not painted inside {area:?}"))
            .min;
        egui::pos2(at.x + 4.0, at.y)
    }

    /// A point `pitches` queue rows below `at`. A row's line sits about a tenth of
    /// a pitch into its row, so `-0.25` is the boundary before it and `0.75` the
    /// boundary after it.
    fn below(at: egui::Pos2, pitches: f32) -> egui::Pos2 {
        egui::pos2(at.x, at.y + QUEUE_ROW_H * pitches)
    }

    /// The rect the ghost is drawn in: the pointer offset, and the carried row's
    /// own size less that offset.
    fn ghost_rect(pointer: egui::Pos2, size: Vec2) -> Rect {
        Rect::from_min_size(
            pointer + Vec2::new(space::S3, space::S4),
            Vec2::new((size.x - space::S3).max(0.0), size.y),
        )
    }

    /// The fill painted into the box the ghost occupies.
    fn ghost_fill(output: &egui::FullOutput, rect: Rect) -> Option<Color32> {
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if (shape.rect.min - rect.min).length() < 0.5
                        && (shape.rect.size() - rect.size()).length() < 0.5 =>
                {
                    Some(shape.fill)
                }
                _ => None,
            })
    }

    /// The fill painted where the ghost starts: the pointer, offset.
    fn ghost_at(output: &egui::FullOutput, pointer: egui::Pos2) -> Option<Color32> {
        let at = pointer + Vec2::new(space::S3, space::S4);
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) if (shape.rect.min - at).length() < 0.5 => {
                    Some(shape.fill)
                }
                _ => None,
            })
    }

    /// The boxes this frame cast a shadow from.
    fn shadows(output: &egui::FullOutput) -> Vec<Rect> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) if shape.blur_width > 0.0 => Some(shape.rect),
                _ => None,
            })
            .collect()
    }

    /// The rule that says where a carried block lands, if one was drawn.
    fn marker(output: &egui::FullOutput) -> Option<Rect> {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.fill == tokens.palette.mark
                        && (shape.rect.height() - stroke::RULE).abs() < 0.01 =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
    }

    /// The box a queue row's focus ring was drawn around, if one was.
    fn ringed_row(output: &egui::FullOutput) -> Option<Rect> {
        let tokens = Tokens::DARK;
        crate::view::test_support::rings(output, &tokens)
            .into_iter()
            .find(|ring| (ring.height() - QUEUE_ROW_H).abs() < 0.6)
    }

    /// A playlist page with one row per title, so a track can be dragged out of
    /// it. The uris are the page's own, so they cannot be mistaken for the
    /// queue's. The page is editable, the way a list the user owns is.
    fn playlist_page(window: &mut Window, titles: &[&str]) {
        playlist_page_with(window, titles, page_access(true));
    }

    /// The same page, read at a revision the user may not write.
    fn read_only_playlist_page(window: &mut Window, titles: &[&str]) {
        playlist_page_with(window, titles, page_access(false));
    }

    /// The page a read that named no state leaves: nothing may be written.
    fn playlist_page_without_access(window: &mut Window, titles: &[&str]) {
        playlist_page_with(window, titles, None);
    }

    fn playlist_page_with(window: &mut Window, titles: &[&str], access: Option<PlaylistAccess>) {
        window.library.apply(&Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
            uri: PLAYLIST.to_owned(),
            name: "Mix".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }]));
        window.library.select(PLAYLIST);
        window.library.apply(&Update::Tracks {
            playlist: PLAYLIST.to_owned(),
            tracks: titles
                .iter()
                .map(|title| TrackEntry {
                    uri: format!("list:{title}"),
                    title: (*title).to_owned(),
                    ..track(0, None)
                })
                .collect(),
            cover: None,
            access,
        });
    }

    /// A revision the pages in these tests were read at.
    fn revision(tag: &str) -> Revision {
        Revision::from_wire(tag.as_bytes()).expect("a non-empty revision")
    }

    /// What a page may change, at a revision of its own.
    fn page_access(editable: bool) -> Option<PlaylistAccess> {
        Some(PlaylistAccess {
            revision: revision("rev-1"),
            editable,
        })
    }

    /// The rows the page holds, in the order it holds them.
    fn page_rows(window: &Window) -> Vec<String> {
        match &window.library.tracks {
            Load::Ready(tracks) => tracks
                .tracks
                .iter()
                .map(|entry| entry.uri.clone())
                .collect(),
            Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
        }
    }

    /// A point at the boundary `at` of the page's rows, from the lines it
    /// painted: 0 is above the first row, `count` below the last. A row's title
    /// sits in the upper half of its row, so a row's own line is the gap it
    /// opens above itself.
    fn page_boundary(
        output: &egui::FullOutput,
        area: Rect,
        titles: &[&str],
        at: usize,
    ) -> egui::Pos2 {
        let last = titles.len() - 1;
        let line = row_line(output, area, titles[at.min(last)]);
        let y = if at > last { line.y + 24.0 } else { line.y };
        egui::pos2(line.x, y)
    }

    /// A point inside the row painted as `title` in `area`, in the half that
    /// decides the boundary it takes: `after` is the gap below it.
    fn row_half(output: &egui::FullOutput, area: Rect, title: &str, after: bool) -> egui::Pos2 {
        let line = row_line(output, area, title);
        egui::pos2(line.x, line.y + if after { 20.0 } else { -4.0 })
    }

    /// A queue whose every row names one album and one credit, each row's words
    /// its own so a test can aim at one.
    fn linked_queue(count: usize, transport: &mut Transport) -> Queue {
        let tracks: Vec<TrackEntry> = (0..count)
            .map(|index| TrackEntry {
                artists: format!("Band {index}"),
                album_uri: Some(QUEUE_ALBUM.to_owned()),
                artist_uris: vec![QUEUE_ARTIST.to_owned()],
                ..track(index, None)
            })
            .collect();
        let mut queue = Queue::default();
        queue.start(&tracks, 0, None, transport);
        queue
    }

    /// The point a click on the words of `label`, painted in `area`, lands on.
    fn on_the_words(output: &egui::FullOutput, area: Rect, label: &str) -> egui::Pos2 {
        row_line(output, area, label) + Vec2::new(0.0, 4.0)
    }

    /// Clicks `at` over the passes egui's hit test needs, returning the command
    /// the shell reported.
    fn click_words(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        at: egui::Pos2,
    ) -> Option<Command> {
        let _ = window.frame(transport, queue, Vec::new());
        let _ = window.frame(transport, queue, vec![egui::Event::PointerMoved(at)]);
        let _ = window.frame(
            transport,
            queue,
            vec![crate::view::test_support::button(at, true)],
        );
        window
            .frame(
                transport,
                queue,
                vec![crate::view::test_support::button(at, false)],
            )
            .0
    }

    /// Clicks the queue row painted as `title` inside `area`.
    fn clicked_row(
        window: &mut Window,
        queue: &mut Queue,
        transport: &mut Transport,
        area: Rect,
        title: &str,
    ) {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let at = row_line(&frame, area, title);
        let _ = window.click(transport, queue, at);
    }

    #[test]
    fn a_queue_link_opens_the_page() {
        let mut transport = playing(cover("abc"));
        let mut queue = linked_queue(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = on_the_words(&frame, pane_rect_when_shown(&window), "t2");

        assert_eq!(
            click_words(&mut window, &mut transport, &mut queue, at),
            Some(Command::LoadAlbum {
                album: QUEUE_ALBUM.to_owned()
            }),
            "a click on a queue row's name opens its album"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Album(QUEUE_ALBUM.to_owned())),
            "and the page is the album it named"
        );
    }

    #[test]
    fn a_now_playing_link_opens_the_page() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        transport.now_playing = Some(NowPlaying {
            album_uri: Some(ALBUM.to_owned()),
            ..NowPlaying::named("spotify:track:x", "A Title", "A Band")
        });
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = on_the_words(&frame, pane_rect_when_shown(&window), "A Title");

        assert_eq!(
            click_words(&mut window, &mut transport, &mut queue, at),
            Some(Command::LoadAlbum {
                album: ALBUM.to_owned()
            }),
            "a click on the panel's title opens its album"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Album(ALBUM.to_owned())),
            "and the page is the album it named"
        );
    }

    #[test]
    fn a_now_playing_artist_link_opens_the_page() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        transport.now_playing = Some(NowPlaying {
            artist_uri: Some(ARTIST.to_owned()),
            ..NowPlaying::named("spotify:track:x", "A Title", "A Band")
        });
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = on_the_words(&frame, pane_rect_when_shown(&window), "A Band");

        assert_eq!(
            click_words(&mut window, &mut transport, &mut queue, at),
            Some(Command::LoadArtist {
                artist: ARTIST.to_owned()
            }),
            "a click on the panel's artist opens their page"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Artist(ARTIST.to_owned())),
            "and the page is the artist it named"
        );
    }

    #[test]
    fn a_queue_artist_link_opens_the_page() {
        let mut transport = playing(cover("abc"));
        let mut queue = linked_queue(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = on_the_words(&frame, pane_rect_when_shown(&window), "Band 2");

        assert_eq!(
            click_words(&mut window, &mut transport, &mut queue, at),
            Some(Command::LoadArtist {
                artist: QUEUE_ARTIST.to_owned()
            }),
            "a click on a queue row's artist opens their page"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Artist(QUEUE_ARTIST.to_owned())),
            "and the page is the artist it named"
        );
    }

    #[test]
    fn a_playbar_link_opens_the_page() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        transport.now_playing = Some(NowPlaying {
            artist_uri: Some(ARTIST.to_owned()),
            ..NowPlaying::named("spotify:track:x", "A Title", "A Band")
        });
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (bar, _) = playbar_parts(&window);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = on_the_words(&frame, bar, "A Band");

        assert_eq!(
            click_words(&mut window, &mut transport, &mut queue, at),
            Some(Command::LoadArtist {
                artist: ARTIST.to_owned()
            }),
            "a click on the playbar's artist opens their page"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Artist(ARTIST.to_owned())),
            "and the page is the artist it named"
        );
    }

    #[test]
    fn a_link_click_is_dropped_when_the_frame_already_has_a_command() {
        let mut transport = playing(cover("abc"));
        let mut queue = linked_queue(4, &mut transport);

        // The control: the same click in a frame whose only command is its own
        // opens the queue's album, so the frame below has an open to drop.
        let mut control = Window::new(Vec2::new(1400.0, 900.0));
        control.library.open(Page::Album(ALBUM.to_owned()));
        control.library.open(Page::Album(ALBUM_B.to_owned()));
        let (_, frame) = control.frame(&mut transport, &mut queue, Vec::new());
        let at = on_the_words(&frame, pane_rect_when_shown(&control), "t2");

        assert_eq!(
            click_words(&mut control, &mut transport, &mut queue, at),
            Some(Command::LoadAlbum {
                album: QUEUE_ALBUM.to_owned()
            }),
            "the same click with no nav opens the queue's album"
        );
        assert_eq!(
            control.library.page(),
            Some(&Page::Album(QUEUE_ALBUM.to_owned())),
            "and the control's page is the album it named"
        );

        // Two albums behind each other, so Back has one to land on and must ask
        // for it: the nav's command is the frame's.
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        window.library.open(Page::Album(ALBUM.to_owned()));
        window.library.open(Page::Album(ALBUM_B.to_owned()));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = on_the_words(&frame, pane_rect_when_shown(&window), "t2");

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(at)],
        );
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![crate::view::test_support::button(at, true)],
        );
        let command = window
            .frame(
                &mut transport,
                &mut queue,
                vec![
                    key(egui::Key::BrowserBack, true),
                    crate::view::test_support::button(at, false),
                ],
            )
            .0;

        assert_eq!(
            command,
            Some(Command::LoadAlbum {
                album: ALBUM.to_owned()
            }),
            "the nav's command is the frame's one"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Album(ALBUM.to_owned())),
            "the nav landed on the album behind and the link's page was dropped"
        );
    }

    #[test]
    fn a_link_click_is_dropped_while_a_block_is_carried() {
        let mut transport = playing(cover("abc"));
        let mut queue = linked_queue(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let pane = pane_rect_when_shown(&window);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, pane, "t2");
        let title = on_the_words(&frame, pane, "t0");
        let _ = window.hold_drag(&mut transport, &mut queue, from, below(from, 0.75));
        assert!(carry::carrying(&window.ctx), "a block is in flight");
        // The gesture that holds the block is a drag, and a drag cannot also
        // click, so the frame the click arrives in is told a block is away.
        let _ = window.frame(&mut transport, &mut queue, vec![window.release(from)]);
        egui::DragAndDrop::set_payload(&window.ctx, ());

        assert_eq!(
            click_words(&mut window, &mut transport, &mut queue, title),
            None,
            "a carried block opened a page"
        );
        assert_ne!(
            window.library.page(),
            Some(&Page::Album(QUEUE_ALBUM.to_owned())),
            "the carried frame moved the library"
        );
    }

    #[test]
    fn a_queue_open_and_a_now_playing_open_in_one_frame_apply_once() {
        // One pointer clicks one link a frame, so the frame that names two pages
        // is written by hand: the panel's page is the one taken, and the queue's
        // is spent with it, so the library can only move once.
        let mut panel = now_playing::Reply {
            open: Some(Page::Album(ALBUM.to_owned())),
            ..now_playing::Reply::default()
        };
        let mut queue = queue_pane::Reply {
            open: Some(Page::Album(QUEUE_ALBUM.to_owned())),
            ..queue_pane::Reply::default()
        };

        assert_eq!(
            take_open(&mut panel.open, &mut queue.open),
            Some(Page::Album(ALBUM.to_owned())),
            "the panel's page is the frame's one page"
        );
        assert_eq!(
            (panel.open, queue.open),
            (None, None),
            "both opens are spent with the frame's one"
        );
    }

    #[test]
    fn a_click_on_a_queue_row_selects_it_and_plays_nothing() {
        // A single click is a press. Scanning the queue with the pointer must
        // never move the transport.
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, pane_rect_when_shown(&window), "t2");

        assert_eq!(window.click(&mut transport, &mut queue, at), None);
        assert!(queue.selected(2), "the click picks the row");
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t0"),
            "a single click must not move the transport"
        );
    }

    #[test]
    fn a_click_on_a_queue_row_keeps_the_keyboard_without_a_ring() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
        assert!(
            ringed_row(&frame).is_none(),
            "nothing holds the keyboard yet"
        );

        let _ = window.click(&mut transport, &mut queue, at);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let id = window
            .ctx
            .memory(|memory| memory.focused())
            .expect("the clicked row holds the keyboard");
        let row = window
            .ctx
            .read_response(id)
            .expect("the row was laid out")
            .rect;
        assert!(
            row.contains(at),
            "the keyboard went to another row: {row:?}"
        );
        assert_eq!(row.height(), QUEUE_ROW_H, "the keyboard is on a row");
        assert!(ringed_row(&frame).is_none(), "a clicked row wears no ring");
    }

    #[test]
    fn a_clicked_queue_row_wears_no_ring() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, pane_rect_when_shown(&window), "t2");

        let _ = window.click(&mut transport, &mut queue, at);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            window.ctx.memory(|memory| memory.focused()).is_some(),
            "the click took the keyboard"
        );
        assert!(
            ringed_row(&frame).is_none(),
            "a clicked queue row wears no ring"
        );
    }

    #[test]
    fn tab_rings_the_row_inside_the_list() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let pane = pane_rect_when_shown(&window);
        clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");

        // Tab walks out of the row's own controls and onto the rows below it.
        let mut row = None;
        for _ in 0..8 {
            let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Tab);
            let focused = window
                .ctx
                .memory(|memory| memory.focused())
                .expect("Tab keeps the keyboard");
            let rect = window
                .ctx
                .read_response(focused)
                .expect("the row was laid out")
                .rect;
            if (rect.height() - QUEUE_ROW_H).abs() < 0.6 {
                row = Some(rect);
                break;
            }
        }
        let row = row.expect("Tab reached a queue row");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let rings = crate::view::test_support::rings(&frame, &Tokens::DARK);
        assert_eq!(rings.len(), 1, "the tabbed row wears the ring: {rings:?}");
        assert!(
            row.contains_rect(rings[0]),
            "the ring left the row: {:?} in {row:?}",
            rings[0]
        );
        assert!(pane.contains_rect(row), "the row left the list: {row:?}");
    }

    #[test]
    fn a_click_on_empty_space_then_tab_rings_the_next_widget() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        let empty = view_rect(&window).center();
        let _ = window.click(&mut transport, &mut queue, empty);
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Tab);

        let focused = window
            .ctx
            .memory(|memory| memory.focused())
            .expect("Tab took the keyboard");
        let rect = window
            .ctx
            .read_response(focused)
            .expect("the widget was laid out")
            .rect;
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let rings = crate::view::test_support::rings(&frame, &Tokens::DARK);
        assert_eq!(rings.len(), 1, "one widget wears the ring: {rings:?}");
        let control_ring = rect.expand(extent::FOCUS_OFFSET + stroke::HAIRLINE);
        assert!(
            rings[0] == rect || rings[0] == control_ring,
            "the ring is not the widget Tab reached: {rings:?} for {rect:?}"
        );
    }

    #[test]
    fn a_click_then_space_wears_no_ring_on_the_clicked_widget() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
        let _ = window.click(&mut transport, &mut queue, at);

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Space);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            ringed_row(&frame).is_none(),
            "a key after a click reveals no ring on the clicked widget"
        );
        let focused = window
            .ctx
            .memory(|memory| memory.focused())
            .expect("the click took the keyboard");
        let row = window
            .ctx
            .read_response(focused)
            .expect("the row was laid out")
            .rect;
        assert!(row.contains(at), "the keyboard left the clicked row");
    }

    #[test]
    fn enter_on_a_focused_queue_row_plays_it() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
        let _ = window.click(&mut transport, &mut queue, at);

        assert_eq!(
            chord(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::Enter,
                egui::Modifiers::NONE
            ),
            Some(Command::Play {
                uri: "t2".to_owned()
            })
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t2")
        );
    }

    #[test]
    fn shift_click_selects_a_span_and_ctrl_click_toggles_in_the_queue() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(7, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let at = |title: &str| row_line(&frame, pane, title);

        let _ = window.click(&mut transport, &mut queue, at("t2"));
        window.held = egui::Modifiers::SHIFT;
        let _ = window.click(&mut transport, &mut queue, at("t4"));
        assert!(
            (2..=4).all(|row| queue.selected(row)),
            "shift extends from the anchor"
        );

        window.held = egui::Modifiers::COMMAND;
        let _ = window.click(&mut transport, &mut queue, at("t3"));
        window.held = egui::Modifiers::NONE;
        assert!(queue.selected(2) && queue.selected(4));
        assert!(!queue.selected(3), "ctrl takes one row out of the span");
    }

    #[test]
    fn dragging_a_queue_row_below_another_reorders_without_a_command() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, pane_rect_when_shown(&window), "t2");
        let to = below(from, 1.75);

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            None,
            "a reorder is not the transport's to play"
        );
        assert_eq!(entries(&queue), ["t0", "t1", "t3", "t2", "t4"]);
        assert!(queue.selected(3), "a move picks the block it landed");
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t0"),
            "the playing entry keeps its place"
        );
    }

    #[test]
    fn dragging_a_selected_block_moves_the_block() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(6, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let _ = window.click(&mut transport, &mut queue, row_line(&frame, pane, "t2"));
        window.held = egui::Modifiers::COMMAND;
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let _ = window.click(&mut transport, &mut queue, row_line(&frame, pane, "t3"));
        window.held = egui::Modifiers::NONE;
        assert!(queue.selected(2) && queue.selected(3));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, pane, "t3");

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, below(from, 2.75)),
            None
        );
        assert_eq!(
            entries(&queue),
            ["t0", "t1", "t4", "t5", "t2", "t3"],
            "the whole block moves, in its own order"
        );
    }

    #[test]
    fn dropping_above_the_first_upcoming_row_cannot_displace_the_playing_entry() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let from = row_line(&frame, pane, "t3");
        // The caption band above the first row is part of the target, and means
        // the first boundary the queue will take a drop at.
        let to = text_in(&frame, "NEXT IN QUEUE", pane)
            .expect("the caption is painted")
            .center();

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
        assert_eq!(
            entries(&queue),
            ["t0", "t3", "t1", "t2"],
            "a drop at the caption is the first upcoming boundary"
        );
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t0"),
            "the playing entry cannot be displaced"
        );
    }

    #[test]
    fn a_release_outside_the_queue_drops_nothing() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, pane_rect_when_shown(&window), "t2");
        let before = entries(&queue);

        assert_eq!(
            window.drag(
                &mut transport,
                &mut queue,
                from,
                view_rect(&window).center()
            ),
            None
        );
        assert_eq!(entries(&queue), before, "a release nowhere changes nothing");
    }

    #[test]
    fn an_empty_queue_takes_the_first_row_and_stays_armed() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        // The caption band is part of the pane's target and sits inside the pane
        // whatever the art box above reserves; the panel's own centre may not.
        let to = text_in(&frame, "NEXT IN QUEUE", pane_rect_when_shown(&window))
            .expect("the caption is painted")
            .center();

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
        assert_eq!(entries(&queue), ["list:p1"]);
        assert!(
            !queue.can_previous(),
            "a drop queues the row, it does not start it"
        );
        assert_eq!(queue.first_upcoming(), 0, "the row waits for a play");

        let (command, _) = tap(
            &mut window,
            &mut transport,
            &mut queue,
            key(egui::Key::Space, true),
        );
        assert_eq!(
            command,
            Some(Command::Play {
                uri: "list:p1".to_owned()
            }),
            "a space starts the waiting row"
        );
        assert_eq!(queue.first_upcoming(), 1, "and it is the current entry now");
    }

    #[test]
    fn a_playlist_row_dropped_on_the_queue_is_still_queued() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let from = row_line(&frame, view_rect(&window), "p1");
        let to = below(row_line(&frame, pane, "t3"), -0.25);

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            None,
            "dropping a track in the queue must not start it"
        );
        assert_eq!(entries(&queue), ["t0", "t1", "t2", "list:p1", "t3"]);
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t0")
        );
        assert_eq!(
            page_rows(&window),
            ["list:p0", "list:p1", "list:p2"],
            "a drop in the queue is no write to the page it came from"
        );
    }

    #[test]
    fn dragging_a_playlist_row_below_the_next_moves_it_and_sends_one_write() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let from = row_line(&frame, view, "p0");
        let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            Some(Command::MoveTrack {
                playlist: PLAYLIST.to_owned(),
                revision: revision("rev-1"),
                moves: vec![Move {
                    from: 0,
                    length: 1,
                    to: 2,
                }],
            }),
            "the page asks for one write on the revision it was read at"
        );
        assert_eq!(
            page_rows(&window),
            ["list:p1", "list:p0", "list:p2"],
            "the row has already moved on screen"
        );
        assert!(!window.library.movable(), "the write is in flight");
        assert_eq!(
            window.frame(&mut transport, &mut queue, Vec::new()).0,
            None,
            "the gesture sends one write, not one a frame"
        );
    }

    fn carried_rows(list: egui::Id, rows: Vec<usize>) -> Carry {
        use crate::cover::Cover;
        use crate::view::queue_row;

        Carry {
            source: Source::List(list),
            rows,
            entries: Vec::new(),
            size: Vec2::new(10.0, 20.0),
            ghost: carry::Ghost::Queue(
                queue_row::Row {
                    index: 0,
                    title: "A Title",
                    artist: "A Band",
                    duration: None,
                    cover: Cover::Missing,
                    title_target: None,
                    artist_target: None,
                    look: crate::view::rows::Look::default(),
                    refocus: false,
                }
                .lifted(),
            ),
            playlist: None,
        }
    }

    #[test]
    fn a_block_dropped_in_the_queue_lands_as_a_block_in_row_order() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let carry = Carry {
            entries: vec![track(8, None), track(9, None)],
            ..carried_rows(egui::Id::unique("list"), vec![0, 2])
        };

        land(
            &mut queue,
            &carry,
            Target::At(Landing {
                boundary: 2,
                marker: Rect::ZERO,
            }),
        );

        assert_eq!(entries(&queue), ["t0", "t1", "t8", "t9", "t2", "t3"]);
    }

    #[test]
    fn a_block_appended_by_the_button_joins_the_end_whole() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let carry = Carry {
            entries: vec![track(8, None), track(9, None)],
            ..carried_rows(egui::Id::unique("list"), vec![0, 2])
        };

        land(&mut queue, &carry, Target::End);

        assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "t8", "t9"]);
    }

    #[test]
    fn the_pick_a_carry_held_before_the_toggle_is_refused_after_it() {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::seeded(9);
        let tracks: Vec<TrackEntry> = (0..5).map(|index| track(index, None)).collect();
        queue.start(&tracks, 0, None, &mut transport);
        queue.press(1, Press::Replace);
        for row in 2..5 {
            queue.press(row, Press::Toggle);
        }
        let rows = queue.lift(1).expect("the coming rows are editable");
        let carry = Carry {
            source: Source::Queue,
            rows: rows.clone(),
            entries: queue.entries_at(&rows),
            ..carried_rows(egui::Id::unique("queue"), rows)
        };
        queue.toggle_shuffle();
        assert_ne!(
            queue.entries_at(&carry.rows),
            carry.entries,
            "the premise: the shuffle moved the carried rows"
        );
        let before = entries(&queue);

        land(
            &mut queue,
            &carry,
            Target::At(Landing {
                boundary: 2,
                marker: Rect::ZERO,
            }),
        );

        assert_eq!(
            entries(&queue),
            before,
            "a pick held across a toggle must not move other tracks"
        );
    }

    #[test]
    fn a_drag_that_starts_inside_a_pick_carries_the_whole_block() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &["a", "b", "c", "d"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let at = |title: &str| row_line(&frame, view, title);

        let _ = window.click(&mut transport, &mut queue, at("b"));
        window.held = egui::Modifiers::SHIFT;
        let _ = window.click(&mut transport, &mut queue, at("d"));
        window.held = egui::Modifiers::NONE;

        // The press is inside the pick, and the pick is what travels.
        let from = at("c");
        let to = page_boundary(&frame, view, &["a", "b", "c", "d"], 0);
        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            Some(Command::MoveTrack {
                playlist: PLAYLIST.to_owned(),
                revision: revision("rev-1"),
                moves: vec![Move {
                    from: 1,
                    length: 3,
                    to: 0,
                }],
            }),
            "a contiguous block is one move of its own length"
        );
        assert_eq!(page_rows(&window), ["b", "c", "d", "a"]);
    }

    #[test]
    fn a_gapped_pick_gathered_at_the_end_is_the_measured_chain() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &["a", "b", "c", "d"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let at = |title: &str| row_line(&frame, view, title);

        let _ = window.click(&mut transport, &mut queue, at("a"));
        window.held = egui::Modifiers::COMMAND;
        let _ = window.click(&mut transport, &mut queue, at("c"));
        window.held = egui::Modifiers::NONE;

        let from = at("c");
        let to = page_boundary(&frame, view, &["a", "b", "c", "d"], 4);
        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            Some(Command::MoveTrack {
                playlist: PLAYLIST.to_owned(),
                revision: revision("rev-1"),
                moves: vec![
                    Move {
                        from: 0,
                        length: 1,
                        to: 4,
                    },
                    Move {
                        from: 1,
                        length: 1,
                        to: 4,
                    },
                ],
            }),
            "a gapped pair is a chain in one delta"
        );
        assert_eq!(page_rows(&window), ["b", "d", "a", "c"]);
    }

    #[test]
    fn a_block_target_is_drawn_exactly_where_a_move_would_land() {
        let len = 6;
        let list = egui::Id::unique("list");
        let rows = Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(300.0, len as f32 * extent::ROW_H),
        );
        let zone = crate::landing::Zone {
            hit: rows,
            list: rows,
            row_h: extent::ROW_H,
            first: 0,
            count: len,
        };
        let offer = Offer {
            list,
            playlist: PLAYLIST.to_owned(),
            zone,
        };
        let pointer = |boundary: usize| Some(egui::pos2(150.0, boundary as f32 * extent::ROW_H));

        for picks in [vec![1], vec![1, 2, 3], vec![1, 3]] {
            let carry = carried_rows(list, picks.clone());
            for boundary in 0..=len {
                let drawn = reorder_target(&carry, Some(&offer), pointer(boundary)).is_some();
                let moves = reorder::gather(len, &picks, boundary).is_some();
                assert_eq!(drawn, moves, "{picks:?} at {boundary}");
            }
        }

        // A single row's own two gaps, and a contiguous block's inner gaps.
        let single = carried_rows(list, vec![1]);
        assert_eq!(reorder_target(&single, Some(&offer), pointer(1)), None);
        assert_eq!(reorder_target(&single, Some(&offer), pointer(2)), None);
        let block = carried_rows(list, vec![1, 2, 3]);
        for boundary in 1..=4 {
            assert_eq!(
                reorder_target(&block, Some(&offer), pointer(boundary)),
                None,
                "the block already sits at {boundary}"
            );
        }

        // A gapped block draws a marker at every boundary but its own pick.
        let gapped = carried_rows(list, vec![1, 3]);
        for boundary in 0..=len {
            assert!(
                reorder_target(&gapped, Some(&offer), pointer(boundary)).is_some(),
                "a gapped block moves at every boundary: {boundary}"
            );
        }
    }

    #[test]
    fn dragging_a_row_to_the_top_writes_index_zero() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let from = row_line(&frame, view, "p2");
        let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 0);

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            Some(Command::MoveTrack {
                playlist: PLAYLIST.to_owned(),
                revision: revision("rev-1"),
                moves: vec![Move {
                    from: 2,
                    length: 1,
                    to: 0,
                }],
            })
        );
        assert_eq!(page_rows(&window), ["list:p2", "list:p0", "list:p1"]);
    }

    #[test]
    fn a_drop_beside_itself_moves_nothing_and_writes_nothing() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        for (title, beside) in [("p1", 1), ("p1", 2), ("p2", 3)] {
            assert_eq!(
                window.drag(
                    &mut transport,
                    &mut queue,
                    row_line(&frame, view, title),
                    page_boundary(&frame, view, &["p0", "p1", "p2"], beside)
                ),
                None,
                "the gap {beside} is where {title} already is"
            );
            assert_eq!(
                page_rows(&window),
                ["list:p0", "list:p1", "list:p2"],
                "nothing moved"
            );
            assert!(window.library.movable(), "and nothing was written");
        }
    }

    #[test]
    fn a_drop_beside_itself_draws_no_marker_and_a_flat_ghost() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        for (title, beside) in [("p0", 1), ("p1", 2)] {
            let from = row_line(&frame, view, title);
            let to = page_boundary(&frame, view, &["p0", "p1", "p2"], beside);
            let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
            assert!(
                carry::carrying(&window.ctx),
                "the gap {beside} of {title} is reached by a drag"
            );
            assert!(
                marker(&frame).is_none(),
                "the gap {beside} of {title} marks no landing"
            );
            assert_eq!(
                ghost_at(&frame, to),
                Some(Tokens::DARK.palette.surface),
                "the gap {beside} of {title} draws the ghost flat"
            );
            let corner = to + Vec2::new(space::S3, space::S4);
            assert!(
                !shadows(&frame)
                    .iter()
                    .any(|cast| (cast.min - corner).length() < 0.5),
                "a flat ghost casts no shadow"
            );
            let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
            assert_eq!(command, None, "the gap {beside} of {title} writes nothing");
            assert_eq!(
                page_rows(&window),
                ["list:p0", "list:p1", "list:p2"],
                "and moves nothing"
            );
        }
    }

    #[test]
    fn a_row_that_changed_under_the_drag_is_refused_on_release() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let from = row_line(&frame, view, "p1");
        let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 0);
        let _ = window.hold_drag(&mut transport, &mut queue, from, to);
        assert!(carry::carrying(&window.ctx), "the row is in flight");

        // A read lands while the block is held, and another entry now sits at `from`.
        window.library.apply(&Update::Tracks {
            playlist: PLAYLIST.to_owned(),
            tracks: ["p2", "p0", "p1"]
                .iter()
                .map(|title| TrackEntry {
                    uri: format!("list:{title}"),
                    title: (*title).to_owned(),
                    ..track(0, None)
                })
                .collect(),
            cover: None,
            access: page_access(true),
        });

        let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
        assert_eq!(command, None, "the carried entry no longer holds the row");
        assert_eq!(
            page_rows(&window),
            ["list:p2", "list:p0", "list:p1"],
            "a refused reorder moves nothing"
        );
        assert!(window.library.movable(), "and writes nothing");
    }

    #[test]
    fn the_marker_shows_where_the_row_lands_in_its_own_list() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let from = row_line(&frame, view, "p0");
        let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);

        let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
        let marker = marker(&frame).expect("the page draws where the row lands");
        assert!(
            view.contains_rect(marker),
            "the marker is the page's own, not the queue's"
        );
        let first = row_line(&frame, view, "p1");
        let second = row_line(&frame, view, "p2");
        assert!(
            marker.top() > first.y && marker.top() < second.y,
            "it sits in the gap the drop takes, between p1 and p2: {marker:?}"
        );
    }

    #[test]
    fn a_read_only_playlist_row_still_queues_but_never_reorders() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        read_only_playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let view = view_rect(&window);
        let from = row_line(&frame, view, "p1");
        let to = below(row_line(&frame, pane, "t3"), -0.25);

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
        assert_eq!(
            entries(&queue),
            ["t0", "t1", "t2", "list:p1", "t3"],
            "a drop into the queue is not a write to the page"
        );

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view, "p0");
        let to = row_half(&frame, view, "p2", false);
        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            None,
            "the page offers no reorder"
        );
        assert_eq!(
            page_rows(&window),
            ["list:p0", "list:p1", "list:p2"],
            "so its rows do not move"
        );
    }

    #[test]
    fn a_page_without_access_never_reorders() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page_without_access(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let from = row_line(&frame, view, "p0");
        let to = row_half(&frame, view, "p2", false);

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
        assert_eq!(page_rows(&window), ["list:p0", "list:p1", "list:p2"]);
    }

    #[test]
    fn while_a_write_is_pending_no_row_lifts() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let from = row_line(&frame, view, "p0");
        let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);
        assert!(
            window.drag(&mut transport, &mut queue, from, to).is_some(),
            "the first move is written"
        );
        assert!(!window.library.movable());

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view, "p2");
        let (_, _) = window.hold_drag(
            &mut transport,
            &mut queue,
            from,
            row_half(&frame, view, "p2", true),
        );
        assert!(
            carry::carried(&window.ctx).is_none(),
            "no row lifts while the revision it would write is already spent"
        );
    }

    #[test]
    fn a_failed_write_puts_the_row_back_and_says_so() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let from = row_line(&frame, view, "p0");
        let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);
        assert!(window.drag(&mut transport, &mut queue, from, to).is_some());
        assert_eq!(page_rows(&window), ["list:p1", "list:p0", "list:p2"]);

        window.library.apply(&Update::MoveFailed {
            playlist: PLAYLIST.to_owned(),
            failure: Failure::Save,
        });
        assert_eq!(
            page_rows(&window),
            ["list:p0", "list:p1", "list:p2"],
            "the row went back"
        );

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let notice = crate::signin::failure_message(Failure::Save);
        let at = painted_at(&frame, notice).expect("the failure is drawn");
        let first = painted_at(&frame, "p0").expect("the first row is drawn");
        assert!(at.y < first.y, "the failure heads the rows it put back");
    }

    #[test]
    fn a_reply_lets_the_next_move_build_on_its_revision() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let from = row_line(&frame, view, "p0");
        let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);
        assert!(window.drag(&mut transport, &mut queue, from, to).is_some());

        window.library.apply(&Update::TrackMoved {
            playlist: PLAYLIST.to_owned(),
            revision: revision("rev-2"),
        });
        assert!(window.library.movable(), "the reply ends the write");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view, "p2");
        let to = page_boundary(&frame, view, &["p1", "p0", "p2"], 0);
        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            Some(Command::MoveTrack {
                playlist: PLAYLIST.to_owned(),
                revision: revision("rev-2"),
                moves: vec![Move {
                    from: 2,
                    length: 1,
                    to: 0,
                }],
            }),
            "the second move is based on the reply's revision"
        );
        assert_eq!(page_rows(&window), ["list:p2", "list:p1", "list:p0"]);
    }

    #[test]
    fn the_selection_after_a_move_is_the_landed_index_holding_the_moved_track() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let from = row_line(&frame, view, "p0");
        let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);
        assert!(window.drag(&mut transport, &mut queue, from, to).is_some());

        let id = egui::Id::unique(("playlist-tracks", PLAYLIST));
        let list = carry::List::open(&window.ctx, id, 3);
        assert!(list.look(1).selected, "the row that landed is picked");
        assert!(
            !list.look(0).selected && !list.look(2).selected,
            "and nothing else is"
        );
        assert_eq!(
            page_rows(&window)[1],
            "list:p0",
            "the pick is on the track that moved"
        );
    }

    #[test]
    fn dragging_a_playlist_below_the_next_moves_it_and_sends_one_write() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("A", None), ("B", None), ("C", None)],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "A");
        let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 2);

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            Some(Command::MovePlaylist {
                revision: revision("root-1"),
                from: 0,
                to: 2,
            }),
            "the boundary is the gap, which is what the wire reads"
        );
        assert_eq!(sidebar_order(&window), ["B", "A", "C"]);
        assert!(!window.library.rootlist_movable(), "the write is in flight");
        assert_eq!(
            window.frame(&mut transport, &mut queue, Vec::new()).0,
            None,
            "the gesture sends one write, not one a frame"
        );
    }

    #[test]
    fn a_query_that_matches_every_row_still_reorders() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("Midnight City", None), ("Midnight Sun", None)],
            Some(rootlist_access(true)),
        );
        // The query matches every row, so the drawn set is the whole list: its
        // slots are true indices and the lift must not be withheld.
        window.library.rail_filter.edit().push_str("midnight");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "Midnight City");
        let to = sidebar_boundary(
            &window,
            &frame,
            &["Midnight City", "Midnight Sun"],
            "Playlist",
            2,
        );

        let (_, held) = window.hold_drag(&mut transport, &mut queue, from, to);
        assert!(
            carry::carried(&window.ctx).is_some(),
            "a query that matches every row still lets the row lift"
        );
        assert!(
            marker(&held).is_some(),
            "a reorder that can land draws its rule"
        );

        let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);

        assert_eq!(
            command,
            Some(Command::MovePlaylist {
                revision: revision("root-1"),
                from: 0,
                to: 2,
            }),
            "the whole-list query blocked the reorder"
        );
        assert_eq!(
            sidebar_order(&window),
            ["Midnight Sun", "Midnight City"],
            "the playlist did not move"
        );
    }

    #[test]
    fn dragging_a_playlist_to_the_top_writes_gap_zero() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("A", None), ("B", None), ("C", None)],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "C");
        let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 0);

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            Some(Command::MovePlaylist {
                revision: revision("root-1"),
                from: 2,
                to: 0,
            })
        );
        assert_eq!(sidebar_order(&window), ["C", "A", "B"]);
    }

    #[test]
    fn dragging_a_playlist_to_the_very_end_writes_the_length() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("A", None), ("B", None), ("C", None)],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "A");
        let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 3);

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            Some(Command::MovePlaylist {
                revision: revision("root-1"),
                from: 0,
                to: 3,
            })
        );
        assert_eq!(sidebar_order(&window), ["B", "C", "A"]);
    }

    #[test]
    fn a_sidebar_drop_beside_itself_moves_nothing_and_writes_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("A", None), ("B", None), ("C", None)],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        for boundary in [1, 2] {
            let from = row_line(&frame, sidebar_rect(&window), "B");
            let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", boundary);
            assert_eq!(
                window.drag(&mut transport, &mut queue, from, to),
                None,
                "the gap {boundary} is where B already is"
            );
            assert_eq!(sidebar_order(&window), ["A", "B", "C"], "nothing moved");
            assert!(window.library.rootlist_movable(), "and nothing was written");
        }
    }

    #[test]
    fn a_failed_rootlist_write_puts_the_row_back_and_says_so() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("A", None), ("B", None), ("C", None)],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "A");
        let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 2);
        assert!(window.drag(&mut transport, &mut queue, from, to).is_some());
        assert_eq!(sidebar_order(&window), ["B", "A", "C"]);

        window.library.apply(&Update::PlaylistMoveFailed);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

        assert_eq!(sidebar_order(&window), ["A", "B", "C"]);
        assert!(
            text_in(
                &frame,
                "Could not move that playlist. Try again.",
                sidebar_rect(&window)
            )
            .is_some(),
            "the list says what could not be done"
        );
        assert!(
            window.library.rootlist_movable(),
            "another drag may try again"
        );
    }

    #[test]
    fn a_rootlist_reply_lets_the_next_move_build_on_its_revision() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("A", None), ("B", None), ("C", None)],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "A");
        let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 2);
        assert!(window.drag(&mut transport, &mut queue, from, to).is_some());
        window.library.apply(&Update::PlaylistMoved {
            revision: revision("root-2"),
        });

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "B");
        let to = sidebar_boundary(&window, &frame, &["B", "A", "C"], "Playlist", 3);

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            Some(Command::MovePlaylist {
                revision: revision("root-2"),
                from: 0,
                to: 3,
            }),
            "the next move names the reply's revision"
        );
    }

    #[test]
    fn a_playlist_carried_between_two_rows_draws_the_rule_and_no_fill() {
        let tokens = Tokens::DARK;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("A", None), ("B", None), ("C", None)],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "A");
        let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 2);

        let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);

        assert!(marker(&frame).is_some(), "a reorder draws the rule");
        assert!(
            !frame
                .shapes
                .iter()
                .any(|clipped| matches!(&clipped.shape, egui::Shape::Rect(shape)
                    if shape.fill == tokens.palette.row_selected
                        && sidebar_rect(&window).contains_rect(shape.rect))),
            "a reorder draws no fill"
        );
    }

    #[test]
    fn a_playlist_carried_over_the_queue_is_never_queued() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("A", None), ("B", None)],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "A");
        let to = pane_rect_when_shown(&window).center();
        let before = entries(&queue);

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);

        assert_eq!(entries(&queue), before, "a playlist is no queue entry");
        assert_eq!(sidebar_order(&window), ["A", "B"]);
        assert!(window.library.rootlist_movable(), "and nothing was written");
    }

    #[test]
    fn a_playlist_carried_over_the_queue_button_is_refused() {
        // The queue column is hidden below this width, so the playbar's button
        // is the only queue target there is.
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1100.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("A", None), ("B", None)],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "A");
        let to = queue_button_at(&window).center();
        let before = entries(&queue);

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);

        assert_eq!(entries(&queue), before);
        assert!(window.library.rootlist_movable());
    }

    #[test]
    fn a_drop_while_a_write_is_pending_is_refused() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("A", None), ("B", None), ("C", None)],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "A");
        let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 2);
        assert!(window.drag(&mut transport, &mut queue, from, to).is_some());

        // A second drop would name a revision the first is already replacing.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "B");
        let to = sidebar_boundary(&window, &frame, &["B", "A", "C"], "Playlist", 3);

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
        assert_eq!(sidebar_order(&window), ["B", "A", "C"]);
    }

    #[test]
    fn a_queue_drop_still_works_while_a_sidebar_write_is_pending() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        sidebar_playlists(
            &mut window,
            &[("A", None), ("B", None), ("C", None)],
            Some(rootlist_access(true)),
        );
        assert!(
            window.library.move_playlist(0, 2, "playlist:A").is_some(),
            "the sidebar write is in flight"
        );

        // A queue insert names no revision, so the gate leaves it alone.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let from = row_line(&frame, view_rect(&window), "p1");
        let to = below(row_line(&frame, pane, "t3"), -0.25);
        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
        assert_eq!(entries(&queue), ["t0", "t1", "t2", "list:p1", "t3", "t4"]);

        // A page move is the other write, and it waits.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let from = row_line(&frame, view, "p0");
        let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);
        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
        assert_eq!(page_rows(&window), ["list:p0", "list:p1", "list:p2"]);
    }

    #[test]
    fn a_sidebar_drop_on_a_frame_another_region_took_moves_nothing() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        // A page ahead of the one the drop returns to, so Back has a load to ask
        // for and takes the frame.
        let _ = window.library.select("spotify:playlist:other");
        sidebar_playlists(
            &mut window,
            &[("A", None), ("B", None), ("C", None)],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "A");
        let to = sidebar_boundary(&window, &frame, &["A", "B", "C"], "Playlist", 2);
        let _ = window.hold_drag(&mut transport, &mut queue, from, to);

        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![window.release(to), key(egui::Key::BrowserBack, true)],
        );

        assert!(
            matches!(command, Some(Command::LoadTracks { .. })),
            "another region took the frame: {command:?}"
        );
        assert_eq!(sidebar_order(&window), ["A", "B", "C"]);
        assert!(window.library.rootlist_movable(), "and writes nothing");
    }

    /// A queue of entries whose URIs are real tracks, so one can be added.
    fn queue_of_tracks(uris: &[&str], transport: &mut Transport) -> Queue {
        let tracks: Vec<TrackEntry> = uris
            .iter()
            .map(|uri| TrackEntry {
                uri: (*uri).to_owned(),
                title: (*uri).to_owned(),
                ..track(0, None)
            })
            .collect();
        let mut queue = Queue::default();
        queue.start(&tracks, 0, None, transport);
        queue
    }

    #[test]
    fn dropping_a_track_on_a_playlist_row_sends_one_add() {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &[TRACK_A]);
        sidebar_playlists(
            &mut window,
            &[("A", Some(2)), ("B", Some(3))],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), TRACK_A);
        let to = sidebar_row_rect(&window, &frame, "B", "Playlist · 3 tracks").center();

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            Some(Command::AddTracks {
                playlist: "playlist:B".to_owned(),
                revision: revision("root-1"),
                from_index: 3,
                tracks: vec![TRACK_A.to_owned()],
            }),
            "the length is the index an append names"
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "Playlist · 3 tracks", sidebar_rect(&window)).is_some(),
            "the count waits for the reply"
        );
    }

    #[test]
    fn a_confirmed_add_counts_one_more_on_the_row() {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &[TRACK_A]);
        sidebar_playlists(
            &mut window,
            &[("A", Some(2)), ("B", Some(3))],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), TRACK_A);
        let to = sidebar_row_rect(&window, &frame, "B", "Playlist · 3 tracks").center();
        assert!(window.drag(&mut transport, &mut queue, from, to).is_some());

        window.library.apply(&Update::TrackAdded {
            playlist: "playlist:B".to_owned(),
            revision: revision("root-2"),
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(
            text_in(&frame, "Playlist · 4 tracks", sidebar_rect(&window)).is_some(),
            "the row answers the add"
        );
        assert_eq!(
            sidebar_order(&window),
            ["A", "B"],
            "an add reorders nothing"
        );
    }

    #[test]
    fn a_failed_add_leaves_the_count_and_says_so() {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &[TRACK_A]);
        sidebar_playlists(
            &mut window,
            &[("A", Some(2)), ("B", Some(3))],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), TRACK_A);
        let to = sidebar_row_rect(&window, &frame, "B", "Playlist · 3 tracks").center();
        assert!(window.drag(&mut transport, &mut queue, from, to).is_some());

        window.library.apply(&Update::AddFailed {
            playlist: "playlist:B".to_owned(),
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(text_in(&frame, "Playlist · 3 tracks", sidebar_rect(&window)).is_some());
        assert!(
            text_in(
                &frame,
                "Could not add those tracks. Try again.",
                sidebar_rect(&window)
            )
            .is_some(),
            "the list says what could not be done"
        );
    }

    #[test]
    fn a_track_over_a_playlist_row_draws_the_fill_and_no_rule() {
        let tokens = Tokens::DARK;
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &[TRACK_A]);
        sidebar_playlists(
            &mut window,
            &[("A", Some(2)), ("B", Some(3))],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), TRACK_A);
        let row = sidebar_row_rect(&window, &frame, "B", "Playlist · 3 tracks");
        let to = row.center();

        let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);

        assert!(
            frame
                .shapes
                .iter()
                .any(|clipped| matches!(&clipped.shape, egui::Shape::Rect(shape)
                if shape.fill == tokens.palette.row_selected
                    && (shape.rect.top() - row.top()).abs() < 1.0)),
            "the row under the pointer takes the fill"
        );
        assert!(marker(&frame).is_none(), "an add draws no rule");
    }

    /// A pick of one real track, which an add can take whole.
    fn a_pick(uri: &str) -> TrackEntry {
        TrackEntry {
            uri: uri.to_owned(),
            title: "A Title".to_owned(),
            artists: "A Band".to_owned(),
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

    /// The column's offer of two drawn rows standing for true rows 3 and 7.
    fn narrowed_column(drawn: f32) -> ColumnOffer {
        let zone = crate::landing::Zone {
            hit: Rect::from_min_size(Pos2::new(10.0, 200.0), Vec2::new(200.0, 8.0 * drawn)),
            list: Rect::from_min_size(Pos2::new(10.0, 200.0), Vec2::new(200.0, 8.0 * drawn)),
            row_h: drawn,
            first: 0,
            count: 2,
        };
        ColumnOffer {
            zone,
            drawn,
            rows: Some(vec![3, 7]),
        }
    }

    /// Eight playlists, all of them able to take a track.
    fn eight_playlists(window: &mut Window) {
        sidebar_playlists(
            window,
            &[
                ("a", Some(0)),
                ("b", Some(1)),
                ("c", Some(2)),
                ("d", Some(3)),
                ("e", Some(4)),
                ("f", Some(5)),
                ("g", Some(6)),
                ("h", Some(7)),
            ],
            Some(rootlist_access(true)),
        );
    }

    #[test]
    fn a_narrowed_offer_adds_to_the_true_playlist() {
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        eight_playlists(&mut window);
        let column = narrowed_column(sidebar::ROW_H);
        let carry = Carry {
            entries: vec![a_pick(TRACK_A)],
            rows: vec![0],
            ..carried_rows(egui::Id::unique("list"), vec![0])
        };
        // The pointer is over the second row the column drew.
        let pointer = Some(Pos2::new(
            column.zone.list.left() + 20.0,
            column.zone.list.top() + column.zone.row_h * 1.5,
        ));

        let target = add_target(&carry, Some(&column), &window.library, pointer)
            .expect("the row under the pointer takes the add");

        let Target::Add(slot) = target else {
            panic!("a drop on a row is an add, not {target:?}");
        };
        assert_eq!(
            slot.row, 7,
            "the drawn slot names the playlist it stands for"
        );
        let drawn = crate::landing::slot_at(pointer, Some(&column.zone), column.drawn)
            .expect("the drawn row");
        assert_eq!(
            slot.rect, drawn.rect,
            "the highlight stays on the row that was drawn"
        );
    }

    #[test]
    fn a_narrowed_offer_refuses_a_rootlist_move() {
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        eight_playlists(&mut window);
        let column = narrowed_column(sidebar::ROW_H);
        let carry = Carry {
            rows: vec![3],
            playlist: Some("playlist:d".to_owned()),
            ..carried_rows(egui::Id::unique("list"), vec![0])
        };
        // Between the two drawn rows: a boundary a reorder would land on.
        let pointer = Some(Pos2::new(
            column.zone.list.left() + 20.0,
            column.zone.list.top() + column.zone.row_h + 1.0,
        ));

        assert_eq!(
            rootlist_target(&carry, Some(&column), &window.library, pointer),
            None,
            "a narrowed column would move the wrong playlist"
        );
    }

    #[test]
    fn a_row_that_can_take_a_track_is_offered_even_when_the_rootlist_is_not() {
        let tokens = Tokens::DARK;
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &[TRACK_A]);
        // The rows name addable playlists of their own; the rootlist's read says
        // the order is not the list's to change, which only a move needs.
        let entry = |name: &str, tracks: usize| PlaylistEntry {
            composed: Vec::new(),
            uri: format!("playlist:{name}"),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: Some(tracks),
            access: Some(rootlist_access(true)),
        };
        window.library.playlists = Load::Ready(vec![entry("A", 2), entry("B", 3)]);
        window
            .library
            .apply(&Update::RootlistAccess(rootlist_access(false)));
        assert!(!window.library.rootlist_movable());
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), TRACK_A);
        let row = sidebar_row_rect(&window, &frame, "B", "Playlist · 3 tracks");
        let to = row.center();

        let (_, held) = window.hold_drag(&mut transport, &mut queue, from, to);
        assert!(
            held.shapes
                .iter()
                .any(|clipped| matches!(&clipped.shape, egui::Shape::Rect(shape)
                if shape.fill == tokens.palette.row_selected
                    && (shape.rect.top() - row.top()).abs() < 1.0)),
            "the row under the pointer takes the fill"
        );
        assert!(marker(&held).is_none(), "an add draws no rule");

        let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
        assert_eq!(
            command,
            Some(Command::AddTracks {
                playlist: "playlist:B".to_owned(),
                revision: revision("root-1"),
                from_index: 3,
                tracks: vec![TRACK_A.to_owned()],
            }),
            "a row that can take the track takes it"
        );

        window.library.apply(&Update::TrackAdded {
            playlist: "playlist:B".to_owned(),
            revision: revision("root-2"),
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "Playlist · 4 tracks", sidebar_rect(&window)).is_some(),
            "the row answers the add"
        );

        // A playlist row still does not lift here: a carry only means a move.
        let from = row_line(&frame, sidebar_rect(&window), "A");
        let (_, held) = window.hold_drag(&mut transport, &mut queue, from, to);
        assert!(
            carry::carried(&window.ctx).is_none(),
            "a row order that is not the wire's does not lift"
        );
        assert!(marker(&held).is_none(), "and no rule is drawn");
    }

    #[test]
    fn a_track_over_a_row_that_cannot_take_it_draws_nothing() {
        let tokens = Tokens::DARK;
        // The rootlist is movable in every case, so the column is offered and
        // only what the entry itself holds keeps it from taking the track.
        let cases: Vec<(&str, Option<usize>, Option<PlaylistAccess>)> = vec![
            ("not editable", Some(3), Some(rootlist_access(false))),
            ("no count", None, Some(rootlist_access(true))),
            ("no revision", Some(3), None),
        ];
        for (why, tracks, entry) in cases {
            let mut transport = playing(cover("abc"));
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            page_of_tracks(&mut window, &[TRACK_A]);
            window.library.playlists = Load::Ready(vec![PlaylistEntry {
                composed: Vec::new(),
                uri: "playlist:B".to_owned(),
                name: "B".to_owned(),
                named: true,
                cover: None,
                description: None,
                owner: None,
                tracks,
                access: entry,
            }]);
            window
                .library
                .apply(&Update::RootlistAccess(rootlist_access(true)));
            assert!(
                window.library.rootlist_movable(),
                "{why}: the column has to be offered"
            );
            let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
            let from = row_line(&frame, view_rect(&window), TRACK_A);
            let to = sidebar_row_rect(&window, &frame, "B", "Playlist").center();

            let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
            assert!(
                !frame.shapes.iter().any(
                    |clipped| matches!(&clipped.shape, egui::Shape::Rect(shape)
                    if shape.fill == tokens.palette.row_selected
                        && sidebar_rect(&window).contains_rect(shape.rect))
                ),
                "{why}: no fill"
            );
            assert!(marker(&frame).is_none(), "{why}: no rule");

            let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
            assert_eq!(command, None, "{why}: nothing is written");
        }
    }

    #[test]
    fn dropping_on_the_open_playlists_own_row_adds_nothing() {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &[TRACK_A]);
        sidebar_playlists(&mut window, &[("A", Some(2))], Some(rootlist_access(true)));
        // The sidebar's row is the page's own playlist, by its own URI.
        if let Load::Ready(rows) = &mut window.library.playlists {
            rows[0].uri = PLAYLIST.to_owned();
        }
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), TRACK_A);
        let to = sidebar_row_rect(&window, &frame, "A", "Playlist · 2 tracks").center();

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    }

    #[test]
    fn an_episode_dropped_on_a_row_is_refused() {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &["spotify:episode:1"]);
        sidebar_playlists(&mut window, &[("A", Some(2))], Some(rootlist_access(true)));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "spotify:episode:1");
        let to = sidebar_row_rect(&window, &frame, "A", "Playlist · 2 tracks").center();

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
    }

    #[test]
    fn a_track_from_the_queue_can_be_added() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of_tracks(&[TRACK_A, TRACK_B], &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(&mut window, &[("A", Some(2))], Some(rootlist_access(true)));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, pane_rect_when_shown(&window), TRACK_B);
        let to = sidebar_row_rect(&window, &frame, "A", "Playlist · 2 tracks").center();

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            Some(Command::AddTracks {
                playlist: "playlist:A".to_owned(),
                revision: revision("root-1"),
                from_index: 2,
                tracks: vec![TRACK_B.to_owned()],
            })
        );
    }

    #[test]
    fn a_block_of_three_rows_is_one_add_in_row_order() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of_tracks(
            &[TRACK_A, TRACK_B, "spotify:track:c", TRACK_A, TRACK_B],
            &mut transport,
        );
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(&mut window, &[("A", Some(2))], Some(rootlist_access(true)));
        // Rows 1, 2 and 3 are one picked block: the first entry is playing, so
        // it is never lifted.
        queue.press(1, Press::Replace);
        queue.press(2, Press::Toggle);
        queue.press(3, Press::Toggle);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let from = row_line(&frame, pane, TRACK_B);
        let to = sidebar_row_rect(&window, &frame, "A", "Playlist · 2 tracks").center();

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            Some(Command::AddTracks {
                playlist: "playlist:A".to_owned(),
                revision: revision("root-1"),
                from_index: 2,
                tracks: vec![
                    TRACK_B.to_owned(),
                    "spotify:track:c".to_owned(),
                    TRACK_A.to_owned(),
                ],
            }),
            "one command, and the pick in the order the rows were taken"
        );
    }

    #[test]
    fn a_pick_with_one_episode_marks_no_target_and_sends_nothing() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of_tracks(&[TRACK_A, "spotify:episode:1", TRACK_B], &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(&mut window, &[("A", Some(2))], Some(rootlist_access(true)));
        queue.press(1, Press::Replace);
        queue.press(2, Press::Toggle);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let from = row_line(&frame, pane, "spotify:episode:1");
        let to = sidebar_row_rect(&window, &frame, "A", "Playlist · 2 tracks").center();

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            None,
            "a mixed pick is refused whole, so nothing is offered"
        );
    }

    #[test]
    fn a_track_between_two_rows_is_not_a_reorder() {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &[TRACK_A]);
        sidebar_playlists(
            &mut window,
            &[("A", Some(2)), ("B", Some(3))],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), TRACK_A);
        // The hairline between two rows: a row above it, a row below it.
        let row = sidebar_row_rect(&window, &frame, "A", "Playlist · 2 tracks");
        let to = egui::pos2(row.center().x, row.bottom() + space::PX / 2.0);

        let command = window.drag(&mut transport, &mut queue, from, to);

        assert_eq!(
            command,
            Some(Command::AddTracks {
                playlist: "playlist:A".to_owned(),
                revision: revision("root-1"),
                from_index: 2,
                tracks: vec![TRACK_A.to_owned()],
            }),
            "the gap belongs to the row above, so it is an add and no reorder"
        );
        assert_eq!(sidebar_order(&window), ["A", "B"], "the list did not move");
    }

    #[test]
    fn one_pointer_position_never_draws_a_rule_and_a_fill() {
        let tokens = Tokens::DARK;
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &[TRACK_A]);
        sidebar_playlists(
            &mut window,
            &[("A", Some(2)), ("B", Some(3)), ("C", Some(4))],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let area = sidebar_rect(&window);
        let pitch = sidebar::ROW_H + space::PX;
        let at = egui::pos2(
            area.left() + 100.0,
            sidebar_row_top(&frame, area, "B", "Playlist · 3 tracks") + pitch,
        );

        let playlist_from = row_line(&frame, area, "A");
        let (_, playlist_frame) = window.hold_drag(&mut transport, &mut queue, playlist_from, at);
        // Let the playlist drag go somewhere it lands nowhere, so the next drag
        // starts from a clean frame.
        let nowhere = view_rect(&window).center();
        let _ = window.frame(&mut transport, &mut queue, vec![window.release(nowhere)]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let track_from = row_line(&frame, view_rect(&window), TRACK_A);
        let (_, track_frame) = window.hold_drag(&mut transport, &mut queue, track_from, at);

        for (what, frame) in [("a playlist", &playlist_frame), ("a track", &track_frame)] {
            let rules = marker(frame).is_some();
            let filled = frame.shapes.iter().any(|clipped| {
                matches!(&clipped.shape, egui::Shape::Rect(shape)
                    if shape.fill == tokens.palette.row_selected
                        && sidebar_rect(&window).contains_rect(shape.rect))
            });
            assert!(
                rules ^ filled,
                "{what} must draw exactly one of a rule or a fill"
            );
        }
        assert!(
            marker(&playlist_frame).is_some(),
            "a playlist draws the rule"
        );
        assert!(
            !marker(&track_frame).is_some(),
            "a track draws the fill and no rule"
        );
    }

    #[test]
    fn a_sidebar_drop_outside_the_column_writes_nothing() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("A", None), ("B", None)],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, sidebar_rect(&window), "A");
        let to = view_rect(&window).center();

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);

        assert_eq!(sidebar_order(&window), ["A", "B"]);
        assert!(window.library.rootlist_movable());
    }

    #[test]
    fn a_drop_on_a_frame_another_region_took_moves_nothing() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let from = row_line(&frame, view, "p0");
        let to = page_boundary(&frame, view, &["p0", "p1", "p2"], 2);
        let _ = window.hold_drag(&mut transport, &mut queue, from, to);

        // Enter plays the row the drag left focused, so the drop finds the one
        // command taken.
        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![window.release(to), key(egui::Key::Enter, true)],
        );
        assert!(
            matches!(command, Some(Command::Play { .. })),
            "another region took the frame: {command:?}"
        );
        assert_eq!(
            page_rows(&window),
            ["list:p0", "list:p1", "list:p2"],
            "a drop behind another region's command moves nothing"
        );
        assert!(window.library.movable(), "and writes nothing");
    }

    #[test]
    fn escape_cancels_a_drag_and_does_not_close_the_drawer() {
        // Only the rail has a drawer, so this window is narrow and the drag
        // happens over its view.
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(800.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let toggle = topbar_buttons(&frame)[0];
        let _ = window.click(&mut transport, &mut queue, toggle.center());
        assert_eq!(window.drawer, Drawer::Open);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        // The drawer covers the left of the view, so the press lands on the row
        // further along it.
        let view = view_rect(&window);
        let from = egui::pos2(view.center().x, row_line(&frame, view, "p1").y);
        let to = below(from, 2.0);
        let _ = window.hold_drag(&mut transport, &mut queue, from, to);
        assert!(
            carry::carried(&window.ctx).is_some(),
            "the drag has to lift before Escape can cancel it"
        );

        let (_, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Escape, true)],
        );
        assert!(
            carry::carried(&window.ctx).is_none(),
            "Escape drops what is carried"
        );
        let release = window.release(to);
        let _ = window.frame(&mut transport, &mut queue, vec![release]);
        assert_eq!(
            window.drawer,
            Drawer::Open,
            "the Escape that cancelled the drag is not the drawer's"
        );
    }

    #[test]
    fn a_three_point_twitch_in_a_double_click_does_not_lift() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
        let before = entries(&queue);
        // The second press of a double click, three points off the first.
        let _ = window.click(&mut transport, &mut queue, at);

        for event in [
            egui::Event::PointerMoved(at),
            crate::view::test_support::button(at, true),
            egui::Event::PointerMoved(at + Vec2::new(3.0, 0.0)),
            crate::view::test_support::button(at + Vec2::new(3.0, 0.0), false),
        ] {
            let (_, _) = window.frame(&mut transport, &mut queue, vec![event]);
            assert!(
                carry::carried(&window.ctx).is_none(),
                "three points is a click, not a drag"
            );
        }
        assert_eq!(entries(&queue), before, "a click reorders nothing");
    }

    #[test]
    fn a_secondary_release_cancels_the_drag_instead_of_landing_it() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let from = row_line(&frame, pane, "t2");
        let to = below(from, 1.75);
        let _ = window.hold_drag(&mut transport, &mut queue, from, to);
        assert!(carry::carried(&window.ctx).is_some());
        let before = entries(&queue);

        // A right press and release over the zone is not the primary release that
        // lands the block, even though egui drops the payload when the drag ends.
        let secondary = |pressed| side(to, egui::PointerButton::Secondary, pressed);
        let _ = window.frame(&mut transport, &mut queue, vec![secondary(true)]);
        let _ = window.frame(&mut transport, &mut queue, vec![secondary(false)]);
        assert!(
            carry::carried(&window.ctx).is_none(),
            "the right release ends the drag"
        );
        assert_eq!(entries(&queue), before, "a right release lands nothing");
    }

    #[test]
    fn a_held_payload_with_no_button_down_is_cleared() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, pane_rect_when_shown(&window), "t2");
        let _ = window.hold_drag(&mut transport, &mut queue, from, below(from, 1.75));
        assert!(carry::carried(&window.ctx).is_some());

        // The guard against a release the window never delivered: the button is
        // forced up under a payload egui has not cleared.
        window
            .ctx
            .input_mut(|input| input.pointer = Default::default());
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            carry::carried(&window.ctx).is_none(),
            "a carry with no button down is dropped"
        );
        assert_eq!(
            entries(&queue),
            ["t0", "t1", "t2", "t3"],
            "a payload dropped by the guard lands nowhere"
        );
    }

    #[test]
    fn a_queue_that_restarted_under_a_drag_takes_no_drop() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let from = row_line(&frame, pane, "t2");
        let to = below(from, 1.75);
        let _ = window.hold_drag(&mut transport, &mut queue, from, to);

        // Something else fills the queue while the block is still held.
        let others: Vec<TrackEntry> = (0..5).map(|index| track(index + 10, None)).collect();
        let _ = queue.start(&others, 0, None, &mut transport);
        let release = window.release(to);
        let _ = window.frame(&mut transport, &mut queue, vec![release]);

        assert_eq!(
            entries(&queue),
            ["t10", "t11", "t12", "t13", "t14"],
            "a drop must not move the rows of a queue that replaced its own"
        );
    }

    #[test]
    fn enter_does_not_play_a_focused_row_while_a_block_is_carried() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let pane = pane_rect_when_shown(&window);
        clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, pane, "t3");
        let _ = window.hold_drag(&mut transport, &mut queue, from, below(from, 1.75));
        assert!(carry::carrying(&window.ctx), "a block is in flight");
        let before = entries(&queue);
        let upcoming = queue.first_upcoming();

        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Enter, true)],
        );
        assert_eq!(command, None, "a jump would move the queue under the drop");
        assert_eq!(queue.first_upcoming(), upcoming);
        assert_eq!(entries(&queue), before);
    }

    #[test]
    fn the_ghost_is_painted_on_the_frame_of_the_lift() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let from = row_line(&frame, pane, "t2");
        let to = below(from, 0.75);
        let size = Vec2::new(pane.width() - 2.0 * space::S3, QUEUE_ROW_H);

        let mut events = crate::view::test_support::drag_events(from, to);
        events.pop();
        let mut pointer = from;
        let mut lifted = None;
        for event in events {
            if let egui::Event::PointerMoved(at) = event {
                pointer = at;
            }
            let (_, frame) = window.frame(&mut transport, &mut queue, vec![event]);
            if carry::carried(&window.ctx).is_some() {
                lifted = Some(frame);
                break;
            }
        }

        let frame = lifted.expect("the drag lifts the row");
        let rect = ghost_rect(pointer, size);
        assert_eq!(
            ghost_fill(&frame, rect),
            Some(Tokens::DARK.palette.surface_raised),
            "the ghost is not drawn raised on the frame that lifted it"
        );
        assert!(
            shadows(&frame)
                .iter()
                .any(|cast| cast.contains(rect.center())),
            "a landed ghost casts a shadow: {:?}",
            shadows(&frame)
        );
        let mark = marker(&frame).expect("a carried block draws its marker");
        assert!(
            pane.contains_rect(mark),
            "the marker is not inside the pane: {mark:?}"
        );

        let release = window.release(to);
        let (_, frame) = window.frame(&mut transport, &mut queue, vec![release]);
        assert!(
            ghost_fill(&frame, ghost_rect(to, size)).is_none(),
            "a dropped block is gone on the frame that dropped it"
        );
        assert!(
            marker(&frame).is_none(),
            "a release marks nothing: it has landed"
        );
    }

    #[test]
    fn a_refused_pointer_paints_a_flat_ghost_and_no_marker() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let from = row_line(&frame, pane, "t2");
        let to = view_rect(&window).center();
        let size = Vec2::new(pane.width() - 2.0 * space::S3, QUEUE_ROW_H);

        let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
        let rect = ghost_rect(to, size);
        assert_eq!(
            ghost_fill(&frame, rect),
            Some(Tokens::DARK.palette.surface),
            "a refused ghost is flat"
        );
        assert!(
            shadows(&frame).is_empty(),
            "a refused ghost casts no shadow: {:?}",
            shadows(&frame)
        );
        assert!(marker(&frame).is_none(), "a refused pointer marks nothing");
    }

    #[test]
    fn nothing_hovers_while_a_row_is_carried() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let from = row_line(&frame, pane, "t2");
        let hover = row_line(&frame, pane, "t4");
        let washes = |output: &egui::FullOutput| {
            let tokens = Tokens::DARK;
            output
                .shapes
                .iter()
                .filter(|clipped| {
                    matches!(&clipped.shape, egui::Shape::Rect(shape)
                        if shape.fill == tokens.palette.row_hover && pane.contains_rect(shape.rect))
                })
                .count()
        };

        // The row's wash eases in, so the pointer rests on it for a few frames.
        let mut resting = None;
        for event in [
            Some(egui::Event::PointerMoved(hover)),
            None,
            None,
            None,
            None,
        ] {
            let events = event.into_iter().collect();
            resting = Some(window.frame(&mut transport, &mut queue, events).1);
        }
        assert_eq!(
            washes(&resting.expect("a frame ran")),
            1,
            "the row under the pointer washes"
        );

        let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, hover);
        assert_eq!(
            washes(&frame),
            0,
            "a row under a carried block must not wash"
        );
    }

    #[test]
    fn the_drawn_fill_and_the_drop_area_share_one_rect() {
        // The production area comes from `queue_button(bar)` and the fill from
        // the draw, so an offset between them flips one of the four edges.
        for width in [900.0, 1100.0] {
            let mut transport = playing(cover("abc"));
            let mut queue = queue_of(4, &mut transport);
            let mut window = Window::new(Vec2::new(width, 900.0));
            playlist_page(&mut window, &["p0", "p1"]);
            let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
            let from = row_line(&frame, view_rect(&window), "p1");
            let (_, frame) = window.hold_drag(
                &mut transport,
                &mut queue,
                from,
                queue_button_at(&window).center(),
            );
            let fill = claimed_fill(&frame, &window)
                .unwrap_or_else(|| panic!("the queue button does not fill at {width}"));
            assert_eq!(fill.size(), Vec2::splat(extent::BUTTON), "at {width}");

            for edge in [
                Vec2::new(22.0, 0.0),
                Vec2::new(-22.0, 0.0),
                Vec2::new(0.0, 22.0),
                Vec2::new(0.0, -22.0),
            ] {
                let (_, frame) = window.frame(
                    &mut transport,
                    &mut queue,
                    vec![egui::Event::PointerMoved(fill.center() + edge)],
                );
                assert_eq!(
                    claimed_fill(&frame, &window),
                    Some(fill),
                    "the inside edge {edge:?} at {width} is outside the drop area"
                );
            }
            for edge in [Vec2::new(22.5, 0.0), Vec2::new(0.0, 22.5)] {
                let (_, frame) = window.frame(
                    &mut transport,
                    &mut queue,
                    vec![egui::Event::PointerMoved(fill.center() + edge)],
                );
                assert_eq!(
                    claimed_fill(&frame, &window),
                    None,
                    "past the edge {edge:?} at {width} still fills"
                );
            }

            let corner = fill.center() + Vec2::new(22.0, 22.0);
            let (command, _) =
                window.frame(&mut transport, &mut queue, vec![window.release(corner)]);
            assert_eq!(command, None);
            assert_eq!(
                entries(&queue),
                ["t0", "t1", "t2", "t3", "list:p1"],
                "the inclusive corner at {width} is outside the drop area"
            );
        }
    }

    #[test]
    fn a_page_list_lift_resolves_its_target_on_the_frame_it_lifts() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(900.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        let to = queue_button_at(&window).center();

        let mut pointer = from;
        let mut lifted = None;
        for event in carried_frame_events(from, to) {
            if let egui::Event::PointerMoved(at) = &event {
                pointer = *at;
            }
            let (_, frame) = window.frame(&mut transport, &mut queue, vec![event]);
            if carry::carried(&window.ctx).is_some() {
                lifted = Some(frame);
                break;
            }
        }

        let frame = lifted.expect("the page list lifts on the jump");
        assert_eq!(pointer, to, "the lift frame is the frame the pointer jumps");
        let fill = claimed_fill(&frame, &window).expect("the button fills on the lift frame");
        assert_eq!(
            fill.size(),
            Vec2::splat(extent::BUTTON),
            "the fill keeps the glyph's box"
        );
        assert_eq!(
            ghost_at(&frame, to),
            Some(Tokens::DARK.palette.surface_raised),
            "the ghost is raised on the lift frame"
        );
        assert!(
            !shadows(&frame).is_empty(),
            "a raised ghost casts a shadow: {:?}",
            shadows(&frame)
        );
        assert!(marker(&frame).is_none(), "an add draws no marker");

        let (_, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
        assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
    }

    #[test]
    fn at_900_a_track_dropped_on_the_queue_button_is_appended_and_nothing_plays() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(900.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        let to = queue_button_at(&window).center();

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
        assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t0"),
            "an add must not start anything"
        );
    }

    #[test]
    fn at_1100_the_extras_queue_button_takes_the_drop() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1100.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        let to = queue_button_at(&window).center();

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
        assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
    }

    #[test]
    fn over_the_queue_button_it_fills_the_ghost_rises_and_no_marker_is_drawn() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(900.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        let to = queue_button_at(&window).center();

        let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
        let fill = claimed_fill(&frame, &window).expect("the button fills under the carry");
        assert_eq!(fill.size(), Vec2::splat(extent::BUTTON));
        assert_eq!(
            ghost_at(&frame, to),
            Some(Tokens::DARK.palette.surface_raised),
            "an add draws the ghost raised"
        );
        assert!(marker(&frame).is_none(), "an add draws no marker");
    }

    #[test]
    fn the_drop_area_reaches_past_the_glyph() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(900.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        let to = queue_button_at(&window).center() + Vec2::new(19.0, 19.0);

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
        assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
    }

    #[test]
    fn the_empty_column_below_a_short_queue_takes_the_row_at_the_end() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(2, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let last = row_line(&frame, pane, "t1");
        let from = row_line(&frame, view_rect(&window), "p0");
        let to = Pos2::new(pane.center().x, pane.bottom() - 4.0);
        assert!(
            to.y > last.y + QUEUE_ROW_H,
            "the pointer must sit below the list, not on a row: {to:?}"
        );

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
        assert_eq!(entries(&queue), ["t0", "t1", "list:p0"]);
        assert_eq!(
            queue.first_upcoming() + queue.upcoming().len(),
            3,
            "the row lands at the end"
        );
    }

    #[test]
    fn an_empty_queue_takes_the_queue_buttons_add() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(900.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        let to = queue_button_at(&window).center();

        let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
        assert!(
            claimed_fill(&frame, &window).is_some(),
            "the button claims the add"
        );
        let (_, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
        assert_eq!(entries(&queue), ["list:p1"]);
        assert!(
            !queue.can_previous(),
            "the button queues the row, it does not start it"
        );
    }

    #[test]
    fn a_queue_row_carried_into_a_narrow_window_is_not_moved_by_the_button() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, pane_rect_when_shown(&window), "t2");
        let _ = window.hold_drag(
            &mut transport,
            &mut queue,
            from,
            view_rect(&window).center(),
        );
        let before = entries(&queue);

        window.size = Vec2::new(900.0, 900.0);
        let to = queue_button_at(&window).center();
        let (_, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(to)],
        );
        assert!(
            claimed_fill(&frame, &window).is_none(),
            "the button never moves a queue row"
        );
        assert_eq!(
            ghost_at(&frame, to),
            Some(Tokens::DARK.palette.surface),
            "the ghost is flat"
        );
        let (_, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
        assert_eq!(entries(&queue), before, "a queue row stays put");
    }

    #[test]
    fn a_page_list_carry_lifted_wide_and_dropped_narrow_is_appended() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        let (_, frame) = window.hold_drag(
            &mut transport,
            &mut queue,
            from,
            view_rect(&window).center(),
        );
        assert!(
            claimed_fill(&frame, &window).is_none(),
            "the pane is on screen, so the button is not a target"
        );

        window.size = Vec2::new(900.0, 900.0);
        let to = queue_button_at(&window).center();
        let (_, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(to)],
        );
        assert!(
            claimed_fill(&frame, &window).is_some(),
            "the narrow button takes the add"
        );
        let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
        assert_eq!(command, None);
        assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
    }

    #[test]
    fn above_the_now_playing_width_the_queue_button_takes_no_drop() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        let to = queue_button_at(&window).center();
        let before = entries(&queue);

        let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
        assert!(
            claimed_fill(&frame, &window).is_none(),
            "the pane is on screen, so the button is not a target"
        );
        assert_eq!(
            ghost_at(&frame, to),
            Some(Tokens::DARK.palette.surface),
            "the ghost is flat"
        );
        assert!(marker(&frame).is_none(), "no target, no marker");
        let (_, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
        assert_eq!(entries(&queue), before, "a refused drop changes nothing");
    }

    #[test]
    fn escape_over_the_queue_button_adds_nothing() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(900.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        let to = queue_button_at(&window).center();
        let _ = window.hold_drag(&mut transport, &mut queue, from, to);
        assert!(
            carry::carried(&window.ctx).is_some(),
            "the drag has to lift"
        );
        let before = entries(&queue);

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Escape, true)],
        );
        assert!(
            carry::carried(&window.ctx).is_none(),
            "Escape drops what is carried"
        );
        let _ = window.frame(&mut transport, &mut queue, vec![window.release(to)]);
        assert_eq!(entries(&queue), before, "a cancelled add changes nothing");
    }

    /// A queued window whose pane the user hid: the view takes the width the
    /// panel had, and the panel draws nothing.
    #[test]
    fn a_hidden_pane_widens_the_view_and_a_shown_one_narrows_it() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1240.0, 900.0));

        let (_, shown) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let narrow = view_rect(&window);
        assert!(
            text_in(&shown, "t2", pane).is_some(),
            "the pane is not drawing its queue"
        );

        window.pane = layout::PanePref::Hidden;
        let (_, hidden) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&hidden, "t2", pane).is_none(),
            "a hidden pane still drew a queue row"
        );
        let wide = view_rect(&window);
        assert!(
            wide.width() > narrow.width() + pane.width(),
            "the view took {} of the pane's {}",
            wide.width() - narrow.width(),
            pane.width()
        );
    }

    #[test]
    fn at_900_and_at_1100_a_click_on_the_queue_button_changes_nothing() {
        for width in [900.0, 1100.0] {
            let mut transport = playing(cover("abc"));
            let mut queue = queue_of(4, &mut transport);
            let mut window = Window::new(Vec2::new(width, 900.0));
            let at = queue_button_at(&window).center();
            let before = entries(&queue);
            assert_eq!(
                layout::pane_button(width, window.pane),
                layout::PaneButton::Unavailable,
                "{width}"
            );

            assert_eq!(window.click(&mut transport, &mut queue, at), None);
            assert_eq!(
                entries(&queue),
                before,
                "{width}: a click on the queue button does nothing"
            );
            assert_eq!(
                transport.now_playing.as_ref().map(|now| now.uri.as_str()),
                Some("t0"),
                "{width}: a click must not move the transport"
            );
            assert_eq!(
                window.pane,
                layout::PanePref::Auto,
                "{width}: a click where the pane cannot exist changed the choice"
            );
        }
    }

    #[test]
    fn at_1400_a_click_on_the_queue_button_hides_the_pane_and_a_second_shows_it() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "t2", pane_rect_when_shown(&window)).is_some(),
            "the pane is not drawn to begin with"
        );

        let at = queue_button_at(&window).center();
        assert_eq!(window.click(&mut transport, &mut queue, at), None);
        assert_eq!(
            window.pane,
            layout::PanePref::Hidden,
            "the click did not hide the pane"
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            painted_at(&frame, "t2").is_none(),
            "a hidden pane still drew a queue row"
        );

        let at = queue_button_at(&window).center();
        assert_eq!(window.click(&mut transport, &mut queue, at), None);
        assert_eq!(
            window.pane,
            layout::PanePref::Auto,
            "the second click did not show the pane"
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "t2", pane_rect_when_shown(&window)).is_some(),
            "a shown pane drew no queue row"
        );
    }

    #[test]
    fn the_choice_survives_navigation() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(&mut window, &[("Mix", None)], None);

        let at = queue_button_at(&window).center();
        assert_eq!(window.click(&mut transport, &mut queue, at), None);
        assert_eq!(window.pane, layout::PanePref::Hidden);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let row = sidebar_row_rect(&window, &frame, "Mix", "Playlist").center();
        let some = window.library.page().cloned();
        assert_eq!(some, None, "the library starts on the root");
        assert_eq!(
            window.click(&mut transport, &mut queue, row),
            Some(Command::LoadTracks {
                playlist: "playlist:Mix".to_owned()
            })
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist("playlist:Mix".to_owned())),
            "the sidebar row did not navigate"
        );
        assert_eq!(
            window.pane,
            layout::PanePref::Hidden,
            "navigation reset the choice about the pane"
        );

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            painted_at(&frame, "t2").is_none(),
            "the pane came back on the way to another page"
        );
    }

    #[test]
    fn above_1120_with_the_pane_hidden_the_queue_button_takes_a_list_drop() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        window.pane = layout::PanePref::Hidden;
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        let to = queue_button_at(&window).center();

        assert_eq!(window.drag(&mut transport, &mut queue, from, to), None);
        assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
    }

    #[test]
    fn a_drop_that_ends_on_the_button_does_not_toggle_the_pane() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        window.pane = layout::PanePref::Hidden;
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        let to = queue_button_at(&window).center();

        let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
        assert!(
            claimed_fill(&frame, &window).is_some(),
            "a hidden pane's button is not the drop target"
        );
        let (_, _) = window.frame(&mut transport, &mut queue, vec![window.release(to)]);

        assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "list:p1"]);
        assert_eq!(
            window.pane,
            layout::PanePref::Hidden,
            "the release that ended the drop toggled the pane"
        );
    }

    #[test]
    fn a_pane_toggle_asks_for_the_frame_that_shows_it() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let at = queue_button_at(&window).center();

        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        for event in crate::view::test_support::click_events(at) {
            let _ = window.frame(&mut transport, &mut queue, vec![event]);
        }
        assert_eq!(window.pane, layout::PanePref::Hidden);
        // The causes are read a pass behind, so the pass after the click carries
        // the ones the click's own pass recorded.
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let causes = window.ctx.repaint_causes();
        let toggle = pane_toggle_repaint_line();
        assert!(
            causes.iter().any(|cause| {
                cause.file.ends_with("view/shell.rs") && Some(cause.line as usize) == toggle
            }),
            "the toggle did not ask for the frame that shows it: {causes:?}"
        );
    }

    /// The line of the `request_repaint` the pane-toggle branch calls, found in
    /// the source so an edit that moves the branch keeps the test honest.
    fn pane_toggle_repaint_line() -> Option<usize> {
        let lines: Vec<&str> = include_str!("shell.rs").lines().collect();
        let start = lines
            .iter()
            .position(|line| line.contains("if playbar_reply.pane"))
            .expect("the pane-toggle branch is in this file");
        lines[start + 1..]
            .iter()
            .take_while(|line| !line.trim_start().starts_with('}'))
            .position(|line| line.contains("request_repaint"))
            .map(|offset| start + offset + 2)
    }

    #[test]
    fn a_resize_to_a_wider_window_shows_the_pane_under_auto_and_still_hides_it_under_hidden() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);

        let mut window = Window::new(Vec2::new(1100.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            painted_at(&frame, "t2").is_none(),
            "the pane is on screen below its own threshold"
        );
        window.size = Vec2::new(1400.0, 900.0);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "t2", pane_rect_when_shown(&window)).is_some(),
            "a wider window under auto kept the pane hidden"
        );

        window.pane = layout::PanePref::Hidden;
        window.size = Vec2::new(1100.0, 900.0);
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(window.pane, layout::PanePref::Hidden);
        window.size = Vec2::new(1400.0, 900.0);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            painted_at(&frame, "t2").is_none(),
            "a wider window under a hidden choice showed the pane"
        );
    }

    /// Every click's passes, flattened, for the assertions below.
    fn fullscreen_clicks(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> Vec<egui::ViewportCommand> {
        let at = fullscreen_button_at(window).center();
        window.click_viewport(transport, queue, at)
    }

    #[test]
    fn fullscreen_click_sends_fullscreen_true_when_the_window_reports_false() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        window.fullscreen = Some(false);

        let commands = fullscreen_clicks(&mut window, &mut transport, &mut queue);

        assert!(
            commands.contains(&egui::ViewportCommand::Fullscreen(true)),
            "the click asked for no fullscreen: {commands:?}"
        );
    }

    #[test]
    fn fullscreen_click_sends_fullscreen_false_when_the_window_reports_true() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        window.fullscreen = Some(true);

        let commands = fullscreen_clicks(&mut window, &mut transport, &mut queue);

        assert!(
            commands.contains(&egui::ViewportCommand::Fullscreen(false)),
            "the click asked for no exit: {commands:?}"
        );
    }

    #[test]
    fn fullscreen_click_sends_nothing_when_the_state_is_unknown() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        window.fullscreen = None;

        let commands = fullscreen_clicks(&mut window, &mut transport, &mut queue);

        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::Fullscreen(_))),
            "a window that cannot say whether it is fullscreen was asked anyway: {commands:?}"
        );
    }

    #[test]
    fn a_window_manager_flip_is_read_not_remembered() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        window.fullscreen = Some(false);
        let at = fullscreen_button_at(&window);
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            !glyph_colours_in(&window.frame(&mut transport, &mut queue, Vec::new()).1, at)
                .contains(&Tokens::DARK.palette.accent),
            "a windowed window lit the fullscreen button"
        );

        // The compositor's own key, with no click from the app.
        window.fullscreen = Some(true);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            glyph_colours_in(&frame, at).contains(&Tokens::DARK.palette.accent),
            "a fullscreen window left the button unlit"
        );
        assert_eq!(
            viewport_commands(&frame)
                .iter()
                .filter(|command| matches!(command, egui::ViewportCommand::Fullscreen(_)))
                .count(),
            0,
            "a window-manager change was answered with a request of our own"
        );

        window.fullscreen = Some(false);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            !glyph_colours_in(&frame, at).contains(&Tokens::DARK.palette.accent),
            "the button stayed lit after the window left fullscreen"
        );
    }

    #[test]
    fn a_refused_request_leaves_the_button_unlit_and_the_next_click_asks_again() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        window.fullscreen = Some(false);
        let at = fullscreen_button_at(&window);

        let first = fullscreen_clicks(&mut window, &mut transport, &mut queue);
        assert!(
            first.contains(&egui::ViewportCommand::Fullscreen(true)),
            "the first click asked for nothing: {first:?}"
        );

        // The compositor refused, so the window still reports itself windowed.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            !glyph_colours_in(&frame, at).contains(&Tokens::DARK.palette.accent),
            "a refused request lit the button anyway"
        );

        let second = fullscreen_clicks(&mut window, &mut transport, &mut queue);
        assert!(
            second.contains(&egui::ViewportCommand::Fullscreen(true)),
            "the second click did not ask again: {second:?}"
        );
    }

    #[test]
    fn the_availability_flip_does_not_change_the_narrow_look() {
        for width in [900.0, 1100.0] {
            let mut transport = playing(cover("abc"));
            let mut queue = queue_of(4, &mut transport);
            let mut window = Window::new(Vec2::new(width, 900.0));
            let at = queue_button_at(&window);
            let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

            let colours = glyph_colours_in(&frame, at);
            assert!(!colours.is_empty(), "{width}: the button drew no glyph");
            for colour in colours {
                assert_eq!(
                    colour,
                    Tokens::DARK.palette.text_disabled,
                    "{width}: a button with no pane to toggle is not drawn disabled"
                );
            }
        }
    }

    #[test]
    fn below_the_now_playing_width_a_drop_off_the_queue_button_is_refused() {
        // The queue column is hidden below this width, so only the playbar's
        // queue button takes a drop here.
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1100.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), "p1");
        // Above the rows: off the queue button and off the page's own list.
        let to = egui::pos2(window.size.x - 80.0, view_rect(&window).top() + 8.0);
        let before = entries(&queue);

        let (_, frame) = window.hold_drag(&mut transport, &mut queue, from, to);
        assert_eq!(
            ghost_at(&frame, to),
            Some(Tokens::DARK.palette.surface),
            "a narrow window paints the ghost flat"
        );
        assert!(
            shadows(&frame).is_empty(),
            "a flat ghost casts no shadow: {:?}",
            shadows(&frame)
        );
        assert!(marker(&frame).is_none(), "no target, no marker");
        assert!(
            claimed_fill(&frame, &window).is_none(),
            "the pointer is off the queue button"
        );

        let release = window.release(to);
        let _ = window.frame(&mut transport, &mut queue, vec![release]);
        assert_eq!(entries(&queue), before, "a refused drop changes nothing");
    }

    #[test]
    fn after_a_drop_the_landed_row_is_in_view_and_the_list_does_not_jump() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(24, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let pane = pane_rect_when_shown(&window);
        let from = row_line(&frame, pane, "t2");
        let to = below(row_line(&frame, pane, "t5"), -0.25);

        let _ = window.drag(&mut transport, &mut queue, from, to);
        assert_eq!(
            entries(&queue).iter().position(|uri| uri == "t2"),
            Some(4),
            "the row landed below t4"
        );
        let (_, first) = window.frame(&mut transport, &mut queue, Vec::new());
        let (_, second) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = text_in(&first, "t2", pane).expect("the landed row is drawn");
        let after = text_in(&second, "t2", pane).expect("the landed row is still drawn");
        let id = window
            .ctx
            .memory(|memory| memory.focused())
            .expect("the landed row takes the keyboard");
        let row = window
            .ctx
            .read_response(id)
            .expect("the landed row was laid out")
            .rect;
        assert!(
            row.contains(after.center()),
            "the keyboard went somewhere else: {row:?}"
        );
        assert!(
            ringed_row(&second).is_none(),
            "a pointer drop shows no ring"
        );
        assert!(
            pane.contains_rect(at),
            "the row just placed is not in view: {at:?}"
        );
        assert_eq!(
            at.min.y, after.min.y,
            "the list moved under a row that was already in view"
        );
    }

    #[test]
    fn autoscroll_advances_the_boundary_while_the_marker_stays_at_the_edge() {
        // How far the dragged track lands when the pointer is held at the pane's
        // bottom edge for `frames` frames, and the markers that were drawn.
        let run = |frames: usize| {
            let mut transport = playing(cover("abc"));
            let mut queue = queue_of(24, &mut transport);
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
            let pane = pane_rect_when_shown(&window);
            let from = row_line(&frame, pane, "t2");
            let to = egui::pos2(pane.center().x, pane.bottom() - 4.0);
            let _ = window.hold_drag(&mut transport, &mut queue, from, to);

            let mut marks = Vec::new();
            for _ in 0..frames {
                let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
                marks.push(marker(&frame).expect("a held block draws its marker"));
            }
            let release = window.release(to);
            let _ = window.frame(&mut transport, &mut queue, vec![release]);

            let landed = entries(&queue)
                .iter()
                .position(|uri| uri == "t2")
                .expect("the dragged entry is still queued");
            (landed, marks, pane)
        };

        let (quick, _, _) = run(0);
        let (slow, marks, pane) = run(30);
        assert!(
            slow > quick,
            "holding at the edge must scroll the boundary further: {quick} then {slow}"
        );
        for mark in &marks {
            assert!(
                mark.top() >= pane.top() && mark.bottom() <= pane.bottom() + 0.5,
                "a marker left the pane: {mark:?} in {pane:?}"
            );
            assert!(
                mark.top() > pane.bottom() - QUEUE_ROW_H,
                "a held marker is not at the edge: {mark:?}"
            );
        }
    }

    #[test]
    fn alt_down_and_alt_up_move_the_selected_rows() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(6, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let pane = pane_rect_when_shown(&window);
        clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");
        window.held = egui::Modifiers::COMMAND;
        clicked_row(&mut window, &mut queue, &mut transport, pane, "t3");
        window.held = egui::Modifiers::NONE;
        assert!(queue.selected(2) && queue.selected(3));

        assert_eq!(
            chord(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::ArrowDown,
                egui::Modifiers::ALT
            ),
            None
        );
        assert_eq!(entries(&queue), ["t0", "t1", "t4", "t2", "t3", "t5"]);

        assert_eq!(
            chord(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::ArrowUp,
                egui::Modifiers::ALT
            ),
            None
        );
        assert_eq!(entries(&queue), ["t0", "t1", "t2", "t3", "t4", "t5"]);
    }

    #[test]
    fn alt_arrows_move_the_focused_row_when_nothing_is_selected() {
        let mut transport = playing(cover("abc"));
        let tracks: Vec<TrackEntry> = (0..6).map(|index| track(index, None)).collect();
        let mut queue = Queue::default();
        queue.start(&tracks, 0, None, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let pane = pane_rect_when_shown(&window);

        clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");
        // A click always picks its row, so the pick is cleared here to leave only
        // the keyboard on it.
        let _ = queue.start(&tracks, 0, None, &mut transport);
        assert!(!queue.selected(2), "nothing is picked");

        assert_eq!(
            chord(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::ArrowUp,
                egui::Modifiers::ALT
            ),
            None
        );
        assert_eq!(entries(&queue), ["t0", "t2", "t1", "t3", "t4", "t5"]);
    }

    #[test]
    fn alt_arrows_do_nothing_when_a_queue_row_is_not_focused() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let before = entries(&queue);
        let field = search_field(&frame).expect("the field is painted");
        let _ = window.click(&mut transport, &mut queue, field.center());

        assert_eq!(
            chord(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::ArrowDown,
                egui::Modifiers::ALT
            ),
            None
        );
        assert_eq!(entries(&queue), before, "no row holds the keyboard");
    }

    #[test]
    fn ctrl_alt_arrows_are_ignored() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let pane = pane_rect_when_shown(&window);
        clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");
        let before = entries(&queue);

        let ctrl_alt = egui::Modifiers {
            alt: true,
            ctrl: true,
            ..egui::Modifiers::NONE
        };
        assert_eq!(
            chord(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::ArrowDown,
                ctrl_alt
            ),
            None
        );
        assert_eq!(entries(&queue), before, "Ctrl+Alt+↓ is somebody else's");
    }

    #[test]
    fn focus_follows_the_moved_row_and_enter_plays_it() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let pane = pane_rect_when_shown(&window);
        clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");

        let _ = chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowDown,
            egui::Modifiers::ALT,
        );
        assert_eq!(entries(&queue), ["t0", "t1", "t3", "t2", "t4"]);

        assert_eq!(
            chord(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::Enter,
                egui::Modifiers::NONE
            ),
            Some(Command::Play {
                uri: "t2".to_owned()
            }),
            "the row the move landed on is the one Enter plays"
        );
    }

    #[test]
    fn a_held_alt_arrow_moves_the_row_once_per_frame_and_keeps_the_keyboard_on_it() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let pane = pane_rect_when_shown(&window);
        clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");

        // A held key: the press and its auto-repeat arrive on separate frames.
        let alt = egui::Modifiers::ALT;
        window.held = alt;
        for event in [
            holding(key(egui::Key::ArrowDown, true), alt),
            holding(key_repeat(egui::Key::ArrowDown), alt),
        ] {
            let _ = window.frame(&mut transport, &mut queue, vec![event]);
        }
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, false)],
        );
        window.held = egui::Modifiers::NONE;

        assert_eq!(
            entries(&queue),
            ["t0", "t1", "t3", "t4", "t2"],
            "each frame moved the row one place"
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let ring = ringed_row(&frame).expect("the moved row holds the keyboard");
        assert!(
            ring.contains(row_line(&frame, pane, "t2")),
            "the keyboard stayed on the index the row left: {ring:?}"
        );
        assert_eq!(
            chord(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::Enter,
                egui::Modifiers::NONE
            ),
            Some(Command::Play {
                uri: "t2".to_owned()
            }),
            "Enter plays the row the repeat moved"
        );
    }

    #[test]
    fn alt_arrow_at_the_boundary_row_does_not_move_focus_and_shows_no_ring() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let pane = pane_rect_when_shown(&window);
        // The pane lists what follows the current entry, so t1 is its first row
        // and the top of what Alt+↑ can move.
        clicked_row(&mut window, &mut queue, &mut transport, pane, "t1");

        let _ = chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowUp,
            egui::Modifiers::ALT,
        );
        assert_eq!(
            entries(&queue),
            ["t0", "t1", "t2", "t3", "t4"],
            "the first row of the list has nowhere to go"
        );

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            ringed_row(&frame).is_none(),
            "focus did not move, so the ring stays hidden"
        );
        assert_eq!(
            chord(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::Enter,
                egui::Modifiers::NONE
            ),
            Some(Command::Play {
                uri: "t1".to_owned()
            }),
            "the keyboard is still on the row that was clicked"
        );
    }

    #[test]
    fn alt_arrows_leave_the_playbar_and_the_search_field_alone() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(5, &mut transport);
        // A seek needs a length and a state, and starting the queue clears both.
        transport.state = TransportState::Playing;
        transport.duration_ms = Some(100_000);
        transport.position_ms = 10_000;
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let pane = pane_rect_when_shown(&window);
        clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");

        // Tab reaches the seek slider, and only the slider turns Home into a seek.
        let mut reached = false;
        for _ in 0..24 {
            let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Tab);
            if press(&mut window, &mut transport, &mut queue, egui::Key::Home)
                == Some(Command::Seek { position_ms: 0 })
            {
                reached = true;
                break;
            }
        }
        assert!(reached, "Tab never reached the seek slider");
        // The slider locks its arrows a frame after it takes focus.
        let (_, _) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            chord(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::ArrowRight,
                egui::Modifiers::SHIFT
            ),
            Some(Command::Seek {
                position_ms: 30_000
            }),
            "Shift still makes one arrow press four seek steps"
        );

        // The shell leaves the seek's keyboard alone: an Alt chord there moves no
        // queue row.
        let held = entries(&queue);
        for arrow in [egui::Key::ArrowRight, egui::Key::ArrowDown] {
            let _ = chord(
                &mut window,
                &mut transport,
                &mut queue,
                arrow,
                egui::Modifiers::ALT,
            );
        }
        assert_eq!(
            entries(&queue),
            held,
            "an Alt chord on the seek moves no queue row"
        );

        // The field keeps its own arrows: Alt is not the shell's to take.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let before = entries(&queue);
        let field = search_field(&frame).expect("the field is painted");
        let _ = window.click(&mut transport, &mut queue, field.center());
        assert_eq!(
            chord(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::ArrowDown,
                egui::Modifiers::ALT
            ),
            None
        );
        assert_eq!(entries(&queue), before);
    }

    #[test]
    fn selection_resets_when_the_page_changes() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, view_rect(&window), "p1");
        let _ = window.click(&mut transport, &mut queue, at);

        let id = egui::Id::unique(("playlist-tracks", PLAYLIST));
        assert!(
            carry::List::open(&window.ctx, id, 3).look(1).selected,
            "the click is stored under the list's own id"
        );

        window.library.open(Page::Album(ALBUM.to_owned()));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            !carry::List::open(&window.ctx, id, 3).look(1).selected,
            "a page change forgets every list's pick"
        );
    }

    /// A track a menu's verbs can all act on: a real track URI, an album and an
    /// artist.
    fn menu_track(index: usize) -> TrackEntry {
        TrackEntry {
            uri: format!("spotify:track:{index}"),
            title: format!("t{index}"),
            artists: "A Band".to_owned(),
            album: Some("A Record".to_owned()),
            album_uri: Some(ALBUM.to_owned()),
            artist_uris: vec!["spotify:artist:b".to_owned()],
            artist_names: vec!["A Band".to_owned()],
            added_at: None,
            duration_ms: None,
            named: true,
            cover: None,
        }
    }

    /// Opens the context menu on these rows, as a row's door would.
    fn open_menu(
        window: &mut Window,
        verb: Verb,
        targets: Vec<TrackEntry>,
        listing: Listing,
        rows: Vec<usize>,
    ) {
        let uris: Vec<&str> = targets.iter().map(|entry| entry.uri.as_str()).collect();
        let standing = window.library.likes.standing(&uris);
        let items: Vec<_> = crate::verbs::track_items(&targets, None, &listing, standing)
            .into_iter()
            .filter(|item| item.verb == verb)
            .collect();
        assert!(!items.is_empty(), "the verb is offered for these rows");
        let request = crate::menu::Request {
            door: crate::menu::Door {
                anchor: crate::menu::Anchor::Button(Rect::from_min_size(
                    egui::Pos2::new(600.0, 300.0),
                    Vec2::new(28.0, 28.0),
                )),
                opener: egui::Id::unique("row-more"),
                device: crate::menu::Device::Keyboard,
            },
            playlists: Vec::new(),
            items,
            targets,
            rows,
            listing,
            // The page it is opened on, as a door posts it, so a page change
            // still closes it.
            page: window.library.page().cloned(),
        };
        assert!(
            window.menu.open(request, Gate::default()),
            "the menu opened"
        );
    }

    /// The page's listing, under the id its rows are picked by.
    fn playlist_listing() -> Listing {
        Listing::Playlist {
            uri: PLAYLIST.to_owned(),
            list: egui::Id::unique(("playlist-tracks", PLAYLIST)),
        }
    }

    /// The page's own entries at `rows`, which is what a request acts on.
    fn page_entries(window: &Window, rows: &[usize]) -> Vec<TrackEntry> {
        match &window.library.tracks {
            Load::Ready(tracks) => rows
                .iter()
                .filter_map(|row| tracks.tracks.get(*row).cloned())
                .collect(),
            Load::Idle | Load::Loading | Load::Failed(_) => Vec::new(),
        }
    }

    /// The row titles of `titles` that the page painted, in `titles`' order.
    ///
    /// `page_rows` reads the whole list; this reads what the user sees, so it
    /// can tell a narrowed page from an unfiltered one.
    fn painted_titles(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        titles: &[&str],
    ) -> Vec<String> {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let area = view_rect(window);
        titles
            .iter()
            .filter(|title| text_in(&frame, title, area).is_some())
            .map(|title| (*title).to_owned())
            .collect()
    }

    /// Clicks the playlist page's row titled `title`, which picks and focuses
    /// it.
    fn click_page_row(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        title: &str,
    ) -> Option<Command> {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let at = row_line(&frame, view_rect(window), title);
        window.click(transport, queue, at)
    }

    /// Right-clicks the playlist page's row titled `title`, the way a mouse
    /// does: move, press, release.
    fn right_click_page_row(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        title: &str,
    ) {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let at = row_line(&frame, view_rect(window), title);
        for event in [
            egui::Event::PointerMoved(at),
            side(at, egui::PointerButton::Secondary, true),
            side(at, egui::PointerButton::Secondary, false),
        ] {
            let _ = window.frame(transport, queue, vec![event]);
        }
    }

    /// The Now Playing head's more button, solved the way the panel solves it.
    fn now_playing_more(window: &Window) -> egui::Pos2 {
        let pane = pane_rect_when_shown(window);
        let head = Rect::from_min_size(
            pane.min + Vec2::splat(space::S3),
            Vec2::new(pane.width() - 2.0 * space::S3, extent::BUTTON),
        );
        egui::Pos2::new(head.right() - extent::BUTTON / 2.0, head.center().y)
    }

    /// A cut the page would post for `rows`, built from the page's own entries.
    fn page_cut(window: &Window, rows: &[usize]) -> Cut {
        Cut {
            listing: playlist_listing(),
            rows: rows.to_vec(),
            entries: page_entries(window, rows),
        }
    }

    /// The playlist filter's field, read from the hairline it paints under
    /// itself. The head's own rule is wider than the field's 15rem cap.
    fn page_filter_field(output: &egui::FullOutput, view: Rect) -> Option<Rect> {
        let tokens = Tokens::DARK;
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if view.contains_rect(shape.rect)
                        && (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                        && shape.rect.width() > 80.0
                        && shape.rect.width() <= 240.5
                        && (shape.fill == tokens.palette.border
                            || shape.fill == tokens.palette.text) =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .min_by(|left, right| left.top().total_cmp(&right.top()))
    }

    /// A request to open the menu on the page's rows, as a door posts it.
    fn page_request(window: &Window, rows: &[usize]) -> crate::menu::Request {
        let targets = page_entries(window, rows);
        crate::menu::Request {
            door: crate::menu::Door {
                anchor: crate::menu::Anchor::Button(Rect::from_min_size(
                    egui::Pos2::new(600.0, 300.0),
                    Vec2::new(28.0, 28.0),
                )),
                opener: egui::Id::unique("row-more"),
                device: crate::menu::Device::Keyboard,
            },
            items: crate::verbs::track_items(
                &targets,
                None,
                &playlist_listing(),
                crate::likes::Standing::Unknown,
            ),
            playlists: Vec::new(),
            targets,
            rows: rows.to_vec(),
            listing: playlist_listing(),
            page: window.library.page().cloned(),
        }
    }

    /// The two frames an `Area` takes to arrive, so the menu's opening frame is
    /// behind us.
    fn settle_menu(window: &mut Window, transport: &mut Transport, queue: &mut Queue) {
        for _ in 0..2 {
            let _ = window.frame(transport, queue, Vec::new());
        }
    }

    /// The `⋯` of the row titled `title`, found as the rightmost glyph on that
    /// row's line: the like sits left of it, and the band's own `⋯` is a row
    /// higher.
    fn more_button(output: &egui::FullOutput, view: Rect, title: &str) -> egui::Pos2 {
        let line = row_line(output, view, title).y;
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Path(_) => Some(clipped.shape.visual_bounding_rect()),
                _ => None,
            })
            .filter(|rect| {
                view.contains(rect.center()) && (rect.center().y - line).abs() < extent::ROW_H
            })
            .max_by(|left, right| left.center().x.total_cmp(&right.center().x))
            .map(|rect| rect.center())
            .expect("the row paints a `⋯`")
    }

    #[test]
    fn clicking_the_open_more_closes_its_menu_and_keeps_the_keyboard() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let more = more_button(&frame, view_rect(&window), "p0");

        let _ = window.click(&mut transport, &mut queue, more);
        settle_menu(&mut window, &mut transport, &mut queue);
        assert!(window.menu.is_open(), "the `⋯` opened no menu");

        // The same `⋯` again closes it instead of resetting it.
        let _ = window.click(&mut transport, &mut queue, more);
        assert!(
            !window.menu.is_open(),
            "the `⋯` reset its own menu instead of closing it"
        );

        // And the keyboard is back on the `⋯`, so Enter opens the menu again.
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Enter, true)],
        );
        assert!(
            window.menu.is_open(),
            "the `⋯` did not take the keyboard back"
        );
    }

    #[test]
    fn a_held_enter_that_opened_the_menu_fires_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let more = more_button(&frame, view_rect(&window), "p0");

        // Open from the `⋯` and close, so the button holds the keyboard.
        let _ = window.click(&mut transport, &mut queue, more);
        settle_menu(&mut window, &mut transport, &mut queue);
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
        assert!(!window.menu.is_open(), "Escape closed the menu");

        // Enter opens it and is held: no release comes before the repeats.
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Enter, true)],
        );
        assert!(window.menu.is_open(), "Enter opened no menu");
        for _ in 0..3 {
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key_repeat(egui::Key::Enter)],
            );
        }

        assert!(window.menu.is_open(), "a held Enter closed the menu");
        assert_eq!(window.menu.lit(), Some(0), "a held Enter moved the lit row");
        assert_eq!(
            queue.upcoming().count(),
            0,
            "a held Enter fired Add to queue"
        );
    }

    #[test]
    fn a_held_enter_after_choosing_does_not_reopen_the_menu() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let more = more_button(&frame, view_rect(&window), "p0");

        let _ = window.click(&mut transport, &mut queue, more);
        settle_menu(&mut window, &mut transport, &mut queue);
        let _ = press(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowDown,
        );
        assert_eq!(window.menu.lit(), Some(0));

        // Enter chooses, closes and hands the keyboard back to the `⋯`.
        let chosen = window
            .frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::Enter, true)],
            )
            .0;
        assert_eq!(chosen, None, "adding to the queue asks for no command");
        assert!(!window.menu.is_open(), "the choice left the menu open");
        assert_eq!(queue.upcoming().count(), 1, "the choice queued nothing");

        // Held, the repeats land on the `⋯` that now holds the keyboard.
        for _ in 0..3 {
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![key_repeat(egui::Key::Enter)],
            );
        }
        assert!(
            !window.menu.is_open(),
            "a held Enter reopened the menu it chose from"
        );
        assert_eq!(
            queue.upcoming().count(),
            1,
            "a held Enter queued the row again"
        );
    }

    #[test]
    fn the_menu_survives_idle_frames() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, view_rect(&window), "p1");
        for event in crate::view::test_support::secondary_click_events(at) {
            let _ = window.frame(&mut transport, &mut queue, vec![event]);
        }
        assert!(window.menu.is_open(), "the right-click opened no menu");
        for index in 0..10 {
            let _ = window.frame(&mut transport, &mut queue, Vec::new());
            assert!(
                window.menu.is_open(),
                "the menu closed on idle frame {index}"
            );
        }
        // The window losing the OS keyboard is not the menu's business until the
        // user acts.
        window.unfocused = true;
        for index in 0..3 {
            let _ = window.frame(&mut transport, &mut queue, Vec::new());
            assert!(
                window.menu.is_open(),
                "the menu closed on unfocused frame {index}"
            );
        }
    }

    #[test]
    fn a_right_click_on_a_playlist_row_opens_the_menu_the_shell_draws() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, view_rect(&window), "p1");

        // The row posts a request; the shell opens the menu on it in the same
        // frame. The area's first frame is its sizing pass, and its second is
        // the opening frame, which ignores keys.
        for event in crate::view::test_support::secondary_click_events(at) {
            let _ = window.frame(&mut transport, &mut queue, vec![event]);
        }
        assert!(window.menu.is_open(), "the right-click opened no menu");
        settle_menu(&mut window, &mut transport, &mut queue);

        // The pointer opened it, so nothing is lit until the keyboard asks.
        assert_eq!(window.menu.lit(), None);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, false)],
        );
        assert_eq!(window.menu.lit(), Some(0));

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
            None,
            "adding to the queue asks for no command"
        );
        assert!(!window.menu.is_open());
        let queued: Vec<&str> = queue
            .upcoming()
            .map(|(_, entry)| entry.uri.as_str())
            .collect();
        assert_eq!(queued, vec!["list:p1"], "the row the menu was opened on");
    }

    #[test]
    fn a_chosen_add_to_queue_appends_and_closes() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        open_menu(
            &mut window,
            Verb::AddToQueue,
            vec![menu_track(0)],
            Listing::Page,
            vec![0],
        );
        settle_menu(&mut window, &mut transport, &mut queue);

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Enter),
            None,
            "adding to the queue asks for no command"
        );
        assert!(
            !window.menu.is_open(),
            "the menu stayed open after the choice"
        );
        let queued: Vec<&str> = queue
            .upcoming()
            .map(|(_, entry)| entry.uri.as_str())
            .collect();
        assert_eq!(queued, vec!["spotify:track:0"]);
    }

    #[test]
    fn a_chosen_copy_link_emits_the_joined_urls() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        open_menu(
            &mut window,
            Verb::CopyLink,
            vec![menu_track(0), menu_track(1)],
            Listing::Page,
            vec![0, 1],
        );
        settle_menu(&mut window, &mut transport, &mut queue);

        let (_, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Enter, true)],
        );
        let copied = frame
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.as_str()),
                _ => None,
            });
        assert_eq!(
            copied,
            Some("https://open.spotify.com/track/0\nhttps://open.spotify.com/track/1"),
            "one URL per line"
        );
    }

    #[test]
    fn a_chosen_go_to_opens_the_page() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        open_menu(
            &mut window,
            Verb::GoToAlbum,
            vec![menu_track(0)],
            Listing::Page,
            vec![0],
        );
        settle_menu(&mut window, &mut transport, &mut queue);

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
        assert_eq!(
            window.library.page(),
            Some(&Page::Album(ALBUM.to_owned())),
            "the page did not open"
        );
        assert!(command.is_some(), "the page's read was not asked for");
        assert!(!window.menu.is_open());
    }

    /// Makes the window's liked set these tracks, as a landed read would.
    fn liked(window: &mut Window, tracks: &[&str]) {
        assert!(window.library.likes.request(true));
        window.library.apply(&Update::LikedTracks(
            tracks.iter().map(|uri| (*uri).to_owned()).collect(),
        ));
    }

    #[test]
    fn choosing_save_issues_one_command_for_the_pick_and_flips_every_effective_row() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        liked(&mut window, &[]);
        open_menu(
            &mut window,
            Verb::Save,
            vec![menu_track(0), menu_track(1)],
            Listing::Page,
            vec![0, 1],
        );
        settle_menu(&mut window, &mut transport, &mut queue);

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

        assert_eq!(
            command,
            Some(Command::SetLiked {
                tracks: vec!["spotify:track:0".to_owned(), "spotify:track:1".to_owned()],
                liked: true,
            })
        );
        assert_eq!(window.library.likes.state("spotify:track:0"), Some(true));
        assert_eq!(window.library.likes.state("spotify:track:1"), Some(true));
        assert!(!window.menu.is_open());
    }

    #[test]
    fn choosing_remove_from_your_library_unlikes() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        liked(&mut window, &["spotify:track:0"]);
        open_menu(
            &mut window,
            Verb::Unsave,
            vec![menu_track(0)],
            Listing::Page,
            vec![0],
        );
        settle_menu(&mut window, &mut transport, &mut queue);

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

        assert_eq!(
            command,
            Some(Command::SetLiked {
                tracks: vec!["spotify:track:0".to_owned()],
                liked: false,
            })
        );
        assert_eq!(window.library.likes.state("spotify:track:0"), Some(false));
    }

    #[test]
    fn a_save_chosen_while_a_like_is_pending_is_dropped() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        liked(&mut window, &[]);
        assert!(
            window
                .library
                .set_liked(vec!["spotify:track:9".to_owned()], true)
                .is_some()
        );
        open_menu(
            &mut window,
            Verb::Save,
            vec![menu_track(1)],
            Listing::Page,
            vec![0],
        );
        settle_menu(&mut window, &mut transport, &mut queue);

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

        assert_eq!(command, None, "a dropped save sent a command");
        assert_eq!(window.library.likes.state("spotify:track:1"), Some(false));
    }

    #[test]
    fn a_pending_like_does_not_hold_a_menu_shut() {
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        liked(&mut window, &[]);
        assert!(
            window
                .library
                .set_liked(vec!["spotify:track:9".to_owned()], true)
                .is_some()
        );
        assert!(
            !window.library.write_in_flight(),
            "a like is not a playlist write"
        );
        open_menu(
            &mut window,
            Verb::Save,
            vec![menu_track(1)],
            Listing::Page,
            vec![0],
        );
        assert!(window.menu.is_open(), "a like held the menu shut");
    }

    #[test]
    fn a_playlist_write_does_not_stop_a_like() {
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        liked(&mut window, &[]);
        let carried = page_entries(&window, &[0]);
        assert!(
            window
                .library
                .remove_tracks(PLAYLIST, &[0], &carried)
                .is_some()
        );
        assert!(
            window
                .library
                .set_liked(vec!["spotify:track:0".to_owned()], true)
                .is_some(),
            "a playlist write stopped a like"
        );
    }

    #[test]
    fn a_menu_opened_before_the_set_is_ready_has_no_library_row() {
        let window = Window::new(Vec2::new(1400.0, 900.0));
        let targets = vec![menu_track(0)];
        let uris: Vec<&str> = targets.iter().map(|entry| entry.uri.as_str()).collect();
        let standing = window.library.likes.standing(&uris);
        assert_eq!(standing, crate::likes::Standing::Unknown);
        let items = crate::verbs::track_items(&targets, None, &Listing::Page, standing);
        assert!(
            items
                .iter()
                .all(|item| !matches!(item.verb, Verb::Save | Verb::Unsave)),
            "a set nobody has read offered a library row"
        );
    }

    #[test]
    fn an_applied_ask_issues_set_liked_and_flips_the_row() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        liked(&mut window, &[]);
        crate::view::heart::post(
            &window.ctx,
            crate::view::heart::Ask {
                tracks: vec!["spotify:track:a".to_owned()],
                liked: true,
            },
        );

        let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());

        assert_eq!(
            command,
            Some(Command::SetLiked {
                tracks: vec!["spotify:track:a".to_owned()],
                liked: true,
            })
        );
        assert_eq!(window.library.likes.state("spotify:track:a"), Some(true));
        assert!(
            crate::view::heart::take(&window.ctx).is_none(),
            "the ask was not taken"
        );
    }

    #[test]
    fn an_ask_with_the_drawer_open_is_dropped() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        liked(&mut window, &[]);
        window.drawer = Drawer::Open;
        crate::view::heart::post(
            &window.ctx,
            crate::view::heart::Ask {
                tracks: vec!["spotify:track:a".to_owned()],
                liked: true,
            },
        );

        let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());

        assert_eq!(command, None, "a dropped ask sent a command");
        assert_eq!(window.library.likes.state("spotify:track:a"), Some(false));
        assert!(
            crate::view::heart::take(&window.ctx).is_none(),
            "a dropped ask outlived its frame"
        );
    }

    #[test]
    fn a_page_change_closes_the_menu() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        open_menu(
            &mut window,
            Verb::AddToQueue,
            vec![menu_track(0)],
            Listing::Page,
            vec![0],
        );
        settle_menu(&mut window, &mut transport, &mut queue);
        assert!(window.menu.is_open());

        let _ = window.library.open(Page::Playlist(PLAYLIST.to_owned()));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            !window.menu.is_open(),
            "the menu outlived the page its rows came from"
        );
    }
    #[test]
    fn a_chosen_remove_on_a_playlist_issues_the_command_and_forgets_the_pick() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        // A frame first, so the page's own list is the one the picks are kept
        // against.
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let listing = playlist_listing();
        let Listing::Playlist { list, .. } = listing.clone() else {
            panic!("the page is a playlist");
        };
        carry::select_rows(&window.ctx, list, 0..2);
        let targets = page_entries(&window, &[0]);
        open_menu(&mut window, Verb::Remove, targets, listing, vec![0]);
        settle_menu(&mut window, &mut transport, &mut queue);

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

        assert_eq!(
            command,
            Some(Command::RemoveTracks {
                playlist: PLAYLIST.to_owned(),
                revision: revision("rev-1"),
                removals: vec![Removal { from: 0, length: 1 }],
            })
        );
        assert_eq!(page_rows(&window), vec!["list:p1", "list:p2"]);
        let list = carry::List::open(&window.ctx, list, 2);
        assert!(
            list.look(0).selected,
            "the pick did not shift with the rows"
        );
        assert!(
            !list.look(1).selected,
            "the removed row's pick survived the removal"
        );
    }

    #[test]
    fn a_chosen_remove_on_the_queue_removes_the_rows_and_refocuses() {
        let mut transport = Transport::default();
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let targets = queue.entries_at(&[1]);
        open_menu(&mut window, Verb::Remove, targets, Listing::Queue, vec![1]);
        settle_menu(&mut window, &mut transport, &mut queue);

        // One press, so the row that takes the keyboard back still names it.
        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Enter, true)],
        );

        assert_eq!(command, None, "the queue removes locally");
        assert_eq!(entries(&queue), vec!["t0", "t2", "t3"]);
        assert_eq!(queue.refocus(), Some(1));
        assert!(!window.menu.is_open());
        assert!(
            window
                .ctx
                .memory(|memory| memory.has_focus(egui::Id::unique("row-more"))),
            "the keyboard did not go back to the button"
        );
    }

    #[test]
    fn delete_in_the_open_menu_chooses_the_removal() {
        let mut transport = Transport::default();
        let mut queue = queue_of(3, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let targets = queue.entries_at(&[1]);
        open_menu(&mut window, Verb::Remove, targets, Listing::Queue, vec![1]);
        settle_menu(&mut window, &mut transport, &mut queue);

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert!(!window.menu.is_open(), "Del left the menu open");
        assert_eq!(entries(&queue), vec!["t0", "t2"]);
    }

    #[test]
    fn a_stale_queue_cut_removes_nothing_and_leaves_the_rows() {
        let mut transport = Transport::default();
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let targets = queue.entries_at(&[1]);
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        // The cut was made while row 1 was upcoming, and the queue moved on.
        let _ = queue.next(&mut transport);
        context_menu::post_cut(
            &window.ctx,
            Cut {
                listing: Listing::Queue,
                rows: vec![1],
                entries: targets,
            },
        );

        let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());

        assert_eq!(command, None, "a refused queue cut reported a command");
        assert_eq!(entries(&queue), vec!["t0", "t1", "t2", "t3"]);
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t1"));
        assert!(!window.menu.is_open());
    }

    #[test]
    fn a_queue_removal_is_not_blocked_by_a_playlist_write() {
        let mut transport = Transport::default();
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let carried = page_entries(&window, &[0]);
        assert!(
            window
                .library
                .remove_tracks(PLAYLIST, &[0], &carried)
                .is_some(),
            "the page started no write"
        );
        assert!(window.library.write_in_flight());
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
        for event in [
            egui::Event::PointerMoved(at),
            side(at, egui::PointerButton::Secondary, true),
            side(at, egui::PointerButton::Secondary, false),
        ] {
            let _ = window.frame(&mut transport, &mut queue, vec![event]);
        }
        settle_menu(&mut window, &mut transport, &mut queue);

        assert!(
            window.menu.is_open(),
            "a playlist write refused the queue's menu"
        );
        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(command, None);
        assert_eq!(entries(&queue), vec!["t0", "t1", "t3"]);
        assert!(
            window.library.write_in_flight(),
            "the queue's removal settled the playlist write"
        );
    }

    #[test]
    fn a_queue_delete_while_a_playlist_write_is_pending_still_removes() {
        let mut transport = Transport::default();
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let carried = page_entries(&window, &[0]);
        assert!(
            window
                .library
                .remove_tracks(PLAYLIST, &[0], &carried)
                .is_some(),
            "the page started no write"
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
        let _ = window.click(&mut transport, &mut queue, at);

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(command, None, "the queue's Del reported a command");
        assert_eq!(entries(&queue), vec!["t0", "t1", "t3"]);
        assert!(
            window.library.write_in_flight(),
            "the queue's Del settled the playlist write"
        );
    }

    #[test]
    fn a_menu_without_a_removal_ignores_delete() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        open_menu(
            &mut window,
            Verb::AddToQueue,
            vec![menu_track(0)],
            Listing::Page,
            vec![0],
        );
        settle_menu(&mut window, &mut transport, &mut queue);

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert!(
            window.menu.is_open(),
            "a menu with no removal closed on Del"
        );
        assert_eq!(queue.upcoming().count(), 0, "the Del fired a verb");
    }

    #[test]
    fn a_refused_cut_leaves_the_rows_alone() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let targets = page_entries(&window, &[0]);
        open_menu(
            &mut window,
            Verb::Remove,
            targets,
            playlist_listing(),
            vec![0],
        );
        settle_menu(&mut window, &mut transport, &mut queue);

        // The drawer owns the frame, so the shell drops the cut rather than
        // writing behind it.
        window.drawer = Drawer::Open;
        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

        assert_eq!(command, None);
        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
        assert!(
            !window.library.write_in_flight(),
            "a dropped cut left a write in flight"
        );
    }
    #[test]
    fn a_right_click_on_an_editable_playlist_offers_remove_last() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);

        right_click_page_row(&mut window, &mut transport, &mut queue, "p0");
        settle_menu(&mut window, &mut transport, &mut queue);

        assert!(window.menu.is_open(), "the right-click opened no menu");
        let last = window.menu.items().last().expect("a command");
        assert_eq!(last.verb, Verb::Remove, "the last row is not the removal");
        assert_eq!(last.label, "Remove from this playlist");
        assert_eq!(last.key, Some("Del"));
        assert!(
            window.menu.cut().is_some_and(|cut| cut.rows == vec![0]),
            "the menu is not about the row it was opened on"
        );
    }

    #[test]
    fn a_right_click_on_a_read_only_page_offers_no_remove() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        read_only_playlist_page(&mut window, &["p0", "p1"]);

        right_click_page_row(&mut window, &mut transport, &mut queue, "p0");
        settle_menu(&mut window, &mut transport, &mut queue);

        assert!(window.menu.is_open(), "the right-click opened no menu");
        assert!(
            window
                .menu
                .items()
                .iter()
                .all(|item| item.verb != Verb::Remove),
            "a list that is not ours offered a removal"
        );
        assert_eq!(window.menu.cut(), None);
    }

    #[test]
    fn a_focused_rows_delete_removes_the_row() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "p1");

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(
            command,
            Some(Command::RemoveTracks {
                playlist: PLAYLIST.to_owned(),
                revision: revision("rev-1"),
                removals: vec![Removal { from: 1, length: 1 }],
            })
        );
        assert_eq!(page_rows(&window), vec!["list:p0", "list:p2"]);
    }

    #[test]
    fn a_focused_row_outside_the_pick_removes_only_itself() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");
        carry::select_rows(
            &window.ctx,
            egui::Id::unique(("playlist-tracks", PLAYLIST)),
            1..3,
        );

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(
            command,
            Some(Command::RemoveTracks {
                playlist: PLAYLIST.to_owned(),
                revision: revision("rev-1"),
                removals: vec![Removal { from: 0, length: 1 }],
            }),
            "Del acted on a pick the focused row is not in"
        );
        assert_eq!(page_rows(&window), vec!["list:p1", "list:p2"]);
    }

    #[test]
    fn delete_with_no_focused_row_removes_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(command, None);
        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
        assert!(!window.library.write_in_flight());
    }

    #[test]
    fn a_focused_more_button_does_not_answer_delete() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let more = more_button(&frame, view_rect(&window), "p0");
        let _ = window.click(&mut transport, &mut queue, more);

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(
            command, None,
            "the focused `⋯` answered Del as if it were the row"
        );
        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    }

    #[test]
    fn shift_or_ctrl_delete_is_not_this_key() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");

        for modifiers in [egui::Modifiers::SHIFT, egui::Modifiers::COMMAND] {
            let command = chord(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::Delete,
                modifiers,
            );
            assert_eq!(command, None, "{modifiers:?}+Del removed a row");
        }
        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    }

    #[test]
    fn a_held_delete_removes_once() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "p1");

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);
        assert!(command.is_some(), "the first Del removed nothing");
        for _ in 0..3 {
            let (held, _) = window.frame(
                &mut transport,
                &mut queue,
                vec![key_repeat(egui::Key::Delete)],
            );
            assert_eq!(held, None, "a held Del removed a second row");
        }
        assert_eq!(page_rows(&window), vec!["list:p0", "list:p2"]);
    }

    #[test]
    fn delete_on_a_filtered_selection_cuts_exactly_the_picked_visible_rows() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(
            &mut window,
            &[
                "p0", "p1", "keep2", "p3", "p4", "keep5", "p6", "p7", "p8", "keep9",
            ],
        );
        window.library.track_filter.edit(PLAYLIST).push_str("keep");
        let expected: Vec<String> = page_entries(&window, &[2, 5, 9])
            .into_iter()
            .map(|entry| entry.title)
            .collect();
        assert_eq!(
            expected,
            ["keep2", "keep5", "keep9"],
            "the page's true rows 2, 5 and 9 are the kept ones"
        );

        let _ = click_page_row(&mut window, &mut transport, &mut queue, "keep2");
        window.held = egui::Modifiers::COMMAND;
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "keep5");
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "keep9");
        window.held = egui::Modifiers::NONE;

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(
            command,
            Some(Command::RemoveTracks {
                playlist: PLAYLIST.to_owned(),
                revision: revision("rev-1"),
                removals: vec![
                    Removal { from: 9, length: 1 },
                    Removal { from: 5, length: 1 },
                    Removal { from: 2, length: 1 },
                ],
            }),
            "one cut, highest from first, on the true indices"
        );
        assert_eq!(
            page_rows(&window),
            vec![
                "list:p0", "list:p1", "list:p3", "list:p4", "list:p6", "list:p7", "list:p8"
            ],
            "and exactly those rows left the page"
        );
    }

    #[test]
    fn delete_in_the_filter_field_edits_text_and_removes_no_row() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let field = page_filter_field(&frame, view_rect(&window)).expect("the filter is painted");
        let _ = window.click(&mut transport, &mut queue, field.center());
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::Text("ab".to_owned())],
        );
        // Del forward-deletes, and the caret sits after the text, so one step
        // left puts a character in front of it.
        let _ = press(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowLeft,
        );

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(command, None);
        assert_eq!(
            window.library.track_filter.edit(PLAYLIST).as_str(),
            "a",
            "the field kept the key"
        );
        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    }

    #[test]
    fn a_filtered_drag_inside_the_page_moves_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["a", "keep0", "b", "keep1"]);
        window.library.track_filter.edit(PLAYLIST).push_str("keep");
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let view = view_rect(&window);
        let from = row_line(&frame, view, "keep0");
        let to = page_boundary(&frame, view, &["keep0", "keep1"], 2);

        assert_eq!(
            window.drag(&mut transport, &mut queue, from, to),
            None,
            "a filtered page reorders nothing"
        );
        let before = vec!["list:a", "list:keep0", "list:b", "list:keep1"];
        assert_eq!(page_rows(&window), before);
        assert!(
            window.library.movable(),
            "the refused drag left a write pending"
        );

        // The same gesture, once the query is gone, is the one that moves.
        window.library.track_filter.clear();
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view, "keep0");
        let to = page_boundary(&frame, view, &["keep0", "keep1"], 2);
        assert!(
            window.drag(&mut transport, &mut queue, from, to).is_some(),
            "clearing the query restores the reorder"
        );
        assert_ne!(
            page_rows(&window),
            before,
            "the same drag now moves the row"
        );
    }

    #[test]
    fn alt_arrows_on_a_filtered_page_move_no_page_row() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let titles = ["a", "keep0", "b", "keep1"];
        playlist_page(&mut window, &titles);
        window.library.track_filter.edit(PLAYLIST).push_str("keep");
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "keep0");

        // The page really is narrowed, so the guard below cannot pass on a page
        // that never filtered. `page_rows` reads the list itself, which keeps
        // all four rows; the painted titles are what the user sees.
        assert_eq!(
            page_rows(&window),
            titles.map(|title| format!("list:{title}")),
            "the page's own list is whole"
        );
        let shown = painted_titles(&mut window, &mut transport, &mut queue, &titles);
        assert_eq!(shown, vec!["keep0", "keep1"], "the query narrows the page");

        let command = chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowDown,
            egui::Modifiers::ALT,
        );

        // There is no page-level Alt handler: `alt_step` moves the queue's rows
        // only, so this is a regression guard for the day one is added rather
        // than a test of a page feature.
        assert_eq!(command, None);
        assert_eq!(
            painted_titles(&mut window, &mut transport, &mut queue, &titles),
            shown,
            "Alt+↓ is the queue's, never a page row's"
        );
        assert_eq!(
            page_rows(&window),
            titles.map(|title| format!("list:{title}")),
            "Alt+↓ leaves the list's own order alone"
        );
        assert_eq!(
            window.library.track_filter.needle(PLAYLIST).as_deref(),
            Some("keep"),
            "the Alt chord does not clear the query either"
        );
    }

    #[test]
    fn del_in_the_search_field_edits_text_and_removes_no_row() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");
        let at = field_box(&mut window, &mut transport, &mut queue).center();
        let _ = window.click(&mut transport, &mut queue, at);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::Text("abc".to_owned())],
        );
        // Del forward-deletes, and the caret sits after the text, so one step
        // left puts a character in front of it.
        let _ = press(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::ArrowLeft,
        );

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(command, None);
        assert_eq!(window.library.search.text, "ab", "the field lost the key");
        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    }

    #[test]
    fn a_cut_posted_while_a_field_is_typing_is_dropped() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let at = field_box(&mut window, &mut transport, &mut queue).center();
        let _ = window.click(&mut transport, &mut queue, at);

        context_menu::post_cut(&window.ctx, page_cut(&window, &[0]));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
        assert!(!window.library.write_in_flight(), "a write went out");
    }

    #[test]
    fn a_cut_posted_with_the_drawer_open_removes_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(800.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        window.drawer = Drawer::Open;

        context_menu::post_cut(&window.ctx, page_cut(&window, &[0]));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    }

    #[test]
    fn a_cut_posted_while_carrying_removes_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        // Any payload says a block is held; the gate reads no more than that.
        egui::DragAndDrop::set_payload(&window.ctx, ());

        context_menu::post_cut(&window.ctx, page_cut(&window, &[0]));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    }

    #[test]
    fn a_cut_posted_while_a_write_is_pending_removes_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        assert!(
            window
                .library
                .remove_tracks(PLAYLIST, &[0], &page_entries(&window, &[0]))
                .is_some()
        );

        context_menu::post_cut(&window.ctx, page_cut(&window, &[0]));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        assert_eq!(
            page_rows(&window),
            vec!["list:p1", "list:p2"],
            "a second removal went out behind the first"
        );
    }

    #[test]
    fn delete_on_a_read_only_page_does_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        read_only_playlist_page(&mut window, &["p0", "p1"]);
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(command, None);
        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    }

    #[test]
    fn del_with_the_menu_open_chooses_the_menu_row_not_the_page() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1", "p2"]);
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");
        let targets = page_entries(&window, &[2]);
        open_menu(
            &mut window,
            Verb::Remove,
            targets,
            playlist_listing(),
            vec![2],
        );
        settle_menu(&mut window, &mut transport, &mut queue);

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(
            command,
            Some(Command::RemoveTracks {
                playlist: PLAYLIST.to_owned(),
                revision: revision("rev-1"),
                removals: vec![Removal { from: 2, length: 1 }],
            }),
            "the page's focused row answered a Del the menu owned"
        );
        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    }

    #[test]
    fn a_cut_on_the_frame_a_menu_opened_is_dropped() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        context_menu::post(&window.ctx, page_request(&window, &[1]));
        context_menu::post_cut(&window.ctx, page_cut(&window, &[0]));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(window.menu.is_open(), "the request did not open a menu");
        assert_eq!(
            page_rows(&window),
            vec!["list:p0", "list:p1"],
            "a cut landed under a menu that opened over it"
        );
    }

    #[test]
    fn a_cut_is_dropped_when_another_region_took_the_command() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        let _ = answered_menu(&mut window, &mut transport, &mut queue, "a", 3);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        context_menu::post_cut(&window.ctx, page_cut(&window, &[0]));

        // The search menu's Enter is the frame's command, so the cut is spent
        // with it rather than applied behind it.
        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "t0".to_owned()
            }),
            "the search menu did not take the frame"
        );
        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
        assert!(!window.library.write_in_flight());
    }
    #[test]
    fn a_right_click_on_a_queue_row_opens_the_queue_menu_and_removes_it() {
        let mut transport = Transport::default();
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, pane_rect_when_shown(&window), "t2");

        for event in [
            egui::Event::PointerMoved(at),
            side(at, egui::PointerButton::Secondary, true),
            side(at, egui::PointerButton::Secondary, false),
        ] {
            let _ = window.frame(&mut transport, &mut queue, vec![event]);
        }

        settle_menu(&mut window, &mut transport, &mut queue);
        assert!(window.menu.is_open(), "the right-click opened no menu");
        assert!(
            window
                .menu
                .items()
                .iter()
                .all(|item| item.verb != Verb::AddToQueue),
            "the queue offered to add what is already there"
        );
        let last = window.menu.items().last().expect("a command");
        assert_eq!(last.verb, Verb::Remove);
        assert_eq!(last.label, "Remove from the queue");

        // A pointer-opened menu lights nothing, so Enter is not its to fire;
        // Del is the destructive row's key.
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(entries(&queue), vec!["t0", "t1", "t3"]);
        assert!(!window.menu.is_open(), "the menu outlived its choice");
    }

    #[test]
    fn shift_f10_on_a_focused_queue_row_opens_the_queue_menu() {
        let mut transport = Transport::default();
        let mut queue = queue_of(4, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, pane_rect_when_shown(&window), "t2");
        let _ = window.click(&mut transport, &mut queue, at);
        assert!(!window.menu.is_open(), "a plain click opened a menu");

        let _ = chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::F10,
            egui::Modifiers::SHIFT,
        );

        assert!(window.menu.is_open(), "Shift+F10 opened no menu");
        assert_eq!(
            window.menu.cut().map(|cut| cut.rows),
            Some(vec![2]),
            "the menu is not about the focused row"
        );
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
        assert!(!window.menu.is_open());
    }
    #[test]
    fn a_click_on_now_playings_more_opens_a_current_menu() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        queue.start(&[menu_track(0)], 0, None, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        let at = now_playing_more(&window);
        let _ = window.click(&mut transport, &mut queue, at);
        settle_menu(&mut window, &mut transport, &mut queue);

        assert!(window.menu.is_open(), "the more button opened no menu");
        assert!(
            window
                .menu
                .items()
                .iter()
                .all(|item| { item.verb != Verb::AddToQueue && item.verb != Verb::Remove }),
            "Now Playing offered a list command"
        );
        assert_eq!(window.menu.cut(), None, "the entry was removable");
        assert_eq!(
            window
                .menu
                .items()
                .iter()
                .map(|item| item.verb)
                .collect::<Vec<_>>(),
            [Verb::GoToAlbum, Verb::GoToArtist, Verb::CopyLink]
        );
    }

    #[test]
    fn now_playings_more_opens_from_the_keyboard_after_a_click() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        queue.start(&[menu_track(0)], 0, None, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let at = now_playing_more(&window);

        // The press opens the menu and leaves the keyboard on the button; the
        // Escape hands it back there, so the key door has something to work from.
        let _ = window.click(&mut transport, &mut queue, at);
        settle_menu(&mut window, &mut transport, &mut queue);
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
        assert!(!window.menu.is_open());

        let _ = chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::F10,
            egui::Modifiers::SHIFT,
        );

        assert!(window.menu.is_open(), "Shift+F10 opened no menu");
    }

    #[test]
    fn now_playings_more_waits_without_a_current_entry() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        let at = now_playing_more(&window);
        let _ = window.click(&mut transport, &mut queue, at);

        assert!(
            !window.menu.is_open(),
            "a waiting more button opened a menu"
        );
    }

    // ---- the library head's new-playlist field ----

    /// The head's create button, from the band the sidebar solves.
    ///
    /// The chips are a 2x2 grid of `extent::BUTTON` cells with `space::S1`
    /// between them, under `space::S4` of padding, and the head row is `space::S2`
    /// below them. The button is one `extent::BUTTON` square at the row's right.
    fn head_create(band: Rect) -> Rect {
        let chips = 2.0 * extent::BUTTON + space::S1;
        let row = Rect::from_min_size(
            egui::pos2(
                band.left() + space::S4,
                band.top() + space::S4 + chips + space::S3 + space::S2,
            ),
            Vec2::new(band.width() - 2.0 * space::S4, extent::BUTTON),
        );
        Rect::from_min_size(
            egui::pos2(row.right() - extent::BUTTON, row.top()),
            Vec2::splat(extent::BUTTON),
        )
    }

    /// The head's button centre, from the band the sidebar solves.
    fn head_plus(band: Rect) -> egui::Pos2 {
        head_create(band).center()
    }

    fn sidebar_plus(window: &Window) -> egui::Pos2 {
        head_plus(sidebar_rect(window))
    }

    /// A sidebar whose playlists and rootlist are ready to create one.
    fn creatable_sidebar(window: &mut Window) {
        sidebar_playlists(
            window,
            &[("a", Some(0)), ("b", Some(0))],
            Some(rootlist_access(true)),
        );
    }

    /// Opens the head's field with the `+`, and waits for the keyboard to follow.
    fn open_naming(window: &mut Window, transport: &mut Transport, queue: &mut Queue) {
        let _ = window.click(transport, queue, sidebar_plus(window));
        assert!(window.library.naming.is_open(), "the `+` opened no field");
        // The field is drawn on the frame after the press, and takes the keyboard
        // on that frame.
        let _ = window.frame(transport, queue, Vec::new());
        assert!(
            window.ctx.text_edit_focused(),
            "the keyboard did not follow the field"
        );
        // One more frame, so the field's own key filter is the stored one: egui
        // stores a filter only for a widget that had focus last frame.
        let _ = window.frame(transport, queue, Vec::new());
    }

    /// Types into whatever holds the keyboard.
    fn type_text(window: &mut Window, transport: &mut Transport, queue: &mut Queue, text: &str) {
        let _ = window.frame(transport, queue, vec![egui::Event::Text(text.to_owned())]);
    }

    /// The row a create's reply hands the sidebar.
    fn new_playlist(name: &str, access: Option<PlaylistAccess>) -> PlaylistEntry {
        PlaylistEntry {
            composed: Vec::new(),
            uri: "spotify:playlist:new".to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            tracks: Some(0),
            owner: None,
            access,
        }
    }

    /// Submits the open field with `name` typed into it, and answers the frame.
    fn submit_name(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        name: &str,
    ) -> Option<Command> {
        open_naming(window, transport, queue);
        type_text(window, transport, queue, name);
        window
            .frame(transport, queue, vec![key(egui::Key::Enter, true)])
            .0
    }

    #[test]
    fn plus_opens_the_field_and_the_keyboard_follows_next_frame() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);

        let _ = window.click(&mut transport, &mut queue, sidebar_plus(&window));
        assert!(window.library.naming.is_open());
        assert!(
            !window.ctx.text_edit_focused(),
            "the field took the keyboard on the frame it opened"
        );

        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(window.ctx.text_edit_focused());
    }

    #[test]
    fn enter_submits_the_trimmed_name_once() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);

        assert_eq!(
            submit_name(&mut window, &mut transport, &mut queue, "  Night  Drive "),
            Some(Command::CreatePlaylist {
                name: "Night  Drive".to_owned(),
                revision: revision("root-1"),
                tracks: Vec::new(),
            })
        );
        assert!(window.library.write_in_flight());
        assert!(window.library.naming.writing());

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Enter, false)],
        );
        assert_eq!(
            window
                .frame(
                    &mut transport,
                    &mut queue,
                    vec![key(egui::Key::Enter, true)]
                )
                .0,
            None,
            "a second Enter sent a second create"
        );
    }

    #[test]
    fn a_held_enter_submits_once() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);

        assert!(submit_name(&mut window, &mut transport, &mut queue, "Mix").is_some());
        assert_eq!(
            window
                .frame(
                    &mut transport,
                    &mut queue,
                    vec![key_repeat(egui::Key::Enter)]
                )
                .0,
            None,
            "a held Enter replayed the submit"
        );
    }

    #[test]
    fn enter_on_blank_text_sends_nothing_and_keeps_the_field() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);

        assert_eq!(
            submit_name(&mut window, &mut transport, &mut queue, "   "),
            None
        );
        assert!(window.library.naming.is_open(), "a blank submit closed it");
        assert!(
            window.ctx.text_edit_focused(),
            "a blank submit dropped the keyboard"
        );
        assert!(!window.library.write_in_flight());
        assert_eq!(window.library.sidebar_notice, None, "a blank submit spoke");
    }

    #[test]
    fn typing_beyond_the_cap_is_truncated() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);
        open_naming(&mut window, &mut transport, &mut queue);

        type_text(&mut window, &mut transport, &mut queue, &"a".repeat(140));

        assert_eq!(
            window.library.naming.text().chars().count(),
            takofy_model::PLAYLIST_NAME_MAX,
            "the field took more than a name"
        );
    }

    #[test]
    fn blur_with_blank_text_cancels_and_blur_with_text_stays() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);

        open_naming(&mut window, &mut transport, &mut queue);
        let _ = window.click(&mut transport, &mut queue, view_rect(&window).center());
        assert!(
            !window.library.naming.is_open(),
            "a blank field stayed open after a click elsewhere"
        );

        open_naming(&mut window, &mut transport, &mut queue);
        type_text(&mut window, &mut transport, &mut queue, "Night Drive");
        let _ = window.click(&mut transport, &mut queue, view_rect(&window).center());
        assert!(
            window.library.naming.is_open(),
            "a click elsewhere threw a typed name away"
        );
        assert_eq!(window.library.naming.text(), "Night Drive");
        assert!(
            !window.ctx.text_edit_focused(),
            "the field kept the keyboard"
        );
    }

    #[test]
    fn the_close_button_cancels() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);
        open_naming(&mut window, &mut transport, &mut queue);
        type_text(&mut window, &mut transport, &mut queue, "Night Drive");

        let _ = window.click(&mut transport, &mut queue, sidebar_plus(&window));

        assert!(!window.library.naming.is_open(), "the × did not close it");
        assert_eq!(window.library.naming.text(), "");
        assert!(!window.library.write_in_flight());
    }

    #[test]
    fn while_writing_the_field_is_read_only_and_cancel_is_inert() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);

        assert!(submit_name(&mut window, &mut transport, &mut queue, "Night Drive").is_some());
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            !window.ctx.text_edit_focused(),
            "a field mid-write still took the keyboard"
        );

        let _ = window.click(&mut transport, &mut queue, sidebar_plus(&window));
        assert!(
            window.library.naming.is_open(),
            "a cancel ended a create in flight"
        );
        assert!(window.library.naming.writing());

        type_text(&mut window, &mut transport, &mut queue, "x");
        assert_eq!(
            window.library.naming.text(),
            "Night Drive",
            "a read-only field took a keystroke"
        );
    }

    #[test]
    fn a_make_failure_refocuses_the_field() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);

        assert!(submit_name(&mut window, &mut transport, &mut queue, "Night Drive").is_some());
        window.library.apply(&Update::PlaylistCreateFailed);
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(window.library.naming.is_open());
        assert_eq!(window.library.naming.text(), "Night Drive");
        assert!(
            window.ctx.text_edit_focused(),
            "the retry had no keyboard in the field"
        );
        assert_eq!(
            window.library.sidebar_notice,
            Some(crate::library::SidebarWrite::Create)
        );
    }

    #[test]
    fn typing_in_the_rail_field_reaches_neither_the_transport_nor_a_cut() {
        let mut transport = playing(cover("a"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        sidebar_playlists(
            &mut window,
            &[("a", Some(0)), ("b", Some(1))],
            Some(rootlist_access(true)),
        );
        liked(&mut window, &[]);
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let field = rail_filter_rect(&frame, sidebar_rect(&window));
        let _ = window.click(&mut transport, &mut queue, field.center());
        // The field's own key filter is stored a frame after the focus lands.
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            window.ctx.text_edit_focused(),
            "the click did not focus the rail's field"
        );
        type_text(&mut window, &mut transport, &mut queue, "a");
        assert_eq!(
            window.library.rail_filter.query(),
            "a",
            "the typed character did not reach the rail's query"
        );

        assert_eq!(
            window
                .frame(
                    &mut transport,
                    &mut queue,
                    vec![key(egui::Key::Space, true)]
                )
                .0,
            None,
            "a space reached the transport through the rail's field"
        );
        assert_eq!(
            window
                .frame(
                    &mut transport,
                    &mut queue,
                    vec![key(egui::Key::Delete, true)]
                )
                .0,
            None,
            "Del cut a row while the rail's field had the keyboard"
        );
        crate::view::heart::post(
            &window.ctx,
            crate::view::heart::Ask {
                tracks: vec!["spotify:track:a".to_owned()],
                liked: true,
            },
        );
        assert_eq!(
            window.frame(&mut transport, &mut queue, Vec::new()).0,
            None,
            "a heart wrote while the rail's field had the keyboard"
        );
        assert_eq!(window.library.likes.state("spotify:track:a"), Some(false));
        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    }

    #[test]
    fn a_drop_on_a_narrowed_rail_row_adds_to_that_playlist() {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &[TRACK_A]);
        sidebar_playlists(
            &mut window,
            &[
                ("Alpha", Some(0)),
                ("Bravo", Some(1)),
                ("Midnight City", Some(2)),
                ("Midnight Sun", Some(3)),
            ],
            Some(rootlist_access(true)),
        );
        window.library.rail_filter.edit().push_str("midnight");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), TRACK_A);
        // The first drawn row stands for the third playlist.
        let row = sidebar_row_rect(&window, &frame, "Midnight City", "Playlist · 2 tracks");
        let (_, held) = window.hold_drag(&mut transport, &mut queue, from, row.center());
        assert!(marker(&held).is_none(), "an add draws no rule");

        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![window.release(row.center())],
        );

        assert_eq!(
            command,
            Some(Command::AddTracks {
                playlist: "playlist:Midnight City".to_owned(),
                revision: revision("root-1"),
                from_index: 2,
                tracks: vec![TRACK_A.to_owned()],
            }),
            "the drop names the playlist the drawn row stands for"
        );
    }

    #[test]
    fn a_drop_between_narrowed_rows_reorders_nothing() {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &[TRACK_A]);
        sidebar_playlists(
            &mut window,
            &[
                ("Alpha", Some(0)),
                ("Midnight City", Some(1)),
                ("Midnight Sun", Some(2)),
            ],
            Some(rootlist_access(true)),
        );

        // The rail row lifts while the column is whole, which is the only way a
        // reorder can be started at all. Row 0 is not next to the gap the drop
        // below lands on, so nothing but the narrowing refuses the move.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let lifted = sidebar_row_rect(&window, &frame, "Alpha", "Playlist · 1 track");
        let (_, _) = window.hold_drag(
            &mut transport,
            &mut queue,
            lifted.center(),
            lifted.center() + Vec2::new(0.0, 20.0),
        );
        let carry = carry::carried(&window.ctx).expect("the rail row lifts");
        assert_eq!(carry.source, Source::Playlist);
        assert_eq!(carry.rows, vec![0]);

        // The query narrows the list under the carry: the two drawn rows stand
        // for playlists 1 and 2, so the gap below them is a boundary of a list
        // the user is no longer looking at.
        window.library.rail_filter.edit().push_str("midnight");
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let boundary = sidebar_boundary(
            &window,
            &frame,
            &["Midnight City", "Midnight Sun"],
            "Playlist · 1 track",
            2,
        );

        let (_, held) = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(boundary)],
        );
        let (command, _) = window.frame(&mut transport, &mut queue, vec![window.release(boundary)]);

        assert!(
            marker(&held).is_none(),
            "a narrowed column draws no reorder rule"
        );
        assert_eq!(
            command, None,
            "a narrowed column moved the playlist its slot happened to name"
        );
    }

    // ---- the rail's field and the page's field, both on screen ----

    #[test]
    fn the_rail_query_survives_a_page_change_while_the_page_filter_clears() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (rail, page) = both_fields(&mut window, &mut transport, &mut queue);

        focus_field(&mut window, &mut transport, &mut queue, rail);
        type_text(&mut window, &mut transport, &mut queue, "mid");
        focus_field(&mut window, &mut transport, &mut queue, page);
        type_text(&mut window, &mut transport, &mut queue, "keep");
        assert_eq!(window.library.rail_filter.query(), "mid");
        assert_eq!(window.library.track_filter.edit(PLAYLIST).as_str(), "keep");

        // A page change is the page's reset and only the page's: the rail keeps
        // what was typed in it.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let row = sidebar_row_rect(&window, &frame, "Midnight City", "Playlist");
        let command = window.click(&mut transport, &mut queue, row.center());

        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: "playlist:Midnight City".to_owned(),
            })
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist("playlist:Midnight City".to_owned()))
        );
        assert_eq!(
            window.library.track_filter.edit(PLAYLIST).as_str(),
            "",
            "the page change left the page's query behind"
        );
        assert_eq!(
            window.library.rail_filter.query(),
            "mid",
            "the page change cleared the rail's query too"
        );
    }

    #[test]
    fn escape_in_the_rail_field_clears_the_rail_and_leaves_the_page_query() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (rail, _page) = both_fields(&mut window, &mut transport, &mut queue);

        focus_field(&mut window, &mut transport, &mut queue, rail);
        type_text(&mut window, &mut transport, &mut queue, "mid");
        window.library.track_filter.edit(PLAYLIST).push_str("keep");

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

        assert_eq!(window.library.rail_filter.query(), "");
        assert_eq!(
            window.library.track_filter.edit(PLAYLIST).as_str(),
            "keep",
            "an Escape in the rail reached the page's field"
        );
        assert!(
            !window.ctx.text_edit_focused(),
            "Escape gave the keyboard back"
        );
    }

    #[test]
    fn escape_in_the_page_field_clears_the_page_and_leaves_the_rail_query() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_rail, page) = both_fields(&mut window, &mut transport, &mut queue);

        focus_field(&mut window, &mut transport, &mut queue, page);
        type_text(&mut window, &mut transport, &mut queue, "keep");
        window.library.rail_filter.edit().push_str("mid");

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

        assert_eq!(window.library.track_filter.edit(PLAYLIST).as_str(), "");
        assert_eq!(
            window.library.rail_filter.query(),
            "mid",
            "an Escape in the page reached the rail's field"
        );
        assert!(
            !window.ctx.text_edit_focused(),
            "Escape gave the keyboard back"
        );
    }

    #[test]
    fn escape_with_focus_on_a_row_clears_neither() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (_rail, _page) = both_fields(&mut window, &mut transport, &mut queue);
        window.library.rail_filter.edit().push_str("mid");
        window.library.track_filter.edit(PLAYLIST).push_str("keep");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = row_line(&frame, view_rect(&window), "keep0");
        let _ = window.click(&mut transport, &mut queue, at);
        assert!(focused_stop(&window).is_some(), "the row took the keyboard");

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

        assert_eq!(
            window.library.rail_filter.query(),
            "mid",
            "an Escape on a row cleared the rail's field"
        );
        assert_eq!(
            window.library.track_filter.edit(PLAYLIST).as_str(),
            "keep",
            "an Escape on a row cleared the page's field"
        );
    }

    #[test]
    fn typing_in_one_field_does_not_move_the_other_fields_text() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (rail, page) = both_fields(&mut window, &mut transport, &mut queue);

        focus_field(&mut window, &mut transport, &mut queue, rail);
        type_text(&mut window, &mut transport, &mut queue, "mid");
        focus_field(&mut window, &mut transport, &mut queue, page);
        type_text(&mut window, &mut transport, &mut queue, "keep");

        assert_eq!(window.library.rail_filter.query(), "mid");
        assert_eq!(window.library.track_filter.edit(PLAYLIST).as_str(), "keep");
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "mid", sidebar_rect(&window)).is_some(),
            "the rail's field still draws its own text"
        );
        assert!(
            text_in(&frame, "keep", view_rect(&window)).is_some(),
            "the page's field still draws its own text"
        );
    }

    #[test]
    fn the_rail_field_is_a_tab_stop_before_the_rows_and_before_the_page_field() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (rail, page) = both_fields(&mut window, &mut transport, &mut queue);
        let stops = tab_stops_until_page_field(&mut window, &mut transport, &mut queue, rail, page);

        let at = |kind: &str| {
            stops
                .iter()
                .position(|stop| *stop == kind)
                .unwrap_or_else(|| panic!("Tab never reached {kind}: {stops:?}"))
        };
        let chips = at("rail-chips");
        let rail_at = at("rail-field");
        let head = at("rail-head");
        let row = at("rail-row");
        let top = at("topbar");
        let page_at = at("page-field");

        assert_eq!(
            stops.first(),
            Some(&"rail-chips"),
            "Tab starts in the rail's chips: {stops:?}"
        );
        assert!(
            chips < rail_at,
            "the chips must come before the rail's field: {stops:?}"
        );
        assert!(
            rail_at < head,
            "the rail's field must come before the create button: {stops:?}"
        );
        assert!(
            head < row,
            "the create button must come before the rail's rows: {stops:?}"
        );
        assert!(
            row < top,
            "the rail's rows must come before the topbar: {stops:?}"
        );
        assert!(
            top < page_at,
            "the topbar must come before the page's field: {stops:?}"
        );
    }

    #[test]
    fn without_a_live_create_button_the_rail_field_still_precedes_the_rows_and_the_page_field() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (rail, page) = both_fields_without_create(&mut window, &mut transport, &mut queue);
        assert!(
            !window.library.creatable(),
            "the fixture can create a playlist, so its create button is a Tab stop"
        );

        let stops = tab_stops_until_page_field(&mut window, &mut transport, &mut queue, rail, page);

        let at = |kind: &str| {
            stops
                .iter()
                .position(|stop| *stop == kind)
                .unwrap_or_else(|| panic!("Tab never reached {kind}: {stops:?}"))
        };
        let chips = at("rail-chips");
        let rail_at = at("rail-field");
        let row = at("rail-row");
        let top = at("topbar");
        let page_at = at("page-field");

        // The inert `+` is no stop at all: with it gone the chain the creatable
        // fixture checks must still hold, and nothing may stand in the head.
        assert!(
            !stops.contains(&"rail-head"),
            "an inert create button is still a Tab stop: {stops:?}"
        );
        assert!(
            chips < rail_at,
            "the chips must come before the rail's field: {stops:?}"
        );
        assert!(
            rail_at < row,
            "the rail's field must come before the rail's rows: {stops:?}"
        );
        assert!(
            row < top,
            "the rail's rows must come before the topbar: {stops:?}"
        );
        assert!(
            top < page_at,
            "the topbar must come before the page's field: {stops:?}"
        );
    }

    #[test]
    fn shift_tab_from_the_first_rail_row_returns_to_the_create_button() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (rail, _page) = both_fields(&mut window, &mut transport, &mut queue);
        let rail_area = sidebar_rect(&window);
        let create = head_create(rail_area);

        let mut row = None;
        for _ in 0..48 {
            let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Tab);
            if let Some(stop) = focused_stop(&window)
                && rail_area.contains_rect(stop)
                && stop.center().y > rail.bottom()
                && (stop.height() - sidebar::ROW_H).abs() < 0.6
            {
                row = Some(stop);
                break;
            }
        }
        assert!(row.is_some(), "Tab reached no rail row");

        let _ = chord(
            &mut window,
            &mut transport,
            &mut queue,
            egui::Key::Tab,
            egui::Modifiers::SHIFT,
        );

        let stop = focused_stop(&window).expect("Shift+Tab put the keyboard somewhere");
        assert!(
            (stop.min - create.min).length() < 0.5 && (stop.max - create.max).length() < 0.5,
            "Shift+Tab from the first rail row landed on {stop:?}, not the create button at \
             {create:?}"
        );
    }

    #[test]
    fn the_rail_offers_its_surviving_rows_as_add_targets_while_a_page_row_is_carried() {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &[TRACK_A]);
        sidebar_playlists(
            &mut window,
            &[
                ("Alpha", Some(0)),
                ("Bravo", Some(1)),
                ("Midnight City", Some(2)),
                ("Midnight Sun", Some(3)),
            ],
            Some(rootlist_access(true)),
        );
        window.library.rail_filter.edit().push_str("midnight");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = row_line(&frame, view_rect(&window), TRACK_A);
        let row = sidebar_row_rect(&window, &frame, "Midnight City", "Playlist · 2 tracks");
        let _ = window.hold_drag(&mut transport, &mut queue, from, row.center());
        let carry = carry::carried(&window.ctx).expect("the page row is carried");
        assert!(
            matches!(carry.source, Source::List(_)),
            "the carried block is the page's row"
        );

        // The shell's own pass read and spent the frame's offer, so the rail is
        // drawn once more over the same carried block to read what it offers.
        let _ = column_frame(
            &window.ctx,
            &mut window.library,
            &mut window.art,
            Vec::new(),
            true,
        );
        let offer = carry::take_column_offer(&window.ctx).expect("the rail offers itself");

        assert_eq!(offer.zone.count, 2, "only the survivors are offered");
        assert_eq!(
            offer.rows,
            Some(vec![2, 3]),
            "the offer names the playlists the drawn slots stand for"
        );
        assert_eq!(offer.source_row(0), Some(2));
        assert_eq!(offer.source_row(1), Some(3));
    }

    #[test]
    fn a_filtered_rail_still_opens_the_page_for_a_clicked_row_and_keeps_the_query() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let (rail, _page) = both_fields(&mut window, &mut transport, &mut queue);
        focus_field(&mut window, &mut transport, &mut queue, rail);
        type_text(&mut window, &mut transport, &mut queue, "mid");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let row = sidebar_row_rect(&window, &frame, "Midnight City", "Playlist");
        let command = window.click(&mut transport, &mut queue, row.center());

        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: "playlist:Midnight City".to_owned(),
            }),
            "the click opened the playlist the drawn row stands for"
        );
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist("playlist:Midnight City".to_owned()))
        );
        assert_eq!(
            window.library.rail_filter.query(),
            "mid",
            "opening a page cleared the query that found it"
        );
    }

    #[test]
    fn a_created_playlist_is_visible_after_the_create_reply() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);
        window.library.rail_filter.edit().push_str("zzz");

        // The create field takes the filter field's slot, so opening it clears
        // the query: the name about to be typed would be hidden by it.
        open_naming(&mut window, &mut transport, &mut queue);
        assert_eq!(
            window.library.rail_filter.query(),
            "",
            "opening the create field left a query that hides the new row"
        );

        type_text(&mut window, &mut transport, &mut queue, "Night Drive");
        let command = window
            .frame(
                &mut transport,
                &mut queue,
                vec![key(egui::Key::Enter, true)],
            )
            .0;
        assert!(matches!(command, Some(Command::CreatePlaylist { .. })));
        window.library.apply(&Update::PlaylistCreated {
            entry: new_playlist("Night Drive", page_access(true)),
            revision: revision("root-2"),
        });

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(
            text_in(&frame, "Night Drive", sidebar_rect(&window)).is_some(),
            "the created row is hidden by the query the create field should have cleared"
        );
    }

    #[test]
    fn space_del_and_hearts_do_nothing_while_the_field_has_the_keyboard() {
        let mut transport = playing(cover("a"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        sidebar_playlists(&mut window, &[("a", Some(0))], Some(rootlist_access(true)));
        liked(&mut window, &[]);
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");
        open_naming(&mut window, &mut transport, &mut queue);
        type_text(&mut window, &mut transport, &mut queue, "Mix");

        assert_eq!(
            window
                .frame(
                    &mut transport,
                    &mut queue,
                    vec![key(egui::Key::Space, true)]
                )
                .0,
            None,
            "a space reached the transport through the field"
        );
        assert_eq!(
            window
                .frame(
                    &mut transport,
                    &mut queue,
                    vec![key(egui::Key::Delete, true)]
                )
                .0,
            None,
            "Del cut a row while the field had the keyboard"
        );
        crate::view::heart::post(
            &window.ctx,
            crate::view::heart::Ask {
                tracks: vec!["spotify:track:a".to_owned()],
                liked: true,
            },
        );
        assert_eq!(
            window.frame(&mut transport, &mut queue, Vec::new()).0,
            None,
            "a heart wrote while the field had the keyboard"
        );
        assert_eq!(window.library.likes.state("spotify:track:a"), Some(false));
        assert_eq!(page_rows(&window), vec!["list:p0", "list:p1"]);
    }

    #[test]
    fn the_count_label_returns_when_naming_closes() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "2 SAVED", sidebar_rect(&window)).is_some(),
            "the field's resting line is missing"
        );

        open_naming(&mut window, &mut transport, &mut queue);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "2 SAVED", sidebar_rect(&window)).is_none(),
            "the count stayed beside the field"
        );

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Escape, true)],
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "2 SAVED", sidebar_rect(&window)).is_some(),
            "the count did not come back"
        );
    }

    // ---- the head drawn alone, where its own widget ids are nameable ----

    /// One frame of the sidebar alone, so a test can name the head's ids.
    struct Column {
        command: Option<Command>,
        slot: egui::Id,
        field: egui::Id,
    }

    /// Draws one frame of the sidebar at the column's own width.
    fn column_frame(
        ctx: &egui::Context,
        library: &mut Library,
        art: &mut Art,
        events: Vec<egui::Event>,
        drag: bool,
    ) -> Column {
        let tokens = Tokens::DARK;
        let band = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(254.0, 800.0));
        let input = egui::RawInput {
            screen_rect: Some(band),
            events,
            ..Default::default()
        };
        let mut slot = egui::Id::NULL;
        let mut field = egui::Id::NULL;
        let mut command = None;
        art.begin_frame();
        let mut output = ctx.run_ui(input, |ui| {
            let control = crate::availability::Control::LibraryCreate;
            slot = ui.scope_id().with((control, ()));
            field = ui.scope_id().with((control, "name"));
            command = if drag {
                sidebar::column(ui, library, art, Instant::now(), &tokens)
            } else {
                sidebar::show(ui, library, art, Instant::now(), &tokens, true).command
            };
        });
        output.textures_delta.clear();
        Column {
            command,
            slot,
            field,
        }
    }

    /// A styled context for the sidebar's own frames.
    fn column_context() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        ctx
    }

    /// Clicks `at` over the passes egui's hit test needs.
    fn column_click(
        ctx: &egui::Context,
        library: &mut Library,
        art: &mut Art,
        drag: bool,
        at: egui::Pos2,
    ) -> Column {
        let _ = column_frame(ctx, library, art, Vec::new(), drag);
        let mut last = column_frame(ctx, library, art, Vec::new(), drag);
        for event in crate::view::test_support::click_events(at) {
            last = column_frame(ctx, library, art, vec![event], drag);
        }
        last
    }

    /// A library whose sidebar holds `names`, with a rootlist revision.
    fn column_library(names: &[&str], rootlist: bool) -> Library {
        let mut library = Library::default();
        library.apply(&Update::Playlists(
            names
                .iter()
                .map(|name| PlaylistEntry {
                    composed: Vec::new(),
                    uri: format!("playlist:{name}"),
                    name: (*name).to_owned(),
                    named: true,
                    cover: None,
                    description: None,
                    owner: None,
                    tracks: None,
                    access: None,
                })
                .collect(),
        ));
        if rootlist {
            library.apply(&Update::RootlistAccess(rootlist_access(true)));
        }
        library
    }

    #[test]
    fn escape_on_the_frame_the_field_opens() {
        let ctx = column_context();
        let mut art = Art::default();
        let mut library = column_library(&["a"], true);
        library.naming.open();
        library.naming.edit().push_str("Night Drive");

        // The field's key filter is stored only on the frame after it first
        // holds the keyboard, so this Escape belongs to nobody: the frame opens
        // the field and the key is dropped rather than closing it.
        let opened = column_frame(
            &ctx,
            &mut library,
            &mut art,
            vec![key(egui::Key::Escape, true)],
            true,
        );

        assert!(
            library.naming.is_open(),
            "the opening frame's Escape closed the field"
        );
        assert_eq!(library.naming.text(), "Night Drive");
        assert!(
            ctx.memory(|memory| memory.has_focus(opened.field)),
            "the opening frame's Escape took the keyboard off the field"
        );
        assert_eq!(opened.command, None);
    }

    #[test]
    fn escape_cancels_and_returns_focus_to_plus() {
        let ctx = column_context();
        let mut art = Art::default();
        let mut library = column_library(&["a"], true);
        library.naming.open();
        library.naming.edit().push_str("Night Drive");

        let opened = column_frame(&ctx, &mut library, &mut art, Vec::new(), true);
        assert!(
            ctx.memory(|memory| memory.has_focus(opened.field)),
            "the field never took the keyboard"
        );
        // One frame for the field's own key filter to be the stored one.
        let _ = column_frame(&ctx, &mut library, &mut art, Vec::new(), true);

        let escaped = column_frame(
            &ctx,
            &mut library,
            &mut art,
            vec![key(egui::Key::Escape, true)],
            true,
        );
        assert!(!library.naming.is_open(), "Escape did not close the field");
        assert_eq!(library.naming.text(), "");
        assert!(
            ctx.memory(|memory| memory.has_focus(escaped.slot)),
            "the keyboard did not go back to the +"
        );
    }

    #[test]
    fn escape_does_nothing_when_the_field_is_closed() {
        let ctx = column_context();
        let mut art = Art::default();
        let mut library = column_library(&["a"], true);

        let closed = column_frame(
            &ctx,
            &mut library,
            &mut art,
            vec![key(egui::Key::Escape, true)],
            true,
        );

        assert!(!library.naming.is_open());
        assert_eq!(closed.command, None);
        assert_eq!(library.naming.text(), "");
    }

    #[test]
    fn tab_leaves_the_field() {
        let ctx = column_context();
        let mut art = Art::default();
        let mut library = column_library(&["a"], true);
        library.naming.open();
        library.naming.edit().push_str("Night Drive");
        let opened = column_frame(&ctx, &mut library, &mut art, Vec::new(), true);
        assert!(ctx.memory(|memory| memory.has_focus(opened.field)));
        // One frame for the field's own key filter to be the stored one.
        let _ = column_frame(&ctx, &mut library, &mut art, Vec::new(), true);

        let _ = column_frame(
            &ctx,
            &mut library,
            &mut art,
            vec![key(egui::Key::Tab, true)],
            true,
        );
        let after = column_frame(&ctx, &mut library, &mut art, Vec::new(), true);

        assert!(
            !ctx.memory(|memory| memory.has_focus(after.field)),
            "Tab left the keyboard in the field"
        );
        assert!(library.naming.is_open(), "Tab closed the field");
        assert_eq!(library.naming.text(), "Night Drive");
    }

    #[test]
    fn the_plus_is_inert_in_the_drawer_form() {
        let ctx = column_context();
        let mut art = Art::default();
        let mut library = column_library(&["a"], true);
        let band = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(254.0, 800.0));

        let _ = column_click(&ctx, &mut library, &mut art, false, head_plus(band));

        assert!(
            !library.naming.is_open(),
            "the drawer's + opened a field whose command is discarded"
        );
    }

    #[test]
    fn a_naming_session_does_not_clear_the_query_in_the_drawer() {
        let ctx = column_context();
        let mut art = Art::default();
        let mut library = column_library(&["Midnight City", "Outro"], true);
        library.rail_filter.edit().push_str("midnight");
        // The drawer's create button is inert, so the head keeps the filter
        // field on screen even while a naming session is open.
        library.naming.open();

        let _ = column_frame(&ctx, &mut library, &mut art, Vec::new(), false);

        assert!(library.naming.is_open());
        assert_eq!(
            library.rail_filter.query(),
            "midnight",
            "the drawer's naming session cleared the query with the column's slot"
        );
    }

    // ---- the drawer's copy of the rail field ----

    /// A narrow window with the drawer open and a few playlists to narrow.
    fn drawer_with_rows(transport: &mut Transport, queue: &mut Queue) -> Window {
        let mut window = Window::new(Vec2::new(800.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[
                ("Midnight City", None),
                ("Outro", None),
                ("Midnight Sun", None),
            ],
            None,
        );
        open_drawer(&mut window, transport, queue);
        window
    }

    #[test]
    fn a_pick_in_the_drawer_loads_the_page_it_names() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = drawer_with_rows(&mut transport, &mut queue);
        let drawer = drawer_rect(&window);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        // The page behind draws the same playlist as a card, so the drawer's own
        // copy is the last one painted.
        let row = frame
            .shapes
            .iter()
            .rev()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text)
                    if text.galley.text() == "Midnight City" && drawer.contains(text.pos) =>
                {
                    Some(Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .expect("the drawer drew the row");

        let command = window.click(&mut transport, &mut queue, row.center());

        assert_eq!(
            command,
            Some(Command::LoadTracks {
                playlist: "playlist:Midnight City".to_owned()
            }),
            "the drawer's pick never reached the frame: {command:?}"
        );
        assert_eq!(window.drawer, Drawer::Closed, "a pick closes the drawer");
        assert_eq!(
            window.library.page(),
            Some(&Page::Playlist("playlist:Midnight City".to_owned())),
            "the page the row named did not open"
        );
    }

    /// A page that is already open asks for nothing, so `settle` returns no
    /// command. The press is still the user picking a row, so the drawer closes
    /// rather than staying open over a page the press did nothing to.
    #[test]
    fn a_pick_of_the_page_already_open_closes_the_drawer_without_a_command() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(800.0, 900.0));
        removable_rows(&mut window);
        assert!(window.library.choose_kind(SidebarKind::Artists));
        open_drawer(&mut window, &mut transport, &mut queue);
        let drawer = drawer_rect(&window);
        let row = |window: &mut Window, transport: &mut Transport, queue: &mut Queue| {
            let (_, frame) = window.frame(transport, queue, Vec::new());
            frame
                .shapes
                .iter()
                .rev()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text)
                        if text.galley.text() == "Vela Moreno" && drawer.contains(text.pos) =>
                    {
                        Some(Rect::from_min_size(text.pos, text.galley.size()))
                    }
                    _ => None,
                })
                .expect("the drawer drew the row")
                .center()
        };

        let first = row(&mut window, &mut transport, &mut queue);
        assert!(window.click(&mut transport, &mut queue, first).is_some());
        assert_eq!(window.drawer, Drawer::Closed);

        open_drawer(&mut window, &mut transport, &mut queue);
        let again = row(&mut window, &mut transport, &mut queue);
        assert_eq!(
            window.click(&mut transport, &mut queue, again),
            None,
            "a page already open asks for nothing"
        );
        assert_eq!(
            window.drawer,
            Drawer::Closed,
            "the press was still a pick, so the drawer closes"
        );
    }

    /// The drawer draws after every region, and a `ScrollArea` takes the wheel
    /// only when its own layer holds the pointer. A drawer whose area is
    /// zero-sized never holds it, so the page behind scrolled and the drawer's
    /// rows did not.
    #[test]
    fn the_wheel_scrolls_the_drawer_and_leaves_the_page_behind() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(800.0, 900.0));
        let tracks: Vec<String> = (0..40).map(|index| format!("t{index}")).collect();
        let titles: Vec<&str> = tracks.iter().map(String::as_str).collect();
        playlist_page(&mut window, &titles);
        let names: Vec<String> = (0..40).map(|index| format!("Row {index}")).collect();
        let rows: Vec<(&str, Option<usize>)> =
            names.iter().map(|name| (name.as_str(), None)).collect();
        sidebar_playlists(&mut window, &rows, None);
        open_drawer(&mut window, &mut transport, &mut queue);
        let drawer = drawer_rect(&window);

        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(drawer.center())],
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let page_before = painted_at(&frame, "t3").expect("the page drew its tracks");
        let drawer_before = text_in(&frame, "Row 6", drawer).expect("the drawer drew its rows");

        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -120.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        };
        for _ in 0..4 {
            let _ = window.frame(&mut transport, &mut queue, vec![wheel.clone()]);
        }
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let page_after = painted_at(&frame, "t3").expect("the page still draws its tracks");
        let drawer_after =
            text_in(&frame, "Row 6", drawer).expect("the drawer still draws its rows");

        assert!(
            drawer_after.top() < drawer_before.top() - 20.0,
            "the wheel did not scroll the drawer: {} then {}",
            drawer_before.top(),
            drawer_after.top()
        );
        assert!(
            (page_after.y - page_before.y).abs() < 0.01,
            "the page behind the drawer moved: {} then {}",
            page_before.y,
            page_after.y
        );
    }

    /// A failed list asks for itself on the frame it is drawn. That is a
    /// command, not a pick: the drawer must stay open, or a list that failed
    /// while the rail was narrow could never be retried from it.
    #[test]
    fn a_failed_list_retries_in_the_drawer_without_closing_it() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(800.0, 900.0));
        window.library.playlists = Load::Failed(Failure::Load);

        open_drawer(&mut window, &mut transport, &mut queue);

        assert_eq!(
            window.drawer,
            Drawer::Open,
            "a retry read as a picked row and closed the drawer"
        );
        assert!(
            matches!(window.library.playlists, Load::Loading),
            "the failed list did not ask for itself"
        );
    }

    #[test]
    fn the_drawer_draws_the_same_field_and_it_is_live() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = drawer_with_rows(&mut transport, &mut queue);
        let field = drawer_field_rect(&mut window, &mut transport, &mut queue);

        focus_field(&mut window, &mut transport, &mut queue, field);
        type_text(&mut window, &mut transport, &mut queue, "midnight");

        assert_eq!(window.library.rail_filter.query(), "midnight");
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let drawer = drawer_rect(&window);
        assert!(
            text_in(&frame, "Midnight City", drawer).is_some()
                && text_in(&frame, "Midnight Sun", drawer).is_some(),
            "the drawer's field did not narrow its rows"
        );
        assert!(
            text_in(&frame, "Outro", drawer).is_none(),
            "the drawer drew a row the query excludes"
        );
    }

    #[test]
    fn a_click_on_the_main_pane_still_closes_the_drawer() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = drawer_with_rows(&mut transport, &mut queue);
        // Below and right of the drawer, where the root grid has drawn nothing.
        let outside = egui::pos2(window.size.x - 50.0, window.size.y - 200.0);

        let _ = window.click(&mut transport, &mut queue, outside);

        assert_eq!(
            window.drawer,
            Drawer::Closed,
            "a click outside the drawer left it open"
        );
    }

    #[test]
    fn a_click_on_the_drawers_own_panel_leaves_it_open_and_moves_no_focus() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = drawer_with_rows(&mut transport, &mut queue);
        let field = drawer_field_rect(&mut window, &mut transport, &mut queue);
        let drawer = drawer_rect(&window);
        // The band below the chips and above the head's rule: the drawer's own
        // panel, with no control drawn on it.
        let chips = 2.0 * extent::BUTTON + space::S1;
        let spot = egui::pos2(
            drawer.center().x,
            drawer.top() + space::S4 + chips + space::S3 / 2.0,
        );
        assert!(
            drawer.contains(spot) && !field.contains(spot),
            "the click lands on the drawer's panel, not its field: {spot:?} in {drawer:?}, \
             field {field:?}"
        );

        let _ = window.click(&mut transport, &mut queue, spot);

        assert_eq!(
            window.drawer,
            Drawer::Open,
            "a click on the drawer's own panel closed it"
        );
        assert!(
            !window.ctx.text_edit_focused(),
            "a click on the drawer's own panel focused a field"
        );
    }

    #[test]
    fn escape_in_the_drawer_field_clears_before_it_closes_the_drawer() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = drawer_with_rows(&mut transport, &mut queue);
        let field = drawer_field_rect(&mut window, &mut transport, &mut queue);
        focus_field(&mut window, &mut transport, &mut queue, field);
        type_text(&mut window, &mut transport, &mut queue, "midnight");

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

        assert_eq!(
            window.library.rail_filter.query(),
            "",
            "the first Escape did not clear the drawer's field"
        );
        assert_eq!(
            window.drawer,
            Drawer::Open,
            "the Escape that cleared the query also closed the drawer"
        );

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
        assert_eq!(
            window.drawer,
            Drawer::Closed,
            "the second Escape left the drawer open"
        );
    }

    #[test]
    fn an_empty_drawer_field_surrenders_focus_on_the_first_escape_and_the_second_closes_it() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = drawer_with_rows(&mut transport, &mut queue);
        let field = drawer_field_rect(&mut window, &mut transport, &mut queue);
        focus_field(&mut window, &mut transport, &mut queue, field);

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

        assert!(
            !window.ctx.text_edit_focused(),
            "the first Escape left the empty field holding the keyboard"
        );
        assert_eq!(
            window.drawer,
            Drawer::Open,
            "the field's Escape closed the drawer"
        );

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);
        assert_eq!(
            window.drawer,
            Drawer::Closed,
            "the second Escape left the drawer open"
        );
    }

    #[test]
    fn the_query_typed_in_the_drawer_is_there_after_widening_to_the_column() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = drawer_with_rows(&mut transport, &mut queue);
        let field = drawer_field_rect(&mut window, &mut transport, &mut queue);
        focus_field(&mut window, &mut transport, &mut queue, field);
        type_text(&mut window, &mut transport, &mut queue, "midnight");

        window.size = Vec2::new(1400.0, 900.0);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

        assert_eq!(
            window.library.rail_filter.query(),
            "midnight",
            "widening to the column lost the drawer's query"
        );
        assert_eq!(
            window.drawer,
            Drawer::Closed,
            "the column kept the drawer open"
        );
        let area = sidebar_rect(&window);
        assert!(
            rail_filter_rect(&frame, area).width() > 0.0,
            "the column's own field is painted"
        );
        assert!(
            text_in(&frame, "midnight", area).is_some(),
            "the column's field does not draw the drawer's text"
        );
        assert!(
            text_in(&frame, "Outro", area).is_none(),
            "the column drew a row the drawer's query excludes"
        );
    }

    #[test]
    fn the_plus_is_inert_when_creatable_is_false() {
        let ctx = column_context();
        let mut art = Art::default();
        let mut library = column_library(&["a"], false);
        let band = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(254.0, 800.0));

        let _ = column_click(&ctx, &mut library, &mut art, true, head_plus(band));

        assert!(!library.creatable());
        assert!(
            !library.naming.is_open(),
            "a + with no rootlist to link against opened a field"
        );
    }

    // ---- the created row in the sidebar's list ----

    #[test]
    fn a_created_row_is_drawn_first() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let area = sidebar_rect(&window);

        window.library.naming.open();
        window.library.naming.edit().push_str("Night Drive");
        assert!(window.library.create_playlist().is_some());
        window.library.apply(&Update::PlaylistCreated {
            entry: new_playlist("Night Drive", page_access(true)),
            revision: revision("root-2"),
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

        let created = text_in(&frame, "Night Drive", area).expect("the new row is painted");
        let first = text_in(&frame, "a", area).expect("the old first row is painted");
        assert!(
            created.top() < first.top(),
            "the created row is not the first one drawn"
        );
    }

    #[test]
    fn the_reveal_scrolls_to_the_top_once() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let names: Vec<String> = (0..30).map(|index| format!("r{index}")).collect();
        let rows: Vec<(&str, Option<usize>)> =
            names.iter().map(|name| (name.as_str(), Some(0))).collect();
        sidebar_playlists(&mut window, &rows, Some(rootlist_access(true)));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let area = sidebar_rect(&window);

        // The pointer has to be over the list for the wheel to reach it.
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(egui::pos2(
                area.center().x,
                area.bottom() - 40.0,
            ))],
        );
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -600.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        };
        for _ in 0..3 {
            let _ = window.frame(&mut transport, &mut queue, vec![wheel.clone()]);
        }
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "r0", area).is_none(),
            "the list never scrolled away from its first row"
        );

        window.library.naming.open();
        window.library.naming.edit().push_str("Night Drive");
        assert!(window.library.create_playlist().is_some());
        window.library.apply(&Update::PlaylistCreated {
            entry: new_playlist("Night Drive", page_access(true)),
            revision: revision("root-2"),
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "Night Drive", area).is_some(),
            "the created row is not on screen"
        );

        // The reveal is one frame's: the wheel still moves the list afterwards.
        for _ in 0..3 {
            let _ = window.frame(&mut transport, &mut queue, vec![wheel.clone()]);
        }
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "Night Drive", area).is_none(),
            "the list was forced back to the top on every frame"
        );
    }

    #[test]
    fn a_create_is_refused_while_a_block_is_carried() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);
        open_naming(&mut window, &mut transport, &mut queue);
        type_text(&mut window, &mut transport, &mut queue, "Night Drive");
        // Any payload says a block is held; the gate reads no more than that.
        egui::DragAndDrop::set_payload(&window.ctx, ());

        assert_eq!(
            window
                .frame(
                    &mut transport,
                    &mut queue,
                    vec![key(egui::Key::Enter, true)]
                )
                .0,
            None,
            "a create went out while a block was carried"
        );
        assert!(!window.library.write_in_flight());
        assert_eq!(
            window.library.naming.text(),
            "Night Drive",
            "the dropped submit lost the name"
        );
    }

    #[test]
    fn the_row_count_updates() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        creatable_sidebar(&mut window);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "2 SAVED", sidebar_rect(&window)).is_some(),
            "the field's resting line is missing"
        );

        window.library.naming.open();
        window.library.naming.edit().push_str("Night Drive");
        assert!(window.library.create_playlist().is_some());
        window.library.apply(&Update::PlaylistCreated {
            entry: new_playlist("Night Drive", page_access(true)),
            revision: revision("root-2"),
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(
            text_in(&frame, "3 SAVED", sidebar_rect(&window)).is_some(),
            "the head did not count the new row"
        );
    }

    // ---- the menu's new-playlist row ----

    /// Opens the `⋯` menu of the page row titled `title`, and answers the verbs
    /// it offers.
    fn open_row_menu(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        title: &str,
    ) -> Vec<Verb> {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let more = more_button(&frame, view_rect(window), title);
        let _ = window.click(transport, queue, more);
        settle_menu(window, transport, queue);
        assert!(window.menu.is_open(), "the `⋯` opened no menu");
        window.menu.items().iter().map(|item| item.verb).collect()
    }

    #[test]
    fn the_menu_row_opens_the_field_and_sends_no_command() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        sidebar_playlists(&mut window, &[("a", Some(0))], Some(rootlist_access(true)));

        let offered = open_row_menu(&mut window, &mut transport, &mut queue, "p0");
        assert!(
            offered.contains(&Verb::NewPlaylist),
            "the row was stripped from a menu that could offer it: {offered:?}"
        );
        // The pointer opened the menu, so it lit nothing: the row is clicked where
        // it is painted.
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = painted_at(&frame, "New playlist…").expect("the row is painted");
        let command = window.click(
            &mut transport,
            &mut queue,
            egui::pos2(at.x + 20.0, at.y + 6.0),
        );

        assert_eq!(command, None, "the new-playlist row sent a command");
        assert!(!window.menu.is_open(), "the menu stayed open");
        assert!(
            window.library.naming.is_open(),
            "the row opened no naming field"
        );
        assert!(!window.library.write_in_flight(), "the row started a write");
    }

    #[test]
    fn a_created_list_takes_the_pick_the_menu_held() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        page_of_tracks(&mut window, &[TRACK_A]);
        sidebar_playlists(&mut window, &[("a", Some(0))], Some(rootlist_access(true)));
        let offered = open_row_menu(&mut window, &mut transport, &mut queue, TRACK_A);
        assert!(offered.contains(&Verb::NewPlaylist), "{offered:?}");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = painted_at(&frame, "New playlist…").expect("the row is painted");
        let _ = window.click(
            &mut transport,
            &mut queue,
            egui::pos2(at.x + 20.0, at.y + 6.0),
        );
        assert!(
            window.library.naming.is_open(),
            "the row opened no naming field"
        );
        assert_eq!(
            window.library.naming.pick().to_vec(),
            vec![TRACK_A.to_owned()],
            "the field did not take the pick the menu held"
        );

        type_text(&mut window, &mut transport, &mut queue, "Night Drive");
        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

        assert_eq!(
            command,
            Some(Command::CreatePlaylist {
                name: "Night Drive".to_owned(),
                revision: revision("root-1"),
                tracks: vec![TRACK_A.to_owned()],
            }),
            "the pick rides the create"
        );
    }

    #[test]
    fn the_menu_row_is_absent_in_the_rail_layout() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(800.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        sidebar_playlists(&mut window, &[("a", Some(0))], Some(rootlist_access(true)));
        assert!(window.library.creatable());

        let offered = open_row_menu(&mut window, &mut transport, &mut queue, "p0");

        assert!(
            !offered.contains(&Verb::NewPlaylist),
            "the rail offered a field it has no column for: {offered:?}"
        );
    }

    #[test]
    fn the_menu_row_is_absent_while_the_playlists_load() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0", "p1"]);
        window.library.playlists = Load::Loading;
        assert!(!window.library.creatable());

        let offered = open_row_menu(&mut window, &mut transport, &mut queue, "p0");

        assert!(
            !offered.contains(&Verb::NewPlaylist),
            "a list that is still loading offered a create: {offered:?}"
        );
    }

    #[test]
    fn a_chosen_row_is_dropped_while_the_drawer_is_open_or_a_block_is_carried() {
        // The keyboard door, because a press that reaches a menu row closes the
        // drawer first: this is the only way to make the choice the gate guards.
        for drawer_open in [true, false] {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            playlist_page(&mut window, &["p0", "p1"]);
            sidebar_playlists(&mut window, &[("a", Some(0))], Some(rootlist_access(true)));
            let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");
            let _ = chord(
                &mut window,
                &mut transport,
                &mut queue,
                egui::Key::F10,
                egui::Modifiers::SHIFT,
            );
            assert!(window.menu.is_open(), "the keyboard opened no menu");
            let offered: Vec<Verb> = window.menu.items().iter().map(|item| item.verb).collect();
            let row = offered
                .iter()
                .position(|verb| *verb == Verb::NewPlaylist)
                .expect("the row");
            for _ in 0..row {
                let _ = press(
                    &mut window,
                    &mut transport,
                    &mut queue,
                    egui::Key::ArrowDown,
                );
            }
            if drawer_open {
                window.drawer = Drawer::Open;
            } else {
                // Any payload says a block is held; the gate reads no more.
                egui::DragAndDrop::set_payload(&window.ctx, ());
            }

            let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

            assert!(
                !window.library.naming.is_open(),
                "a chosen row opened the field (drawer open: {drawer_open})"
            );
        }
    }

    #[test]
    fn a_submit_is_dropped_when_another_region_took_the_command() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        playlist_page(&mut window, &["p0"]);
        sidebar_playlists(&mut window, &[("a", Some(0))], Some(rootlist_access(true)));
        // An album on top, so Back returns to a playlist whose page was dropped
        // and has to be asked for again.
        window.library.open(Page::Album(ALBUM.to_owned()));
        window.library.tracks = Load::Idle;
        open_naming(&mut window, &mut transport, &mut queue);
        type_text(&mut window, &mut transport, &mut queue, "Night Drive");

        let command = window
            .frame(
                &mut transport,
                &mut queue,
                vec![
                    key(egui::Key::Enter, true),
                    key(egui::Key::BrowserBack, true),
                ],
            )
            .0;

        assert!(
            matches!(command, Some(Command::LoadTracks { .. })),
            "the frame's one command is the nav's: {command:?}"
        );
        assert!(
            !window.library.naming.writing(),
            "a dropped submit started a write"
        );
        assert!(!window.library.write_in_flight());
        assert_eq!(
            window.library.naming.text(),
            "Night Drive",
            "a dropped submit lost the name"
        );
    }

    /// A window whose sidebar holds one row the user may unlink.
    fn sidebar_row(window: &mut Window, uri: &str, name: &str) {
        window.library.apply(&Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
            uri: uri.to_owned(),
            name: name.to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: page_access(true),
        }]));
        window
            .library
            .apply(&Update::RootlistAccess(PlaylistAccess {
                revision: revision("rev-1"),
                editable: true,
            }));
    }

    /// Opens the menu a sidebar row's door posts, without a pointer.
    fn open_library_menu(window: &mut Window, row: LibraryItem, removable: bool) {
        let request = crate::menu::Request {
            door: crate::menu::Door {
                anchor: crate::menu::Anchor::Button(Rect::from_min_size(
                    egui::Pos2::new(60.0, 300.0),
                    Vec2::new(28.0, 28.0),
                )),
                opener: egui::Id::unique("library-row"),
                device: crate::menu::Device::Keyboard,
            },
            playlists: Vec::new(),
            items: crate::verbs::library_items(
                &row,
                crate::verbs::LibraryRights {
                    renamable: false,
                    removable,
                    saved: Some(true),
                },
            ),
            targets: Vec::new(),
            rows: Vec::new(),
            listing: Listing::Library(row),
            page: window.library.page().cloned(),
        };
        assert!(
            window.menu.open(request, Gate::default()),
            "the menu opened"
        );
    }

    fn library_row(kind: SidebarKind, uri: &str, name: &str) -> LibraryItem {
        LibraryItem {
            kind,
            uri: uri.to_owned(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn copy_link_on_a_library_row_writes_its_url() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let uri = "spotify:playlist:4uLU6hMCjMI75M1A2tKUQC";
        sidebar_row(&mut window, uri, "Deep Focus");
        open_library_menu(
            &mut window,
            library_row(SidebarKind::Playlists, uri, "Deep Focus"),
            true,
        );
        settle_menu(&mut window, &mut transport, &mut queue);

        let (_, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Enter, true)],
        );
        let copied = frame
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.as_str()),
                _ => None,
            });
        assert_eq!(
            copied,
            Some("https://open.spotify.com/playlist/4uLU6hMCjMI75M1A2tKUQC"),
            "an empty target list is not what decides a library menu's verb"
        );
    }

    #[test]
    fn a_library_rows_removal_issues_its_one_command() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let uri = "spotify:playlist:4uLU6hMCjMI75M1A2tKUQC";
        sidebar_row(&mut window, uri, "Deep Focus");
        open_library_menu(
            &mut window,
            library_row(SidebarKind::Playlists, uri, "Deep Focus"),
            true,
        );
        settle_menu(&mut window, &mut transport, &mut queue);

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(
            command,
            Some(Command::RemovePlaylist {
                playlist: uri.to_owned(),
                revision: revision("rev-1"),
                index: 0,
            })
        );
        assert!(window.library.write_in_flight());
    }

    #[test]
    fn a_library_rows_removal_is_dropped_while_a_gate_holds() {
        let uri = "spotify:album:4uLU6hMCjMI75M1A2tKUQC";

        // The drawer covers the window.
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let row = library_row(SidebarKind::Albums, uri, "A Record");
        window.library.apply(&Update::SavedAlbums(vec![SavedAlbum {
            uri: uri.to_owned(),
            name: "A Record".to_owned(),
            named: true,
            artists: String::new(),
            year: None,
            cover: None,
            tracks: None,
        }]));
        open_library_menu(&mut window, row.clone(), true);
        settle_menu(&mut window, &mut transport, &mut queue);
        window.drawer = Drawer::Open;
        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
            None,
            "a removal was applied under the drawer"
        );
        assert!(!window.library.write_in_flight());

        // A block is carried.
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        window.library.apply(&Update::SavedAlbums(vec![SavedAlbum {
            uri: uri.to_owned(),
            name: "A Record".to_owned(),
            named: true,
            artists: String::new(),
            year: None,
            cover: None,
            tracks: None,
        }]));
        open_library_menu(&mut window, row, true);
        settle_menu(&mut window, &mut transport, &mut queue);
        egui::DragAndDrop::set_payload(&window.ctx, ());
        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
            None,
            "a removal was applied while a block was carried"
        );
        assert!(!window.library.write_in_flight());
    }

    #[test]
    fn a_library_rows_removal_is_dropped_while_a_field_holds_the_keyboard() {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.set_global_style(style(&Tokens::DARK));
        let uri = "spotify:album:4uLU6hMCjMI75M1A2tKUQC";
        let mut library = Library::default();
        library.apply(&Update::SavedAlbums(vec![SavedAlbum {
            uri: uri.to_owned(),
            name: "A Record".to_owned(),
            named: true,
            artists: String::new(),
            year: None,
            cover: None,
            tracks: None,
        }]));
        let row = library_row(SidebarKind::Albums, uri, "A Record");
        let mut state = SignIn::SignedIn;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut drawer = Drawer::default();
        let mut menu = crate::menu::Menu::default();
        let mut art = Art::default();
        let mut devices = Devices::default();
        let mut list = DeviceList::default();
        let mut pane = layout::PanePref::default();
        let mut typed = String::new();
        let mut command = None;

        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1400.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| {
                let id = egui::Id::unique("field");
                egui::TextEdit::singleline(&mut typed).id(id).show(ui);
                ui.memory_mut(|memory| memory.request_focus(id));
                assert!(ui.ctx().text_edit_focused(), "the field holds the key");
                let mut screen = Screen {
                    state: &mut state,
                    library: &mut library,
                    transport: &mut transport,
                    queue: &mut queue,
                    devices: &mut devices,
                    list: &mut list,
                    drawer: &mut drawer,
                    menu: &mut menu,
                    position_ms: 0,
                    art: &mut art,
                    now: Instant::now(),
                    now_ms: 0,
                    pane: &mut pane,
                };
                command = apply_remove_from_library(ui, &mut screen, &row);
            },
        );

        output.textures_delta.clear();

        assert_eq!(command, None, "a removal was applied over a typing field");
        assert!(!library.write_in_flight());
    }

    // ---- the library root's list rows ----

    /// Draws the root in the list density on `kind`'s chip.
    fn library_list(window: &mut Window, kind: RootKind) {
        window.library.layout = MediaLayout::List;
        window.library.root_filter.choose(kind);
    }

    /// Clicks the `⋯` on the root's list row painted as `title`.
    fn click_root_more(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        title: &str,
    ) -> Option<Command> {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let more = more_button(&frame, view_rect(window), title);
        window.click(transport, queue, more)
    }

    /// Opens the menu the root's list row painted as `title` offers.
    fn open_root_menu(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        title: &str,
    ) {
        let _ = click_root_more(window, transport, queue, title);
        settle_menu(window, transport, queue);
        assert!(window.menu.is_open(), "the row's `⋯` opened no menu");
    }

    #[test]
    fn a_root_rows_more_opens_the_sidebar_rows_menu() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let uri = "spotify:playlist:a";
        sidebar_row(&mut window, uri, "Evening");
        library_list(&mut window, RootKind::Playlists);

        open_root_menu(&mut window, &mut transport, &mut queue, "Evening");

        assert_eq!(
            window.menu.listing(),
            &Listing::Library(library_row(SidebarKind::Playlists, uri, "Evening"))
        );
        let labels: Vec<&str> = window
            .menu
            .items()
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels, ["Copy link", "Remove from library"]);
        assert_eq!(
            window.menu.items().last().map(|item| item.kind),
            Some(crate::menu::Kind::Destructive),
            "the removal is the destructive row, drawn last"
        );
    }

    #[test]
    fn a_root_rows_removal_issues_its_kinds_own_write() {
        // An editable playlist is unlinked from the rootlist.
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let uri = "spotify:playlist:a";
        sidebar_row(&mut window, uri, "Evening");
        library_list(&mut window, RootKind::Playlists);
        open_root_menu(&mut window, &mut transport, &mut queue, "Evening");

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
            Some(Command::RemovePlaylist {
                playlist: uri.to_owned(),
                revision: revision("rev-1"),
                index: 0,
            })
        );
        assert!(window.library.write_in_flight());

        // A saved album is put back instead.
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let uri = "spotify:album:r";
        window.library.apply(&Update::Playlists(Vec::new()));
        window.library.apply(&Update::SavedAlbums(vec![SavedAlbum {
            uri: uri.to_owned(),
            name: "A Record".to_owned(),
            named: true,
            artists: String::new(),
            year: None,
            cover: None,
            tracks: None,
        }]));
        library_list(&mut window, RootKind::Albums);
        open_root_menu(&mut window, &mut transport, &mut queue, "A Record");

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
            Some(Command::SetSaved {
                uri: uri.to_owned(),
                saved: false,
            })
        );
        assert!(window.library.write_in_flight(), "the album's write is out");
    }

    #[test]
    fn a_root_rows_copy_link_writes_its_url() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let uri = "spotify:playlist:4uLU6hMCjMI75M1A2tKUQC";
        sidebar_row(&mut window, uri, "Deep Focus");
        library_list(&mut window, RootKind::Playlists);
        open_root_menu(&mut window, &mut transport, &mut queue, "Deep Focus");

        // The pointer opened the menu and lit nothing; one step down puts the
        // keyboard on the first row, the way a user reaching for the keys does.
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::ArrowDown, true)],
        );
        let (_, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Enter, true)],
        );

        let copied = frame
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.as_str()),
                _ => None,
            });
        assert_eq!(
            copied,
            Some("https://open.spotify.com/playlist/4uLU6hMCjMI75M1A2tKUQC")
        );
        assert!(!window.menu.is_open());
    }

    #[test]
    fn a_root_rows_more_opens_nothing_while_a_write_is_in_flight() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let uri = "spotify:playlist:a";
        sidebar_row(&mut window, uri, "Evening");
        library_list(&mut window, RootKind::Playlists);
        open_root_menu(&mut window, &mut transport, &mut queue, "Evening");
        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
            Some(Command::RemovePlaylist {
                playlist: uri.to_owned(),
                revision: revision("rev-1"),
                index: 0,
            })
        );
        assert!(window.library.write_in_flight());

        let _ = click_root_more(&mut window, &mut transport, &mut queue, "Evening");
        settle_menu(&mut window, &mut transport, &mut queue);

        // The gate shuts the menu whole: there is no menu to ask what it omits.
        assert!(!window.menu.is_open());

        // The write's reply clears the gate and leaves the row where it was, so
        // the same point pressed again opens the row's menu. Only the pair
        // proves the gate is what shut it: the second press shows a live `⋯`
        // sits under this point in this setup, the first shows the write alone
        // holds it.
        window.library.apply(&Update::PlaylistRemoveFailed {
            playlist: uri.to_owned(),
        });
        assert!(!window.library.write_in_flight());

        let _ = click_root_more(&mut window, &mut transport, &mut queue, "Evening");
        settle_menu(&mut window, &mut transport, &mut queue);
        assert!(
            window.menu.is_open(),
            "the `⋯` opened nothing with no write in flight"
        );
    }

    #[test]
    fn a_root_rows_refused_removal_is_a_dead_press() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let uri = "spotify:artist:a";
        window.library.apply(&Update::Playlists(Vec::new()));
        window
            .library
            .apply(&Update::FollowedArtists(vec![FollowedArtist {
                uri: uri.to_owned(),
                name: "Vela Moreno".to_owned(),
                named: true,
                portrait: None,
                releases: None,
            }]));
        library_list(&mut window, RootKind::Artists);
        open_root_menu(&mut window, &mut transport, &mut queue, "Vela Moreno");
        assert_eq!(
            window.menu.items().last().map(|item| item.verb),
            Some(Verb::RemoveFromLibrary),
            "the row was stripped of a removal the library still allowed"
        );

        // The list empties under the open menu, so the row the menu names is no
        // longer saved and the write refuses.
        window.library.apply(&Update::FollowedArtists(Vec::new()));
        assert_eq!(
            window.library.is_saved(uri),
            Some(false),
            "the removal was refused for another reason"
        );

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
            None,
            "a refused removal produced a command"
        );
        assert!(!window.menu.is_open());
        assert!(!window.library.write_in_flight());
        assert!(
            matches!(&window.library.followed_artists, Load::Ready(artists) if artists.is_empty()),
            "a refused removal changed the list"
        );
    }

    /// A point inside the sidebar row painted as `name`.
    fn sidebar_line(output: &egui::FullOutput, window: &Window, name: &str) -> egui::Pos2 {
        let rect = text_in(output, name, sidebar_rect(window))
            .unwrap_or_else(|| panic!("{name:?} is painted in the sidebar"));
        egui::Pos2::new(rect.left() + 4.0, rect.center().y)
    }

    /// Right-clicks the sidebar row painted as `name`, the way a mouse does.
    fn right_click_sidebar_row(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        name: &str,
    ) {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let at = sidebar_line(&frame, window, name);
        for event in [
            egui::Event::PointerMoved(at),
            side(at, egui::PointerButton::Secondary, true),
            side(at, egui::PointerButton::Secondary, false),
        ] {
            let _ = window.frame(transport, queue, vec![event]);
        }
    }

    /// A sidebar holding one row of each kind, each one the user may remove.
    fn removable_rows(window: &mut Window) {
        sidebar_playlists(
            window,
            &[("Night Drive", None)],
            Some(rootlist_access(true)),
        );
        window
            .library
            .apply(&Update::FollowedShows(vec![FollowedShow {
                uri: "spotify:show:s".to_owned(),
                name: "Field Notes".to_owned(),
                named: true,
                publisher: None,
                cover: None,
                episodes: None,
            }]));
        window
            .library
            .apply(&Update::FollowedArtists(vec![FollowedArtist {
                uri: "spotify:artist:a".to_owned(),
                name: "Vela Moreno".to_owned(),
                named: true,
                portrait: None,
                releases: None,
            }]));
        window.library.apply(&Update::SavedAlbums(vec![SavedAlbum {
            uri: "spotify:album:r".to_owned(),
            name: "A Record".to_owned(),
            named: true,
            artists: String::new(),
            year: None,
            cover: None,
            tracks: None,
        }]));
    }

    #[test]
    fn a_sidebar_rows_removal_asks_for_its_kinds_own_write() {
        for (kind, name) in [
            (SidebarKind::Playlists, "Night Drive"),
            (SidebarKind::Albums, "A Record"),
            (SidebarKind::Artists, "Vela Moreno"),
            (SidebarKind::Podcasts, "Field Notes"),
        ] {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            removable_rows(&mut window);
            assert!(window.library.choose_kind(kind));

            right_click_sidebar_row(&mut window, &mut transport, &mut queue, name);
            assert!(window.menu.is_open(), "{kind:?} opened no menu");
            settle_menu(&mut window, &mut transport, &mut queue);

            let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

            let expected = match kind {
                SidebarKind::Playlists => Command::RemovePlaylist {
                    playlist: "playlist:Night Drive".to_owned(),
                    revision: revision("root-1"),
                    index: 0,
                },
                SidebarKind::Albums => Command::SetSaved {
                    uri: "spotify:album:r".to_owned(),
                    saved: false,
                },
                SidebarKind::Artists => Command::SetFollow {
                    artist: "spotify:artist:a".to_owned(),
                    followed: false,
                },
                SidebarKind::Podcasts => Command::SetSaved {
                    uri: "spotify:show:s".to_owned(),
                    saved: false,
                },
            };
            assert_eq!(command, Some(expected), "{kind:?}");
            assert!(window.library.write_in_flight(), "{kind:?}");
        }
    }

    #[test]
    fn a_confirmed_removal_drops_the_row_and_its_open_page_goes_home() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("Night Drive", None), ("Deep Focus", None)],
            Some(rootlist_access(true)),
        );
        let _ = window.library.select("playlist:Night Drive");
        assert!(window.library.page().is_some());

        right_click_sidebar_row(&mut window, &mut transport, &mut queue, "Night Drive");
        settle_menu(&mut window, &mut transport, &mut queue);
        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);
        assert!(
            matches!(command, Some(Command::RemovePlaylist { .. })),
            "{command:?}"
        );

        window.library.apply(&Update::PlaylistRemoved {
            playlist: "playlist:Night Drive".to_owned(),
            revision: revision("root-2"),
        });

        assert_eq!(window.library.page(), None, "the removed page stayed open");
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            painted_at(&frame, "Deep Focus").is_some(),
            "the other row went with it"
        );
        assert!(
            painted_at(&frame, "Night Drive").is_none(),
            "the removed row stayed in the sidebar"
        );
    }

    #[test]
    fn a_confirmed_removal_lands_while_the_drawer_covers_the_column() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("Night Drive", None), ("Deep Focus", None)],
            Some(rootlist_access(true)),
        );
        let _ = window.library.select("playlist:Night Drive");

        right_click_sidebar_row(&mut window, &mut transport, &mut queue, "Night Drive");
        settle_menu(&mut window, &mut transport, &mut queue);
        assert!(
            matches!(
                press(&mut window, &mut transport, &mut queue, egui::Key::Delete),
                Some(Command::RemovePlaylist { .. })
            ),
            "the removal never went out"
        );

        window.drawer = Drawer::Open;
        window.library.apply(&Update::PlaylistRemoved {
            playlist: "playlist:Night Drive".to_owned(),
            revision: revision("root-2"),
        });

        assert_eq!(
            window.library.page(),
            None,
            "the drawer dropped the removed page's way home"
        );
        match &window.library.playlists {
            Load::Ready(playlists) => assert!(
                playlists
                    .iter()
                    .all(|entry| entry.uri != "playlist:Night Drive"),
                "the removed row stayed in the list"
            ),
            other => panic!("the list was dropped: {other:?}"),
        }
    }

    #[test]
    fn del_on_a_bare_focused_row_removes_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("Night Drive", None)],
            Some(rootlist_access(true)),
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = sidebar_line(&frame, &window, "Night Drive");
        for pressed in [true, false] {
            let _ = window.frame(
                &mut transport,
                &mut queue,
                vec![side(at, egui::PointerButton::Primary, pressed)],
            );
        }
        assert!(
            window.ctx.memory(|memory| memory.focused()).is_some(),
            "the click left the row without the keyboard"
        );

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(command, None, "a bare row unlinked a playlist");
        assert!(!window.library.write_in_flight());
    }

    #[test]
    fn a_sidebar_row_opens_no_menu_while_a_write_is_in_flight() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("Night Drive", None), ("Deep Focus", None)],
            Some(rootlist_access(true)),
        );
        assert!(
            window
                .library
                .move_playlist(0, 2, "playlist:Night Drive")
                .is_some()
        );

        right_click_sidebar_row(&mut window, &mut transport, &mut queue, "Deep Focus");

        assert!(!window.menu.is_open(), "a menu opened over a write");
    }

    #[test]
    fn escape_closes_a_sidebar_rows_menu_back_to_its_row() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("Night Drive", None)],
            Some(rootlist_access(true)),
        );
        right_click_sidebar_row(&mut window, &mut transport, &mut queue, "Night Drive");
        assert!(window.menu.is_open());
        let opener = window.menu.opener().expect("a door names its opener");
        settle_menu(&mut window, &mut transport, &mut queue);

        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

        assert!(!window.menu.is_open());
        assert_eq!(
            window.ctx.memory(|memory| memory.focused()),
            Some(opener),
            "the keyboard went somewhere other than the row"
        );
    }
    /// Applies a rename the way the menu's Rename row would.
    fn try_rename(
        window: &mut Window,
        uri: &str,
        surface: crate::naming::Surface,
    ) -> Option<Command> {
        let ctx = window.ctx.clone();
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let item = library_row(SidebarKind::Playlists, uri, uri);
        let mut command = None;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, window.size)),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let mut screen = Screen {
                state: &mut window.state,
                library: &mut window.library,
                transport: &mut transport,
                queue: &mut queue,
                devices: &mut window.devices,
                list: &mut window.list,
                drawer: &mut window.drawer,
                menu: &mut window.menu,
                position_ms: 0,
                art: &mut window.art,
                now: window.now,
                now_ms: 0,
                pane: &mut window.pane,
            };
            command = apply_rename(ui, &mut screen, &item, surface);
        });
        output.textures_delta.clear();
        command
    }

    #[test]
    fn apply_rename_opens_the_field_on_the_row_or_the_band() {
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");

        assert_eq!(
            try_rename(
                &mut window,
                "spotify:playlist:a",
                crate::naming::Surface::Row
            ),
            None
        );
        assert!(
            window
                .library
                .renaming
                .is_open_on("spotify:playlist:a", crate::naming::Surface::Row)
        );

        window.library.renaming.force_close();
        assert_eq!(
            try_rename(
                &mut window,
                "spotify:playlist:a",
                crate::naming::Surface::Band
            ),
            None
        );
        assert!(
            window
                .library
                .renaming
                .is_open_on("spotify:playlist:a", crate::naming::Surface::Band)
        );
    }

    #[test]
    fn apply_rename_drops_the_intent_when_the_field_cannot_be_seen() {
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");

        window.drawer = Drawer::Open;
        assert_eq!(
            try_rename(
                &mut window,
                "spotify:playlist:a",
                crate::naming::Surface::Row
            ),
            None
        );
        assert!(
            !window.library.renaming.is_open(),
            "the drawer hid the field"
        );
        window.drawer = Drawer::Closed;

        carry::lift(&window.ctx, carried_rows(egui::Id::unique("list"), vec![0]));
        assert_eq!(
            try_rename(
                &mut window,
                "spotify:playlist:a",
                crate::naming::Surface::Row
            ),
            None
        );
        assert!(
            !window.library.renaming.is_open(),
            "a carried block held the field shut"
        );
        egui::DragAndDrop::clear_payload(&window.ctx);
    }

    #[test]
    fn apply_rename_leaves_a_typed_field_alone() {
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        creatable_sidebar(&mut window);
        open_naming(&mut window, &mut transport, &mut queue);
        assert!(window.ctx.text_edit_focused());

        assert_eq!(
            try_rename(&mut window, "playlist:a", crate::naming::Surface::Row),
            None
        );
        assert!(
            !window.library.renaming.is_open(),
            "a typed field was replaced"
        );
    }

    /// Opens a sidebar row's menu with Rename on it.
    fn open_rename_menu(window: &mut Window, row: LibraryItem) {
        let request = crate::menu::Request {
            door: crate::menu::Door {
                anchor: crate::menu::Anchor::Button(Rect::from_min_size(
                    egui::Pos2::new(60.0, 300.0),
                    Vec2::new(28.0, 28.0),
                )),
                opener: egui::Id::unique("library-row"),
                device: crate::menu::Device::Keyboard,
            },
            playlists: Vec::new(),
            items: crate::verbs::library_items(
                &row,
                crate::verbs::LibraryRights {
                    renamable: true,
                    removable: true,
                    saved: Some(true),
                },
            ),
            targets: Vec::new(),
            rows: Vec::new(),
            listing: Listing::Library(row),
            page: window.library.page().cloned(),
        };
        assert!(
            window.menu.open(request, Gate::default()),
            "the menu opened"
        );
    }

    /// Opens a row's rename field from its menu and waits for the keyboard.
    fn open_row_rename(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        uri: &str,
        name: &str,
    ) {
        open_rename_menu(window, library_row(SidebarKind::Playlists, uri, name));
        settle_menu(window, transport, queue);
        let _ = press(window, transport, queue, egui::Key::Enter);
        let _ = window.frame(transport, queue, Vec::new());
        let _ = window.frame(transport, queue, Vec::new());
        assert!(
            window
                .library
                .renaming
                .is_open_on(uri, crate::naming::Surface::Row),
            "the Rename row opened no field"
        );
        assert!(
            window.ctx.text_edit_focused(),
            "the keyboard did not follow the field"
        );
    }

    #[test]
    fn a_row_menu_rename_opens_the_field_with_the_current_name() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");

        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );

        assert_eq!(window.library.renaming.text(), "A");
        assert!(!window.library.write_in_flight());
    }

    #[test]
    fn typing_and_enter_sends_the_trimmed_name() {
        for (typed, expected) in [
            ("  Night É  ".to_owned(), "Night É".to_owned()),
            ("é".repeat(100), "é".repeat(100)),
        ] {
            let mut transport = Transport::default();
            let mut queue = Queue::default();
            let mut window = Window::new(Vec2::new(1400.0, 900.0));
            sidebar_row(&mut window, "spotify:playlist:a", "A");
            open_row_rename(
                &mut window,
                &mut transport,
                &mut queue,
                "spotify:playlist:a",
                "A",
            );

            window.library.renaming.edit().clear();
            type_text(&mut window, &mut transport, &mut queue, &typed);
            let command = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);

            assert_eq!(
                command,
                Some(Command::RenamePlaylist {
                    playlist: "spotify:playlist:a".to_owned(),
                    name: expected,
                })
            );
            assert!(window.library.renaming.writing());
        }
    }

    #[test]
    fn escape_closes_the_row_field_and_sends_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );

        window.library.renaming.edit().clear();
        type_text(&mut window, &mut transport, &mut queue, "New");
        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

        assert_eq!(command, None);
        assert!(!window.library.renaming.is_open());
        assert!(!window.library.write_in_flight());
    }

    #[test]
    fn a_blank_blur_cancels_but_a_named_blur_stays_open() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );

        window.library.renaming.edit().clear();
        let id = window
            .ctx
            .memory(|memory| memory.focused())
            .expect("a focus");
        window.ctx.memory_mut(|memory| memory.surrender_focus(id));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(!window.library.renaming.is_open(), "a blank blur stayed");

        sidebar_row(&mut window, "spotify:playlist:a", "A");
        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );
        let id = window
            .ctx
            .memory(|memory| memory.focused())
            .expect("a focus");
        window.ctx.memory_mut(|memory| memory.surrender_focus(id));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            window.library.renaming.is_open(),
            "a named blur lost the name"
        );
        assert_eq!(window.library.renaming.text(), "A");
    }

    #[test]
    fn delete_inside_the_row_field_edits_text_and_removes_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );

        type_text(&mut window, &mut transport, &mut queue, "B");
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Home);
        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(command, None, "Del started a removal");
        assert_eq!(window.library.renaming.text(), "B", "Del left the text");
        assert_eq!(sidebar_order(&window), ["A"], "Del removed the row");
        assert!(!window.library.write_in_flight());
    }

    #[test]
    fn a_drag_from_the_open_row_begins_no_carry() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let from = sidebar_line(&frame, &window, "A");
        let to = from + Vec2::new(0.0, sidebar::ROW_H + 8.0);

        let command = window.drag(&mut transport, &mut queue, from, to);

        assert_eq!(command, None);
        assert!(
            !carry::carrying(&window.ctx),
            "a drag out of the field began a carry"
        );
        assert!(window.library.renaming.is_open());
    }

    #[test]
    fn no_menu_opens_on_another_row_while_the_rename_writes() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_playlists(
            &mut window,
            &[("A", Some(0)), ("B", Some(0))],
            Some(rootlist_access(true)),
        );
        open_row_rename(&mut window, &mut transport, &mut queue, "playlist:A", "A");
        type_text(&mut window, &mut transport, &mut queue, "New");
        assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());
        assert!(window.library.write_in_flight());

        right_click_sidebar_row(&mut window, &mut transport, &mut queue, "B");

        assert_eq!(
            crate::view::context_menu::take_request(&window.ctx),
            None,
            "a menu opened over a rename in flight"
        );
    }

    #[test]
    fn a_confirmed_row_rename_paints_the_new_name() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );
        type_text(&mut window, &mut transport, &mut queue, "New Name");
        assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());

        window.library.apply(&Update::PlaylistRenamed {
            playlist: "spotify:playlist:a".to_owned(),
            name: "New Name".to_owned(),
            revision: revision("rev-2"),
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(
            text_in(&frame, "New Name", sidebar_rect(&window)).is_some(),
            "the row still paints its old name"
        );
        assert!(!window.library.renaming.is_open());
    }

    #[test]
    fn a_failed_row_rename_keeps_the_field_and_shows_the_foot_sentence() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );
        window.library.renaming.edit().clear();
        type_text(&mut window, &mut transport, &mut queue, "New");
        assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());

        window.library.apply(&Update::PlaylistRenameFailed {
            playlist: "spotify:playlist:a".to_owned(),
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(window.library.renaming.is_open());
        assert_eq!(window.library.renaming.text(), "New");
        assert!(
            text_in(
                &frame,
                crate::signin::failure_message(Failure::Rename),
                sidebar_rect(&window)
            )
            .is_some(),
            "the foot did not say why the field stayed"
        );
    }

    /// The row's own widget id, taken from a press on it: a press on the row
    /// takes the keyboard, which is the place Tab continues from.
    fn press_sidebar_row(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        name: &str,
    ) -> egui::Id {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let at = sidebar_line(&frame, window, name);
        for event in crate::view::test_support::click_events(at) {
            let _ = window.frame(transport, queue, vec![event]);
        }
        window
            .ctx
            .memory(|memory| memory.focused())
            .expect("the row took no keyboard")
    }

    /// The band title's widget id, taken from the press that opens its field:
    /// a press on the title takes the keyboard before the field draws.
    fn press_band_title(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
        name: &str,
    ) -> egui::Id {
        let (_, frame) = window.frame(transport, queue, Vec::new());
        let at = text_in(&frame, name, view_rect(window))
            .unwrap_or_else(|| panic!("{name:?} is painted in the band"))
            .center();
        for event in crate::view::test_support::click_events(at) {
            let _ = window.frame(transport, queue, vec![event]);
        }
        assert!(
            window
                .library
                .renaming
                .is_open_on("playlist:Mix", crate::naming::Surface::Band),
            "the title opened no field"
        );
        window
            .ctx
            .memory(|memory| memory.focused())
            .expect("the title took no keyboard")
    }

    #[test]
    fn escape_on_the_row_field_hands_the_keyboard_back_to_its_row() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        let row = press_sidebar_row(&mut window, &mut transport, &mut queue, "A");

        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );
        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

        assert_eq!(command, None);
        assert!(!window.library.renaming.is_open());
        assert!(
            window.ctx.memory(|memory| memory.has_focus(row)),
            "Escape left the keyboard nowhere"
        );
    }

    #[test]
    fn a_confirmed_row_rename_hands_the_keyboard_back_to_its_row() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        let row = press_sidebar_row(&mut window, &mut transport, &mut queue, "A");

        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );
        type_text(&mut window, &mut transport, &mut queue, "New");
        assert!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some(),
            "Enter sent nothing"
        );
        window.library.apply(&Update::PlaylistRenamed {
            playlist: "spotify:playlist:a".to_owned(),
            name: "New".to_owned(),
            revision: revision("rev-2"),
        });
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(!window.library.renaming.is_open());
        assert!(
            window.ctx.memory(|memory| memory.has_focus(row)),
            "a confirmed rename left the keyboard nowhere"
        );
    }

    #[test]
    fn escape_on_the_band_field_hands_the_keyboard_back_to_the_title() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        band_page(&mut window);
        let title = press_band_title(&mut window, &mut transport, &mut queue, "Mix");
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            window.ctx.text_edit_focused(),
            "the band field has no keyboard"
        );

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

        assert_eq!(command, None);
        assert!(!window.library.renaming.is_open());
        assert!(
            window.ctx.memory(|memory| memory.has_focus(title)),
            "Escape left the keyboard nowhere"
        );
    }

    #[test]
    fn a_confirmed_band_rename_hands_the_keyboard_back_to_the_title() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        band_page(&mut window);
        let title = press_band_title(&mut window, &mut transport, &mut queue, "Mix");
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        type_text(&mut window, &mut transport, &mut queue, "New");
        assert!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some(),
            "Enter sent nothing"
        );
        window.library.apply(&Update::PlaylistRenamed {
            playlist: "playlist:Mix".to_owned(),
            name: "New".to_owned(),
            revision: revision("rev-2"),
        });
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(!window.library.renaming.is_open());
        assert!(
            window.ctx.memory(|memory| memory.has_focus(title)),
            "a confirmed rename left the keyboard nowhere"
        );
    }

    #[test]
    fn a_row_field_closes_when_the_sidebar_leaves_the_column() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );

        // Below the rail width the column is gone: the drawer is the sidebar,
        // and it is closed.
        window.size = Vec2::new(900.0, 900.0);
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(
            !window.library.renaming.is_open(),
            "the rail kept a field no row draws"
        );
    }

    #[test]
    fn escape_after_a_failed_row_rename_takes_the_sentence_with_it() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );
        window.library.renaming.edit().clear();
        type_text(&mut window, &mut transport, &mut queue, "New");
        assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());
        window.library.apply(&Update::PlaylistRenameFailed {
            playlist: "spotify:playlist:a".to_owned(),
        });
        // The reply asks for the keyboard back. The field owns the Escape
        // after that, even though egui stores a filter only for a widget that
        // had focus last frame.
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            window.library.sidebar_notice,
            Some(crate::library::SidebarWrite::Rename)
        );
        assert!(
            text_in(
                &frame,
                crate::signin::failure_message(Failure::Rename),
                sidebar_rect(&window)
            )
            .is_some(),
            "the foot never said why"
        );

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

        assert_eq!(command, None);
        assert!(!window.library.renaming.is_open());
        assert_eq!(
            window.library.sidebar_notice, None,
            "the failed rename's sentence outlived its field"
        );
    }

    #[test]
    fn escape_after_a_failed_band_rename_takes_both_sentences_with_it() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        band_page(&mut window);
        let _ = press_band_title(&mut window, &mut transport, &mut queue, "Mix");
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        type_text(&mut window, &mut transport, &mut queue, "New");
        assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());
        window.library.apply(&Update::PlaylistRenameFailed {
            playlist: "playlist:Mix".to_owned(),
        });
        // The reply asks for the keyboard back, and the field owns the Escape
        // after that.
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            window.library.sidebar_notice,
            Some(crate::library::SidebarWrite::Rename)
        );
        assert_eq!(page_notice(&window), Some(Failure::Rename));

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

        assert_eq!(command, None);
        assert!(!window.library.renaming.is_open());
        assert_eq!(
            window.library.sidebar_notice, None,
            "the foot's sentence outlived the field"
        );
        assert_eq!(
            page_notice(&window),
            None,
            "the page's sentence outlived the field"
        );
    }

    /// A failed rename asks for the keyboard back in the reply's own frame.
    /// The first Escape after that must close the field: a second press is not
    /// the user's first answer, and the field may not have one coming.
    #[test]
    fn one_escape_closes_a_row_field_after_a_failed_rename() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );
        window.library.renaming.edit().clear();
        type_text(&mut window, &mut transport, &mut queue, "New");
        assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());
        window.library.apply(&Update::PlaylistRenameFailed {
            playlist: "spotify:playlist:a".to_owned(),
        });
        // The reply's frame is the one that asks for the keyboard back.
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            window.ctx.text_edit_focused(),
            "the failed rename left the field without the keyboard"
        );

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

        assert_eq!(command, None);
        assert!(
            !window.library.renaming.is_open(),
            "one Escape after the failure was swallowed"
        );
    }

    /// The band field's half of the same claim: the failed reply hands the
    /// keyboard back, and the very next Escape is the one that closes it.
    #[test]
    fn one_escape_closes_a_band_field_after_a_failed_rename() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        band_page(&mut window);
        let _ = press_band_title(&mut window, &mut transport, &mut queue, "Mix");
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        type_text(&mut window, &mut transport, &mut queue, "New");
        assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());
        window.library.apply(&Update::PlaylistRenameFailed {
            playlist: "playlist:Mix".to_owned(),
        });
        // The reply's frame is the one that asks for the keyboard back.
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            window.ctx.text_edit_focused(),
            "the failed rename left the band field without the keyboard"
        );

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Escape);

        assert_eq!(command, None);
        assert!(
            !window.library.renaming.is_open(),
            "one Escape after the failure was swallowed"
        );
    }

    #[test]
    fn a_dismissed_row_field_takes_the_failed_rename_sentence_with_it() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );
        window.library.renaming.edit().clear();
        type_text(&mut window, &mut transport, &mut queue, "New");
        assert!(press(&mut window, &mut transport, &mut queue, egui::Key::Enter).is_some());
        window.library.apply(&Update::PlaylistRenameFailed {
            playlist: "spotify:playlist:a".to_owned(),
        });
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            window.library.sidebar_notice,
            Some(crate::library::SidebarWrite::Rename)
        );

        // A chip change dismisses the row's field rather than cancelling it.
        assert!(window.library.choose_kind(SidebarKind::Podcasts));
        let _ = window.frame(&mut transport, &mut queue, Vec::new());

        assert!(!window.library.renaming.is_open());
        assert_eq!(
            window.library.sidebar_notice, None,
            "a dismissed field kept the sentence it explained"
        );
    }

    /// The sentence a playlist page carries, when it carries one.
    fn page_notice(window: &Window) -> Option<Failure> {
        match &window.library.tracks {
            Load::Ready(tracks) => tracks.notice,
            Load::Idle | Load::Loading | Load::Failed(_) => None,
        }
    }

    #[test]
    fn a_chip_change_dismisses_an_open_row_field() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        sidebar_row(&mut window, "spotify:playlist:a", "A");
        open_row_rename(
            &mut window,
            &mut transport,
            &mut queue,
            "spotify:playlist:a",
            "A",
        );

        assert!(window.library.choose_kind(SidebarKind::Podcasts));

        assert!(!window.library.renaming.is_open());
    }

    /// Posts a band menu the way the band's "…" does.
    fn open_band_menu(window: &mut Window, row: LibraryItem, rights: crate::verbs::LibraryRights) {
        let request = crate::menu::Request {
            door: crate::menu::Door {
                anchor: crate::menu::Anchor::Button(Rect::from_min_size(
                    egui::Pos2::new(60.0, 300.0),
                    Vec2::new(28.0, 28.0),
                )),
                opener: egui::Id::unique("band-more"),
                device: crate::menu::Device::Keyboard,
            },
            playlists: Vec::new(),
            items: crate::verbs::library_items(&row, rights),
            targets: Vec::new(),
            rows: Vec::new(),
            listing: Listing::Band(row),
            page: window.library.page().cloned(),
        };
        assert!(
            window.menu.open(request, Gate::default()),
            "the menu opened"
        );
    }

    /// A playlist page whose sidebar row is the same playlist.
    fn band_page(window: &mut Window) {
        sidebar_playlists(window, &[("Mix", Some(0))], Some(rootlist_access(true)));
        window.library.select("playlist:Mix");
        window.library.apply(&Update::Tracks {
            playlist: "playlist:Mix".to_owned(),
            tracks: vec![TrackEntry {
                uri: "list:p0".to_owned(),
                title: "p0".to_owned(),
                ..track(0, None)
            }],
            cover: None,
            access: page_access(true),
        });
    }

    #[test]
    fn removing_from_the_band_menu_unlinks_and_takes_the_page_home() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        band_page(&mut window);
        let row = library_row(SidebarKind::Playlists, "playlist:Mix", "Mix");
        open_band_menu(
            &mut window,
            row,
            crate::verbs::LibraryRights {
                renamable: true,
                removable: true,
                saved: Some(true),
            },
        );
        settle_menu(&mut window, &mut transport, &mut queue);

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Delete);

        assert_eq!(
            command,
            Some(Command::RemovePlaylist {
                playlist: "playlist:Mix".to_owned(),
                revision: revision("root-1"),
                index: 0,
            })
        );
        window.library.apply(&Update::PlaylistRemoved {
            playlist: "playlist:Mix".to_owned(),
            revision: revision("root-2"),
        });
        assert!(
            window.library.page().is_none(),
            "the page stayed after its list left"
        );
    }

    #[test]
    fn copy_link_on_the_band_menu_writes_the_playlist_url() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let uri = "spotify:playlist:4uLU6hMCjMI75M1A2tKUQC";
        sidebar_row(&mut window, uri, "Deep Focus");
        window.library.select(uri);
        let row = library_row(SidebarKind::Playlists, uri, "Deep Focus");
        open_band_menu(
            &mut window,
            row,
            crate::verbs::LibraryRights {
                renamable: false,
                removable: false,
                saved: Some(true),
            },
        );
        settle_menu(&mut window, &mut transport, &mut queue);

        let (_, frame) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Enter, true)],
        );
        let copied = frame
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.as_str()),
                _ => None,
            });
        assert_eq!(
            copied,
            Some("https://open.spotify.com/playlist/4uLU6hMCjMI75M1A2tKUQC")
        );
    }

    // -----------------------------------------------------------------------
    // The seam under the view: a page's pinned height belongs to its page.
    // -----------------------------------------------------------------------

    /// The action row's box once it pins: the page's surface fill at the height
    /// it condenses to, inside the view.
    fn action_row(output: &egui::FullOutput, window: &Window) -> Option<Rect> {
        let tokens = Tokens::DARK;
        let view = view_rect(window);
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.fill == tokens.palette.surface
                        && (shape.rect.height() - crate::theme::actionbar::STUCK_H).abs()
                            < 0.01
                        && view.contains_rect(shape.rect) =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
    }

    /// The chip row's own surface once it pins: a band taller than the bar's
    /// condensed height, so the two fills are told apart.
    fn chip_band(output: &egui::FullOutput, window: &Window) -> Option<Rect> {
        let tokens = Tokens::DARK;
        let view = view_rect(window);
        output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.fill == tokens.palette.surface
                        && (shape.rect.height() - crate::theme::actionbar::STUCK_H).abs()
                            > 0.01
                        && view.contains_rect(shape.rect) =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
    }

    /// A playlist page long enough to scroll its band and action row away.
    fn long_playlist_page(window: &mut Window) {
        let titles: Vec<String> = (0..60).map(|index| format!("p{index}")).collect();
        let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
        playlist_page(window, &titles);
    }

    /// Wheels the view until its action row pins, answering the frame it pinned
    /// on.
    fn stuck_page_frame(
        window: &mut Window,
        transport: &mut Transport,
        queue: &mut Queue,
    ) -> egui::FullOutput {
        let at = view_rect(window).center();
        let _ = window.frame(transport, queue, vec![egui::Event::PointerMoved(at)]);
        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -600.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        };
        let mut output = window.frame(transport, queue, Vec::new()).1;
        for _ in 0..8 {
            if action_row(&output, window).is_some() {
                return output;
            }
            output = window.frame(transport, queue, vec![wheel.clone()]).1;
        }
        panic!("the page's action row never pinned");
    }

    #[test]
    fn the_panels_queue_reveal_does_not_inherit_the_pages_inset() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(8, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        long_playlist_page(&mut window);

        // A queue row in the panel takes the keyboard, so the panel's rows ask to
        // be seen through the same seam a page's rows do. t2 is far enough down
        // the pane that scrolling the page leaves it where it is.
        let pane = pane_rect_when_shown(&window);
        clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");
        let focused = window
            .ctx
            .memory(|memory| memory.focused())
            .expect("the queue row took the keyboard");

        // The page pins its action row, and the panel draws its rows in the same
        // frame.
        let stuck = stuck_page_frame(&mut window, &mut transport, &mut queue);
        assert!(
            action_row(&stuck, &window).is_some(),
            "the page's action row is stuck before the panel draws"
        );
        let row = window
            .ctx
            .read_response(focused)
            .expect("the focused queue row is still drawn")
            .rect;
        assert!(
            pane.contains_rect(row),
            "the focused queue row left the panel: {row:?}"
        );
        assert_eq!(
            crate::view::pin::inset(&window.ctx),
            0.0,
            "the panel's queue rows were revealed clear of the page's bar"
        );
    }

    #[test]
    fn a_page_without_a_bar_starts_the_frame_at_no_inset() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        // The panel forgets the seam for its own rows, so it is hidden here: no
        // region of this window would otherwise leave the page's inset standing
        // for the next frame to inherit.
        window.pane = layout::PanePref::Hidden;
        long_playlist_page(&mut window);

        let stuck = stuck_page_frame(&mut window, &mut transport, &mut queue);
        assert!(
            action_row(&stuck, &window).is_some(),
            "the page's action row is stuck"
        );
        assert!(
            crate::view::pin::inset(&window.ctx) > 0.0,
            "the stuck page left its height in the seam"
        );

        // The library root is the page with no action row at all.
        window.library.to_playlists();
        let (_, root) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            action_row(&root, &window).is_none(),
            "the library root has no action row"
        );
        assert_eq!(
            crate::view::pin::inset(&window.ctx),
            0.0,
            "a page with no bar inherited the last page's pinned height"
        );
    }

    /// An artist page with a discography long enough to push its chip row under
    /// the bar.
    fn long_artist_page(window: &mut Window) {
        let releases: Vec<Release> = (0..8)
            .map(|index| Release {
                uri: format!("spotify:release:{index}"),
                name: format!("Release {index}"),
                kind: if index % 2 == 0 {
                    ReleaseKind::Album
                } else {
                    ReleaseKind::Ep
                },
                year: Some(2024),
                cover: None,
                tracks: vec![format!("T{index}.1"), format!("T{index}.2")],
            })
            .collect();
        let _ = window.library.open(Page::Artist(ARTIST.to_owned()));
        window.library.apply(&Update::Artist(ArtistDetail {
            uri: ARTIST.to_owned(),
            name: "Vela Moreno".to_owned(),
            portrait: None,
            top_tracks: Vec::new(),
            releases,
            related: Vec::new(),
        }));
    }

    #[test]
    fn an_artist_page_leaves_the_chip_rows_reach_in_the_seam_and_the_panel_forgets_it() {
        let mut transport = playing(cover("abc"));
        let mut queue = queue_of(8, &mut transport);
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        long_artist_page(&mut window);
        // The panel clears the seam for its own rows after the page draws, so it
        // is hidden while the page's own reach is read.
        window.pane = layout::PanePref::Hidden;

        let wheel = egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -space::S4),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        };
        let at = view_rect(&window).center();
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![egui::Event::PointerMoved(at)],
        );
        let mut output = window.frame(&mut transport, &mut queue, Vec::new()).1;
        for _ in 0..200 {
            if crate::view::pin::inset(&window.ctx) > crate::theme::actionbar::STUCK_H + 0.5 {
                break;
            }
            output = window
                .frame(&mut transport, &mut queue, vec![wheel.clone()])
                .1;
        }
        action_row(&output, &window).expect("the action row pins");
        let reach = crate::view::pin::inset(&window.ctx);
        assert!(
            reach > crate::theme::actionbar::STUCK_H + 0.5,
            "the chip row reaches further than the bar: {reach}"
        );
        assert!(
            reach < view_rect(&window).height(),
            "the chip row is under the bar, not the whole view: {reach}"
        );

        // The panel's rows are not under the page's chip row. A focused queue
        // row asks to be seen, and the panel clears the seam before it draws.
        window.pane = layout::PanePref::Auto;
        let pane = pane_rect_when_shown(&window);
        clicked_row(&mut window, &mut queue, &mut transport, pane, "t2");
        let focused = window
            .ctx
            .memory(|memory| memory.focused())
            .expect("the queue row took the keyboard");
        let (_, panel) = window.frame(&mut transport, &mut queue, Vec::new());
        // The page still held its chip row on the frame the panel drew, so the
        // seam the panel cleared is one the page raised this frame and not a
        // stale value from an earlier one.
        let bar = action_row(&panel, &window).expect("the action row is still pinned");
        let band = chip_band(&panel, &window).expect("the chip row is still pinned");
        assert!(
            (band.top() - bar.bottom()).abs() < 0.5,
            "the page did not raise its chip row on the frame the panel cleared: {band:?} against {bar:?}"
        );
        let row = window
            .ctx
            .read_response(focused)
            .expect("the focused queue row is still drawn")
            .rect;
        assert!(
            pane.contains_rect(row),
            "the focused queue row left the panel: {row:?}"
        );
        assert_eq!(
            crate::view::pin::inset(&window.ctx),
            0.0,
            "the panel's queue rows were revealed clear of the artist's chip row"
        );
    }

    // ---- the devices card and the holder's transport ----

    fn device(id: &str, name: &str, kind: DeviceKind) -> Device {
        Device {
            id: id.to_owned(),
            name: name.to_owned(),
            kind,
        }
    }

    /// The account's roster, with the phone holding the sound.
    fn roster() -> Update {
        Update::Devices(DeviceUpdate::Roster {
            devices: vec![
                device("a", "Phone", DeviceKind::Phone),
                device("b", "Laptop", DeviceKind::Computer),
            ],
            holder: Some("a".to_owned()),
            held_here: false,
        })
    }

    /// What the holder is playing, as the cluster reports it.
    fn playback(state: RemoteState, steerable: bool) -> Update {
        Update::Devices(DeviceUpdate::Playing(Some(RemotePlayback {
            track: track(9, None),
            state,
            position_ms: 3_000,
            duration_ms: Some(60_000),
            steerable,
        })))
    }

    /// A window whose watch answered with the phone playing.
    fn remote_window() -> Window {
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let now = window.now;
        assert_eq!(
            window.devices.watch_request(true),
            Some(Command::WatchDevices)
        );
        window.devices.apply(&roster(), now);
        window
            .devices
            .apply(&playback(RemoteState::Playing, true), now);
        window
    }

    /// A window that holds the sound itself, so the card's rows can move it.
    fn holding_window() -> Window {
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        let now = window.now;
        assert_eq!(
            window.devices.watch_request(true),
            Some(Command::WatchDevices)
        );
        window.devices.apply(
            &Update::Devices(DeviceUpdate::Roster {
                devices: vec![
                    device("a", "Phone", DeviceKind::Phone),
                    device("b", "Laptop", DeviceKind::Computer),
                ],
                holder: None,
                held_here: true,
            }),
            now,
        );
        window
    }

    /// The playbar's own rect and the form it was drawn in.
    fn playbar_parts(window: &Window) -> (Rect, Playbar) {
        let content = Rect::from_min_size(egui::Pos2::ZERO, window.size);
        let regions = layout::regions_with(content.width(), window.pane);
        let shell = layout::shell_for(&regions);
        let bar = layout::grid_for(content, &shell)
            .playbar
            .shrink(stroke::HAIRLINE);
        (bar, regions.playbar)
    }

    fn devices_button_at(window: &Window) -> Rect {
        let (bar, form) = playbar_parts(window);
        playbar::devices_button(bar, form).expect("the full extras draw the devices button")
    }

    /// One transport button, solved the way the bar solves it: play is centred
    /// and the runs walk out from it.
    fn transport_button_at(window: &Window, control: crate::availability::Control) -> Rect {
        use crate::availability::Control;
        let (bar, _) = playbar_parts(window);
        let stack = extent::BUTTON_LG + space::S1 + extent::SLIDER_H;
        let top = bar.center().y - stack / 2.0;
        let down_w = extent::BUTTON + space::S1 + extent::BUTTON;
        let row = layout::playbar::transport_row(bar.center().x, down_w);
        let square =
            |x: f32, y: f32| Rect::from_min_size(egui::pos2(x, y), Vec2::splat(extent::BUTTON));
        match control {
            Control::PlayPause => {
                Rect::from_min_size(egui::pos2(row.play_x, top), Vec2::splat(extent::BUTTON_LG))
            }
            Control::Previous => square(
                row.down_x + extent::BUTTON + space::S1,
                top + extent::BUTTON / 2.0,
            ),
            Control::Next => square(row.up_x, top + extent::BUTTON / 2.0),
            other => panic!("{other:?} is not a transport button"),
        }
    }

    /// An armed local queue: rows with nothing current, which a space would
    /// start, and the index a remote press must not move.
    fn armed_queue() -> Queue {
        let mut queue = Queue::default();
        let rows = vec![track(0, None), track(1, None)];
        assert!(queue.append(rows).is_some());
        queue
    }

    #[test]
    fn the_devices_button_opens_the_card() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = remote_window();

        let command = window.click(
            &mut transport,
            &mut queue,
            devices_button_at(&window).center(),
        );

        assert!(window.list.is_open(), "the press did not open the card");
        assert_eq!(
            command, None,
            "the open asked for the refresh itself instead of leaving it to `logic`"
        );
    }

    #[test]
    fn a_second_press_of_the_button_closes_the_card() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = remote_window();
        let button = devices_button_at(&window).center();

        window.click(&mut transport, &mut queue, button);
        assert!(window.list.is_open());

        let command = window.click(&mut transport, &mut queue, button);
        assert!(!window.list.is_open(), "a second press left the card open");
        assert_eq!(command, None, "a close asks for nothing");
    }

    #[test]
    fn escape_closes_the_card_and_hands_the_keyboard_back_to_the_button() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = remote_window();
        window.click(
            &mut transport,
            &mut queue,
            devices_button_at(&window).center(),
        );
        assert!(window.list.is_open());

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Escape),
            None
        );
        assert!(!window.list.is_open(), "Escape left the card open");

        // The button holds the keyboard again, so Enter presses it.
        let _ = press(&mut window, &mut transport, &mut queue, egui::Key::Enter);
        assert!(
            window.list.is_open(),
            "the keyboard went somewhere other than the devices button"
        );
    }

    #[test]
    fn a_taken_command_slot_leaves_the_refresh_want_for_logic() {
        use crate::view::test_support::click_events;
        let mut transport = Transport {
            state: TransportState::Playing,
            ..Transport::default()
        };
        let mut queue = Queue::default();
        assert!(
            queue
                .start(&[track(0, None)], 0, None, &mut transport)
                .is_some()
        );
        transport.state = TransportState::Playing;
        let mut window = remote_window();
        let at = devices_button_at(&window).center();

        // The release that presses the button shares its frame with a space,
        // which takes the frame's one command first.
        let [moved, down, up] = click_events(at);
        let _ = window.frame(&mut transport, &mut queue, Vec::new());
        let _ = window.frame(&mut transport, &mut queue, vec![moved]);
        let _ = window.frame(&mut transport, &mut queue, vec![down]);
        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![up, key(egui::Key::Space, true)],
        );

        assert!(window.list.is_open(), "the press did not open the card");
        assert!(
            matches!(command, Some(Command::Pause | Command::Resume)),
            "the space did not take the frame's command: {command:?}"
        );
        let (next, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![key(egui::Key::Space, false)],
        );
        assert_eq!(next, None, "the shell sent the refresh itself");
        assert_eq!(
            window.devices.refresh_request(),
            Some(Command::RefreshDevices),
            "a taken slot dropped the refresh instead of leaving it for `logic`"
        );
    }

    #[test]
    fn a_pick_on_the_card_asks_for_the_sound_to_move() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = holding_window();

        let _ = window.click(
            &mut transport,
            &mut queue,
            devices_button_at(&window).center(),
        );
        assert!(window.list.is_open(), "the press did not open the card");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let at = painted_at(&frame, "Laptop").expect("the card names the other device");

        let command = window.click(&mut transport, &mut queue, at);

        assert_eq!(
            command,
            Some(Command::Transfer { to: "b".to_owned() }),
            "the pick did not reach the bridge"
        );
        assert!(!window.list.is_open(), "a pick left the card up");
    }

    #[test]
    fn a_pause_press_while_remote_steers_the_holder_and_never_the_queue() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        assert!(
            queue
                .start(&[track(0, None)], 0, None, &mut transport)
                .is_some()
        );
        transport.state = TransportState::Idle;
        let mut window = remote_window();

        let command = window.click(
            &mut transport,
            &mut queue,
            transport_button_at(&window, crate::availability::Control::PlayPause).center(),
        );

        assert_eq!(
            command,
            Some(Command::Steer {
                device: "a".to_owned(),
                steer: Steer::Pause,
            }),
            "a remote pause was not the holder's"
        );
        assert_eq!(
            queue.current().map(|entry| entry.uri.as_str()),
            Some("t0"),
            "the remote press moved the local queue"
        );
    }

    #[test]
    fn a_remote_skip_leaves_the_local_queue_where_it_was() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let rows = vec![track(0, None), track(1, None)];
        assert!(queue.start(&rows, 1, None, &mut transport).is_some());
        transport.state = TransportState::Idle;
        let mut window = remote_window();

        let command = window.click(
            &mut transport,
            &mut queue,
            transport_button_at(&window, crate::availability::Control::Next).center(),
        );

        assert_eq!(
            command,
            Some(Command::Steer {
                device: "a".to_owned(),
                steer: Steer::SkipNext,
            }),
            "a remote skip did not steer the holder"
        );
        assert_eq!(
            queue.current().map(|entry| entry.uri.as_str()),
            Some("t1"),
            "the remote skip moved the local queue"
        );
    }

    #[test]
    fn the_bar_draws_the_holders_caption_and_title() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = remote_window();

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let (bar, _) = playbar_parts(&window);
        assert!(
            text_in(&frame, "Phone", bar).is_some(),
            "the bar does not name the device holding the sound"
        );
        assert!(
            text_in(&frame, "Nothing playing", bar).is_none(),
            "the bar claims nothing plays while the holder's track is unnamed"
        );

        let mut named = track(9, None);
        named.title = "A Title".to_owned();
        named.named = true;
        window
            .devices
            .apply(&Update::Devices(DeviceUpdate::Labelled(named)), window.now);
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        assert!(
            text_in(&frame, "A Title", bar).is_some(),
            "the label that arrived did not reach the bar"
        );
        assert!(
            text_in(&frame, "Phone", bar).is_some(),
            "the caption went missing once the track was named"
        );
    }

    #[test]
    fn a_local_play_flips_the_bar_back_to_this_window() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        assert!(
            queue
                .start(&[track(0, None)], 0, None, &mut transport)
                .is_some()
        );
        let mut window = remote_window();

        transport.state = TransportState::Playing;
        transport.now_playing = Some(NowPlaying {
            uri: "t0".to_owned(),
            title: "Local Track".to_owned(),
            artists: String::new(),
            ..Default::default()
        });
        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let (bar, _) = playbar_parts(&window);
        assert!(
            text_in(&frame, "Phone", bar).is_none(),
            "the bar kept the holder after this window started playing"
        );
        assert!(
            text_in(&frame, "Local Track", bar).is_some(),
            "the bar did not draw this window's own track"
        );

        let command = window.click(
            &mut transport,
            &mut queue,
            transport_button_at(&window, crate::availability::Control::PlayPause).center(),
        );
        assert_eq!(
            command,
            Some(Command::Pause),
            "a local play/pause press stopped going through the queue"
        );
    }

    #[test]
    fn space_while_remote_toggles_the_holder_and_leaves_an_armed_queue_alone() {
        let mut transport = Transport::default();
        let mut queue = armed_queue();
        let mut window = remote_window();

        let command = press(&mut window, &mut transport, &mut queue, egui::Key::Space);

        assert_eq!(
            command,
            Some(Command::Steer {
                device: "a".to_owned(),
                steer: Steer::Pause,
            }),
            "a space while remote did not toggle the holder"
        );
        assert!(queue.armed(), "the remote space started the local queue");
        assert_eq!(queue.current(), None, "the remote space moved the queue");
    }

    #[test]
    fn the_lyrics_tab_asks_for_the_holders_track_only_once() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = remote_window();
        let mut named = track(9, None);
        named.title = "A Title".to_owned();
        window
            .devices
            .apply(&Update::Devices(DeviceUpdate::Labelled(named)), window.now);
        window.library.lyrics.select(crate::lyrics::Tab::Lyrics);

        let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            command,
            Some(Command::LoadLyrics {
                track: "t9".to_owned()
            }),
            "the open tab asked for a track that is not on screen"
        );
        assert!(
            matches!(window.library.lyrics.load(), Load::Loading),
            "the load did not start for the holder's track"
        );

        // The backdrop's track never changes, so nothing is asked for again: a
        // request reading the silent local transport would starve or reset it.
        for _ in 0..3 {
            let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());
            assert_eq!(command, None, "the same lyrics were asked for again");
            assert!(
                matches!(window.library.lyrics.load(), Load::Loading),
                "the load was reset under the reader"
            );
        }
    }

    #[test]
    fn a_local_play_mid_drag_takes_the_holders_preview_with_its_bar() {
        use crate::view::test_support::drag_events;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = remote_window();
        playlist_page(&mut window, &["p0", "p1"]);
        let _ = click_page_row(&mut window, &mut transport, &mut queue, "p0");

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let (bar, form) = playbar_parts(&window);
        let centre = layout::playbar_for(bar, form).columns(bar)[1];
        let track = painted_seek_track(&frame, centre);
        let from = egui::Pos2::new(track.left() + track.width() * 0.2, track.center().y);
        let to = egui::Pos2::new(track.left() + track.width() * 0.8, track.center().y);

        let mut held = drag_events(from, to);
        held.pop();
        for event in held {
            let _ = window.frame(&mut transport, &mut queue, vec![event]);
        }
        assert!(
            window.devices.display().dragging,
            "the drag left no preview on the holder's bar"
        );

        // Enter on the focused page row starts a local track, so this frame's
        // bar is this window's own and the release reaches the holder's slider
        // never.
        let release = window.release(to);
        let _ = window.frame(
            &mut transport,
            &mut queue,
            vec![release, key(egui::Key::Enter, true)],
        );
        assert_eq!(
            window.devices.display().scrub,
            None,
            "the holder's bar is gone, so its preview must not outlive it"
        );
        assert!(!window.devices.display().dragging);
    }

    #[test]
    fn now_playings_more_is_inert_while_the_holder_plays_a_track_this_queue_lacks() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        // This window's own row has verbs, and the holder's push is another
        // track: the button must not reach the row the pane is not drawing.
        queue.start(&[menu_track(0)], 0, None, &mut transport);
        transport.state = TransportState::Idle;
        let mut window = remote_window();

        let at = now_playing_more(&window);
        let _ = window.click(&mut transport, &mut queue, at);
        settle_menu(&mut window, &mut transport, &mut queue);

        assert!(
            !window.menu.is_open(),
            "the more button acted on the local row while the holder played"
        );
    }

    #[test]
    fn now_playings_more_acts_on_the_holders_track_while_the_queue_holds_it() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut held = track(9, None);
        held.album_uri = Some(ALBUM.to_owned());
        held.artist_uris = vec!["spotify:artist:b".to_owned()];
        held.artist_names = vec!["A Band".to_owned()];
        queue.start(&[track(0, None), held], 0, None, &mut transport);
        transport.state = TransportState::Idle;
        let mut window = remote_window();

        let at = now_playing_more(&window);
        let _ = window.click(&mut transport, &mut queue, at);
        settle_menu(&mut window, &mut transport, &mut queue);

        assert!(window.menu.is_open(), "the more button opened no menu");
        assert_eq!(
            window
                .menu
                .targets()
                .iter()
                .map(|entry| entry.uri.as_str())
                .collect::<Vec<_>>(),
            ["t9"],
            "the menu is about a track the pane is not drawing"
        );
        assert!(window.menu.cut().is_none(), "the entry was removable");
    }

    #[test]
    fn now_playings_more_acts_on_this_windows_row_while_it_plays() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        queue.start(&[menu_track(0)], 0, None, &mut transport);
        // A holder is still there behind the local play, so the source rule is
        // what keeps this frame on the local row.
        let mut window = remote_window();

        let at = now_playing_more(&window);
        let _ = window.click(&mut transport, &mut queue, at);
        settle_menu(&mut window, &mut transport, &mut queue);

        assert!(window.menu.is_open(), "the more button opened no menu");
        assert_eq!(
            window
                .menu
                .targets()
                .iter()
                .map(|entry| entry.uri.as_str())
                .collect::<Vec<_>>(),
            ["spotify:track:0"],
            "the menu is about the holder's track while this window plays"
        );
    }

    #[test]
    fn a_dropped_steer_clears_the_preview_the_drag_left() {
        use crate::view::test_support::drag_events;
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = remote_window();
        // A Back settles a stale playlist in this frame, taking its one command
        // before the playbar draws, so the seek the release asks for is dropped.
        stale_playlist_behind_an_album(&mut window.library);

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let (bar, form) = playbar_parts(&window);
        let centre = layout::playbar_for(bar, form).columns(bar)[1];
        let track = painted_seek_track(&frame, centre);
        let from = egui::Pos2::new(track.left() + track.width() * 0.2, track.center().y);
        let to = egui::Pos2::new(track.left() + track.width() * 0.8, track.center().y);

        // The drag stops with the button still down, so the bar holds a preview.
        let mut held = drag_events(from, to);
        held.pop();
        for event in held {
            let _ = window.frame(&mut transport, &mut queue, vec![event]);
        }
        assert!(
            window.devices.display().scrub.is_some(),
            "the drag left no preview to lose"
        );

        let release = window.release(to);
        let (command, _) = window.frame(
            &mut transport,
            &mut queue,
            vec![release, key(egui::Key::BrowserBack, true)],
        );
        assert!(
            matches!(command, Some(Command::LoadTracks { .. })),
            "the nav did not take the frame's command: {command:?}"
        );
        assert_eq!(
            window.devices.display().scrub,
            None,
            "a dropped seek left its preview standing until a push never comes"
        );
    }

    #[test]
    fn space_emits_nothing_while_the_holder_cannot_be_steered() {
        let mut transport = Transport::default();
        let mut queue = armed_queue();
        let mut window = remote_window();
        let now = window.now;
        window
            .devices
            .apply(&playback(RemoteState::Playing, false), now);

        assert_eq!(
            press(&mut window, &mut transport, &mut queue, egui::Key::Space),
            None,
            "a space steered a holder that forbids it"
        );
        assert!(queue.armed(), "a refused space started the local queue");
    }

    /// A library root holding one named playlist, with its rows already loaded.
    fn shelf_with_rows(window: &mut Window, rows: usize) {
        window.library.apply(&Update::Playlists(vec![PlaylistEntry {
            composed: Vec::new(),
            uri: PLAYLIST.to_owned(),
            name: "Mix".to_owned(),
            named: true,
            cover: None,
            description: None,
            owner: None,
            tracks: None,
            access: None,
        }]));
        window.library.select(PLAYLIST);
        window.library.apply(&Update::Tracks {
            playlist: PLAYLIST.to_owned(),
            tracks: (0..rows).map(|index| track(index, None)).collect(),
            cover: None,
            access: None,
        });
        window.library.to_playlists();
    }

    #[test]
    fn landed_shelf_rows_start_the_queue_at_the_first_track_with_the_object_as_origin() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        shelf_with_rows(&mut window, 2);
        assert!(window.library.want_play(PLAYLIST, SidebarKind::Playlists));
        assert!(window.library.has_landed_play());

        let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());

        assert_eq!(
            command,
            Some(Command::Play {
                uri: "t0".to_owned()
            })
        );
        assert_eq!(queue.origin(), Some(PLAYLIST), "the queue named no object");
        assert_eq!(queue.current().map(|entry| entry.uri.as_str()), Some("t0"));
        assert_eq!(
            transport.now_playing.as_ref().map(|now| now.uri.as_str()),
            Some("t0"),
            "the start never reached the transport"
        );
        assert!(
            !window.library.has_landed_play(),
            "the landed rows were kept after a start"
        );
    }

    #[test]
    fn a_frame_that_took_a_command_keeps_the_landed_rows_for_the_next() {
        let mut transport = playing(cover("abc"));
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        window.library.lyrics.select(crate::lyrics::Tab::Lyrics);
        window.library.apply(&Update::Playlists(Vec::new()));
        assert!(window.library.want_play(PLAYLIST, SidebarKind::Playlists));
        window.library.apply(&Update::Tracks {
            playlist: PLAYLIST.to_owned(),
            tracks: vec![track(0, None), track(1, None)],
            cover: None,
            access: None,
        });

        let (command, output) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            command,
            Some(Command::LoadLyrics {
                track: "spotify:track:x".to_owned()
            }),
            "the frame was expected to take the lyrics read"
        );
        assert!(
            window.library.has_landed_play(),
            "a taken frame dropped the landed rows"
        );
        let delay = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|viewport| viewport.repaint_delay);
        assert_eq!(
            delay,
            Some(std::time::Duration::ZERO),
            "a held start asked for no frame of its own"
        );
        assert!(
            queue.current().is_none(),
            "a taken frame started the play anyway"
        );

        let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            command,
            Some(Command::Play {
                uri: "t0".to_owned()
            }),
            "the held play never got its frame"
        );
        assert_eq!(queue.origin(), Some(PLAYLIST));
    }

    #[test]
    fn a_navigation_in_the_frame_the_rows_land_starts_nothing() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        shelf_with_rows(&mut window, 1);
        let _ = window.library.select(PLAYLIST);
        window.library.apply(&Update::Tracks {
            playlist: PLAYLIST.to_owned(),
            tracks: vec![track(0, None)],
            cover: None,
            access: None,
        });

        let (_, frame) = window.frame(&mut transport, &mut queue, Vec::new());
        let crumb = text_in(&frame, "PLAYLISTS", topbar_rect(&window))
            .expect("the trail's Playlists crumb is painted");
        let [arrive, press, release] = crate::view::test_support::click_events(crumb.center());
        let _ = window.frame(&mut transport, &mut queue, vec![arrive, press]);

        assert!(window.library.want_play(PLAYLIST, SidebarKind::Playlists));
        window.library.apply(&Update::Tracks {
            playlist: PLAYLIST.to_owned(),
            tracks: vec![track(1, None)],
            cover: None,
            access: None,
        });
        assert!(window.library.has_landed_play());

        let _ = window.frame(&mut transport, &mut queue, vec![release]);
        assert!(
            queue.current().is_none(),
            "a navigation frame started the play"
        );
        assert_eq!(
            window.library.page(),
            None,
            "the press was expected to go home"
        );

        let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(command, None);
        assert!(
            queue.current().is_none(),
            "a play outlived the page it was pressed on"
        );
    }

    #[test]
    fn a_carry_holds_a_landed_shelf_play_until_it_ends() {
        let mut transport = Transport::default();
        let mut queue = Queue::default();
        let mut window = Window::new(Vec2::new(1400.0, 900.0));
        shelf_with_rows(&mut window, 1);
        assert!(window.library.want_play(PLAYLIST, SidebarKind::Playlists));

        egui::DragAndDrop::set_payload(&window.ctx, ());
        let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());
        egui::DragAndDrop::clear_payload(&window.ctx);

        assert_eq!(command, None, "a carried block started the play");
        assert!(
            window.library.has_landed_play(),
            "a carried frame dropped the landed rows"
        );

        let (command, _) = window.frame(&mut transport, &mut queue, Vec::new());
        assert_eq!(
            command,
            Some(Command::Play {
                uri: "t0".to_owned()
            }),
            "the play was not started once the carry ended"
        );
    }
}
