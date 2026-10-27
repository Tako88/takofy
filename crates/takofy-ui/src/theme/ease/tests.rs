use super::*;

#[test]
fn the_curve_starts_at_zero_and_ends_at_one() {
    assert_eq!(out(0.0), 0.0);
    assert_eq!(out(1.0), 1.0);
}

#[test]
fn it_never_decreases() {
    let mut previous = out(0.0);
    let mut t = 0.0;
    while t <= 1.0 {
        let value = out(t);
        assert!(value >= previous - 1e-6, "out({t}) went backwards");
        previous = value;
        t += 0.01;
    }
}

#[test]
fn it_is_ahead_of_linear_at_the_midpoint() {
    assert!(out(0.5) > 0.5);
}

#[test]
fn out_of_range_inputs_clamp() {
    assert_eq!(out(-1.0), 0.0);
    assert_eq!(out(2.0), 1.0);
}
