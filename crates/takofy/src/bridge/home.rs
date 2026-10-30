//! Home's shelves: the read each ask names, and the answer it lands.

use takofy_model::home::{HomeAnswer, HomeAsk};
use takofy_ui::message::{Failure, Update};

use super::errors::{library_failure, related_failure};
use super::log_device_error;
use super::wake::Notifier;
use crate::session::SessionHost;

/// Performs one shelf's read and announces its answer.
///
/// A failure carries the ask it belongs to, so a shelf tells its own failure
/// from another's.
pub(super) async fn load_home<H: SessionHost>(host: &H, notifier: &Notifier, ask: HomeAsk) {
    let asked = ask.clone();
    match ask {
        // The neighbours come from the station answer, where the artist payload's
        // own related list comes back empty, and their faces from one batched
        // portrait read: a neighbour is an artist the reader does not follow, so
        // nothing else on the page has looked one up.
        HomeAsk::Related { artist } => {
            let mut cards = match host.neighbours(&artist).await {
                Ok(cards) => cards,
                Err(error) => {
                    failed(notifier, asked, related_failure(&error));
                    return;
                }
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
            answered(notifier, HomeAnswer::Related { artist, cards });
        }
        HomeAsk::Episodes { shows } => match host.episodes(&shows).await {
            Ok(details) => answered(notifier, HomeAnswer::Episodes(details)),
            Err(error) => failed(notifier, asked, library_failure(&error)),
        },
        HomeAsk::Station { seed } => match host.station(&seed).await {
            Ok(station) => answered(notifier, HomeAnswer::Station(station)),
            Err(error) => failed(notifier, asked, library_failure(&error)),
        },
        HomeAsk::Portraits { artists } => match host.portraits(artists.clone()).await {
            // A read that answered nothing was refused or abandoned, and the
            // window has no way to tell the two apart: both are a failed read.
            Some(found) => answered(
                notifier,
                HomeAnswer::Portraits {
                    asked: artists,
                    found,
                },
            ),
            None => failed(notifier, asked, Failure::Load),
        },
        HomeAsk::Labels { tracks } => match host.release_tracks(&tracks).await {
            // Every asked track is answered, named or fallen back to its URI:
            // the shelf waits for the whole list before it draws.
            Ok(named) => answered(
                notifier,
                HomeAnswer::Labelled {
                    asked: tracks,
                    named,
                },
            ),
            Err(error) => {
                log_device_error(&error);
                failed(notifier, asked, Failure::Load)
            }
        },
    }
}

/// Announces an answer and lets the batch carry on with its next read.
fn answered(notifier: &Notifier, answer: HomeAnswer) {
    notifier.send(Update::Home(answer));
}

/// Announces a failure with the ask it belongs to.
fn failed(notifier: &Notifier, ask: HomeAsk, failure: Failure) {
    notifier.send(Update::HomeFailed { ask, failure });
}
