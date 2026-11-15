//! The outside service's bodies, read into the few shapes the lookup needs.
//!
//! Every endpoint answers a `data` array. A body that is not JSON, carries an
//! `error` object, or has no `data` array is an error; an empty `data` array is
//! a valid empty list. The service answers HTTP 200 with an error object, so a
//! status is never evidence that a body answered.

use serde_json::Value;

/// A body that did not carry the shape the endpoint promises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WireError;

/// One artist from a search or a related list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArtistHit {
    pub id: i64,
    pub name: String,
}

/// One album from an artist's release list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AlbumHit {
    pub title: String,
    pub genre_id: Option<i64>,
    pub release_date: Option<String>,
}

/// The service's genre names, in the order the table gave them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GenreTable {
    entries: Vec<(i64, String)>,
}

impl GenreTable {
    pub(crate) fn from_entries(entries: Vec<(i64, String)>) -> Self {
        Self { entries }
    }

    pub(crate) fn entries(&self) -> &[(i64, String)] {
        &self.entries
    }

    pub(crate) fn name_at(&self, index: usize) -> Option<&str> {
        self.entries.get(index).map(|(_, name)| name.as_str())
    }

    pub(crate) fn index_of(&self, id: i64) -> Option<usize> {
        self.entries.iter().position(|(entry, _)| *entry == id)
    }
}

/// What a 200 body's error object means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyError {
    /// The quota ran out, so the outside has to pause.
    Quota,
    /// The record is not there, which no pause will change.
    Absent,
}

/// The service's quota error code.
const QUOTA_CODE: i64 = 4;

/// Whether a 200 body is the service's error object.
///
/// Every code but the quota's is "no such record": a removed artist id answers
/// one, so pausing the whole outside over it would stall every row.
pub(crate) fn body_error(body: &[u8]) -> Option<BodyError> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let error = value.as_object()?.get("error")?.as_object()?;
    Some(match error.get("code").and_then(Value::as_i64) {
        Some(QUOTA_CODE) => BodyError::Quota,
        _ => BodyError::Absent,
    })
}

pub(crate) fn search_artists(body: &[u8]) -> Result<Vec<ArtistHit>, WireError> {
    artists(body)
}

pub(crate) fn related_artists(body: &[u8]) -> Result<Vec<ArtistHit>, WireError> {
    artists(body)
}

pub(crate) fn top_titles(body: &[u8]) -> Result<Vec<String>, WireError> {
    let entries = data(body)?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            let title = entry.get("title")?.as_str()?.trim().to_owned();
            (!title.is_empty()).then_some(title)
        })
        .collect())
}

pub(crate) fn albums(body: &[u8]) -> Result<Vec<AlbumHit>, WireError> {
    let entries = data(body)?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            let title = entry.get("title")?.as_str()?.trim().to_owned();
            if title.is_empty() {
                return None;
            }
            Some(AlbumHit {
                title,
                genre_id: entry.get("genre_id").and_then(Value::as_i64),
                release_date: entry
                    .get("release_date")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect())
}

pub(crate) fn genre_table(body: &[u8]) -> Result<GenreTable, WireError> {
    let records = data(body)?;
    let entries = records
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_i64()?;
            let name = entry.get("name")?.as_str()?.trim().to_owned();
            (!name.is_empty()).then_some((id, name))
        })
        .collect();
    Ok(GenreTable::from_entries(entries))
}

fn artists(body: &[u8]) -> Result<Vec<ArtistHit>, WireError> {
    let entries = data(body)?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_i64()?;
            let name = entry.get("name")?.as_str()?.trim().to_owned();
            (!name.is_empty()).then_some(ArtistHit { id, name })
        })
        .collect())
}

fn data(body: &[u8]) -> Result<Vec<Value>, WireError> {
    let mut value: Value = serde_json::from_slice(body).map_err(|_| WireError)?;
    let object = value.as_object_mut().ok_or(WireError)?;
    if object.contains_key("error") {
        return Err(WireError);
    }
    match object.remove("data") {
        Some(Value::Array(entries)) => Ok(entries),
        _ => Err(WireError),
    }
}

#[cfg(test)]
mod tests;
