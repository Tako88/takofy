//! The artist page's Follow state.
//!
//! The button reports what the server confirmed and nothing else: a press does
//! not flip the label, it sends the write, and the label moves when the answer
//! says so. While a write is in flight the button cannot be pressed again, and a
//! failure leaves the label where the server last left it.
//!
//! The page's probe and the write's own reply arrive as the same update, so a
//! write in flight is remembered by the value it asked for: only that value
//! ends the wait.

use crate::message::{Command, Failure, Update};

/// The artist the page is following, and what the server has said about it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Follow {
    /// The artist this state concerns; `None` before any artist page opened.
    artist: Option<String>,
    /// The last confirmed state. `None` means the probe has not answered, which
    /// reads as not followed and stays pressable: the write is idempotent.
    confirmed: Option<bool>,
    /// The value the in-flight write asked for. `None` means no write is in
    /// flight. The probe's answer and the write's answer both arrive as
    /// `Followed`, so the requested value is what says which one answered.
    pending: Option<bool>,
    /// The last write's failure, cleared when the write's own answer arrives.
    failed: Option<Failure>,
}

impl Follow {
    /// Points the state at an artist.
    ///
    /// Opening another artist resets everything, because a label confirmed for
    /// one artist says nothing about another. Re-opening the same artist keeps
    /// what is known, so a page that reloads does not flash `Follow` over a
    /// followed artist.
    pub fn open(&mut self, artist: &str) {
        if self.artist.as_deref() == Some(artist) {
            return;
        }
        *self = Self {
            artist: Some(artist.to_owned()),
            ..Self::default()
        };
    }

    /// The artist this state concerns.
    pub fn artist(&self) -> Option<&str> {
        self.artist.as_deref()
    }

    /// Whether the server has been heard to follow this artist, or `None` when
    /// the page's own probe did not answer.
    pub fn confirmed(&self) -> Option<bool> {
        self.confirmed
    }

    /// The label the button draws.
    pub fn label(&self) -> &'static str {
        if self.confirmed == Some(true) {
            "Following"
        } else {
            "Follow"
        }
    }

    /// Whether the button can be pressed at all.
    pub fn pressable(&self) -> bool {
        self.artist.is_some() && self.pending.is_none()
    }

    /// Whether a write of the button's is in flight.
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The value the in-flight write asked for, if one is in flight.
    pub fn asking(&self) -> Option<bool> {
        self.pending
    }

    /// The failure the button's hint shows, if the last write failed.
    pub fn failed(&self) -> Option<Failure> {
        self.failed
    }

    /// Presses the button: the write that asks the server for the opposite of
    /// what it last confirmed.
    ///
    /// Returns nothing while a write is in flight or before an artist page has
    /// opened, so a double press cannot send two writes.
    pub fn pressed(&mut self) -> Option<Command> {
        let artist = self.artist.clone()?;
        if self.pending.is_some() {
            return None;
        }
        let followed = !self.confirmed.unwrap_or(false);
        self.pending = Some(followed);
        Some(Command::SetFollow { artist, followed })
    }

    /// Folds one worker update into the state.
    ///
    /// Only an answer for the artist this state concerns is read. Every such
    /// answer moves the label; only the answer the in-flight write asked for
    /// ends the wait and clears the failure. A `Followed` that reports some
    /// other value is the page's own probe, which says nothing about the write.
    pub fn apply(&mut self, update: &Update) {
        match update {
            Update::Followed { artist, followed }
                if self.artist.as_deref() == Some(artist.as_str()) =>
            {
                self.confirmed = Some(*followed);
                if self.pending == Some(*followed) {
                    self.pending = None;
                    self.failed = None;
                }
            }
            Update::FollowFailed { artist, failure }
                if self.artist.as_deref() == Some(artist.as_str()) =>
            {
                self.pending = None;
                self.failed = Some(*failure);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
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
}
