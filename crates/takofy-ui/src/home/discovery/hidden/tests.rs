use takofy_model::home::Day;

use super::*;

fn artist(id: usize) -> String {
    format!("spotify:artist:{id}")
}

fn refused(line: usize, reason: Reason) -> Refusal {
    Refusal { line, reason }
}

fn hidden(day: Option<i32>, positions: [usize; 3], artists: &[&str]) -> Hidden {
    let mut hidden = Hidden::default();
    if let Some(day) = day {
        hidden.set_positions(Day::from_days(day), positions);
    }
    for artist in artists {
        assert!(hidden.hide(artist), "{artist} is an artist address");
    }
    hidden
}

#[test]
fn a_round_trip_reads_back_what_it_wrote() {
    let set = hidden(
        Some(19_000),
        [2, 5, 7],
        &["spotify:artist:a", "spotify:artist:b"],
    );

    assert_eq!(
        Hidden::parse(&set.render()),
        Ok(set.clone()),
        "everything the file holds is in the value the file came from"
    );
    assert_eq!(
        set.render(),
        "format=1\nday=19000\nplaylist=2\nartist=5\nalbum=7\nhide=spotify:artist:a\nhide=spotify:artist:b\n",
        "the file is one format line, the day's positions, then one hide per artist"
    );
}

#[test]
fn an_empty_file_is_the_default_set() {
    for text in ["", "\n\n   \n", "format=1\n"] {
        assert_eq!(
            Hidden::parse(text),
            Ok(Hidden::default()),
            "nothing written is nothing hidden and no positions"
        );
    }
}

#[test]
fn a_file_written_with_a_bom_and_crlf_still_reads() {
    let text = "\u{feff}format=1\r\nday=42\r\nplaylist=1\r\nartist=0\r\nalbum=3\r\nhide=spotify:artist:a\r\n";

    let set = Hidden::parse(text).expect("a BOM and CRLF are tolerated");

    assert_eq!(set.positions_for(Day::from_days(42)), Some([1, 0, 3]));
    assert!(set.contains("spotify:artist:a"));
}

#[test]
fn a_line_without_an_equals_refuses_the_file() {
    let refused_line =
        Hidden::parse("format=1\nday\n").expect_err("a line that is not key=value is refused");

    assert_eq!(refused_line, refused(2, Reason::NoEquals));
    assert_eq!(refused_line.to_string(), "line 2: no `=`");
}

#[test]
fn space_around_a_key_refuses_the_file() {
    for text in [" day=4\n", "day =4\n", "hide =spotify:artist:a\n"] {
        let refused_line = Hidden::parse(text).expect_err("a padded key is refused whole");
        assert_eq!(refused_line.reason, Reason::SpaceAroundKey);
    }
    let refused_line = Hidden::parse("format=1\n day=4\n").expect_err("the key is padded");
    assert_eq!(refused_line, refused(2, Reason::SpaceAroundKey));
}

#[test]
fn a_format_other_than_one_refuses_the_file() {
    for text in ["format=2\n", "format=\n", "format=1.0\n"] {
        let refused_line = Hidden::parse(text).expect_err("only format 1 is this version's file");
        assert_eq!(refused_line, refused(1, Reason::BadValue(Key::Format)));
    }
}

#[test]
fn a_bad_number_refuses_the_file() {
    for (text, key) in [
        ("day=soon\n", Key::Day),
        ("day=1\nplaylist=many\n", Key::Playlist),
        ("day=1\nartist=-\n", Key::Artist),
        ("day=1\nalbum=3.5\n", Key::Album),
    ] {
        let refused_line = Hidden::parse(text).expect_err("a value outside the table is refused");
        assert_eq!(
            refused_line,
            Refusal {
                line: text.lines().count(),
                reason: Reason::BadValue(key),
            },
            "the value at fault is named by its line and key"
        );
    }
}

#[test]
fn a_hide_that_is_not_an_artist_address_refuses_the_file() {
    for text in [
        "hide=spotify:album:a\n",
        "hide=spotify:artist:\n",
        "hide=https://example.test/artist/a\n",
        "hide=Burial\n",
        "hide=\n",
    ] {
        let refused_line = Hidden::parse(text).expect_err("a hidden thing is an artist address");
        assert_eq!(refused_line, refused(1, Reason::NotAnArtist));
        assert!(
            !refused_line.to_string().contains("example.test")
                && !refused_line.to_string().contains("Burial"),
            "the text the user wrote is never quoted back"
        );
    }
}

#[test]
fn a_duplicate_hide_refuses_the_file() {
    let refused_line = Hidden::parse("hide=spotify:artist:a\nhide=spotify:artist:a\n")
        .expect_err("one artist hidden twice is one artist with two lines and no honest winner");

    assert_eq!(refused_line, refused(2, Reason::DuplicateHide));
    assert_eq!(refused_line.to_string(), "line 2: `hide` appears twice");
}

#[test]
fn a_duplicate_singular_key_refuses_the_file() {
    for (text, key) in [
        ("format=1\nformat=1\n", Key::Format),
        ("day=1\nday=2\n", Key::Day),
        ("day=1\nplaylist=0\nplaylist=1\n", Key::Playlist),
        ("day=1\nartist=0\nartist=1\n", Key::Artist),
        ("day=1\nalbum=0\nalbum=1\n", Key::Album),
    ] {
        let refused_line =
            Hidden::parse(text).expect_err("two values for one key have no honest winner");
        assert_eq!(refused_line.line, text.lines().count());
        assert_eq!(refused_line.reason, Reason::Duplicate(key));
    }
}

#[test]
fn a_position_without_a_day_refuses_the_file() {
    for text in ["playlist=1\n", "playlist=1\nartist=2\nalbum=3\n"] {
        let refused_line = Hidden::parse(text).expect_err("a position belongs to a day");
        assert_eq!(refused_line.reason, Reason::PositionsWithoutDay);
        assert_eq!(
            refused_line.line, 1,
            "the first position is the line that made no sense"
        );
    }
}

#[test]
fn a_refusal_never_prints_the_value() {
    for (text, secret) in [
        ("format=secret-format\n", "secret-format"),
        ("day=secret-day\n", "secret-day"),
        ("day=1\nartist=secret-position\n", "secret-position"),
        ("hide=secret-artist\n", "secret-artist"),
    ] {
        let message = Hidden::parse(text)
            .expect_err("a value outside the table is refused")
            .to_string();
        assert!(
            !message.contains(secret),
            "the text the user wrote is never quoted back: {message}"
        );
    }
}

#[test]
fn a_hide_needs_no_day() {
    let set = Hidden::parse("hide=spotify:artist:a\n").expect("hiding is not a position");

    assert!(set.contains("spotify:artist:a"));
    assert_eq!(set.positions_for(Day::from_days(1)), None);
}

#[test]
fn an_unknown_key_is_ignored_with_whatever_it_holds() {
    assert_eq!(
        Hidden::parse("band=on\nfuture=whatever\nday=4\nalbum=2\n")
            .expect("a key this version does not know is skipped"),
        hidden(Some(4), [0, 0, 2], &[])
    );
}

#[test]
fn a_hide_is_idempotent_and_bumps_the_revision_once() {
    let mut set = Hidden::default();
    let start = set.revision();

    assert!(set.hide("spotify:artist:a"), "a new artist is one change");
    let once = set.revision();
    assert!(once > start, "a real change bumps the revision");
    assert!(!set.hide("spotify:artist:a"), "hiding twice writes nothing");
    assert_eq!(set.revision(), once, "and bumps nothing");
    assert!(set.contains("spotify:artist:a"));

    assert!(set.unhide("spotify:artist:a"), "an unhide is a change");
    assert!(set.revision() > once);
    let twice = set.revision();
    assert!(
        !set.unhide("spotify:artist:a"),
        "unhiding twice writes nothing"
    );
    assert!(!set.contains("spotify:artist:a"));
    assert_eq!(set.revision(), twice);
}

#[test]
fn a_hide_that_is_not_an_artist_address_is_refused_by_the_set() {
    let mut set = Hidden::default();

    assert!(!set.hide("spotify:album:a"));
    assert!(
        Hidden::parse(&set.render()).is_ok(),
        "the set never writes a file it cannot read"
    );
}

#[test]
fn the_positions_belong_to_their_own_day() {
    let mut set = Hidden::default();

    set.set_positions(Day::from_days(4), [1, 2, 3]);
    assert_eq!(set.positions_for(Day::from_days(4)), Some([1, 2, 3]));
    assert_eq!(
        set.positions_for(Day::from_days(5)),
        None,
        "another day's positions would start its rows in the wrong place"
    );
}

#[test]
fn setting_the_same_positions_writes_nothing() {
    let mut set = Hidden::default();
    set.set_positions(Day::from_days(4), [1, 2, 3]);
    let before = set.revision();

    set.set_positions(Day::from_days(4), [1, 2, 3]);

    assert_eq!(set.revision(), before, "nothing changed");
    set.set_positions(Day::from_days(4), [1, 2, 4]);
    assert_eq!(set.revision(), before + 1, "a moved row is a change");
    set.set_positions(Day::from_days(5), [1, 2, 4]);
    assert_eq!(
        set.revision(),
        before + 2,
        "a new day records its own positions even when the numbers match"
    );
    assert_eq!(set.positions_for(Day::from_days(5)), Some([1, 2, 4]));
    assert_eq!(set.positions_for(Day::from_days(4)), None);
}

#[test]
fn the_cap_refuses_more_hides() {
    let mut set = Hidden::default();
    for id in 0..HIDE_MAX {
        assert!(set.hide(&artist(id)), "a hide within the cap is kept");
    }

    assert!(!set.hide(&artist(HIDE_MAX)), "the cap refuses one more");
    assert_eq!(set.artists.len(), HIDE_MAX);
    assert_eq!(
        Hidden::parse(&set.render()),
        Ok(set),
        "a set at the cap is still a file this version reads"
    );
}

#[test]
fn an_oversize_file_refuses_the_whole_file() {
    let mut text = String::from("format=1\n");
    for id in 0..=HIDE_MAX {
        text.push_str(&format!("hide={}\n", artist(id)));
    }

    let refused_line =
        Hidden::parse(&text).expect_err("more hides than the cap is not a file we read");

    assert_eq!(refused_line.reason, Reason::TooManyHides);
    assert_eq!(refused_line.line, HIDE_MAX + 2);
    assert!(
        !refused_line.to_string().contains("spotify:artist:"),
        "the message never quotes a hidden address"
    );
}

#[test]
fn each_reason_has_its_own_message() {
    for (reason, message) in [
        (Reason::NoEquals, "line 3: no `=`"),
        (Reason::SpaceAroundKey, "line 3: space around a key"),
        (
            Reason::BadValue(Key::Format),
            "line 3: bad value for `format`",
        ),
        (Reason::BadValue(Key::Day), "line 3: bad value for `day`"),
        (
            Reason::BadValue(Key::Playlist),
            "line 3: bad value for `playlist`",
        ),
        (
            Reason::BadValue(Key::Artist),
            "line 3: bad value for `artist`",
        ),
        (
            Reason::BadValue(Key::Album),
            "line 3: bad value for `album`",
        ),
        (Reason::Duplicate(Key::Day), "line 3: `day` appears twice"),
        (Reason::DuplicateHide, "line 3: `hide` appears twice"),
        (Reason::NotAnArtist, "line 3: not an artist address"),
        (
            Reason::PositionsWithoutDay,
            "line 3: a position without `day`",
        ),
        (
            Reason::TooManyHides,
            "line 3: more than 10000 hidden artists",
        ),
    ] {
        assert_eq!(
            refused(3, reason).to_string(),
            message,
            "each reason spells its own sentence: {reason:?}"
        );
    }
}
