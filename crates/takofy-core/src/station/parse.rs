//! A station answer's JSON, read into the model's [`Station`].
//!
//! Pure: the bytes of one `radio-apollo` answer in, the station they name out.
//! The JSON is read by the small reader at the bottom rather than by a crate of
//! its own, because this is the one answer in the workspace that is JSON and one
//! field list does not warrant a dependency for it.

use takofy_model::TrackEntry;
use takofy_model::home::{Station, StationSeed};

use super::StationError;
use super::art::{art_of, cover_of};

/// The wire's track prefix. A station may list other things, and only a track is
/// a row the band can draw and play.
const TRACK: &str = "spotify:track:";

/// The station an answer names, read for `seed`.
///
/// The answer's own URI is what makes it a station: a body without one is
/// refused rather than drawn under the seed's name.
pub(super) fn station(bytes: &[u8], seed: &StationSeed) -> Result<Station, StationError> {
    let text = std::str::from_utf8(bytes).map_err(|_| StationError::Parse)?;
    let answer = json::value(text).ok_or(StationError::Parse)?;
    let uri = json::field(&answer, "uri")
        .and_then(json::text)
        .ok_or(StationError::Parse)?;
    Ok(Station {
        seed: seed.clone(),
        uri: uri.to_owned(),
        title: title_of(&answer),
        subtitle: subtitle_of(&answer),
        art: json::field(&answer, "imageUri")
            .and_then(json::text)
            .and_then(art_of),
        rows: rows_of(&answer),
    })
}

/// The label read's rows where it named one, and the answer's own row where it
/// did not.
///
/// The lists are read by position, so a label can never name a row the station
/// did not list, and a row the label read missed keeps drawing.
pub(super) fn merge(fallback: Vec<TrackEntry>, named: Vec<TrackEntry>) -> Vec<TrackEntry> {
    fallback
        .into_iter()
        .enumerate()
        .map(|(position, row)| match named.get(position) {
            Some(label) if label.named => label.clone(),
            Some(_) | None => row,
        })
        .collect()
}

/// The station's own name, which is the answer's `title`.
fn title_of(answer: &json::Json) -> String {
    json::field(answer, "title")
        .and_then(json::text)
        .map(str::trim)
        .unwrap_or_default()
        .to_owned()
}

/// The station's own subtitle: the answer's `subtitle` when it is not blank,
/// else the first `subtitles[].name` that is not.
///
/// A station that names neither has none, so the head reads the absent answer
/// rather than an empty word.
fn subtitle_of(answer: &json::Json) -> Option<String> {
    let named = |value: Option<&json::Json>| {
        value
            .and_then(json::text)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    if let Some(subtitle) = named(json::field(answer, "subtitle")) {
        return Some(subtitle);
    }
    let json::Json::List(subtitles) = json::field(answer, "subtitles")? else {
        return None;
    };
    subtitles
        .iter()
        .find_map(|subtitle| named(json::field(subtitle, "name")))
}

/// The rows the station lists that are tracks, in the answer's order.
fn rows_of(answer: &json::Json) -> Vec<TrackEntry> {
    let Some(json::Json::List(rows)) = json::field(answer, "tracks") else {
        return Vec::new();
    };
    rows.iter().filter_map(row).collect()
}

/// One listed track as the row the band draws, from the answer's own fields.
///
/// This is the fallback a missed label leaves in place: the station's own title,
/// artists, album and picture are what the row has before the label read names
/// it.
fn row(listed: &json::Json) -> Option<TrackEntry> {
    let uri = json::field(listed, "uri").and_then(json::text)?;
    if !uri.starts_with(TRACK) {
        return None;
    }
    let metadata = json::field(listed, "metadata");
    let said = |key: &str| {
        metadata
            .and_then(|metadata| json::field(metadata, key))
            .and_then(json::text)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let title = json::field(listed, "name")
        .and_then(json::text)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .or_else(|| said("title"))
        .unwrap_or(uri);
    let (artist_names, artist_uris) = credits(metadata);
    Some(TrackEntry {
        uri: uri.to_owned(),
        title: title.to_owned(),
        artists: artist_names.join(", "),
        album: said("album_title").map(str::to_owned),
        album_uri: json::field(listed, "album_uri")
            .and_then(json::text)
            .filter(|album| takofy_model::parse(album).is_some())
            .map(str::to_owned),
        artist_uris,
        artist_names,
        added_at: None,
        duration_ms: None,
        named: title != uri,
        cover: said("image_url").and_then(cover_of),
    })
}

/// The artists one row credits, as names and URIs in the answer's own order.
///
/// The wire numbers the second and later artists, and both lists stop at the
/// first one it leaves unnamed or unaddressable, so a name is never paired with
/// another artist's id.
fn credits(metadata: Option<&json::Json>) -> (Vec<String>, Vec<String>) {
    let mut names = Vec::new();
    let mut uris = Vec::new();
    for index in 0.. {
        let suffix = match index {
            0 => String::new(),
            index => format!(":{index}"),
        };
        let said = |prefix: &str| {
            metadata
                .and_then(|metadata| json::field(metadata, &format!("{prefix}{suffix}")))
                .and_then(json::text)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        };
        let (Some(name), Some(uri)) = (said("artist_name"), said("artist_uri")) else {
            break;
        };
        if takofy_model::parse(uri).is_none() {
            break;
        }
        names.push(name.to_owned());
        uris.push(uri.to_owned());
    }
    (names, uris)
}

/// A JSON value, as much of one as a station answer uses.
mod json {
    /// One value of a JSON body.
    ///
    /// Fields keep the answer's own order and are searched by name: an object
    /// this big is cheaper to scan than to hash, and the order is what makes two
    /// reads of the same body compare equal.
    #[derive(Debug, Clone, PartialEq)]
    pub(super) enum Json {
        Null,
        Bool(bool),
        Number(f64),
        Text(String),
        List(Vec<Self>),
        Map(Vec<(String, Self)>),
    }

    /// The value `text` spells, or `None` when it is not one JSON value.
    pub(super) fn value(text: &str) -> Option<Json> {
        let mut reader = Reader {
            bytes: text.as_bytes(),
            at: 0,
        };
        let value = reader.value(0)?;
        reader.space();
        (reader.at == reader.bytes.len()).then_some(value)
    }

    /// The value `key` names in `json`'s object.
    pub(super) fn field<'a>(json: &'a Json, key: &str) -> Option<&'a Json> {
        let Json::Map(fields) = json else {
            return None;
        };
        fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// The text `json` is, when it is text.
    pub(super) fn text(json: &Json) -> Option<&str> {
        match json {
            Json::Text(text) => Some(text),
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::List(_) | Json::Map(_) => None,
        }
    }

    /// How deep a body may nest before it is not a station answer: the deepest
    /// one nests four, and the cap keeps a hostile body off the stack.
    const DEPTH: usize = 32;

    struct Reader<'a> {
        bytes: &'a [u8],
        at: usize,
    }

    impl Reader<'_> {
        fn space(&mut self) {
            while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                self.at += 1;
            }
        }

        fn eat(&mut self, byte: u8) -> bool {
            if self.bytes.get(self.at) == Some(&byte) {
                self.at += 1;
                return true;
            }
            false
        }

        fn word(&mut self, word: &str) -> bool {
            if self.bytes[self.at..].starts_with(word.as_bytes()) {
                self.at += word.len();
                return true;
            }
            false
        }

        fn value(&mut self, depth: usize) -> Option<Json> {
            if depth > DEPTH {
                return None;
            }
            self.space();
            match *self.bytes.get(self.at)? {
                b'{' => self.object(depth),
                b'[' => self.list(depth),
                b'"' => self.string().map(Json::Text),
                b't' => self.word("true").then_some(Json::Bool(true)),
                b'f' => self.word("false").then_some(Json::Bool(false)),
                b'n' => self.word("null").then_some(Json::Null),
                _ => self.number(),
            }
        }

        fn object(&mut self, depth: usize) -> Option<Json> {
            if !self.eat(b'{') {
                return None;
            }
            let mut fields = Vec::new();
            self.space();
            if self.eat(b'}') {
                return Some(Json::Map(fields));
            }
            loop {
                self.space();
                let name = self.string()?;
                self.space();
                if !self.eat(b':') {
                    return None;
                }
                let value = self.value(depth + 1)?;
                fields.push((name, value));
                self.space();
                if self.eat(b',') {
                    continue;
                }
                return self.eat(b'}').then_some(Json::Map(fields));
            }
        }

        fn list(&mut self, depth: usize) -> Option<Json> {
            if !self.eat(b'[') {
                return None;
            }
            let mut items = Vec::new();
            self.space();
            if self.eat(b']') {
                return Some(Json::List(items));
            }
            loop {
                items.push(self.value(depth + 1)?);
                self.space();
                if self.eat(b',') {
                    continue;
                }
                return self.eat(b']').then_some(Json::List(items));
            }
        }

        fn string(&mut self) -> Option<String> {
            if !self.eat(b'"') {
                return None;
            }
            let mut out: Vec<u8> = Vec::new();
            loop {
                let byte = *self.bytes.get(self.at)?;
                self.at += 1;
                match byte {
                    b'"' => return String::from_utf8(out).ok(),
                    b'\\' => self.escape(&mut out)?,
                    // A raw control byte is not text.
                    byte if byte < 0x20 => return None,
                    byte => out.push(byte),
                }
            }
        }

        /// One escape sequence, appended to `out`.
        ///
        /// The escaped byte itself decides the length, so a lone `\` at the end
        /// of the body is no string at all.
        fn escape(&mut self, out: &mut Vec<u8>) -> Option<()> {
            match *self.bytes.get(self.at)? {
                b'"' | b'\\' | b'/' => out.push(self.bytes[self.at]),
                b'b' => out.push(0x08),
                b'f' => out.push(0x0c),
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'u' => {
                    self.at += 1;
                    let character = self.character()?;
                    let mut buffer = [0u8; 4];
                    out.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                    return Some(());
                }
                _ => return None,
            }
            self.at += 1;
            Some(())
        }

        /// One `\u` escape, pairing a surrogate with the low half that follows it.
        fn character(&mut self) -> Option<char> {
            let high = self.hex4()?;
            let scalar = if (0xd800..0xdc00).contains(&high) {
                if !self.word("\\u") {
                    return None;
                }
                let low = self.hex4()?;
                if !(0xdc00..0xe000).contains(&low) {
                    return None;
                }
                0x1_0000 + ((high - 0xd800) << 10) + (low - 0xdc00)
            } else {
                high
            };
            char::from_u32(scalar)
        }

        fn hex4(&mut self) -> Option<u32> {
            let digits = self.bytes.get(self.at..self.at + 4)?;
            self.at += 4;
            let mut value = 0;
            for digit in digits {
                value = value * 16 + char::from(*digit).to_digit(16)?;
            }
            Some(value)
        }

        fn number(&mut self) -> Option<Json> {
            let start = self.at;
            while matches!(
                self.bytes.get(self.at),
                Some(b'0'..=b'9' | b'+' | b'-' | b'.' | b'e' | b'E')
            ) {
                self.at += 1;
            }
            let text = std::str::from_utf8(self.bytes.get(start..self.at)?).ok()?;
            text.parse::<f64>().ok().map(Json::Number)
        }
    }
}

#[cfg(test)]
mod tests;
