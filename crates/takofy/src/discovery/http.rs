//! The outside service over HTTPS, with the body and status mapped to
//! [`Fetch`](super::Fetch)'s errors.
//!
//! The client refuses redirects and cleartext, carries no cookie store and has
//! its own small pool. A body is read in chunks under the shared limit, and a
//! 429 carries its `Retry-After` as integer seconds only.

use std::time::Duration;

use super::{Fetch, FetchError, HTTP_TIMEOUT, MAX_BODY};

/// The service's anonymous host.
const BASE: &str = "https://api.deezer.com";

/// The production [`Fetch`].
pub(crate) struct Http {
    client: reqwest::Client,
}

impl Http {
    pub(crate) fn new() -> Option<Self> {
        match reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .https_only(true)
            .user_agent("takofy")
            .pool_max_idle_per_host(2)
            .build()
        {
            Ok(client) => Some(Self { client }),
            Err(error) => {
                log::warn!("outside client could not be built: {error}");
                None
            }
        }
    }

    async fn request(&self, path_and_query: &str) -> Result<Vec<u8>, FetchError> {
        let (path, pairs) = split_query(path_and_query);
        let mut request = self.client.get(format!("{BASE}{path}"));
        if !pairs.is_empty() {
            request = request.query(&pairs);
        }
        let response = request.send().await.map_err(|_| FetchError::Transport)?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.trim().parse::<u64>().ok())
                .map(Duration::from_secs);
            return Err(FetchError::Status(429, retry_after));
        }
        if !status.is_success() {
            return Err(FetchError::Status(status.as_u16(), None));
        }
        let mut response = response;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| FetchError::Transport)? {
            if body.len() + chunk.len() > MAX_BODY {
                return Err(FetchError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

impl Fetch for Http {
    async fn get(&self, path_and_query: &str) -> Result<Vec<u8>, FetchError> {
        self.request(path_and_query).await
    }
}

/// Splits a path-and-query into the path and the pairs `.query()` takes.
fn split_query(path_and_query: &str) -> (&str, Vec<(String, String)>) {
    let Some((path, query)) = path_and_query.split_once('?') else {
        return (path_and_query, Vec::new());
    };
    let pairs = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect();
    (path, pairs)
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let high = hex(bytes[index + 1]);
                let low = hex(bytes[index + 2]);
                match (high, low) {
                    (Some(high), Some(low)) => {
                        decoded.push(high * 16 + low);
                        index += 3;
                    }
                    _ => {
                        decoded.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
