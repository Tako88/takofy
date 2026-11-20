use super::super::*;
use crate::message::{Command, SoloistAsk};
use crate::secret::Secret;

fn key() -> Command {
    Command::Soloist(SoloistAsk::StoreKey(Secret::from_text("hunter2")))
}

#[test]
fn a_press_carrying_a_key_prints_nothing_of_it() {
    let pressed = Pressed {
        command: Some(key()),
        steer: None,
        control: None,
    };
    assert!(!format!("{pressed:?}").contains("hunter2"));
    assert!(!format!("{pressed:#?}").contains("hunter2"));
}

#[test]
fn a_reply_carrying_a_key_prints_nothing_of_it() {
    let reply = Reply {
        command: Some(key()),
        ..Reply::default()
    };
    assert!(!format!("{reply:?}").contains("hunter2"));
    assert!(!format!("{reply:#?}").contains("hunter2"));
}
