//! Which account a saved library belongs to, as a fingerprint.

/// A stable 64-bit fingerprint of the account's name.
///
/// A cached library is written per account, and this is how a later session
/// tells whose copy it is reading. It is a hand-written FNV-1a hash rather than
/// [`std::hash::DefaultHasher`], which promises nothing about its output across
/// releases: a copy would then belong to no account after an upgrade.
///
/// The hash is not secrecy. An account name is guessable and the file is
/// readable by whoever runs the app; the point is only that the plain name is
/// never written, and never reaches a log line.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Account(u64);

/// FNV-1a's offset basis.
const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a's prime.
const PRIME: u64 = 0x0000_0100_0000_01b3;

impl Account {
    /// Reads a wire account name, trimmed.
    ///
    /// A name blank after trimming names no account, so nothing may be saved
    /// under it.
    pub fn from_wire(name: &str) -> Option<Self> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        Some(Self(fingerprint(name)))
    }

    /// The fingerprint, as the cache file's header holds it.
    pub fn key(&self) -> u64 {
        self.0
    }

    /// Reads a fingerprint already computed, as a cache header carries one.
    pub fn from_key(key: u64) -> Self {
        Self(key)
    }
}

fn fingerprint(name: &str) -> u64 {
    let mut hash = OFFSET;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Account(..)")
    }
}

#[cfg(test)]
mod tests;
