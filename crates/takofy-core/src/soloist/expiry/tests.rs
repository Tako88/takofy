use super::*;

#[test]
fn an_rfc3339_timestamp_parses() {
    assert_eq!(
        parse_build("soloist 1.2.3 (2026-01-02T03:04:05Z) linux x86_64"),
        Some(1_767_323_045)
    );
    assert_eq!(parse_build("2024-02-29T00:00:00Z"), Some(1_709_164_800));
    assert_eq!(parse_build("1970-01-01T00:00:00Z"), Some(0));
}

#[test]
fn a_space_between_date_and_time_parses() {
    assert_eq!(
        parse_build("built 2026-01-02 03:04:05"),
        Some(1_767_323_045)
    );
}

#[test]
fn a_large_integer_parses_as_unix_seconds() {
    assert_eq!(parse_build("version 1770000000"), Some(1_770_000_000));
}

#[test]
fn garbage_and_small_numbers_are_unknown() {
    assert_eq!(parse_build("soloist 1.2.3 linux x86_64"), None);
    assert_eq!(parse_build("2026-13-40T99:99:99Z"), None);
    assert_eq!(parse_build("42"), None);
    assert_eq!(parse_build(""), None);
}

#[test]
fn a_leap_day_is_counted() {
    assert_eq!(
        parse_build("2024-03-01T00:00:00Z"),
        Some(1_709_251_200),
        "the day after a leap day is one ordinary day later"
    );
}

#[test]
fn freshness_is_measured_from_the_build_time() {
    let build = 1_700_000_000;
    assert_eq!(classify_build(build, build), Build::Fresh);
    assert_eq!(
        classify_build(build, build + 75 * DAY_SECONDS),
        Build::Fresh
    );
}

#[test]
fn the_expiring_boundary_is_fourteen_days_left() {
    let build = 1_700_000_000;
    assert_eq!(
        classify_build(build, build + 76 * DAY_SECONDS),
        Build::Expiring { days_left: 14 }
    );
    assert_eq!(
        classify_build(build, build + 89 * DAY_SECONDS),
        Build::Expiring { days_left: 1 }
    );
}

#[test]
fn the_expiry_boundary_is_ninety_days() {
    let build = 1_700_000_000;
    assert_eq!(
        classify_build(build, build + 90 * DAY_SECONDS),
        Build::Expired
    );
    assert_eq!(
        classify_build(build, build + 200 * DAY_SECONDS),
        Build::Expired
    );
}

#[test]
fn unparseable_output_is_unknown_and_still_playable() {
    let build = classify("no date here", 1_700_000_000);
    assert_eq!(build, Build::Unknown);
    assert!(build.playable());
}

#[test]
fn only_exit_code_ten_means_expired() {
    assert!(exit_code_means_expired(10));
    for code in [0, 1, 2, 9, 11, 255] {
        assert!(!exit_code_means_expired(code), "{code}");
    }
}
