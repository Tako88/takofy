//! The reference's wording for the words and numbers the shell prints: the
//! playbar's clock, the hero's byline, the playlist band's description, and the
//! age a playlist row gives its date.

use takofy_model::AddedAt;

/// One day in milliseconds.
const DAY_MS: u64 = 86_400_000;

/// Formats a position as minutes and seconds.
pub fn format_position(position_ms: u32) -> String {
    let seconds = position_ms / 1_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Formats the time left, in the reference's `-m:ss` form.
pub fn format_remaining(position_ms: u32, duration_ms: u32) -> String {
    format!(
        "-{}",
        format_position(duration_ms.saturating_sub(position_ms))
    )
}

/// The song count as the reference writes it: `1 song`, `N songs`.
///
/// Zero is `0 songs`, not a special case.
pub fn count_label(count: usize) -> String {
    if count == 1 {
        "1 song".to_owned()
    } else {
        format!("{count} songs")
    }
}

/// The total runtime as the reference writes it, or `None` when there is no
/// total.
///
/// Past an hour it is `H hr M min`; under one it is `M min`. It rounds to the
/// nearest whole minute, so 59.5 minutes carries into `1 hr 0 min` rather than
/// becoming a 60th minute. A total of exactly zero is the absent total.
pub fn total_label(total_ms: u32) -> Option<String> {
    if total_ms == 0 {
        return None;
    }
    let minutes = (f64::from(total_ms) / 60_000.0).round() as u32;
    if minutes < 60 {
        Some(format!("{minutes} min"))
    } else {
        Some(format!("{} hr {} min", minutes / 60, minutes % 60))
    }
}

/// The hero's byline parts: the song count, then the total when there is one.
///
/// The parts are separate so the painter can place the dot separator between
/// them and leave no trailing dot when the total is absent.
pub fn hero_meta(songs: usize, total_ms: u32) -> Vec<String> {
    let mut parts = vec![count_label(songs)];
    if let Some(total) = total_label(total_ms) {
        parts.push(total);
    }
    parts
}

/// The reference's relative wording for a date, or `None` for a date after
/// `now_ms`.
///
/// A day is a fixed 24-hour span rather than a calendar day: the function has
/// only an instant and a clock, and a local calendar would need a timezone.
/// Months are `days / 30`. The 28–59 band is its own arm so the run after three
/// weeks reads as one month instead of falling through to `0 months ago`.
pub fn added_label(added: AddedAt, now_ms: u64) -> Option<String> {
    let days = now_ms.checked_sub(added.as_millis())? / DAY_MS;
    Some(match days {
        0 => "Today".to_owned(),
        1 => "1 day ago".to_owned(),
        2..=6 => format!("{days} days ago"),
        7..=13 => "1 week ago".to_owned(),
        14..=27 => format!("{} weeks ago", days / 7),
        28..=59 => "1 month ago".to_owned(),
        _ => format!("{} months ago", days / 30),
    })
}

/// The band's byline parts: the owner, then the count and the total.
///
/// The owner is dropped when it is blank, so a playlist whose wire owner is
/// empty reads `62 songs · 3 hr 51 min` rather than `Made by ` with no name.
pub fn byline(owner: Option<&str>, tracks: Option<(usize, u32)>) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some(owner) = owner.map(str::trim).filter(|owner| !owner.is_empty()) {
        parts.push(format!("Made by {owner}"));
    }
    if let Some((songs, total_ms)) = tracks {
        parts.extend(hero_meta(songs, total_ms));
    }
    parts
}

/// The playlist's description as one line: whitespace collapsed and trimmed.
///
/// `None` when nothing is left, so a blank description draws no line at all.
/// This collapses whitespace and nothing else: markup in the wire string — HTML
/// entities such as `&#x27;` or `&amp;`, and `<a href="spotify:…">` tags — is
/// passed through unchanged and draws literally. The name promises no more than
/// that.
pub fn plain_description(raw: &str) -> Option<String> {
    let line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() { None } else { Some(line) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed clock: 2023-11-14T22:13:20Z.
    const NOW: u64 = 1_700_000_000_000;
    const D: u64 = 86_400_000;

    fn at(millis: u64) -> AddedAt {
        AddedAt::from_wire(&millis.to_string()).expect("a date above the floor")
    }

    /// The label for a track `age` milliseconds old at `NOW`.
    fn aged(age: u64) -> Option<String> {
        added_label(at(NOW - age), NOW)
    }

    #[test]
    fn added_label_under_24_hours_is_today() {
        assert_eq!(aged(0).as_deref(), Some("Today"));
        assert_eq!(aged(86_399_999).as_deref(), Some("Today"));
    }

    #[test]
    fn added_label_one_day_is_singular() {
        assert_eq!(aged(D).as_deref(), Some("1 day ago"));
        assert_eq!(aged(2 * D - 1).as_deref(), Some("1 day ago"));
    }

    #[test]
    fn added_label_two_to_six_days_counts_days() {
        assert_eq!(aged(2 * D).as_deref(), Some("2 days ago"));
        assert_eq!(aged(6 * D + 86_399_999).as_deref(), Some("6 days ago"));
    }

    #[test]
    fn added_label_weeks_cover_7_to_27_days() {
        assert_eq!(aged(7 * D).as_deref(), Some("1 week ago"));
        assert_eq!(aged(13 * D).as_deref(), Some("1 week ago"));
        assert_eq!(aged(14 * D).as_deref(), Some("2 weeks ago"));
        assert_eq!(aged(20 * D).as_deref(), Some("2 weeks ago"));
        assert_eq!(aged(21 * D).as_deref(), Some("3 weeks ago"));
        assert_eq!(aged(27 * D).as_deref(), Some("3 weeks ago"));
    }

    #[test]
    fn added_label_28_to_59_days_is_one_month() {
        assert_eq!(aged(28 * D - 1).as_deref(), Some("3 weeks ago"));
        assert_eq!(aged(28 * D).as_deref(), Some("1 month ago"));
        assert_eq!(aged(59 * D).as_deref(), Some("1 month ago"));
    }

    #[test]
    fn added_label_months_are_days_over_30_unbounded() {
        assert_eq!(aged(60 * D).as_deref(), Some("2 months ago"));
        assert_eq!(aged(89 * D).as_deref(), Some("2 months ago"));
        assert_eq!(aged(90 * D).as_deref(), Some("3 months ago"));
        assert_eq!(aged(420 * D).as_deref(), Some("14 months ago"));
        assert_eq!(aged(1200 * D).as_deref(), Some("40 months ago"));
    }

    #[test]
    fn added_label_in_the_future_is_none() {
        assert_eq!(added_label(at(NOW + 1), NOW), None);
        assert_eq!(added_label(at(NOW + 365 * D), NOW), None);
        assert_eq!(added_label(at(NOW), NOW).as_deref(), Some("Today"));
    }

    #[test]
    fn added_label_extremes_do_not_overflow() {
        assert_eq!(added_label(at(u64::MAX), 0), None);
        assert_eq!(
            added_label(at(1_000_000_000_000), u64::MAX).as_deref(),
            Some("7116799025 months ago")
        );
    }

    #[test]
    fn format_remaining_counts_down_and_saturates() {
        assert_eq!(format_remaining(74_000, 222_000), "-2:28");
        assert_eq!(format_remaining(0, 222_000), "-3:42");
        assert_eq!(format_remaining(222_000, 222_000), "-0:00");
        assert_eq!(format_remaining(300_000, 222_000), "-0:00");
        assert_eq!(
            format_remaining(73_250, 222_000),
            "-2:28",
            "sub-second truncates"
        );
    }

    #[test]
    fn a_position_formats_as_minutes_and_seconds() {
        assert_eq!(format_position(0), "0:00");
        assert_eq!(format_position(5_000), "0:05");
        assert_eq!(format_position(65_000), "1:05");
        assert_eq!(format_position(3_600_000), "60:00");
    }

    #[test]
    fn count_label_is_singular_only_for_one() {
        assert_eq!(count_label(1), "1 song");
        assert_eq!(count_label(0), "0 songs");
        assert_eq!(count_label(2), "2 songs");
        assert_eq!(count_label(62), "62 songs");
    }

    #[test]
    fn total_label_reads_hours_and_minutes_past_an_hour() {
        assert_eq!(total_label(13_860_000).as_deref(), Some("3 hr 51 min"));
    }

    #[test]
    fn total_label_reads_minutes_only_under_an_hour() {
        assert_eq!(total_label(2_700_000).as_deref(), Some("45 min"));
    }

    #[test]
    fn total_label_is_none_for_an_absent_total() {
        assert_eq!(total_label(0), None);
    }

    #[test]
    fn total_label_rounds_to_the_nearest_minute_and_carries_into_the_hour() {
        // 59.5 minutes rounds up to the hour, not to a 60th minute.
        assert_eq!(total_label(3_570_000).as_deref(), Some("1 hr 0 min"));
        // 59.4 minutes stays in the minute form.
        assert_eq!(total_label(3_564_000).as_deref(), Some("59 min"));
    }

    #[test]
    fn hero_meta_omits_the_total_when_there_is_none() {
        // One part means the painter draws no trailing dot.
        assert_eq!(hero_meta(62, 0), vec!["62 songs".to_owned()]);
        assert_eq!(hero_meta(1, 0), vec!["1 song".to_owned()]);
    }

    #[test]
    fn hero_meta_puts_the_count_before_the_total() {
        assert_eq!(
            hero_meta(62, 13_860_000),
            vec!["62 songs".to_owned(), "3 hr 51 min".to_owned()]
        );
    }

    #[test]
    fn byline_puts_the_owner_before_the_count_and_the_total() {
        assert_eq!(
            byline(Some("alice"), Some((62, 13_860_000))),
            vec![
                "Made by alice".to_owned(),
                "62 songs".to_owned(),
                "3 hr 51 min".to_owned()
            ]
        );
    }

    #[test]
    fn byline_keeps_the_owner_while_the_tracks_are_unknown() {
        assert_eq!(
            byline(Some("alice"), None),
            vec!["Made by alice".to_owned()]
        );
    }

    #[test]
    fn byline_draws_the_count_alone_when_the_owner_is_absent() {
        assert_eq!(byline(None, Some((1, 0))), vec!["1 song".to_owned()]);
        assert_eq!(byline(None, Some((0, 0))), vec!["0 songs".to_owned()]);
    }

    #[test]
    fn byline_is_empty_without_an_owner_or_tracks() {
        assert!(byline(None, None).is_empty());
    }

    #[test]
    fn byline_drops_a_blank_owner() {
        assert_eq!(
            byline(Some(""), Some((62, 13_860_000))),
            vec!["62 songs".to_owned(), "3 hr 51 min".to_owned()]
        );
        assert_eq!(
            byline(Some("  "), None),
            Vec::<String>::new(),
            "a spaces-only owner is not a name"
        );
    }

    #[test]
    fn plain_description_collapses_whitespace_and_drops_a_blank() {
        assert_eq!(plain_description(""), None);
        assert_eq!(plain_description("  \n "), None);
        assert_eq!(plain_description("a\n\nb   c").as_deref(), Some("a b c"));
        assert_eq!(
            plain_description("  a single line  ").as_deref(),
            Some("a single line")
        );
    }
}
