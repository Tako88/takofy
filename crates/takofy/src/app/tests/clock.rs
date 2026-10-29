use super::super::*;

use chrono::{FixedOffset, Utc};

/// An instant in a zone, built from a date the test pins.
fn at(hour: u32, minute: u32, offset_hours: i32) -> DateTime<FixedOffset> {
    let offset = FixedOffset::east_opt(offset_hours * 3_600).expect("a whole-hour zone");
    Utc.with_ymd_and_hms(2024, 3, 1, hour, minute, 0)
        .single()
        .expect("a real instant")
        .with_timezone(&offset)
}

#[test]
fn local_hour_reads_the_zone_and_not_utc() {
    assert_eq!(local_hour_at(&at(23, 30, 0)), 23);
    assert_eq!(
        local_hour_at(&at(23, 30, 2)),
        1,
        "a zone ahead of UTC crosses midnight with it"
    );
    assert_eq!(local_hour_at(&at(23, 30, -5)), 18);
}

#[test]
fn local_hour_behind_utc_reads_the_previous_day() {
    assert_eq!(local_hour_at(&at(0, 30, -1)), 23);
    assert_eq!(local_hour_at(&at(0, 30, 0)), 0);
}
