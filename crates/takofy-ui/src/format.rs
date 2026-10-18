//! Position and count formatting: the playbar's clock and the hero's byline.

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
