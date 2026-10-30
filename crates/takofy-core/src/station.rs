//! One station, read from its seed.
//!
//! Everything that reads a station and parses it belongs here; the engine
//! reaches it through [`read_station`] alone.

mod art;
mod parse;

use librespot::core::Session;
use takofy_model::FollowedArtist;
use takofy_model::home::{Station, StationSeed};

use crate::context::PageTrack;
use crate::remote::{LabelCache, label_tracks};

/// How many rows a neighbour read asks for.
///
/// None are drawn — the answer's rows are what a station is and this read wants
/// only the artists named beside them — but the count is not asked for as zero:
/// a small answer is what the probe measured the endpoint answering, and one
/// row was never tried.
const NO_ROWS: usize = 20;

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

/// The artists a station names as related to `artist`, for the shelf built from
/// one followed artist.
///
/// Read through the station endpoint rather than the artist payload: the
/// payload's own related list answers empty, and this route carries the names
/// with it, so the shelf needs no second read to label its cards.
pub(crate) async fn read_neighbours(
    session: &Session,
    artist: &str,
) -> Result<Vec<FollowedArtist>, StationError> {
    let answer = session
        .spclient()
        .get_apollo_station("stations", artist, Some(NO_ROWS), Vec::new(), false)
        .await
        .map_err(|error| StationError::Source(error.to_string()))?;
    parse::related(&answer)
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
