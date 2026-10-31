//! The snapshot file's grammar: one whole copy, or nothing.
//!
//! A cache that cannot be read whole is a miss, never a partial draw, so a line
//! this build cannot read refuses the file and the caller then draws from the
//! network as if no copy existed. Unknown versions and sections, repeated or
//! out-of-order sections, and any escape the grammar does not name all refuse.
//!
//! Tabs separate fields and a string escapes its own, so a name holding a tab
//! cannot forge a field. Every value goes back through the model's checked
//! constructors, so a hand-edited file cannot reach a cover host, an owner name
//! or a date the model would not have allowed from the wire. A declared count is
//! checked against the lines that remain before a record is read, so a file that
//! claims a million rows of a hundred lines is refused rather than allocated
//! for.

use std::fmt::{self, Write as _};

use takofy_model::{
    Account, CoverOrigin, CoverSet, CoverUrl, CoverVariant, FollowedArtist, FollowedShow,
    LikedTrack, Owner, OwnerName, PlaylistCover, PlaylistEntry, SavedAlbum, SavedAt,
};
use takofy_ui::library::Snapshot;

/// The format version this build writes and reads.
pub const VERSION: u32 = 1;

/// The first field of the first line, which is what the file is.
const MAGIC: &str = "takofy-library";

/// A line the grammar refused.
///
/// The text that failed is never kept, so a warning cannot print what the file
/// held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    line: usize,
    reason: Reason,
}

impl Refusal {
    fn new(line: usize, reason: Reason) -> Self {
        Self { line, reason }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.reason)
    }
}

/// Why a line refused the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reason {
    Empty,
    ByteOrderMark,
    CarriageReturn,
    NoFinalNewline,
    BadHeader,
    BadVersion,
    MissingAccount,
    BadAccount,
    Unexpected,
    BadSection,
    UnknownSection,
    DuplicateSection,
    SectionOrder,
    BadCount,
    RecordFields,
    BadBool,
    BadNumber,
    BadDate,
    BadText,
    BadEscape,
    BadCover,
    BadCoverUrl,
    BadSize,
    BadOwner,
    ArtistLists,
    MissingEnd,
    DataAfterEnd,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "the file is empty",
            Self::ByteOrderMark => "the file starts with a byte-order mark",
            Self::CarriageReturn => "the file has a carriage return",
            Self::NoFinalNewline => "the file does not end with a newline",
            Self::BadHeader => "not a takofy library",
            Self::BadVersion => "a version this build does not read",
            Self::MissingAccount => "the account line is missing",
            Self::BadAccount => "the account is not 16 lowercase hex digits",
            Self::Unexpected => "expected a section or `end`",
            Self::BadSection => "a section line is malformed",
            Self::UnknownSection => "an unknown section",
            Self::DuplicateSection => "a section appears twice",
            Self::SectionOrder => "the sections are out of order",
            Self::BadCount => "the record count does not fit the file",
            Self::RecordFields => "a record has the wrong number of fields",
            Self::BadBool => "a flag is not `0` or `1`",
            Self::BadNumber => "a number is not digits",
            Self::BadDate => "a date is not a positive millisecond count",
            Self::BadText => "an optional text is neither `-` nor `+`",
            Self::BadEscape => "a string has an escape this build does not know",
            Self::BadCover => "a cover is not a list of sizes and URLs",
            Self::BadCoverUrl => "a cover URL is not one the model allows",
            Self::BadSize => "a cover size is unusable",
            Self::BadOwner => "an owner is not a name",
            Self::ArtistLists => "an album's artist lists differ in length",
            Self::MissingEnd => "the file ends without `end`",
            Self::DataAfterEnd => "there is data after `end`",
        })
    }
}

/// One of the five lists, in the order the file must hold them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Playlists,
    Shows,
    Artists,
    Albums,
    Liked,
}

impl Section {
    const ALL: [Self; 5] = [
        Self::Playlists,
        Self::Shows,
        Self::Artists,
        Self::Albums,
        Self::Liked,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Playlists => "playlists",
            Self::Shows => "shows",
            Self::Artists => "artists",
            Self::Albums => "albums",
            Self::Liked => "liked",
        }
    }

    /// The position in the fixed order, which is also the bit that marks it seen.
    fn rank(self) -> u8 {
        match self {
            Self::Playlists => 0,
            Self::Shows => 1,
            Self::Artists => 2,
            Self::Albums => 3,
            Self::Liked => 4,
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|section| section.name() == name)
    }
}

/// The file's text for `snapshot`, whole.
///
/// Every list the copy holds is written, in the fixed order; a list it does not
/// hold is left out rather than written empty.
///
/// A cover's variants are written in one canonical order rather than the order
/// the set holds them, so a set read from the wire and the same set read back
/// from this file render byte for byte alike.
pub fn render(snapshot: &Snapshot) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{MAGIC}\t{VERSION}");
    let _ = writeln!(out, "account\t{:016x}", snapshot.account.key());
    if let Some(rows) = &snapshot.playlists {
        section_line(&mut out, Section::Playlists, rows.len());
        for row in rows {
            playlist(&mut out, row);
        }
    }
    if let Some(rows) = &snapshot.shows {
        section_line(&mut out, Section::Shows, rows.len());
        for row in rows {
            show(&mut out, row);
        }
    }
    if let Some(rows) = &snapshot.artists {
        section_line(&mut out, Section::Artists, rows.len());
        for row in rows {
            artist(&mut out, row);
        }
    }
    if let Some(rows) = &snapshot.albums {
        section_line(&mut out, Section::Albums, rows.len());
        for row in rows {
            album(&mut out, row);
        }
    }
    if let Some(rows) = &snapshot.liked {
        section_line(&mut out, Section::Liked, rows.len());
        for row in rows {
            liked(&mut out, row);
        }
    }
    out.push_str("end\n");
    out
}

/// Reads the file's whole text.
pub fn parse(text: &str) -> Result<Snapshot, Refusal> {
    if text.is_empty() {
        return Err(Refusal::new(1, Reason::Empty));
    }
    if text.starts_with('\u{feff}') {
        return Err(Refusal::new(1, Reason::ByteOrderMark));
    }
    if let Some(at) = text.find('\r') {
        return Err(Refusal::new(line_of(text, at), Reason::CarriageReturn));
    }
    let Some(body) = text.strip_suffix('\n') else {
        return Err(Refusal::new(
            text.matches('\n').count() + 1,
            Reason::NoFinalNewline,
        ));
    };
    let mut lines = Lines::new(body);

    let header = lines.next().map(|(_, line)| line).unwrap_or("");
    let version = header
        .strip_prefix(MAGIC)
        .and_then(|rest| rest.strip_prefix('\t'))
        .ok_or_else(|| Refusal::new(1, Reason::BadHeader))?;
    match digits(version).and_then(|text| text.parse::<u32>().ok()) {
        Some(version) if version == VERSION => {}
        _ => return Err(Refusal::new(1, Reason::BadVersion)),
    }

    let (number, line) = lines
        .next()
        .ok_or_else(|| Refusal::new(2, Reason::MissingAccount))?;
    let account = account(line, number)?;

    let mut snapshot = Snapshot::new(account);
    let mut seen = 0u8;
    let mut last = 0u8;
    loop {
        let Some((number, line)) = lines.next() else {
            return Err(Refusal::new(lines.number() + 1, Reason::MissingEnd));
        };
        if line == "end" {
            if let Some((number, _)) = lines.next() {
                return Err(Refusal::new(number, Reason::DataAfterEnd));
            }
            return Ok(snapshot);
        }
        let (section, count) = section(line, number)?;
        let rank = section.rank();
        if seen & (1 << rank) != 0 {
            return Err(Refusal::new(number, Reason::DuplicateSection));
        }
        if rank < last {
            return Err(Refusal::new(number, Reason::SectionOrder));
        }
        seen |= 1 << rank;
        last = rank;
        if count > lines.remaining() {
            return Err(Refusal::new(number, Reason::BadCount));
        }
        match section {
            Section::Playlists => {
                snapshot.playlists = Some(records(&mut lines, count, playlist_record)?);
            }
            Section::Shows => {
                snapshot.shows = Some(records(&mut lines, count, show_record)?);
            }
            Section::Artists => {
                snapshot.artists = Some(records(&mut lines, count, artist_record)?);
            }
            Section::Albums => {
                snapshot.albums = Some(records(&mut lines, count, album_record)?);
            }
            Section::Liked => {
                snapshot.liked = Some(records(&mut lines, count, liked_record)?);
            }
        }
    }
}

/// The 1-based line holding the byte at `at`.
fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// The text's lines, numbered, with the final newline already stripped.
struct Lines<'a> {
    rest: std::str::Split<'a, char>,
    number: usize,
}

impl<'a> Lines<'a> {
    fn new(body: &'a str) -> Self {
        Self {
            rest: body.split('\n'),
            number: 0,
        }
    }

    fn next(&mut self) -> Option<(usize, &'a str)> {
        let line = self.rest.next()?;
        self.number += 1;
        Some((self.number, line))
    }

    /// The number of the line last handed out.
    fn number(&self) -> usize {
        self.number
    }

    /// How many lines are left, which is the most records a count may name.
    fn remaining(&self) -> usize {
        self.rest.clone().count()
    }
}

/// Reads `count` record lines through `read`.
fn records<'a, T>(
    lines: &mut Lines<'a>,
    count: usize,
    read: fn(&str) -> Result<T, Reason>,
) -> Result<Vec<T>, Refusal> {
    let mut rows = Vec::new();
    for _ in 0..count {
        let Some((number, line)) = lines.next() else {
            return Err(Refusal::new(lines.number() + 1, Reason::BadCount));
        };
        rows.push(read(line).map_err(|reason| Refusal::new(number, reason))?);
    }
    Ok(rows)
}

/// Reads a section line into the section it names and its record count.
fn section(line: &str, number: usize) -> Result<(Section, usize), Refusal> {
    let mut fields = line.split('\t');
    if fields.next() != Some("section") {
        return Err(Refusal::new(number, Reason::Unexpected));
    }
    let (Some(name), Some(count)) = (fields.next(), fields.next()) else {
        return Err(Refusal::new(number, Reason::BadSection));
    };
    if fields.next().is_some() {
        return Err(Refusal::new(number, Reason::BadSection));
    }
    let section =
        Section::from_name(name).ok_or_else(|| Refusal::new(number, Reason::UnknownSection))?;
    let count = digits(count)
        .and_then(|text| text.parse::<usize>().ok())
        .ok_or_else(|| Refusal::new(number, Reason::BadCount))?;
    Ok((section, count))
}

/// Reads the account line.
fn account(line: &str, number: usize) -> Result<Account, Refusal> {
    let mut fields = line.split('\t');
    if fields.next() != Some("account") {
        return Err(Refusal::new(number, Reason::MissingAccount));
    }
    let bad = || Refusal::new(number, Reason::BadAccount);
    let hex = fields.next().ok_or_else(bad)?;
    if fields.next().is_some()
        || hex.len() != 16
        || !hex
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(bad());
    }
    let key = u64::from_str_radix(hex, 16).map_err(|_| bad())?;
    Ok(Account::from_key(key))
}

/// The fields of one record line.
struct Fields<'a> {
    fields: std::str::Split<'a, char>,
}

impl<'a> Fields<'a> {
    fn new(line: &'a str) -> Self {
        Self {
            fields: line.split('\t'),
        }
    }

    fn raw(&mut self) -> Result<&'a str, Reason> {
        self.fields.next().ok_or(Reason::RecordFields)
    }

    /// Refuses the record when a field is left over.
    fn end(&mut self) -> Result<(), Reason> {
        if self.fields.next().is_some() {
            return Err(Reason::RecordFields);
        }
        Ok(())
    }

    fn text(&mut self) -> Result<String, Reason> {
        unescape(self.raw()?).ok_or(Reason::BadEscape)
    }

    fn flag(&mut self) -> Result<bool, Reason> {
        match self.raw()? {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(Reason::BadBool),
        }
    }

    fn count(&mut self) -> Result<usize, Reason> {
        let text = self.raw()?;
        digits(text)
            .and_then(|text| text.parse().ok())
            .ok_or(Reason::BadNumber)
    }

    /// `-`, or a digit count.
    fn optional<T: std::str::FromStr>(&mut self) -> Result<Option<T>, Reason> {
        let text = self.raw()?;
        if text == "-" {
            return Ok(None);
        }
        digits(text)
            .and_then(|text| text.parse().ok())
            .map(Some)
            .ok_or(Reason::BadNumber)
    }

    /// `-`, or `+` and the escaped text.
    fn optional_text(&mut self) -> Result<Option<String>, Reason> {
        let text = self.raw()?;
        if text == "-" {
            return Ok(None);
        }
        let rest = text.strip_prefix('+').ok_or(Reason::BadText)?;
        unescape(rest).map(Some).ok_or(Reason::BadEscape)
    }

    fn optional_saved(&mut self) -> Result<Option<SavedAt>, Reason> {
        let text = self.raw()?;
        if text == "-" {
            return Ok(None);
        }
        let millis = digits(text)
            .and_then(|text| text.parse::<u64>().ok())
            .ok_or(Reason::BadNumber)?;
        let millis = i64::try_from(millis).map_err(|_| Reason::BadDate)?;
        SavedAt::from_millis(millis)
            .map(Some)
            .ok_or(Reason::BadDate)
    }

    fn cover(&mut self) -> Result<Option<CoverSet>, Reason> {
        cover_set(self.raw()?)
    }

    fn playlist_cover(&mut self) -> Result<Option<PlaylistCover>, Reason> {
        let text = self.raw()?;
        if text == "-" {
            return Ok(None);
        }
        let (origin, rest) = if let Some(rest) = text.strip_prefix("chosen:") {
            (CoverOrigin::Chosen, rest)
        } else if let Some(rest) = text.strip_prefix("derived:") {
            (CoverOrigin::Derived, rest)
        } else {
            return Err(Reason::BadCover);
        };
        let set = cover_set(rest)?.ok_or(Reason::BadCover)?;
        Ok(Some(PlaylistCover { set, origin }))
    }

    fn owner(&mut self) -> Result<Option<Owner>, Reason> {
        let text = self.raw()?;
        if text == "-" {
            return Ok(None);
        }
        if text == "you" {
            return Ok(Some(Owner::You));
        }
        let rest = text.strip_prefix("named:").ok_or(Reason::BadOwner)?;
        let name = unescape(rest).ok_or(Reason::BadEscape)?;
        OwnerName::from_wire(&name)
            .map(|name| Some(Owner::Named(name)))
            .ok_or(Reason::BadOwner)
    }
}

fn playlist_record(line: &str) -> Result<PlaylistEntry, Reason> {
    let mut fields = Fields::new(line);
    let uri = fields.text()?;
    let name = fields.text()?;
    let named = fields.flag()?;
    let cover = fields.playlist_cover()?;
    let composed_count = fields.count()?;
    let mut composed = Vec::new();
    for _ in 0..composed_count {
        composed.push(fields.cover()?.ok_or(Reason::BadCover)?);
    }
    let description = fields.optional_text()?;
    let tracks = fields.optional::<usize>()?;
    let owner = fields.owner()?;
    let saved_at = fields.optional_saved()?;
    fields.end()?;
    Ok(PlaylistEntry {
        uri,
        name,
        named,
        cover,
        composed,
        description,
        tracks,
        owner,
        access: None,
        saved_at,
    })
}

fn show_record(line: &str) -> Result<FollowedShow, Reason> {
    let mut fields = Fields::new(line);
    let uri = fields.text()?;
    let name = fields.text()?;
    let named = fields.flag()?;
    let publisher = fields.optional_text()?;
    let episodes = fields.optional::<usize>()?;
    let cover = fields.cover()?;
    fields.end()?;
    Ok(FollowedShow {
        uri,
        name,
        named,
        publisher,
        episodes,
        cover,
    })
}

fn artist_record(line: &str) -> Result<FollowedArtist, Reason> {
    let mut fields = Fields::new(line);
    let uri = fields.text()?;
    let name = fields.text()?;
    let named = fields.flag()?;
    let portrait = fields.cover()?;
    let releases = fields.optional::<u32>()?;
    fields.end()?;
    Ok(FollowedArtist {
        uri,
        name,
        named,
        portrait,
        releases,
    })
}

fn album_record(line: &str) -> Result<SavedAlbum, Reason> {
    let mut fields = Fields::new(line);
    let uri = fields.text()?;
    let name = fields.text()?;
    let named = fields.flag()?;
    let artists = fields.text()?;
    let uris_count = fields.count()?;
    let mut artist_uris = Vec::new();
    for _ in 0..uris_count {
        artist_uris.push(fields.text()?);
    }
    let names_count = fields.count()?;
    let mut artist_names = Vec::new();
    for _ in 0..names_count {
        artist_names.push(fields.text()?);
    }
    if artist_uris.len() != artist_names.len() {
        return Err(Reason::ArtistLists);
    }
    let year = fields.optional::<u16>()?;
    let cover = fields.cover()?;
    let tracks = fields.optional::<u32>()?;
    let saved_at = fields.optional_saved()?;
    fields.end()?;
    Ok(SavedAlbum {
        uri,
        name,
        named,
        artists,
        artist_uris,
        artist_names,
        year,
        cover,
        tracks,
        saved_at,
    })
}

fn liked_record(line: &str) -> Result<LikedTrack, Reason> {
    let mut fields = Fields::new(line);
    let uri = fields.text()?;
    let saved_at = fields.optional_saved()?;
    fields.end()?;
    Ok(LikedTrack { uri, saved_at })
}

/// A cover field: `-`, or space-separated `WxH@url` and `-@url`.
fn cover_set(text: &str) -> Result<Option<CoverSet>, Reason> {
    if text == "-" {
        return Ok(None);
    }
    let mut sizes = Vec::new();
    for variant in text.split(' ') {
        let (size, url) = variant.split_once('@').ok_or(Reason::BadCover)?;
        let url = CoverUrl::from_wire(url).ok_or(Reason::BadCoverUrl)?;
        let size = if size == "-" {
            None
        } else {
            let (width, height) = size.split_once('x').ok_or(Reason::BadCover)?;
            let width = digits(width)
                .and_then(|text| text.parse::<u32>().ok())
                .ok_or(Reason::BadNumber)?;
            let height = digits(height)
                .and_then(|text| text.parse::<u32>().ok())
                .ok_or(Reason::BadNumber)?;
            if width == 0 || height == 0 {
                return Err(Reason::BadSize);
            }
            Some((width, height))
        };
        sizes.push((size, url));
    }
    CoverSet::from_sizes(sizes).map(Some).ok_or(Reason::BadSize)
}

/// The text, or `None` when it is not digits at all.
fn digits(text: &str) -> Option<&str> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(text)
}

/// The text with every reserved character escaped.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

/// The text an escape spells, or `None` for an escape the grammar has no word for.
fn unescape(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        out.push(match characters.next()? {
            '\\' => '\\',
            't' => '\t',
            'n' => '\n',
            'r' => '\r',
            _ => return None,
        });
    }
    Some(out)
}

fn section_line(out: &mut String, section: Section, count: usize) {
    let _ = writeln!(out, "section\t{}\t{count}", section.name());
}

fn flag(out: &mut String, value: bool) {
    out.push(if value { '1' } else { '0' });
}

/// A variant's place in the written order: sized ones by width then height, an
/// unsized one last, and the URL breaking a tie.
fn variant_key(variant: &CoverVariant) -> (u8, u32, u32, &str) {
    match (variant.width, variant.height) {
        (Some(width), Some(height)) => (0, width, height, variant.url.as_str()),
        _ => (1, 0, 0, variant.url.as_str()),
    }
}

fn cover_field(out: &mut String, set: &CoverSet) {
    let mut variants: Vec<&CoverVariant> = set.variants().iter().collect();
    variants.sort_by_key(|variant| variant_key(variant));
    variants.dedup_by_key(|variant| variant_key(variant));
    for (index, variant) in variants.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        match (variant.width, variant.height) {
            (Some(width), Some(height)) => {
                let _ = write!(out, "{width}x{height}");
            }
            _ => out.push('-'),
        }
        out.push('@');
        out.push_str(variant.url.as_str());
    }
}

fn optional_text(out: &mut String, text: Option<&str>) {
    match text {
        Some(text) => {
            out.push('+');
            out.push_str(&escape(text));
        }
        None => out.push('-'),
    }
}

fn optional_number<T: fmt::Display>(out: &mut String, number: Option<T>) {
    match number {
        Some(number) => {
            let _ = write!(out, "{number}");
        }
        None => out.push('-'),
    }
}

fn optional_saved(out: &mut String, saved: Option<SavedAt>) {
    match saved {
        Some(saved) => {
            let _ = write!(out, "{}", saved.as_millis());
        }
        None => out.push('-'),
    }
}

fn playlist(out: &mut String, row: &PlaylistEntry) {
    out.push_str(&escape(&row.uri));
    out.push('\t');
    out.push_str(&escape(&row.name));
    out.push('\t');
    flag(out, row.named);
    out.push('\t');
    match &row.cover {
        Some(picture) => {
            out.push_str(match picture.origin {
                CoverOrigin::Chosen => "chosen:",
                CoverOrigin::Derived => "derived:",
            });
            cover_field(out, &picture.set);
        }
        None => out.push('-'),
    }
    out.push('\t');
    let _ = write!(out, "{}", row.composed.len());
    for composed in &row.composed {
        out.push('\t');
        cover_field(out, composed);
    }
    out.push('\t');
    optional_text(out, row.description.as_deref());
    out.push('\t');
    optional_number(out, row.tracks);
    out.push('\t');
    match &row.owner {
        Some(Owner::You) => out.push_str("you"),
        Some(Owner::Named(name)) => {
            out.push_str("named:");
            out.push_str(&escape(name.as_str()));
        }
        None => out.push('-'),
    }
    out.push('\t');
    optional_saved(out, row.saved_at);
    out.push('\n');
}

fn show(out: &mut String, row: &FollowedShow) {
    out.push_str(&escape(&row.uri));
    out.push('\t');
    out.push_str(&escape(&row.name));
    out.push('\t');
    flag(out, row.named);
    out.push('\t');
    optional_text(out, row.publisher.as_deref());
    out.push('\t');
    optional_number(out, row.episodes);
    out.push('\t');
    match &row.cover {
        Some(picture) => cover_field(out, picture),
        None => out.push('-'),
    }
    out.push('\n');
}

fn artist(out: &mut String, row: &FollowedArtist) {
    out.push_str(&escape(&row.uri));
    out.push('\t');
    out.push_str(&escape(&row.name));
    out.push('\t');
    flag(out, row.named);
    out.push('\t');
    match &row.portrait {
        Some(portrait) => cover_field(out, portrait),
        None => out.push('-'),
    }
    out.push('\t');
    optional_number(out, row.releases);
    out.push('\n');
}

fn album(out: &mut String, row: &SavedAlbum) {
    out.push_str(&escape(&row.uri));
    out.push('\t');
    out.push_str(&escape(&row.name));
    out.push('\t');
    flag(out, row.named);
    out.push('\t');
    out.push_str(&escape(&row.artists));
    out.push('\t');
    let _ = write!(out, "{}", row.artist_uris.len());
    for uri in &row.artist_uris {
        out.push('\t');
        out.push_str(&escape(uri));
    }
    out.push('\t');
    let _ = write!(out, "{}", row.artist_names.len());
    for name in &row.artist_names {
        out.push('\t');
        out.push_str(&escape(name));
    }
    out.push('\t');
    optional_number(out, row.year);
    out.push('\t');
    match &row.cover {
        Some(picture) => cover_field(out, picture),
        None => out.push('-'),
    }
    out.push('\t');
    optional_number(out, row.tracks);
    out.push('\t');
    optional_saved(out, row.saved_at);
    out.push('\n');
}

fn liked(out: &mut String, row: &LikedTrack) {
    out.push_str(&escape(&row.uri));
    out.push('\t');
    optional_saved(out, row.saved_at);
    out.push('\n');
}

#[cfg(test)]
mod tests;
