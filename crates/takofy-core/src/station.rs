//! One station, read from its seed.
//!
//! Everything that reads a station and parses it belongs here; the engine
//! reaches it through [`read_station`] alone.

use librespot::core::Session;
use takofy_model::home::{Station, StationSeed};

use crate::remote::LabelCache;

/// Why a station read failed.
#[derive(Debug, thiserror::Error)]
pub enum StationError {
    /// The seed names a station this session has no way to read.
    #[error("no station can be read")]
    Unavailable,
}

/// One station and its labelled rows, or [`StationError::Unavailable`].
pub(crate) async fn read_station(
    _session: &Session,
    _labels: &LabelCache,
    _seed: &StationSeed,
) -> Result<Station, StationError> {
    Err(StationError::Unavailable)
}
