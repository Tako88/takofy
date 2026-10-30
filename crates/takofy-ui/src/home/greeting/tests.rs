use super::*;

#[test]
fn every_boundary_has_its_word() {
    for (hour, expected) in [
        (0, "Still up"),
        (4, "Still up"),
        (5, "Good morning"),
        (11, "Good morning"),
        (12, "Good afternoon"),
        (17, "Good afternoon"),
        (18, "Good evening"),
        (23, "Good evening"),
        (24, "Good evening"),
        (255, "Good evening"),
    ] {
        assert_eq!(greeting(hour), expected, "hour {hour}");
    }
}
