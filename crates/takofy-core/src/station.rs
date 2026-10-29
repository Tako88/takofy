//! One station, read from its seed.
//!
//! Everything that reads a station and parses it belongs here; the engine
//! reaches it through [`read_station`] alone.

mod art;
mod parse;

use librespot::core::Session;
use takofy_model::home::{Station, StationSeed};

use crate::context::PageTrack;
use crate::remote::{LabelCache, label_tracks};

/// How many rows one read asks the station for.
///
/// The band draws three; the rest are what the station's own play queues, so a
/// short answer is a shorter station rather than a second read.
const ROWS: usize = 50;

/// Why a station read failed.
#[derive(Debug, thiserror::Error)]
pub enum StationError {
    /// The request itself failed; the message is the source's.
    #[error("{0}")]
    Source(String),
    /// The answer was not the JSON a station is.
    #[error("the station answer was not a station")]
    Parse,
}

/// One station and its rows, named by this session's label read.
///
/// A row the read did not name keeps what the station said about itself, so a
/// partial miss costs a name and never a row.
pub(crate) async fn read_station(
    session: &Session,
    labels: &LabelCache,
    seed: &StationSeed,
) -> Result<Station, StationError> {
    let answer = session
        .spclient()
        .get_apollo_station("stations", &seed.uri, Some(ROWS), Vec::new(), false)
        .await
        .map_err(|error| StationError::Source(error.to_string()))?;
    let mut station = parse::station(&answer, seed)?;
    if station.rows.is_empty() {
        return Ok(station);
    }
    let positions: Vec<PageTrack> = station
        .rows
        .iter()
        .map(|row| PageTrack {
            uri: row.uri.clone(),
            added_at: None,
        })
        .collect();
    let named = label_tracks(session, positions, labels).await;
    station.rows = parse::merge(station.rows, named);
    Ok(station)
}
