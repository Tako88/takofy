//! Transport orders: the phone's order goes to the window, whose answer is its own.

use takofy_core::ORDER_MARGIN;
use takofy_core::instruction::Kind;
use takofy_model::{Order, RequestId};
use takofy_ui::message::{RemoteUpdate, Update};
use tokio::runtime::Runtime;

use super::rebuild::Step;
use super::wake::Notifier;
use crate::session::SessionHost;

/// Says which way one order's answer landed, in the same words from the loop
/// and from a rebuild that answers it while it runs.
pub(super) fn deliver_answer(answered: bool) {
    if answered {
        log::info!(target: LANDING_LOG_TARGET, "answer delivered");
    } else {
        log::info!(
            target: LANDING_LOG_TARGET,
            "answer found the entry gone"
        );
    }
}

/// Hands one transport order to the window, which applies it and answers.
///
/// The entry stays pending here: the window's answer is the phone's, and until
/// it arrives the registry's own budget is what ends the order. Nothing is
/// applied without an answer, so a phone that hears nothing is never obeyed in
/// secret.
pub(super) fn serve_order<H: SessionHost>(
    host: &H,
    notifier: &Notifier,
    id: RequestId,
    kind: Kind,
    order: Order,
) -> Step {
    if !host.holding() {
        log::info!(
            target: LANDING_LOG_TARGET,
            "order refused, nothing is held: {}",
            kind.word()
        );
        host.refuse(id);
        return Step::Done;
    }
    // A deadline is what a pending entry has: one the budget already answered is
    // gone, and the phone has been told it failed.
    let Some(deadline) = host.expires(id) else {
        log::info!(
            target: LANDING_LOG_TARGET,
            "order refused, entry gone: {}",
            kind.word()
        );
        host.refuse(id);
        return Step::Done;
    };
    // The window drops an order it receives within the margin of the budget:
    // the effect could not be answered before the phone is told it failed.
    let expires = deadline.checked_sub(ORDER_MARGIN).unwrap_or(deadline);
    if !notifier.send(Update::Remote(RemoteUpdate::Order { id, order, expires })) {
        // A window that has closed cannot carry the order out, and leaving the
        // phone to wait out its budget costs it the whole of it.
        log::info!(
            target: LANDING_LOG_TARGET,
            "order refused, no window: {}",
            kind.word()
        );
        host.refuse(id);
        return Step::Done;
    }
    log::info!(
        target: LANDING_LOG_TARGET,
        "order forwarded: {}",
        kind.word()
    );
    Step::Done
}

/// The hand-over path's own log target. It carries decisions, never a URI: which
/// arm refused, whether the player was ready, and what the put answered.
pub(crate) const LANDING_LOG_TARGET: &str = "takofy::bridge";

/// Runs the window with the runtime entered, then teardown, then drops the
/// runtime.
///
/// `window` must return. eframe's `run_and_return` option is what makes it do
/// so, and the runtime is dropped only after teardown has awaited the worker.
pub(crate) fn run_ordered<R, F: Future<Output = ()>>(
    runtime: Runtime,
    window: impl FnOnce() -> R,
    teardown: impl FnOnce() -> F,
) -> R {
    let entered = runtime.enter();
    let result = window();
    drop(entered);
    runtime.block_on(teardown());
    drop(runtime);
    result
}
