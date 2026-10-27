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
