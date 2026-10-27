use super::super::hero; // the band recipe, which the tests' own `hero` module shadows
use super::super::*; // the module under test
use super::*; // the slice's shared fixtures, from tests/mod.rs

#[test]
fn the_frame_loop_requests_only_visible_covers() {
    let ctx = context();
    let tokens = Tokens::DARK;
    // A thousand distinct covers in a short window: only the dozen rows the
    // viewport can show are ever laid out, even as the list is scrolled.
    let tracks: Vec<TrackEntry> = (0..1_000).map(track_with_cover).collect();
    let mut library = library_with(tracks);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let now = Instant::now();
    let mut requested: std::collections::HashMap<CoverUrl, usize> =
        std::collections::HashMap::new();
    let mut painted: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let centre = Pos2::new(PROBE.x / 2.0, PROBE.y / 2.0);

    // Scroll from the top down. Every requested cover must belong to a row
    // the window painted in some frame, and the outstanding set must never
    // exceed the cap.
    for step in 0..240 {
        art.begin_frame();
        let mut events = vec![Event::PointerMoved(centre)];
        if step > 0 {
            events.push(Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: vec2(0.0, -extent::ROW_H),
                phase: TouchPhase::Move,
                source: MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            });
        }
        let mut output = ctx.run_ui(input(events), |ui| {
            let _ = tracks_view(
                ui,
                Table::Full,
                &mut library,
                &mut transport,
                &mut queue,
                &mut art,
                now,
                NOW_MS,
                &tokens,
            );
        });
        output.textures_delta.clear();
        painted.extend(painted_rows(&output));

        let commands = art.requests(now);
        assert!(
            art.outstanding() <= crate::cover::MAX_IN_FLIGHT,
            "{} fetches outstanding",
            art.outstanding()
        );
        for command in commands {
            if let Command::Fetch { url } = command {
                requested.insert(url.clone(), cover_row(&url));
                // Answer it, so the next frame can ask for the next row.
                art.apply(
                    &Update::Cover {
                        url,
                        image: image(),
                    },
                    now,
                );
            }
        }
    }

    assert!(!requested.is_empty(), "the visible rows must be fetched");
    assert!(
        painted.iter().copied().max().unwrap_or(0) > 50,
        "the frame loop must scroll through the list to exercise the cap"
    );
    // The loop stops one row short of painting the row that enters at the
    // fold, so a request may name it; nothing further off may be asked for.
    let furthest = painted.iter().copied().max().unwrap_or(0);
    for (url, row) in &requested {
        assert!(
            painted.contains(row) || *row <= furthest + 1,
            "{url:?} was requested for row {row}, which no frame ever painted"
        );
    }
}

#[test]
fn a_grid_card_switches_to_the_context_image_when_it_arrives() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let first = cover_url("first");
    let context_cover = cover_url("context");
    let mut library = library_idle(vec![playlist_with_cover("a", "A", first.clone())]);
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let now = Instant::now();

    art.begin_frame();
    run(&ctx, input(Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
            now,
            NOW_MS,
            &tokens,
        );
    });
    assert_eq!(fetch_urls(art.requests(now)), vec![first.clone()]);

    // A context image for an unselected playlist still upgrades its entry,
    // because the upgrade is keyed by the update's own playlist.
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: Vec::new(),
        cover: Some(CoverSet::single(context_cover.clone())),
        access: None,
    });

    art.begin_frame();
    run(&ctx, input(Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
            now,
            NOW_MS,
            &tokens,
        );
    });
    assert_eq!(fetch_urls(art.requests(now)), vec![context_cover.clone()]);
}

#[test]
fn the_band_waits_for_the_context_image_and_then_shows_it() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let first = cover_url("first");
    let context_cover = cover_url("context");
    let mut library = Library::default();
    library.apply(&Update::Playlists(vec![playlist_with_cover(
        "a",
        "Evening",
        first.clone(),
    )]));
    library.select("a");
    let mut transport = Transport::default();
    let mut queue = Queue::default();
    let mut art = Art::default();
    let now = Instant::now();

    // While the tracks load, the band is drawn waiting and asks for nothing:
    // it must not fetch the first-track cover it is about to replace.
    art.begin_frame();
    let mut output = ctx.run_ui(input(Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
            now,
            NOW_MS,
            &tokens,
        );
    });
    output.textures_delta.clear();
    assert!(
        drawn_texts(&output).contains(&"PLAYLIST"),
        "the band must head a loading playlist"
    );
    assert!(art.requests(now).is_empty(), "no cover yet");

    // The first page lands with the context image: the entry is upgraded and
    // the band wants it on the same frame.
    library.apply(&Update::Tracks {
        playlist: "a".to_owned(),
        tracks: vec![track("t", "t")],
        cover: Some(CoverSet::single(context_cover.clone())),
        access: None,
    });
    art.begin_frame();
    run(&ctx, input(Vec::new()), |ui| {
        let _ = tracks_view(
            ui,
            Table::Full,
            &mut library,
            &mut transport,
            &mut queue,
            &mut art,
            now,
            NOW_MS,
            &tokens,
        );
    });
    assert_eq!(fetch_urls(art.requests(now)), vec![context_cover.clone()]);
}

#[test]
fn the_band_keeps_an_already_held_cover_while_the_tracks_reload() {
    let ctx = context();
    let cover = cover_url("held");
    let entry = playlist_with_cover("a", "Evening", cover.clone());
    let now = Instant::now();
    let mut art = Art::default();

    // An earlier visit to the playlist fetched and held its cover.
    art.begin_frame();
    assert_eq!(
        art.look(Some(&CoverSet::single(cover.clone())), 176, now),
        Cover::Loading
    );
    assert_eq!(fetch_urls(art.requests(now)), vec![cover.clone()]);
    art.apply(
        &Update::Cover {
            url: cover.clone(),
            image: CoverImage::new(1, 1, vec![255, 0, 0, 255]).expect("a 1x1 image"),
        },
        now,
    );
    art.upload(&ctx, crate::theme::TintParams::DARK.alpha_floor);
    assert!(matches!(
        art.look(Some(&CoverSet::single(cover.clone())), 176, now),
        Cover::Ready(_)
    ));

    // Re-selecting the playlist puts the tracks back into `Loading`. The
    // band still draws the picture it already holds rather than flashing the
    // placeholder, and does not fetch it again. Its tint is kept with it, so
    // the band does not flash grey either.
    art.begin_frame();
    let (held, tint) = band_cover_for(
        &entry,
        false,
        &mut art,
        176,
        &crate::theme::TintParams::DARK,
        now,
    );
    assert!(matches!(held, Cover::Ready(_)), "the band keeps its cover");
    assert_eq!(
        tint,
        Some(crate::theme::tint_color([116, 27, 27])),
        "a pending reload keeps its tint instead of flashing grey"
    );
    assert!(
        fetch_urls(art.requests(now)).is_empty(),
        "a held cover is not fetched again"
    );

    // A playlist whose cover is not held still waits without fetching it.
    let other = playlist_with_cover("b", "Other", cover_url("unheld"));
    art.begin_frame();
    let (held, tint) = band_cover_for(
        &other,
        false,
        &mut art,
        176,
        &crate::theme::TintParams::DARK,
        now,
    );
    assert_eq!(held, Cover::Loading);
    assert_eq!(tint, None, "nothing held means no colour yet");
    assert!(fetch_urls(art.requests(now)).is_empty());
}

const META: &str = "Made by alice \u{b7} 62 songs \u{b7} 3 hr 51 min";
/// Long enough that two rows of the band's column cannot hold it.
const LONG_DESCRIPTION: &str = "Windows down, city behind you. Updated every \
    Sunday, and again whenever the mood takes me, with no plan and no promise, \
    only the long way home through the sodium light of the ring road, past the \
    depot and the all-night garage, until the city runs out.";

/// A band whose description, owner and tracks are all known: the widest case
/// the tests below read.
fn library_full() -> Library {
    library_detailed(
        timed_tracks(62, 223_548),
        "Evening",
        Some(DESCRIPTION),
        Some("alice"),
    )
}

#[test]
fn the_band_draws_the_description_and_the_owner() {
    let ctx = context();
    let mut library = library_full();
    let output = tracklist_frame(&ctx, Table::Full, &mut library);
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&DESCRIPTION),
        "the band must draw the description: {texts:?}"
    );
    assert!(
        texts.contains(&META),
        "the band must draw the owner ahead of the count and the total: {texts:?}"
    );
}

#[test]
fn the_band_stacks_the_eyebrow_the_title_the_description_and_the_meta() {
    let ctx = context();
    let mut library = library_full();
    let output = tracklist_frame(&ctx, Table::Full, &mut library);
    let drawn = drawn_galleys(&output);
    let eyebrow = galley_top(&drawn, "PLAYLIST");
    let title = galley_top(&drawn, "Evening");
    let description = galley_top(&drawn, DESCRIPTION);
    let meta = galley_top(&drawn, META);
    assert!(
        eyebrow < title && title < description && description < meta,
        "the stack reads eyebrow, title, description, meta downwards: \
         {eyebrow} {title} {description} {meta}"
    );
}

#[test]
fn a_playlist_without_a_description_or_owner_draws_neither_and_leaves_no_gap() {
    let ctx = context();
    let mut library = library_detailed(timed_tracks(2, 1_000), "Evening", None, None);
    let output = tracklist_frame(&ctx, Table::Full, &mut library);
    let texts = drawn_texts(&output);
    assert!(
        !texts.iter().any(|text| text.starts_with("Made by")),
        "no owner means no byline: {texts:?}"
    );
    assert!(
        !texts.contains(&DESCRIPTION),
        "no description means no description line: {texts:?}"
    );

    // With no description the title and the meta are one stack gap apart,
    // not two: an absent description must not leave its own space behind.
    let drawn = drawn_galleys(&output);
    let title_bottom = galley_top(&drawn, "Evening") + galley_height(&drawn, "Evening");
    let meta = galley_top(&drawn, "2 songs \u{b7} 0 min");
    assert!(
        (meta - title_bottom - space::S2).abs() < 0.01,
        "the gap between the title and the meta is {} with no description, \
         not the stack's own {}",
        meta - title_bottom,
        space::S2
    );
}

#[test]
fn a_description_without_an_owner_or_tracks_draws_only_the_description() {
    let ctx = context();
    // The listing names a description but no owner, and the tracks are not
    // asked for: the band has one byline and no meta line at all.
    let mut library = library_listed("Evening", Some(DESCRIPTION), None);
    let output = tracklist_frame(&ctx, Table::Full, &mut library);
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&DESCRIPTION),
        "the description draws with no owner and no tracks: {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text.starts_with("Made by")),
        "no owner means no byline: {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text.contains("songs")),
        "no ready tracks means no count: {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text.contains('\u{b7}')),
        "the meta line is absent, not drawn empty: {texts:?}"
    );
    assert_eq!(
        texts.len(),
        3,
        "the band paints the eyebrow, the title and the description, and
         nothing where the meta would be: {texts:?}"
    );
}

#[test]
fn the_band_names_the_owner_before_the_tracks_are_ready() {
    let ctx = context();
    let mut library = library_listed("Evening", None, Some("alice"));
    let output = tracklist_frame(&ctx, Table::Full, &mut library);
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"Made by alice"),
        "the owner must show while the tracks load: {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text.contains("songs")),
        "the count waits for the tracks: {texts:?}"
    );
}

#[test]
fn the_band_says_made_by_you_for_our_own_playlist() {
    let ctx = context();
    let mut library = library_ours(timed_tracks(62, 223_548), "Evening");
    let output = tracklist_frame(&ctx, Table::Full, &mut library);
    let texts = drawn_texts(&output);
    assert!(
        texts.contains(&"Made by you \u{b7} 62 songs \u{b7} 3 hr 51 min"),
        "our own list says Made by you: {texts:?}"
    );
}

#[test]
fn a_numeric_owner_draws_no_made_by_line() {
    let ctx = context();
    let mut library = library_with(timed_tracks(2, 1_000));
    let mut entry = listed("Evening", None, None);
    entry.owner = OwnerName::from_wire("31415926535").map(Owner::Named);
    assert_eq!(entry.owner, None, "a numeric id is not an owner");
    library.apply(&Update::Playlists(vec![entry]));
    let output = tracklist_frame(&ctx, Table::Full, &mut library);
    let texts = drawn_texts(&output);
    assert!(
        !texts.iter().any(|text| text.starts_with("Made by")),
        "a numeric owner names nobody: {texts:?}"
    );
}

#[test]
fn a_card_for_our_own_playlist_draws_the_kind_word_alone() {
    let mut art = Art::default();
    let tokens = Tokens::DARK;
    let mut select = None;
    let mut open = None;
    let mut play = None;
    let library = Library::default();
    let mut draw = MediaDraw {
        art: &mut art,
        now: Instant::now(),
        tokens: &tokens,
        library: &library,
        select: &mut select,
        open: &mut open,
        play: &mut play,
    };

    let mut ours = playlist("spotify:playlist:a", "A");
    ours.owner = Some(Owner::You);
    assert_eq!(
        draw.media_of(&Item::Playlist(&ours), MediaLayout::Grid, 176)
            .sub,
        "Playlist"
    );

    let mut named = playlist("spotify:playlist:b", "B");
    named.owner = Some(Owner::Named(
        OwnerName::from_wire("Vela Moreno").expect("a name"),
    ));
    assert_eq!(
        draw.media_of(&Item::Playlist(&named), MediaLayout::Grid, 176)
            .sub,
        "Playlist \u{b7} Vela Moreno"
    );
}

#[test]
fn a_track_row_asks_for_the_small_size_at_two_x() {
    let ctx = context();
    ctx.set_pixels_per_point(2.0);
    let now = Instant::now();
    let mut art = Art::default();
    let (set, small, _, large) = sized("track");
    let tracks = vec![TrackEntry {
        cover: Some(set),
        ..track("t0", "t0")
    }];
    let mut library = library_with(tracks);

    art.begin_frame();
    let _ = band_frame_with(&ctx, &mut library, &mut art, now);
    let fetched = fetch_urls(art.requests(now));

    assert!(
        fetched.contains(&small),
        "a 72px row takes the 64: {fetched:?}"
    );
    assert!(
        !fetched.contains(&large),
        "a row must not fetch the 640: {fetched:?}"
    );
}

#[test]
fn a_mosaic_quadrant_asks_for_half_the_box_at_two_x() {
    let ctx = context();
    ctx.set_pixels_per_point(2.0);
    let now = Instant::now();
    let mut art = Art::default();
    let mut middle = Vec::new();
    let mut large = Vec::new();
    let tracks: Vec<TrackEntry> = (0..crate::mosaic::QUADRANTS)
        .map(|index| {
            let (set, _, mid, big) = sized(&format!("m{index}"));
            middle.push(mid);
            large.push(big);
            TrackEntry {
                cover: Some(set),
                ..track(&format!("t{index}"), &format!("t{index}"))
            }
        })
        .collect();
    let mut library = library_detailed(tracks, "Evening", None, None);

    art.begin_frame();
    let _ = band_frame_with(&ctx, &mut library, &mut art, now);
    let fetched = fetch_urls(art.requests(now));

    for mid in middle {
        assert!(
            fetched.contains(&mid),
            "an 88pt quadrant takes the 300: {fetched:?}"
        );
    }
    for big in large {
        assert!(
            !fetched.contains(&big),
            "a quadrant must not take the whole box's 640: {fetched:?}"
        );
    }
}

#[test]
fn a_playlist_with_four_held_covers_draws_the_mosaic() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let mut library = mosaic_library(&["m0", "m1", "m2", "m3"]);
    let urls: Vec<CoverUrl> = (0..4)
        .map(|index| cover_url(&format!("m{index}")))
        .collect();
    hold(&ctx, &mut art, &urls, now);

    let output = band_frame_with(&ctx, &mut library, &mut art, now);
    let area = band_art(&output);
    assert_eq!(
        textured(&output, area),
        crate::layout::hero::quadrants(area).to_vec(),
        "four covers quarter the band's art box"
    );
}

#[test]
fn the_band_draws_the_single_cover_until_every_mosaic_cover_is_held() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let mut library = mosaic_library_with_picture(&["m0", "m1", "m2", "m3"]);
    let urls: Vec<CoverUrl> = (0..4)
        .map(|index| cover_url(&format!("m{index}")))
        .collect();
    hold(&ctx, &mut art, &urls[..3], now);

    let output = band_frame_with(&ctx, &mut library, &mut art, now);
    let area = band_art(&output);
    assert_eq!(
        textured(&output, area),
        vec![area],
        "one picture fills the box while a quadrant is missing"
    );

    hold(&ctx, &mut art, &urls[3..], now);
    let output = band_frame_with(&ctx, &mut library, &mut art, now);
    let area = band_art(&output);
    assert_eq!(
        textured(&output, area).len(),
        4,
        "the mosaic swaps in whole once the fourth cover lands"
    );
}

#[test]
fn a_mosaic_cover_not_yet_held_is_fetched_but_not_drawn() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let mut library = mosaic_library_with_picture(&["m0", "m1", "m2", "m3"]);
    let entry = library
        .selected_entry()
        .cloned()
        .expect("the page names its playlist");

    let (_, picture, _) = band_picture(
        &entry,
        &library.tracks,
        &mut library.mosaic_kept,
        &mut art,
        1.0,
        &crate::theme::TintParams::DARK,
        now,
    );
    assert_eq!(
        picture,
        hero::Picture::Cover,
        "nothing is drawn until every cover lands"
    );
    let fetched = fetch_urls(art.requests(now));
    for tail in ["m0", "m1", "m2", "m3"] {
        assert!(
            fetched.contains(&cover_url(tail)),
            "{tail} is fetched for the mosaic: {fetched:?}"
        );
    }

    let output = band_frame_with(&ctx, &mut library, &mut art, now);
    let area = band_art(&output);
    assert!(
        textured(&output, area).is_empty(),
        "a cover that has not landed draws nothing"
    );
}

#[test]
fn two_distinct_covers_draw_two_quadrants_and_two_holes() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let mut library = mosaic_library(&["m0", "m1"]);
    hold(&ctx, &mut art, &[cover_url("m0"), cover_url("m1")], now);

    let output = band_frame_with(&ctx, &mut library, &mut art, now);
    let area = band_art(&output);
    let quadrants = crate::layout::hero::quadrants(area);
    assert_eq!(
        textured(&output, area),
        vec![quadrants[0], quadrants[1]],
        "the two covers take the first quadrants in reading order"
    );
    assert_eq!(
        holes(&output, area),
        vec![quadrants[2], quadrants[3]],
        "the missing corners are holes"
    );
}

#[test]
fn a_mosaic_quadrant_that_failed_for_good_keeps_the_single_picture() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let mut library = mosaic_library_with_picture(&["m0", "m1", "m2", "m3"]);
    let urls: Vec<CoverUrl> = (0..4)
        .map(|index| cover_url(&format!("m{index}")))
        .collect();
    hold(&ctx, &mut art, &urls[..3], now);
    let at = fail_permanently(&mut art, &urls[3], now);
    let entry = library
        .selected_entry()
        .cloned()
        .expect("the page names its playlist");

    let (_, picture, _) = band_picture(
        &entry,
        &library.tracks,
        &mut library.mosaic_kept,
        &mut art,
        1.0,
        &crate::theme::TintParams::DARK,
        at,
    );
    assert_eq!(
        picture,
        hero::Picture::Cover,
        "a quadrant that will never arrive holds the mosaic back"
    );

    let output = band_frame_with(&ctx, &mut library, &mut art, at);
    let area = band_art(&output);
    assert_eq!(
        textured(&output, area),
        vec![area],
        "the band stays on its single picture"
    );
}

#[test]
fn a_mosaic_band_tints_from_its_first_quadrant() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let mut library = mosaic_library(&["m0", "m1", "m2", "m3"]);
    let entry = library
        .selected_entry()
        .cloned()
        .expect("the page names its playlist");
    hold_pixel(&ctx, &mut art, &cover_url("m0"), [255, 0, 0, 255], now);
    hold_pixel(&ctx, &mut art, &cover_url("m1"), [0, 255, 0, 255], now);
    hold_pixel(&ctx, &mut art, &cover_url("m2"), [0, 0, 255, 255], now);
    hold_pixel(&ctx, &mut art, &cover_url("m3"), [255, 255, 255, 255], now);

    let (_, picture, tint) = band_picture(
        &entry,
        &library.tracks,
        &mut library.mosaic_kept,
        &mut art,
        1.0,
        &crate::theme::TintParams::DARK,
        now,
    );
    assert!(
        matches!(picture, hero::Picture::Mosaic(_)),
        "four held covers quarter the band"
    );
    assert_eq!(
        tint,
        Some(crate::theme::tint_color([116, 27, 27])),
        "the first quadrant's red, not the mean of the four"
    );
}

#[test]
fn a_chosen_single_picture_tints_the_band() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let mut entry = playlist_with_cover("a", "Evening", cover_url("chosen"));
    entry.cover = Some(PlaylistCover::chosen(CoverSet::single(cover_url("chosen"))));
    hold_pixel(&ctx, &mut art, &cover_url("chosen"), [255, 0, 0, 255], now);
    let tracks: Load<TracksFor> = Load::Idle;
    let mut kept = None;

    let (cover, picture, tint) = band_picture(
        &entry,
        &tracks,
        &mut kept,
        &mut art,
        1.0,
        &crate::theme::TintParams::DARK,
        now,
    );
    assert!(matches!(cover, Cover::Ready(_)));
    assert_eq!(picture, hero::Picture::Cover, "a chosen picture is alone");
    assert_eq!(tint, Some(crate::theme::tint_color([116, 27, 27])));
}

#[test]
fn a_reload_keeps_the_mosaic_that_was_held() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let mut library = mosaic_library(&["m0", "m1", "m2", "m3"]);
    let urls: Vec<CoverUrl> = (0..4)
        .map(|index| cover_url(&format!("m{index}")))
        .collect();
    hold(&ctx, &mut art, &urls, now);

    let output = band_frame_with(&ctx, &mut library, &mut art, now);
    assert_eq!(textured(&output, band_art(&output)).len(), 4);

    // Re-selecting the same playlist puts its tracks back into `Loading`.
    library.select("spotify:playlist:a");
    let output = band_frame_with(&ctx, &mut library, &mut art, now);
    let area = band_art(&output);
    assert_eq!(
        textured(&output, area).len(),
        4,
        "a held mosaic survives the reload"
    );
    assert!(
        fetch_urls(art.requests(now)).is_empty(),
        "a held mosaic is not fetched again"
    );
}

#[test]
fn a_ready_list_of_another_playlist_draws_no_mosaic() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let mut library = mosaic_library_with_picture(&["m0", "m1", "m2", "m3"]);
    let urls: Vec<CoverUrl> = (0..4)
        .map(|index| cover_url(&format!("m{index}")))
        .collect();
    hold(&ctx, &mut art, &urls, now);
    let tracks = match &library.tracks {
        Load::Ready(ready) => ready.tracks.clone(),
        Load::Idle | Load::Loading | Load::Failed(_) => panic!("the page is ready"),
    };
    library.tracks = Load::Ready(TracksFor {
        playlist: "spotify:playlist:other".to_owned(),
        tracks,
        access: None,
        notice: None,
    });

    let entry = library
        .selected_entry()
        .cloned()
        .expect("the page names its playlist");
    let (_, picture, _) = band_picture(
        &entry,
        &library.tracks,
        &mut library.mosaic_kept,
        &mut art,
        1.0,
        &crate::theme::TintParams::DARK,
        now,
    );
    assert_eq!(
        picture,
        hero::Picture::Cover,
        "another list's read never quarter this band"
    );

    let output = band_frame_with(&ctx, &mut library, &mut art, now);
    let area = band_art(&output);
    assert_eq!(
        textured(&output, area),
        vec![area],
        "the band keeps its own picture"
    );
}

#[test]
fn a_drag_in_flight_keeps_the_mosaic() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let mut library = mosaic_library(&["m0", "m1", "m2", "m3"]);
    let urls: Vec<CoverUrl> = (0..4)
        .map(|index| cover_url(&format!("m{index}")))
        .collect();
    hold(&ctx, &mut art, &urls, now);

    let before = {
        let output = band_frame_with(&ctx, &mut library, &mut art, now);
        textured(&output, band_art(&output))
    };
    egui::DragAndDrop::set_payload(&ctx, ());
    let after = {
        let output = band_frame_with(&ctx, &mut library, &mut art, now);
        textured(&output, band_art(&output))
    };
    egui::DragAndDrop::clear_payload(&ctx);

    assert_eq!(after, before, "a drag in flight does not disturb the band");
    assert_eq!(after.len(), 4);
}

#[test]
fn a_reorder_that_changes_the_first_four_covers_changes_the_mosaic_once_ready() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let mut library = mosaic_library(&["m0", "m1", "m2", "m3"]);
    let all: Vec<CoverUrl> = (0..8)
        .map(|index| cover_url(&format!("m{index}")))
        .collect();
    hold(&ctx, &mut art, &all, now);
    let entry = library
        .selected_entry()
        .cloned()
        .expect("the page names its playlist");

    let first = match band_picture(
        &entry,
        &library.tracks,
        &mut library.mosaic_kept,
        &mut art,
        1.0,
        &crate::theme::TintParams::DARK,
        now,
    )
    .1
    {
        hero::Picture::Mosaic(quadrants) => quadrants,
        hero::Picture::Cover => panic!("four held covers quarter the band"),
    };
    assert_eq!(
        first,
        [
            drawn_cover(&mut art, "m0", now),
            drawn_cover(&mut art, "m1", now),
            drawn_cover(&mut art, "m2", now),
            drawn_cover(&mut art, "m3", now),
        ]
    );

    // The reload between orders keeps the quadrants that were held.
    library.select("spotify:playlist:a");
    let reloading = match band_picture(
        &entry,
        &library.tracks,
        &mut library.mosaic_kept,
        &mut art,
        1.0,
        &crate::theme::TintParams::DARK,
        now,
    )
    .1
    {
        hero::Picture::Mosaic(quadrants) => quadrants,
        hero::Picture::Cover => panic!("the kept mosaic stays during the reload"),
    };
    assert_eq!(
        reloading, first,
        "the held quadrants stay while the list reloads"
    );

    // The reordered list lands ready, and the quadrants follow it.
    let mut tracks: Vec<TrackEntry> = (0..4)
        .map(|index| track(&format!("t{index}"), &format!("t{index}")))
        .collect();
    for (index, track) in tracks.iter_mut().enumerate() {
        track.cover = Some(cover_set(&format!("m{}", index + crate::mosaic::QUADRANTS)));
    }
    library.apply(&Update::Tracks {
        playlist: "spotify:playlist:a".to_owned(),
        tracks,
        cover: None,
        access: None,
    });
    let after = match band_picture(
        &entry,
        &library.tracks,
        &mut library.mosaic_kept,
        &mut art,
        1.0,
        &crate::theme::TintParams::DARK,
        now,
    )
    .1
    {
        hero::Picture::Mosaic(quadrants) => quadrants,
        hero::Picture::Cover => panic!("the reordered covers quarter the band"),
    };
    assert_eq!(
        after,
        [
            drawn_cover(&mut art, "m4", now),
            drawn_cover(&mut art, "m5", now),
            drawn_cover(&mut art, "m6", now),
            drawn_cover(&mut art, "m7", now),
        ],
        "the quadrants follow the reordered list"
    );
}

#[test]
fn a_chosen_cover_keeps_its_single_picture() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let mut library = mosaic_library(&["m0", "m1", "m2", "m3"]);
    library.apply(&Update::Playlists(vec![PlaylistEntry {
        composed: Vec::new(),
        cover: Some(PlaylistCover::chosen(CoverSet::single(cover_url("own")))),
        ..listed("Evening", Some(DESCRIPTION), None)
    }]));
    let urls: Vec<CoverUrl> = (0..4)
        .map(|index| cover_url(&format!("m{index}")))
        .collect();
    hold(&ctx, &mut art, &urls, now);
    hold(&ctx, &mut art, &[cover_url("own")], now);

    let output = band_frame_with(&ctx, &mut library, &mut art, now);
    let area = band_art(&output);
    assert_eq!(
        textured(&output, area),
        vec![area],
        "a picture its users chose is never replaced by a mosaic"
    );
}

/// The words and picture the root's grid builds for one playlist card.
fn card_media(library: &Library, art: &mut Art, entry: &PlaylistEntry, now: Instant) -> MediaWords {
    let tokens = Tokens::DARK;
    let mut select = None;
    let mut open = None;
    let mut play = None;
    let mut draw = MediaDraw {
        art,
        now,
        tokens: &tokens,
        library,
        select: &mut select,
        open: &mut open,
        play: &mut play,
    };
    draw.media_of(&Item::Playlist(entry), MediaLayout::Grid, 176)
}

/// The single cover the card draws, and its art box.
fn card_picture(
    ctx: &Context,
    art: &mut Art,
    entry: &PlaylistEntry,
    now: Instant,
) -> (Vec<Rect>, hero::Picture) {
    let library = Library::default();
    let words = card_media(&library, art, entry, now);
    let tokens = Tokens::DARK;
    let mut area = Rect::ZERO;
    let output = run(ctx, input(Vec::new()), |ui| {
        let rect = Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(176.0, 176.0 + media::extra_height(ui)),
        );
        let _ = media::tile(ui, rect, &words.as_media(), &tokens);
        area = media::art_rect(rect);
    });
    (textured(&output, area), words.picture)
}

#[test]
fn a_card_waits_for_every_quadrant_before_swapping() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let entry = PlaylistEntry {
        cover: Some(PlaylistCover::derived(CoverSet::single(cover_url("m0")))),
        composed: (0..4)
            .map(|index| cover_set(&format!("m{index}")))
            .collect(),
        ..listed("Evening", None, None)
    };
    // Three of the four are held; the fourth is still arriving.
    let held: Vec<CoverUrl> = (0..3)
        .map(|index| cover_url(&format!("m{index}")))
        .collect();
    hold(&ctx, &mut art, &held, now);

    let (drawn, picture) = card_picture(&ctx, &mut art, &entry, now);
    assert_eq!(
        picture,
        hero::Picture::Cover,
        "one quadrant still loading keeps the single cover"
    );
    assert_eq!(drawn.len(), 1, "no partial mosaic is drawn");

    hold(&ctx, &mut art, &[cover_url("m3")], now);
    let (drawn, picture) = card_picture(&ctx, &mut art, &entry, now);
    assert!(
        matches!(picture, hero::Picture::Mosaic(_)),
        "the card swaps whole once every quadrant is held"
    );
    assert_eq!(drawn.len(), 4);
}

#[test]
fn a_list_row_asks_only_for_the_cover_it_draws() {
    let now = Instant::now();
    let mut art = Art::default();
    let library = Library::default();
    let entry = PlaylistEntry {
        cover: Some(PlaylistCover::derived(CoverSet::single(cover_url("m0")))),
        composed: (0..4)
            .map(|index| cover_set(&format!("m{index}")))
            .collect(),
        ..listed("Evening", None, None)
    };
    let tokens = Tokens::DARK;
    let mut select = None;
    let mut open = None;
    let mut play = None;
    art.begin_frame();
    let mut draw = MediaDraw {
        art: &mut art,
        now,
        tokens: &tokens,
        library: &library,
        select: &mut select,
        open: &mut open,
        play: &mut play,
    };
    let _ = draw.media_of(&Item::Playlist(&entry), MediaLayout::List, 36);

    assert_eq!(
        fetch_urls(draw.art.requests(now)),
        vec![cover_url("m0")],
        "a row asks only for the cover it draws"
    );
}

#[test]
fn a_long_single_album_head_keeps_the_single_cover_on_the_card() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    // A head of twelve rows, all from one album, is one cover: no mosaic.
    let head: Vec<CoverSet> = (0..12).map(|_| cover_set("one")).collect();
    let composed = takofy_model::composed_from(None, head.iter().map(Some));
    assert!(
        composed.is_empty(),
        "one album repeated has nothing to tile"
    );
    let entry = PlaylistEntry {
        cover: Some(PlaylistCover::derived(CoverSet::single(cover_url("one")))),
        composed,
        ..listed("Evening", None, None)
    };
    hold(&ctx, &mut art, &[cover_url("one")], now);

    let (drawn, picture) = card_picture(&ctx, &mut art, &entry, now);
    assert_eq!(picture, hero::Picture::Cover);
    assert_eq!(drawn.len(), 1, "the accepted gap: the card keeps one cover");
}

#[test]
fn one_distinct_cover_keeps_the_single_look() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let tracks = (0..4)
        .map(|index| TrackEntry {
            cover: Some(cover_set("one")),
            ..track(&format!("t{index}"), &format!("t{index}"))
        })
        .collect();
    let mut library = library_detailed(tracks, "Evening", None, None);
    library.apply(&Update::Playlists(vec![PlaylistEntry {
        composed: Vec::new(),
        cover: Some(PlaylistCover::derived(CoverSet::single(cover_url("one")))),
        ..listed("Evening", None, None)
    }]));
    hold(&ctx, &mut art, &[cover_url("one")], now);

    let output = band_frame_with(&ctx, &mut library, &mut art, now);
    let area = band_art(&output);
    assert_eq!(
        textured(&output, area),
        vec![area],
        "one album is one picture, not four"
    );
}

#[test]
fn album_and_show_bands_still_draw_one_cover() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let now = Instant::now();
    let mut art = Art::default();
    hold(&ctx, &mut art, &[cover_url("one")], now);
    let cover = art.look(Some(&cover_set("one")), 176, now);
    assert!(matches!(cover, Cover::Ready(_)));

    for eyebrow in ["Album", "Show"] {
        let mut output = run(&ctx, input(Vec::new()), |ui| {
            let band = hero::Band {
                eyebrow,
                title: "Signal Decay",
                meta: Vec::new(),
                credit: None,
                description: "",
                cover,
                tint: None,
            };
            let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
        });
        let area = band_art(&output);
        assert_eq!(
            textured(&output, area),
            vec![area],
            "the {eyebrow} band still draws its one cover"
        );
        output.textures_delta.clear();
    }
}

#[test]
fn the_band_text_does_not_move_with_the_covers_held() {
    let ctx = context();
    let now = Instant::now();
    let mut art = Art::default();
    let mut library = mosaic_full();
    let urls: Vec<CoverUrl> = (0..4)
        .map(|index| cover_url(&format!("m{index}")))
        .collect();

    let mut held = 0;
    let mut positions = Vec::new();
    let mut pictures = Vec::new();
    for want in [0, 2, 4] {
        if want > held {
            hold(&ctx, &mut art, &urls[held..want], now);
            held = want;
        }
        let output = band_frame_with(&ctx, &mut library, &mut art, now);
        let area = band_art(&output);
        pictures.push(textured(&output, area).len());
        positions.push((
            galley_at(&output, "Evening"),
            galley_at(&output, META),
            galley_at(&output, DESCRIPTION),
        ));
    }

    assert_eq!(positions[0], positions[1], "two covers moved the words");
    assert_eq!(positions[1], positions[2], "four covers moved the words");
    assert_eq!(
        pictures,
        vec![0, 0, 4],
        "the picture itself changed, so the texts were compared across frames"
    );
}

#[test]
fn a_long_description_is_clamped_to_two_rows_and_elided() {
    let ctx = context();
    let mut library = library_detailed(
        timed_tracks(2, 1_000),
        "Evening",
        Some(LONG_DESCRIPTION),
        None,
    );
    let output = tracklist_frame(&ctx, Table::Full, &mut library);
    let galley = galley_of(&output, LONG_DESCRIPTION);
    assert_eq!(galley.rows.len(), 2, "two rows, no more");
    assert!(galley.elided, "a description that does not fit elides");
    let last = galley.rows[1].row.text();
    assert!(
        last.ends_with('\u{2026}'),
        "the elided last row must end in an ellipsis: {last:?}"
    );
}

#[test]
fn a_narrow_band_elides_the_byline() {
    let ctx = context();
    let mut library = library_detailed(
        timed_tracks(62, 223_548),
        "Evening",
        None,
        Some("alice-the-very-long-username"),
    );
    let output = tracklist_frame_in(&ctx, Vec2::new(560.0, 800.0), &mut library);
    let galley = galley_of(
        &output,
        "Made by alice-the-very-long-username \u{b7} 62 songs \u{b7} 3 hr 51 min",
    );
    assert!(
        galley.elided,
        "a byline wider than the band elides: {:?}",
        galley.text()
    );
}

#[test]
fn the_band_does_not_clip_the_eyebrow_at_its_tallest_content() {
    let ctx = context();
    let tokens = Tokens::DARK;
    let mut band_top = None;
    let output = run(
        &ctx,
        sized_input(Vec2::new(1600.0, 900.0), Vec::new()),
        |ui| {
            band_top = Some(ui.cursor().min.y);
            let band = hero::Band {
                eyebrow: "Playlist",
                title: "Late Night Drive",
                meta: vec![
                    "Made by alice".to_owned(),
                    "62 songs".to_owned(),
                    "3 hr 51 min".to_owned(),
                ],
                credit: None,
                description: LONG_DESCRIPTION,
                cover: Cover::Missing,
                tint: None,
            };
            let _ = hero::show_with(ui, &band, &tokens, hero::Title::Plain);
        },
    );
    let band_top = band_top.expect("the band starts at the cursor");
    let drawn = drawn_galleys(&output);
    let eyebrow = galley_top(&drawn, "PLAYLIST");
    assert!(
        eyebrow >= band_top,
        "the eyebrow's top {eyebrow} is above the band's top {band_top}: the \
         tallest content does not fit the band's height"
    );
}
