//! Plain data shared by the engine and the window.
//!
//! Both `takofy-core` and `takofy-ui` depend on this crate, and it depends on
//! nothing. That is the point: the window can hold what it draws without
//! depending on the crate that talks to Spotify, so "a view cannot reach the
//! network" stays enforced by the compiler.

/// An absolute URL for a cover image, restricted to the hosts Takofy fetches.
///
/// Only the two checked constructors build one, so a URL that reaches the
/// fetcher is one this crate chose rather than one a server supplied.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CoverUrl(String);

/// The only hosts a cover may be fetched from.
const COVER_HOSTS: [&str; 2] = ["i.scdn.co", "pickasso.spotifycdn.com"];

impl CoverUrl {
    /// Builds a CDN URL from a 20-byte image file id.
    ///
    /// Anything that is not exactly 20 bytes with at least one non-zero byte is
    /// `None`: a short id is zero-padded by the wire and is not a real image.
    pub fn from_file_id(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 20 || bytes.iter().all(|byte| *byte == 0) {
            return None;
        }
        use std::fmt::Write as _;
        let mut hex = String::with_capacity(40);
        for byte in bytes {
            let _ = write!(hex, "{byte:02x}");
        }
        Some(Self(format!("https://i.scdn.co/image/{hex}")))
    }

    /// Accepts only an absolute `https://` URL on a known image host.
    ///
    /// Characters that could make the fetching parser disagree about the host
    /// are rejected, so a server-supplied URL can never point somewhere else.
    pub fn from_wire(url: &str) -> Option<Self> {
        let rest = url.strip_prefix("https://")?;
        // A backslash or userinfo makes a lenient parser read a different host.
        if url
            .chars()
            .any(|c| c == '\\' || c == '@' || c.is_ascii_control() || c.is_whitespace())
        {
            return None;
        }
        let authority_end = rest.find(['/', '?', '#'])?;
        if !rest[authority_end..].starts_with('/') {
            return None;
        }
        let host_port = &rest[..authority_end];
        let host = match host_port.rsplit_once(':') {
            Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
                host
            }
            Some(_) => return None,
            None => host_port,
        };
        if !COVER_HOSTS.contains(&host) {
            return None;
        }
        Some(Self(url.to_owned()))
    }

    /// The URL as it will be fetched.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Decoded, tightly packed RGBA8 pixels for one cover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl CoverImage {
    /// Builds an image, rejecting zero dimensions and a mismatched buffer.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))?;
        if rgba.len() != expected {
            return None;
        }
        Some(Self {
            width,
            height,
            rgba,
        })
    }
}

/// A playlist as listed in the rootlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    pub uri: String,
    pub name: String,
    /// `false` when `name` is the URI fallback.
    pub named: bool,
    /// The playlist's cover, once one of the three sources names it.
    pub cover: Option<CoverUrl>,
}

/// A track inside a playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackEntry {
    pub uri: String,
    pub title: String,
    pub artists: String,
    /// The album the track belongs to, when the metadata carried one.
    pub album: Option<String>,
    /// The track's length in milliseconds, when the metadata carried a length.
    pub duration_ms: Option<u32>,
    /// `false` when `title` is the URI fallback.
    pub named: bool,
    /// The track's own album cover, when the metadata carried one.
    pub cover: Option<CoverUrl>,
}

#[cfg(test)]
mod tests {
    use super::{CoverImage, CoverUrl};

    #[test]
    fn a_20_byte_file_id_becomes_a_cdn_url() {
        let id: Vec<u8> = (1..=20).collect();
        let url = CoverUrl::from_file_id(&id).expect("a valid id");
        assert_eq!(
            &url.as_str()["https://i.scdn.co/image/".len()..],
            "0102030405060708090a0b0c0d0e0f1011121314"
        );
        assert_eq!(
            url.as_str().len(),
            "https://i.scdn.co/image/".len() + 40,
            "the hex is forty characters"
        );
    }

    #[test]
    fn a_file_id_containing_a_zero_byte_is_still_a_file_id() {
        // Only an all-zero id is padding; a real id may hold a zero byte.
        let mut id = [1u8; 20];
        id[3] = 0;
        assert!(CoverUrl::from_file_id(&id).is_some());
    }

    #[test]
    fn a_file_id_of_the_wrong_length_is_rejected() {
        assert_eq!(CoverUrl::from_file_id(&[]), None);
        assert_eq!(CoverUrl::from_file_id(&[1; 16]), None);
        assert_eq!(CoverUrl::from_file_id(&[1; 21]), None);
    }

    #[test]
    fn an_all_zero_file_id_is_rejected() {
        assert_eq!(CoverUrl::from_file_id(&[0; 20]), None);
    }

    #[test]
    fn from_wire_accepts_the_known_hosts() {
        for url in [
            "https://i.scdn.co/image/abc",
            "https://pickasso.spotifycdn.com/image/ab67c0de",
        ] {
            assert_eq!(
                CoverUrl::from_wire(url).map(|cover| cover.as_str().to_owned()),
                Some(url.to_owned())
            );
        }
    }

    #[test]
    fn from_wire_rejects_everything_else() {
        for url in [
            "http://i.scdn.co/image/abc",
            "/image/abc",
            "https://i.scdn.co.example.com/image/abc",
            "https://i.scdn.co@evil.example/image/abc",
            "https://evil.example\\@i.scdn.co/x",
            "https://i.scdn.co\\@evil.example/x",
            "https://i.scdn.co\t.evil/x",
            "https://evil.example/i.scdn.co.png",
            "https://i.scdn.co",
        ] {
            assert_eq!(CoverUrl::from_wire(url), None, "must reject {url}");
        }
    }

    #[test]
    fn cover_image_requires_a_positive_shape() {
        assert!(CoverImage::new(2, 2, vec![0; 16]).is_some());
        assert_eq!(CoverImage::new(0, 2, vec![0; 0]), None);
        assert_eq!(CoverImage::new(2, 0, vec![0; 0]), None);
    }

    #[test]
    fn cover_image_rejects_a_mismatched_buffer() {
        assert_eq!(CoverImage::new(2, 2, vec![0; 15]), None);
        assert_eq!(CoverImage::new(2, 2, vec![0; 17]), None);
    }
}
