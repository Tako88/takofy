use super::*;

fn set_follow(artist: &str, followed: bool) -> Command {
    Command::SetFollow {
        artist: artist.to_owned(),
        followed,
    }
}

#[test]
fn an_unknown_artist_reads_follow_and_cannot_be_pressed() {
    let mut follow = Follow::default();
    assert_eq!(follow.label(), "Follow");
    assert!(!follow.pressable());
    assert_eq!(follow.pressed(), None);
}

#[test]
fn a_confirmed_artist_reads_following() {
    let mut follow = Follow::default();
    follow.open("a");
    follow.apply(&Update::Followed {
        artist: "a".to_owned(),
        followed: true,
    });
    assert_eq!(follow.label(), "Following");

    follow.apply(&Update::Followed {
        artist: "a".to_owned(),
        followed: false,
    });
    assert_eq!(follow.label(), "Follow");
}

#[test]
fn a_press_targets_the_opposite_of_what_was_confirmed() {
    let mut follow = Follow::default();
    follow.open("a");
    assert_eq!(follow.pressed(), Some(set_follow("a", true)));

    let mut followed = Follow::default();
    followed.open("a");
    followed.apply(&Update::Followed {
        artist: "a".to_owned(),
        followed: true,
    });
    assert_eq!(followed.pressed(), Some(set_follow("a", false)));
}

#[test]
fn a_press_while_pending_does_nothing() {
    let mut follow = Follow::default();
    follow.open("a");
    assert_eq!(follow.pressed(), Some(set_follow("a", true)));
    assert!(!follow.pressable());
    assert_eq!(follow.pressed(), None, "a double press sends one write");
}

#[test]
fn the_label_flips_only_when_the_server_answers() {
    let mut follow = Follow::default();
    follow.open("a");
    let _ = follow.pressed();
    assert_eq!(
        follow.label(),
        "Follow",
        "the press alone must not flip the label"
    );

    follow.apply(&Update::Followed {
        artist: "a".to_owned(),
        followed: true,
    });
    assert_eq!(follow.label(), "Following");
    assert!(follow.pressable());
}

#[test]
fn an_answer_for_another_artist_is_discarded() {
    let mut follow = Follow::default();
    follow.open("a");
    let _ = follow.pressed();

    follow.apply(&Update::Followed {
        artist: "b".to_owned(),
        followed: true,
    });
    assert_eq!(follow.label(), "Follow");
    assert!(!follow.pressable(), "A's write is still in flight");

    follow.apply(&Update::Followed {
        artist: "a".to_owned(),
        followed: true,
    });
    assert_eq!(follow.label(), "Following");
}

#[test]
fn a_failure_keeps_the_confirmed_label_and_is_retryable() {
    let mut follow = Follow::default();
    follow.open("a");
    follow.apply(&Update::Followed {
        artist: "a".to_owned(),
        followed: true,
    });
    let _ = follow.pressed();

    follow.apply(&Update::FollowFailed {
        artist: "a".to_owned(),
        failure: Failure::Save,
    });
    assert_eq!(
        follow.label(),
        "Following",
        "a failed unfollow leaves the artist followed"
    );
    assert_eq!(follow.failed(), Some(Failure::Save));
    assert!(follow.pressable(), "the retry is the next press");
    assert_eq!(follow.pressed(), Some(set_follow("a", false)));
}

#[test]
fn a_probe_answer_while_a_write_is_pending_keeps_it_pending() {
    let mut follow = Follow::default();
    follow.open("a");
    follow.apply(&Update::Followed {
        artist: "a".to_owned(),
        followed: true,
    });
    assert_eq!(follow.pressed(), Some(set_follow("a", false)));

    // The page's own probe answers the artist it opened, with the value the
    // server confirmed before the press. The write has not answered yet.
    follow.apply(&Update::Followed {
        artist: "a".to_owned(),
        followed: true,
    });
    assert!(
        !follow.pressable(),
        "the probe must not let a second write be sent"
    );
    assert_eq!(follow.pressed(), None);
}

#[test]
fn the_reply_the_write_asked_for_clears_the_pending_write() {
    let mut follow = Follow::default();
    follow.open("a");
    assert_eq!(follow.pressed(), Some(set_follow("a", true)));

    follow.apply(&Update::Followed {
        artist: "a".to_owned(),
        followed: true,
    });
    assert_eq!(follow.label(), "Following");
    assert!(follow.pressable());
}

#[test]
fn a_mismatched_reply_moves_the_label_and_clears_nothing() {
    let mut follow = Follow::default();
    follow.open("a");
    follow.apply(&Update::FollowFailed {
        artist: "a".to_owned(),
        failure: Failure::Save,
    });
    let _ = follow.pressed();

    // Not the value the write asked for, so the write is still in flight and
    // the failure it must clear is still shown.
    follow.apply(&Update::Followed {
        artist: "a".to_owned(),
        followed: false,
    });
    assert_eq!(follow.label(), "Follow", "the reply still moved the label");
    assert_eq!(follow.failed(), Some(Failure::Save), "nothing was cleared");
    assert!(!follow.pressable(), "the write is still in flight");
}

#[test]
fn a_successful_answer_clears_the_failure() {
    let mut follow = Follow::default();
    follow.open("a");
    let _ = follow.pressed();
    follow.apply(&Update::FollowFailed {
        artist: "a".to_owned(),
        failure: Failure::Save,
    });
    let _ = follow.pressed();
    follow.apply(&Update::Followed {
        artist: "a".to_owned(),
        followed: true,
    });
    assert_eq!(follow.failed(), None);
}

#[test]
fn opening_another_artist_resets_and_the_same_keeps() {
    let mut follow = Follow::default();
    follow.open("a");
    follow.apply(&Update::Followed {
        artist: "a".to_owned(),
        followed: true,
    });

    follow.open("a");
    assert_eq!(
        follow.label(),
        "Following",
        "re-opening keeps what is known"
    );

    follow.open("b");
    assert_eq!(follow.label(), "Follow", "another artist is not followed");
    assert_eq!(follow.artist(), Some("b"));
    assert!(follow.pressable());
    assert_eq!(follow.failed(), None);
}
