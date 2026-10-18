//! Position formatting for the playbar.

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
}
