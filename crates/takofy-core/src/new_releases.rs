//! The release shelf's pure half: an album answer dated, and the releases of a
//! followed artist that fall inside the shelf's window.
//!
//! Pure: protobuf messages and parsed metadata in, plain data out. The requests
//! themselves are in `remote::read_new_releases`.
//!
//! This does not reuse [`takofy_model::Release`]: that drops the month and the
//! day and drags in a track list the shelf never draws.

use std::collections::HashSet;

use librespot::metadata::Artist;
use librespot::protocol::metadata::Album as AlbumMessage;
use librespot::protocol::metadata::Date as DateMessage;
use protobuf::Message as _;
use takofy_model::home::{Day, NewRelease, ReleaseDate};
use takofy_model::{CoverSet, ReleaseKind};

use crate::artist::kind_of;
use crate::cover::covers_in_group;

/// The day number of 1970-01-01 in the count [`Day`] keeps from 0001-01-01.
const EPOCH_DAY: i32 = 719_163;

/// One release as the album answer carries it, dated to whatever the wire gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DatedAlbum {
    pub(crate) uri: String,
    pub(crate) name: String,
    pub(crate) kind: ReleaseKind,
    /// `None` when the wire dated the release with no usable year, which is a
    /// release the shelf cannot place.
    pub(crate) date: Option<ReleaseDate>,
    pub(crate) cover: Option<CoverSet>,
}

/// One release, from the URI the answer echoed and the album it carried.
///
/// The name falls back to the URI exactly as [`crate::artist::release_of`] does.
pub(crate) fn dated_album_of(uri: &str, album: &AlbumMessage) -> DatedAlbum {
    DatedAlbum {
        uri: uri.to_owned(),
        name: album
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| uri.to_owned()),
        kind: kind_of(album.type_()),
        date: date_of(album.date.as_ref()),
        cover: covers_in_group(album.cover_group.as_ref()),
    }
}

/// The release one encoded `ALBUM_V4` answer carries.
///
/// Shaped as the seam `remote::fetch_chunked_sized` takes, like
/// [`crate::discovery::album_from`]: the echoed URI names the release.
pub(crate) fn dated_album_from(bytes: &[u8], echoed: &str) -> Option<DatedAlbum> {
    let album = AlbumMessage::parse_from_bytes(bytes).ok()?;
    Some(dated_album_of(echoed, &album))
}

/// The date parts the wire gave, or `None` when it gave no usable year.
///
/// A part the wire left out is `None` rather than a zero the page would have to
/// test for. A day with no month cannot be placed, so it is dropped with it.
pub(crate) fn date_of(date: Option<&DateMessage>) -> Option<ReleaseDate> {
    let date = date?;
    let year = u16::try_from(date.year()).ok().filter(|year| *year != 0)?;
    let month = u8::try_from(date.month())
        .ok()
        .filter(|month| (1..=12).contains(month));
    let day = month
        .and_then(|_| u8::try_from(date.day()).ok())
        .filter(|day| (1..=31).contains(day));
    Some(ReleaseDate { year, month, day })
}

/// The day number `date` names, or `None` when it names no whole day.
pub(crate) fn civil_day(date: &ReleaseDate) -> Option<Day> {
    let (month, day) = (date.month?, date.day?);
    if day == 0 || day > days_in_month(i32::from(date.year), month) {
        return None;
    }
    Some(Day::from_days(
        EPOCH_DAY + days_from_civil(i32::from(date.year), month, day),
    ))
}

/// How many days `month` of `year` has.
fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days from 1970-01-01 to `year`-`month`-`day`, by Hinnant's civil calendar
/// algorithm, so no calendar table is kept here.
fn days_from_civil(year: i32, month: u8, day: u8) -> i32 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted = i32::from((month + 9) % 12);
    let day_of_year = (153 * shifted + 2) / 5 + i32::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The URIs of an artist's current albums and singles, deduped.
///
/// Compilations and appearances are not the artist's own new releases, so they
/// are left out rather than dated and drawn.
pub(crate) fn new_release_uris(artist: &Artist) -> Vec<String> {
    let mut seen = HashSet::new();
    artist
        .albums_current()
        .chain(artist.singles_current())
        .filter_map(|uri| uri.to_uri().ok())
        .filter(|uri| seen.insert(uri.clone()))
        .collect()
}

/// One release as the shelf holds it, or `None` when the shelf cannot place it.
///
/// Only albums, EPs and singles are new releases; a compilation is a regrouping
/// and an audiobook is not one. A release dated by its year alone names no day,
/// so it never reaches the shelf.
pub(crate) fn new_release_of(artist: &str, album: &DatedAlbum, since: Day) -> Option<NewRelease> {
    if !matches!(
        album.kind,
        ReleaseKind::Album | ReleaseKind::Ep | ReleaseKind::Single
    ) {
        return None;
    }
    let date = album.date?;
    let day = civil_day(&date)?;
    if day.days() < since.days() {
        return None;
    }
    Some(NewRelease {
        artist: artist.to_owned(),
        uri: album.uri.clone(),
        name: album.name.clone(),
        date,
        day,
        cover: album.cover.clone(),
    })
}

#[cfg(test)]
mod tests;
