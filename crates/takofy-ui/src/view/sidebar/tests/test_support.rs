use super::super::*;

use egui::{Color32, Event};

use crate::message::Update;
use crate::theme::{fonts, style};
use crate::view::test_support::click_events;

/// One show, the way the followed list carries it.
pub(super) fn followed_show(uri: &str, name: &str) -> FollowedShow {
    FollowedShow {
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        publisher: None,
        cover: None,
        episodes: None,
    }
}

pub(super) const PLAYLIST_IDS: Ids = Ids {
    scope: "library-list",
    kind: "library-row",
    shelf: SidebarKind::Playlists,
    lifts: true,
};

pub(super) fn followed_artist(uri: &str, name: &str) -> FollowedArtist {
    FollowedArtist {
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        portrait: None,
        releases: None,
    }
}

pub(super) fn saved_album(uri: &str, name: &str, artists: &str) -> SavedAlbum {
    SavedAlbum {
        saved_at: None,
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        artists: artists.to_owned(),
        artist_uris: Vec::new(),
        artist_names: Vec::new(),
        year: None,
        cover: None,
        tracks: None,
    }
}

/// The context the sidebar draws into, the way the window builds it.
pub(super) fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts::definitions());
    ctx.set_global_style(style(&Tokens::DARK));
    ctx
}

/// One frame of the full sidebar over the characterised band: `events` go
/// into the pass, and the frame's output and command come back.
pub(super) fn run(
    ctx: &egui::Context,
    events: Vec<Event>,
    library: &mut Library,
    art: &mut Art,
) -> (egui::FullOutput, Option<Command>) {
    run_gated(ctx, events, library, art, true)
}

/// The same frame with the drawer's own room for a command: `free` false is
/// what the shell passes when an earlier region has already taken the frame.
pub(super) fn run_gated(
    ctx: &egui::Context,
    events: Vec<Event>,
    library: &mut Library,
    art: &mut Art,
    free: bool,
) -> (egui::FullOutput, Option<Command>) {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
        events,
        ..Default::default()
    };
    let tokens = Tokens::DARK;
    let mut command = None;
    let mut output = ctx.run_ui(input, |ui| {
        crate::view::focus::begin_frame(ui.ctx());
        if let Some(next) = show(ui, library, art, Instant::now(), &tokens, free).command {
            command = Some(next);
        }
    });
    output.textures_delta.clear();
    (output, command)
}

/// The three passes of one click at `pos`, one pass each; the last command
/// any of them returned.
pub(super) fn click(
    ctx: &egui::Context,
    pos: Pos2,
    library: &mut Library,
    art: &mut Art,
) -> Option<Command> {
    let mut command = None;
    for event in click_events(pos) {
        let (_, next) = run(ctx, vec![event], library, art);
        if next.is_some() {
            command = next;
        }
    }
    command
}

/// The playlist list's one row, ready to draw.
pub(super) fn playlist(uri: &str, name: &str) -> PlaylistEntry {
    PlaylistEntry {
        saved_at: None,
        composed: Vec::new(),
        uri: uri.to_owned(),
        name: name.to_owned(),
        named: true,
        cover: None,
        description: None,
        owner: None,
        tracks: None,
        access: None,
    }
}

/// Where a label was painted, and how big it was.
pub(super) fn painted_text(output: &egui::FullOutput, label: &str) -> (Pos2, Vec2) {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some((text.pos, text.galley.size()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{label:?} is drawn"))
}

/// The rail's box on the frame's screen.
pub(super) fn rail() -> Rect {
    Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))
}

/// One frame of the column form: its rows may lift and offer themselves.
pub(super) fn run_column(
    ctx: &egui::Context,
    events: Vec<Event>,
    library: &mut Library,
    art: &mut Art,
) -> (egui::FullOutput, Option<Command>) {
    run_with(ctx, events, library, art, true)
}

/// One frame of either form, over the characterised band.
pub(super) fn run_with(
    ctx: &egui::Context,
    events: Vec<Event>,
    library: &mut Library,
    art: &mut Art,
    drag: bool,
) -> (egui::FullOutput, Option<Command>) {
    let frame = frame_of(ctx, events, library, art, drag);
    (frame.output, frame.command)
}

/// What one frame of the rail leaves behind.
pub(super) struct Frame {
    output: egui::FullOutput,
    command: Option<Command>,
}

/// One frame of either form, with the head's ids read out.
fn frame_of(
    ctx: &egui::Context,
    events: Vec<Event>,
    library: &mut Library,
    art: &mut Art,
    drag: bool,
) -> Frame {
    frame_probing(ctx, events, library, art, drag, |_| ()).0
}

/// One frame of either form, read inside the pass so its AccessKit nodes are
/// still standing.
pub(super) fn frame_probing<T>(
    ctx: &egui::Context,
    events: Vec<Event>,
    library: &mut Library,
    art: &mut Art,
    drag: bool,
    probe: impl FnOnce(&egui::Ui) -> T,
) -> (Frame, T) {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(254.0, 800.0))),
        events,
        ..Default::default()
    };
    let tokens = Tokens::DARK;
    let mut command = None;
    let mut probed = None;
    let mut probe = Some(probe);
    let mut output = ctx.run_ui(input, |ui| {
        crate::view::focus::begin_frame(ui.ctx());
        let next = if drag {
            column(ui, library, art, Instant::now(), &tokens)
        } else {
            show(ui, library, art, Instant::now(), &tokens, true).command
        };
        if let Some(next) = next {
            command = Some(next);
        }
        if let Some(probe) = probe.take() {
            probed = Some(probe(ui));
        }
    });
    output.textures_delta.clear();
    (
        Frame { output, command },
        probed.expect("the probe runs once"),
    )
}

/// Drags from a row's line towards the middle of the list, stopping with the
/// button still down, and returns the last frame.
pub(super) fn hold_from(
    ctx: &egui::Context,
    from: Pos2,
    library: &mut Library,
    art: &mut Art,
    drag: bool,
) -> egui::FullOutput {
    let mut events = crate::view::test_support::drag_events(from, from + Vec2::new(0.0, 60.0));
    events.pop();
    let _ = run_with(ctx, Vec::new(), library, art, drag);
    let mut last = None;
    for event in events {
        let (output, _) = run_with(ctx, vec![event], library, art, drag);
        last = Some(output);
    }
    last.expect("a drag moves the pointer at least once")
}

/// A library whose sidebar holds the named playlists, movable at `byte`.
pub(super) fn movable_playlists(
    names: &[&str],
    byte: u8,
    editable: bool,
    tracks: Option<usize>,
) -> Library {
    let mut library = Library::default();
    let entries = names
        .iter()
        .map(|name| {
            let mut entry = playlist(&format!("spotify:playlist:{name}"), name);
            entry.tracks = tracks;
            entry.access = Some(takofy_model::PlaylistAccess {
                revision: takofy_model::Revision::from_wire(&[byte]).expect("a revision"),
                editable,
            });
            entry
        })
        .collect();
    library.apply(&Update::Playlists(entries));
    library.apply(&Update::RootlistAccess(takofy_model::PlaylistAccess {
        revision: takofy_model::Revision::from_wire(&[byte]).expect("a revision"),
        editable,
    }));
    library
}

pub(super) fn name_line(output: &egui::FullOutput, label: &str) -> Pos2 {
    let (pos, size) = painted_text(output, label);
    Pos2::new(pos.x + 10.0, pos.y + size.y / 2.0)
}

/// The top of the row painted as `label`, from the two lines it draws.
///
/// A row centres its name and kind in itself, so the name's own top plus
/// half the pair, less half a row, is the box's top.
pub(super) fn row_top(output: &egui::FullOutput, label: &str, kind: &str) -> f32 {
    let (name, name_size) = painted_text(output, label);
    let (_, kind_size) = painted_text(output, kind);
    name.y + (name_size.y + kind_size.y) / 2.0 - ROW_H / 2.0
}

/// Whether a line of text was painted at all.
pub(super) fn painted(output: &egui::FullOutput, label: &str) -> bool {
    output.shapes.iter().any(|clipped| {
        matches!(&clipped.shape,
            egui::epaint::Shape::Text(text) if text.galley.text() == label)
    })
}

/// A line painted as `text`, if the frame painted it.
pub(super) fn drawn_text(output: &egui::FullOutput) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

/// A library holding one row of each kind, with an editable rootlist.
pub(super) fn four_kinds() -> Library {
    let mut library = movable_playlists(&["A"], 7, true, None);
    library.apply(&Update::FollowedShows(vec![followed_show(
        "spotify:show:s",
        "A show",
    )]));
    library.apply(&Update::FollowedArtists(vec![followed_artist(
        "spotify:artist:a",
        "An artist",
    )]));
    library.apply(&Update::SavedAlbums(vec![saved_album(
        "spotify:album:r",
        "An album",
        "Vela",
    )]));
    library
}

/// The hairlines a frame painted that could be the rail filter's underline:
/// the field's own width, an ink that means focused or not.
pub(super) fn filter_hairlines(output: &egui::FullOutput) -> Vec<(Rect, Color32)> {
    let tokens = Tokens::DARK;
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(shape)
                if (shape.rect.height() - stroke::HAIRLINE).abs() < 0.01
                    && shape.rect.width() > 80.0
                    && shape.rect.width() < 200.0
                    && (shape.fill == tokens.palette.border
                        || shape.fill == tokens.palette.text) =>
            {
                Some((shape.rect, shape.fill))
            }
            _ => None,
        })
        .collect()
}

/// The rail filter's box, read from the hairline under it.
pub(super) fn filter_field_of(output: &egui::FullOutput) -> Rect {
    let (hairline, _) = filter_hairlines(output)
        .into_iter()
        .next()
        .expect("the rail filter paints its bottom hairline");
    Rect::from_min_max(
        Pos2::new(hairline.left(), hairline.bottom() - extent::BUTTON),
        Pos2::new(hairline.right(), hairline.bottom()),
    )
}

/// A library whose playlists are the names, each ready and drawable.
pub(super) fn named_playlists(names: &[&str]) -> Library {
    let mut library = Library::default();
    library.apply(&Update::Playlists(
        names
            .iter()
            .map(|name| playlist(&format!("spotify:playlist:{name}"), name))
            .collect(),
    ));
    library
}

/// The names a frame painted, in the rows' own order.
pub(super) fn painted_rows(output: &egui::FullOutput, names: &[&str]) -> Vec<String> {
    let mut drawn: Vec<(f32, String)> = names
        .iter()
        .filter_map(|name| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::epaint::Shape::Text(text) if text.galley.text() == *name => {
                        Some((text.pos.y, (*name).to_owned()))
                    }
                    _ => None,
                })
        })
        .collect();
    drawn.sort_by(|left, right| left.0.total_cmp(&right.0));
    drawn.into_iter().map(|(_, name)| name).collect()
}
