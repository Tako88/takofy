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
        screen.now_ms,
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
                screen.local_hour,
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

    // The panel and the playbar draw the track the source names. Each looks up
    // its own size: the panel inside its own stack, the bar here.
    let playing = match source {
        DeviceSource::Local => screen.transport.now_playing.as_ref(),
        DeviceSource::Remote => screen.devices.display().now_playing.as_ref(),
    };
    let playbar_cover = screen.art.look(
        playing.and_then(|now| now.cover.as_ref()),
        want_px(crate::theme::extent::PLAYBAR_ART, ui.pixels_per_point()),
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
            playbar_cover,
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
    // width and never the layout's guess.
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
                    // The loaded row's glyph is pause, and its action is the
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
                        // The queue becomes the whole answer, entered at the
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
        command = screen
            .queue
            .start(&start.tracks, 0, start.origin.as_deref(), screen.transport);
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
    now_ms: u64,
) -> Option<Command> {
    if command.is_some() {
        return command;
    }
    match nav {
        None => None,
        Some(topbar::Nav::Back) => library.back(),
        Some(topbar::Nav::Forward) => library.forward(),
        // Home is a page of its own: opening it draws a new shelf, and Home on
        // Home keeps the one already on screen.
        Some(topbar::Nav::Home) => {
            library.go_home(now_ms);
            None
        }
        // The root is local state and never needs a fetch, so it adds no command
        // of its own.
        Some(topbar::Nav::Library) => {
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
mod tests;
