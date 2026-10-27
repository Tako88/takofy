//! The search route: one query, one `spotify:search:` URI.

/// The context URI for one search query, or `None` when the query is blank.
///
/// The walker takes a context URI, not a bare query: the wire's context
/// endpoint routes on the `spotify:search:` authority, and a bare `never+gonna`
/// is not a URI it resolves. So the encoded query is what follows the prefix.
///
/// The measured wire: every rule below comes from these answers rather than a
/// guess about what the server does.
///
/// | sent | result |
/// |---|---|
/// | `AC%2FDC` | 20 tracks, all AC/DC — the server **percent-decodes** |
/// | `100%25` | 20 tracks, all "100%" — `%` must be escaped or the route is a 400 |
/// | `AC/DC` (raw) | 404 |
/// | `100%` (raw) | 400 |
/// | `a:b`, `a%3Ab` | the query arrives as `a` — `:` truncates even percent-encoded |
/// | `what?`, `what%3F`, `a%23b` | truncated the same way |
/// | `c++`, `c%2B%2B` | "c" — `+` decodes to a space, so a literal `+` is unreachable |
/// | `宇多田ヒカル` (raw) | 400 — raw UTF-8 is rejected |
/// | `%E5%AE%87…` (UTF-8 percent-encoded) | 20 tracks, all Hikaru Utada |
/// | `AC DC` | **19** tracks — the count is not always 20 |
///
/// So the rule is:
/// 1. Trim, and collapse runs of whitespace to one space.
/// 2. Anything that is not ASCII alphanumeric, `-`, `_`, `.` or `~` is UTF-8
///    percent-encoded — **except** whitespace, which becomes `+` (the
///    convention, and verified), and `:`, `?`, `#`, `+`, which become a space,
///    because those truncate or vanish whatever we do about them. A separator
///    keeps the words; a silent cut loses them.
/// 3. Re-collapse after the substitutions, and return `None` if nothing is
///    left.
/// 4. Prefix what is left with `spotify:search:`, the context URI a walker
///    resolves.
///
/// The `+`/space substitution is applied before encoding, so a collided `+`
/// cannot be double-encoded.
pub(crate) fn search_uri(query: &str) -> Option<String> {
    // Step 2's substitutions first, so a `+` the user typed is a separator
    // rather than something to encode, and so `a:b` can still yield two words.
    let separated: String = query
        .chars()
        .map(|c| match c {
            ':' | '?' | '#' | '+' => ' ',
            c => c,
        })
        .collect();
    // Steps 1 and 3: runs of whitespace become one space, and the ends go.
    let collapsed = collapse(&separated)?;

    let mut uri = String::with_capacity(collapsed.len());
    for c in collapsed.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            uri.push(c);
        } else if c == ' ' {
            uri.push('+');
        } else {
            let mut buffer = [0_u8; 4];
            for byte in c.encode_utf8(&mut buffer).bytes() {
                uri.push('%');
                uri.push(hex_digit(byte >> 4));
                uri.push(hex_digit(byte & 0xf));
            }
        }
    }
    Some(format!("spotify:search:{uri}"))
}

/// Trims and joins runs of whitespace with one space, or `None` when blank.
fn collapse(raw: &str) -> Option<String> {
    let joined = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// The upper-case hex digit for `nibble`, which is always below 16.
fn hex_digit(nibble: u8) -> char {
    const DIGITS: [char; 16] = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F',
    ];
    DIGITS[usize::from(nibble & 0xf)]
}

#[cfg(test)]
mod tests {
    use super::search_uri;

    /// The route for a query that survives normalisation.
    fn uri(query: &str) -> String {
        search_uri(query).expect("the query is not blank")
    }

    #[test]
    fn whitespace_runs_collapse_to_one_plus() {
        assert_eq!(uri("  never   gonna  "), "spotify:search:never+gonna");
        assert_eq!(uri("never\tgonna"), "spotify:search:never+gonna");
        assert_eq!(uri("never\ngonna"), "spotify:search:never+gonna");
    }

    #[test]
    fn an_empty_or_blank_query_is_none() {
        assert_eq!(search_uri(""), None);
        assert_eq!(search_uri("   "), None);
        assert_eq!(search_uri("\t\n "), None);
    }

    #[test]
    fn the_case_is_left_alone() {
        assert_eq!(uri("NeVeR Gonna"), "spotify:search:NeVeR+Gonna");
    }

    #[test]
    fn a_slash_is_encoded_because_it_is_a_path_separator() {
        assert_eq!(uri("AC/DC"), "spotify:search:AC%2FDC");
    }

    #[test]
    fn a_percent_is_encoded_or_the_route_is_a_bad_request() {
        assert_eq!(uri("100%"), "spotify:search:100%25");
    }

    #[test]
    fn non_ascii_is_percent_encoded_as_utf8() {
        assert_eq!(
            uri("宇多田ヒカル"),
            "spotify:search:%E5%AE%87%E5%A4%9A%E7%94%B0%E3%83%92%E3%82%AB%E3%83%AB"
        );
        assert_eq!(uri("Café Tacvba"), "spotify:search:Caf%C3%A9+Tacvba");
    }

    #[test]
    fn a_colon_becomes_a_space_so_the_words_survive() {
        // The server decodes before it parses, so `a%3Ab` would arrive as `a`.
        assert_eq!(uri("a:b"), "spotify:search:a+b");
        assert_eq!(uri("a # b"), "spotify:search:a+b");
        assert_eq!(uri("what?"), "spotify:search:what");
    }

    #[test]
    fn a_plus_is_a_separator_because_it_cannot_be_a_literal() {
        assert_eq!(uri("c++"), "spotify:search:c");
        assert_eq!(uri("c++ rust"), "spotify:search:c+rust");
    }

    #[test]
    fn the_unreserved_marks_are_kept_as_they_are() {
        assert_eq!(uri("a-b_c.d~e"), "spotify:search:a-b_c.d~e");
    }

    #[test]
    fn a_query_of_separators_only_is_none() {
        assert_eq!(search_uri(":?+#+"), None);
    }
}
