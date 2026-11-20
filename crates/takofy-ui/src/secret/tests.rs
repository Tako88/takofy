use super::*;
use crate::message::{Command, SoloistAsk};

#[test]
fn a_secret_prints_nothing_of_its_text() {
    let secret = Secret::from_text("hunter2");
    assert_eq!(format!("{secret:?}"), "Secret(..)");
    assert_eq!(format!("{secret:#?}"), "Secret(..)");
    assert!(!format!("{secret:?}").contains("hunter2"));
    assert!(!format!("{secret:#?}").contains("hunter2"));
}

#[test]
fn a_command_carrying_a_secret_prints_nothing_of_it() {
    let command = Command::Soloist(SoloistAsk::StoreKey(Secret::from_text("hunter2")));
    assert!(!format!("{command:?}").contains("hunter2"));
    assert!(!format!("{command:#?}").contains("hunter2"));
}

#[test]
fn a_reply_and_a_settings_page_holding_a_secret_print_nothing_of_it() {
    let reply = crate::view::playbar::Reply {
        command: Some(Command::Soloist(SoloistAsk::StoreKey(Secret::from_text(
            "hunter2",
        )))),
        ..crate::view::playbar::Reply::default()
    };
    assert!(!format!("{reply:?}").contains("hunter2"));
    assert!(!format!("{reply:#?}").contains("hunter2"));

    let mut settings = crate::settings::Settings::default();
    settings.key_field_mut().push_str("hunter2");
    assert!(!format!("{settings:?}").contains("hunter2"));
    assert!(!format!("{settings:#?}").contains("hunter2"));
}

#[test]
fn taking_a_secret_empties_the_source() {
    let mut field = Secret::from_text("hunter2");
    let taken = field.take();
    assert!(field.is_empty());
    assert_eq!(taken.into_inner(), "hunter2");
}

#[test]
fn whitespace_alone_is_no_key() {
    let mut secret = Secret::from_text("  \n ");
    assert!(secret.is_empty());
    secret.edit().push('a');
    assert!(!secret.is_empty());
}

#[test]
fn the_edit_handle_writes_the_text() {
    let mut secret = Secret::default();
    secret.edit().push_str("abc");
    assert_eq!(secret.into_inner(), "abc");
}
