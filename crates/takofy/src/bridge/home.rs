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
/// from another's.
pub(super) async fn load_home<H: SessionHost>(
    host: &mut H,
    stop: &mut StopReceiver<()>,
    notifier: &Notifier,
    ask: HomeAsk,
) -> Step {
    let asked = ask.clone();
    match ask {
        // The neighbours come from the station answer, where the artist payload's
        // own related list comes back empty, and their faces from one batched
        // portrait read: a neighbour is an artist the reader does not follow, so
        // nothing else on the page has looked one up.
        HomeAsk::Related { artist } => {
            let mut cards = match stop_or(stop, host.neighbours(&artist)).await {
                None => return Step::Stopped,
                Some(Ok(cards)) => cards,
                Some(Err(error)) => return failed(notifier, asked, related_failure(&error)),
            };
            if !cards.is_empty() {
                let uris: Vec<String> = cards.iter().map(|card| card.uri.clone()).collect();
                // A face is decoration: the read is silent on failure, and the
                // card draws its hole rather than holding the shelf.
                if let Some(faces) = host.portraits(uris).await {
                    for card in &mut cards {
                        if let Some(face) = faces.iter().find(|face| face.artist == card.uri) {
                            card.portrait = face.portrait.clone();
                        }
                    }
                }
            }
            answered(notifier, HomeAnswer::Related { artist, cards })
        }
        HomeAsk::Episodes { shows } => match stop_or(stop, host.episodes(&shows)).await {
            None => Step::Stopped,
            Some(Ok(details)) => answered(notifier, HomeAnswer::Episodes(details)),
            Some(Err(error)) => failed(notifier, asked, library_failure(&error)),
        },
        HomeAsk::Station { seed } => match stop_or(stop, host.station(&seed)).await {
            None => Step::Stopped,
            Some(Ok(station)) => answered(notifier, HomeAnswer::Station(station)),
            Some(Err(error)) => failed(notifier, asked, library_failure(&error)),
        },
        HomeAsk::Portraits { artists } => {
            match stop_or(stop, host.portraits(artists.clone())).await {
                None => Step::Stopped,
                // A read that answered nothing was refused or abandoned, and the
                // window has no way to tell the two apart: both are a failed read.
                Some(Some(found)) => answered(
                    notifier,
                    HomeAnswer::Portraits {
                        asked: artists,
                        found,
                    },
                ),
                Some(None) => failed(notifier, asked, Failure::Load),
            }
        }
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
