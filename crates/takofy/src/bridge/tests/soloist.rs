//! What the settings page's asks do on their way through the loop.

use super::super::*;
use super::test_support::*;
use std::time::Duration;
use takofy_ui::message::{KeyState, SoloistAsk, SoloistUpdate};
use takofy_ui::secret::Secret;

/// A key whose text must not come back to the window in any form.
const KEY: &str = "sentinel-key-7c1d";

#[test]
fn a_soloist_ask_is_claimed_and_answered() {
    let store = Arc::new(MemoryStore::seeded("stored"));
    let source = Arc::new(FixedSource::on_refresh(Ok(token_set("access", None))));
    let host = FakeHost::new(ConnectScript::Ok, RootlistScript::Ready(Vec::new())).with_desk();

    run_window(runtime(), host, store, source, move |ui_end, slot| {
        slot.install(Arc::new(RecordingWaker::default()));
        let _ = expect_library(&ui_end);
        ui_end
            .commands
            .send(Command::Soloist(SoloistAsk::StoreKey(Secret::from_text(
                KEY,
            ))))
            .expect("the ask");
        loop {
            let update = ui_end
                .updates
                .recv_timeout(Duration::from_secs(5))
                .expect("an answer to the ask");
            assert!(
                !format!("{update:?}").contains(KEY),
                "the key is stored and forgotten, never handed back"
            );
            let Update::Soloist(SoloistUpdate::Facts(facts)) = update else {
                continue;
            };
            assert_eq!(
                facts.key,
                KeyState::Stored,
                "the desk writes the key and answers with what the keychain holds"
            );
            break;
        }
    });
}
