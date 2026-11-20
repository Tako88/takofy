use super::fake::{FakeKeyStore, FakeStore};
use super::{AccessToken, CredentialStore, RefreshToken, SoloistKey, SoloistKeyStore, StoreError};

fn token(value: &str) -> RefreshToken {
    RefreshToken::new(value.to_owned()).expect("non-empty token")
}

#[test]
fn debug_hides_the_secret() {
    let refresh = token("super-secret-refresh");
    assert_eq!(format!("{refresh:?}"), "RefreshToken(<redacted>)");

    let access = AccessToken::new("super-secret-access".to_owned()).expect("non-empty");
    assert_eq!(format!("{access:?}"), "AccessToken(<redacted>)");
}

#[test]
fn empty_refresh_token_is_rejected() {
    assert!(RefreshToken::new(String::new()).is_none());
    assert!(RefreshToken::new("  \n\t ".to_owned()).is_none());
}

#[test]
fn load_on_empty_is_none() {
    let store = FakeStore::new();
    assert!(store.load().expect("load succeeds").is_none());
}

#[test]
fn save_then_load_round_trips() {
    let store = FakeStore::new();
    store.save(&token("abc")).expect("save succeeds");
    let loaded = store.load().expect("load succeeds").expect("token present");
    assert_eq!(loaded.expose(), "abc");
    assert_eq!(store.save_count(), 1);
}

#[test]
fn seeded_token_loads_without_counting_a_save() {
    let store = FakeStore::seeded(token("seeded"));
    let loaded = store.load().expect("load succeeds").expect("token present");
    assert_eq!(loaded.expose(), "seeded");
    assert_eq!(store.save_count(), 0);
}

#[test]
fn save_overwrites() {
    let store = FakeStore::new();
    store.save(&token("first")).expect("save succeeds");
    store.save(&token("second")).expect("save succeeds");
    let loaded = store.load().expect("load succeeds").expect("token present");
    assert_eq!(loaded.expose(), "second");
}

#[test]
fn clear_is_idempotent() {
    let store = FakeStore::new();
    store.save(&token("abc")).expect("save succeeds");
    store.clear().expect("clear succeeds");
    assert!(store.load().expect("load succeeds").is_none());
    store.clear().expect("second clear succeeds");
    assert!(store.load().expect("load succeeds").is_none());
}

#[test]
fn failing_store_surfaces_its_error() {
    let store = FakeStore::new();
    store.fail_load();
    assert!(matches!(store.load(), Err(StoreError::Unavailable)));

    let store = FakeStore::new();
    store.fail_save();
    assert!(matches!(
        store.save(&token("abc")),
        Err(StoreError::Unavailable)
    ));
}

fn soloist_key(value: &str) -> SoloistKey {
    SoloistKey::new(value.to_owned()).expect("non-empty key")
}

#[test]
fn soloist_key_debug_never_carries_the_key() {
    let key = soloist_key("soloist-super-secret");
    let shown = format!("{key:?}");
    assert_eq!(shown, "SoloistKey(<redacted>)");
    assert!(!shown.contains("soloist-super-secret"), "leaked: {shown}");
}

#[test]
fn empty_soloist_key_is_rejected() {
    assert!(SoloistKey::new(String::new()).is_none());
    assert!(SoloistKey::new("  \n\t ".to_owned()).is_none());
}

#[test]
fn soloist_key_save_then_load_round_trips() {
    let store = FakeKeyStore::new();
    assert!(store.load().expect("load succeeds").is_none());
    store.save(&soloist_key("abc")).expect("save succeeds");
    let loaded = store.load().expect("load succeeds").expect("key present");
    assert_eq!(loaded.expose(), "abc");
    assert_eq!(store.save_count(), 1);
}

#[test]
fn soloist_key_clear_is_idempotent() {
    let store = FakeKeyStore::new();
    store.save(&soloist_key("abc")).expect("save succeeds");
    store.clear().expect("clear succeeds");
    assert!(store.load().expect("load succeeds").is_none());
    store.clear().expect("second clear succeeds");
}

#[test]
fn soloist_key_store_surfaces_its_error() {
    let store = FakeKeyStore::new();
    store.fail_load();
    assert!(matches!(store.load(), Err(StoreError::Unavailable)));
}
