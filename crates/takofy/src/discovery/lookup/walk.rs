//! One ring's walk out from the cards a row already draws.
//!
//! It reads the outside's related lists and asks Spotify only for what resolves.
//! An artist a row holds, the library owns or the reader hid is dropped before
//! an attempt is spent, so a ring that draws nothing means the union ran out.

use std::collections::HashSet;

use takofy_core::ReadFailed;
use takofy_model::home::{DigAsk, DigOutcome};

use super::{Outside, OutsideError, RING, Spotify, anchor_for, neighbours, related, stamp};
use crate::discovery::{Fetch, wire};

/// Walks one ring from `ask`'s frontier and answers what it may append.
pub(crate) async fn walk<S, F>(spotify: &S, outside: &Outside<F>, ask: &DigAsk) -> DigOutcome
where
    S: Spotify,
    F: Fetch + Sync,
{
    if !outside.is_open() {
        return DigOutcome::Off;
    }
    let (_, own) = match anchor_for(spotify, &ask.seed).await {
        Ok(Some(found)) => found,
        // The seed's source is gone, so there is nothing out here to walk from.
        Ok(None) => return empty(),
        Err(ReadFailed) => return DigOutcome::Unreachable,
    };
    let mut skip: HashSet<String> = own.into_iter().collect();
    skip.extend(ask.skip.iter().cloned());

    let mut union: Vec<wire::ArtistHit> = Vec::new();
    let mut failed = 0usize;
    for id in &ask.frontier {
        match related(outside, id.id()).await {
            Ok(list) => union.extend(list),
            Err(OutsideError::Off) => return DigOutcome::Off,
            Err(OutsideError::Backoff) => return DigOutcome::Unreachable,
            Err(OutsideError::Absent) => {}
            Err(OutsideError::Failed) => failed += 1,
        }
    }
    if failed > 0 && failed == ask.frontier.len() {
        return DigOutcome::Unreachable;
    }

    let mut exclude: HashSet<i64> = HashSet::new();
    exclude.extend(ask.frontier.iter().map(|id| id.id()));
    exclude.extend(ask.held.iter().map(|id| id.id()));
    let resolved = match neighbours(spotify, outside, &union, &skip, &exclude, &RING).await {
        Ok(resolved) => resolved,
        Err(OutsideError::Off) => return DigOutcome::Off,
        Err(OutsideError::Backoff | OutsideError::Failed | OutsideError::Absent) => {
            return DigOutcome::Unreachable;
        }
    };

    let cards = if resolved.uris.is_empty() {
        Vec::new()
    } else {
        match spotify.cards(&resolved.uris).await {
            Ok(cards) => stamp(cards, &resolved),
            Err(ReadFailed) => return DigOutcome::Unreachable,
        }
    };
    DigOutcome::Ring {
        cards,
        passed: resolved.passed,
        exhausted: resolved.attempted >= resolved.candidates,
    }
}

/// A ring with nothing in it, which only an absent anchor answers.
fn empty() -> DigOutcome {
    DigOutcome::Ring {
        cards: Vec::new(),
        passed: Vec::new(),
        exhausted: true,
    }
}

#[cfg(test)]
mod tests;
