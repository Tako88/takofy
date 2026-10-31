//! One station, read from its seed.
//!
//! Everything that reads a station and parses it belongs here; the engine
//! reaches it through [`read_station`] alone.

mod art;
mod parse;

use librespot::core::Session;
use takofy_model::FollowedArtist;
use takofy_model::TrackEntry;
use takofy_model::home::{SeedKind, Station, StationSeed};
use takofy_model::uri::{self, UriKind};

use crate::context::{PageTrack, first_pages};
use crate::remote::{LabelCache, RemoteContext, label_tracks, read_release_tracks};

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

/// How many pages a radio read walks: the inline page and its one `next`.
///
/// The page after that is dead, so a longer walk would spend a request on
/// nothing (#80).
const RADIO_PAGES: usize = 2;

/// Why a station read failed.
#[derive(Debug, thiserror::Error)]
pub enum StationError {
    /// The request itself failed; the message is the source's.
    #[error("{0}")]
    Source(String),
    /// The answer was not the JSON a station is.
    #[error("the station answer was not a station")]
    Parse,
    /// The seed's URI does not name its kind, so there is no station address to read.
    #[error("the seed names no station to start a radio from")]
    Seed,
    /// The station answered with nothing to play.
    #[error("the station had no rows")]
    Empty,
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

/// The station address a seed's radio is read from, or `None` when its URI
/// does not name the kind the seed says.
pub(crate) fn radio_uri(seed: &StationSeed) -> Option<String> {
    let uri = uri::parse(&seed.uri)?;
    let segment = match (seed.kind, uri.kind) {
        (SeedKind::Song, UriKind::Track) => "track",
        (SeedKind::Playlist, UriKind::Playlist) => "playlist",
        (SeedKind::Album, UriKind::Album) => "album",
        (SeedKind::Artist, UriKind::Artist) => "artist",
        _ => return None,
    };
    // A station is read by one id, and an address carrying colons is not one.
    if uri.id.contains(':') {
        return None;
    }
    Some(format!("spotify:station:{segment}:{}", uri.id))
}

/// A station's rows as the addresses the label read takes, in the station's own
/// order.
///
/// Repeats stay: only the station decides what it plays twice.
pub(crate) fn radio_rows(positions: Vec<PageTrack>) -> Result<Vec<String>, StationError> {
    if positions.is_empty() {
        return Err(StationError::Empty);
    }
    Ok(positions.into_iter().map(|position| position.uri).collect())
}

/// A radio read from one seed, its rows labelled in the station's order.
///
/// The read is a bounded walk of the station context rather than the station
/// endpoint, because the context's pages carry the order the radio plays in.
pub(crate) async fn read_radio(
    session: &Session,
    labels: &LabelCache,
    seed: &StationSeed,
) -> Result<Vec<TrackEntry>, StationError> {
    let uri = radio_uri(seed).ok_or(StationError::Seed)?;
    let positions = first_pages(RemoteContext { session }, &uri, RADIO_PAGES)
        .await
        .map_err(|error| StationError::Source(error.to_string()))?;
    let uris = radio_rows(positions)?;
    Ok(read_release_tracks(session, &uris, labels).await)
}

#[cfg(test)]
mod tests;
