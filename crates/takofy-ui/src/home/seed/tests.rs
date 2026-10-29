use super::*;

#[test]
fn a_held_seed_draws_the_same_shelf() {
    let seed = Seed::for_open(1_700_000_000_000, 3);
    let draw = || {
        let mut state = seed.state();
        (0..8).map(|_| below(&mut state, 24)).collect::<Vec<_>>()
    };
    assert_eq!(draw(), draw());
}

#[test]
fn a_known_open_pins_the_seed() {
    assert_eq!(
        Seed::for_open(1_700_000_000_000, 1),
        Seed(5_782_050_450_661_280_561)
    );
}

#[test]
fn a_later_clock_draws_another_seed() {
    assert_ne!(
        Seed::for_open(1_700_000_000_000, 1),
        Seed::for_open(1_700_000_000_001, 1)
    );
}

#[test]
fn another_open_in_the_same_millisecond_draws_another_seed() {
    assert_ne!(
        Seed::for_open(1_700_000_000_000, 1),
        Seed::for_open(1_700_000_000_000, 2)
    );
}

#[test]
fn below_stays_inside_its_range() {
    let mut state = Seed::for_open(1_700_000_000_000, 1).state();
    for n in 1..=40 {
        for _ in 0..20 {
            assert!(below(&mut state, n) < n, "n = {n}");
        }
    }
}

#[test]
fn below_of_nothing_is_zero() {
    let mut state = Seed::for_open(1, 1).state();
    assert_eq!(below(&mut state, 0), 0);
    assert_eq!(
        state,
        Seed::for_open(1, 1).state(),
        "an empty range draws nothing"
    );
}
