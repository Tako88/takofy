//! Home's shelves: the read each ask names, and the answer it lands.

use takofy_model::home::{HomeAnswer, HomeAsk};
use takofy_ui::message::{Failure, Update};
use tokio::sync::oneshot::Receiver as StopReceiver;

use super::errors::{library_failure, related_failure};
use super::log_device_error;
use super::rebuild::Step;
use super::wake::Notifier;
use crate::session::{SessionHost, stop_or};

/// Performs one shelf's read and announces its answer.
///
/// A failure carries the ask it belongs to, so a shelf tells its own failure
/// from another's. A related read is two calls: the artist names the URIs, and
/// the likeness read names the cards.
pub(super) async fn load_home<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    ask: HomeAsk,
) -> Step {
    let asked = ask.clone();
    match ask {
        HomeAsk::Related { artist } => {
            let page = match stop_or(stop, host.artist(&artist)).await {
                None => return Step::Stopped,
                Some(Ok(page)) => page,
                Some(Err(error)) => return failed(notifier, asked, related_failure(&error)),
            };
            match stop_or(stop, host.related(page.artist.related)).await {
                None => Step::Stopped,
                Some(Ok(cards)) => answered(notifier, HomeAnswer::Related { artist, cards }),
                Some(Err(error)) => failed(notifier, asked, related_failure(&error)),
            }
        }
        HomeAsk::Show { show } => match stop_or(stop, host.show(&show)).await {
            None => Step::Stopped,
            Some(Ok(detail)) => answered(notifier, HomeAnswer::Show(detail)),
            Some(Err(error)) => failed(notifier, asked, library_failure(&error)),
        },
        HomeAsk::Station { seed } => match stop_or(stop, host.station(&seed)).await {
            None => Step::Stopped,
            Some(Ok(station)) => answered(notifier, HomeAnswer::Station(station)),
            Some(Err(error)) => failed(notifier, asked, library_failure(&error)),
        },
        HomeAsk::Labels { tracks } => match stop_or(stop, host.release_tracks(&tracks)).await {
            None => Step::Stopped,
            // Every asked track is answered, named or fallen back to its URI:
            // the shelf waits for the whole list before it draws.
            Some(Ok(named)) => answered(
                notifier,
                HomeAnswer::Labelled {
                    asked: tracks,
                    named,
                },
            ),
            Some(Err(error)) => {
                log_device_error(&error);
                failed(notifier, asked, Failure::Load)
            }
        },
    }
}

/// Announces an answer and lets the loop carry on with the next command.
fn answered(notifier: &Notifier, answer: HomeAnswer) -> Step {
    notifier.send(Update::Home(answer));
    Step::Done
}

/// Announces a failure with the ask it belongs to.
fn failed(notifier: &Notifier, ask: HomeAsk, failure: Failure) -> Step {
    notifier.send(Update::HomeFailed { ask, failure });
    Step::Done
}
