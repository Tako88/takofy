use super::*;

fn url(tail: &str) -> CoverUrl {
    CoverUrl::from_wire(&format!("https://i.scdn.co/image/{tail}")).expect("a cover url")
}

/// The album sizes: 64, 300 and 640 squares.
fn album() -> CoverSet {
    CoverSet::from_sizes([
        (Some((64, 64)), url("small")),
        (Some((300, 300)), url("default")),
        (Some((640, 640)), url("large")),
    ])
    .expect("a cover set")
}

#[test]
fn a_small_box_takes_the_smallest_variant() {
    let set = album();
    for want in [0, 36, 72, 80, 88, 112, SMALL_MAX_PX] {
        assert_eq!(set.pick(want), Some(&url("small")), "a {want}px box");
    }
}

#[test]
fn a_box_past_the_threshold_takes_the_smallest_covering_variant() {
    let set = album();
    assert_eq!(set.pick(176), Some(&url("default")));
    assert_eq!(set.pick(224), Some(&url("default")));
    assert_eq!(set.pick(300), Some(&url("default")));
    assert_eq!(set.pick(352), Some(&url("large")));
    assert_eq!(set.pick(512), Some(&url("large")));
}

#[test]
fn a_box_past_every_variant_takes_the_largest() {
    assert_eq!(album().pick(4000), Some(&url("large")));
}

#[test]
fn a_single_variant_answers_a_large_box_too() {
    let set = CoverSet::from_sizes([(Some((64, 64)), url("small"))]).expect("a cover set");
    assert_eq!(set.pick(512), Some(&url("small")));
}

#[test]
fn an_unsized_variant_never_wins_while_a_sized_one_exists() {
    let set = CoverSet::from_sizes([(Some((64, 64)), url("small")), (None, url("unsized"))])
        .expect("a cover set");
    for want in [36, SMALL_MAX_PX, 176, 352, 4000] {
        assert_eq!(set.pick(want), Some(&url("small")), "a {want}px box");
    }

    let mixed = CoverSet::from_sizes([
        (Some((64, 64)), url("small")),
        (Some((640, 640)), url("large")),
        (None, url("unsized")),
    ])
    .expect("a cover set");
    assert_eq!(
        mixed.pick(4000),
        Some(&url("large")),
        "the largest sized wins"
    );

    let only_unsized = CoverSet::from_sizes([(None, url("unsized"))]).expect("a cover set");
    assert_eq!(only_unsized.pick(4000), Some(&url("unsized")));
}

#[test]
fn single_answers_any_box_with_its_one_url() {
    let set = CoverSet::single(url("picture"));
    assert_eq!(set.pick(36), Some(&url("picture")));
    assert_eq!(set.pick(4000), Some(&url("picture")));
    assert_eq!(set.variants().len(), 1);
    assert_eq!(set.variants()[0].effective(), None);
}

#[test]
fn no_sizes_makes_no_set() {
    assert_eq!(CoverSet::from_sizes([]), None);
}

#[test]
fn a_zero_dimension_is_dropped() {
    let set = CoverSet::from_sizes([
        (Some((0, 300)), url("zero-width")),
        (Some((300, 0)), url("zero-height")),
        (Some((300, 300)), url("default")),
    ])
    .expect("a cover set");
    assert_eq!(
        set.variants(),
        &[CoverVariant {
            width: Some(300),
            height: Some(300),
            url: url("default"),
        }]
    );
    assert_eq!(CoverSet::from_sizes([(Some((0, 0)), url("zero"))]), None);
}

#[test]
fn duplicate_sizes_keep_the_first() {
    let set = CoverSet::from_sizes([
        (Some((300, 300)), url("first")),
        (Some((300, 300)), url("second")),
    ])
    .expect("a cover set");
    assert_eq!(set.variants().len(), 1);
    assert_eq!(set.variants()[0].url, url("first"));
    assert_eq!(set.pick(224), Some(&url("first")));
}

#[test]
fn a_wide_pair_is_picked_by_its_height() {
    let set = CoverSet::from_sizes([
        (Some((1280, 720)), url("wide")),
        (Some((640, 360)), url("wide-small")),
    ])
    .expect("a cover set");
    assert_eq!(set.pick(352), Some(&url("wide-small")));
    assert_eq!(set.pick(720), Some(&url("wide")));
}

#[test]
fn construction_order_does_not_change_the_pick() {
    let forward = CoverSet::from_sizes([
        (Some((64, 64)), url("small")),
        (Some((300, 300)), url("default")),
        (Some((640, 640)), url("large")),
    ])
    .expect("a cover set");
    let backward = CoverSet::from_sizes([
        (Some((640, 640)), url("large")),
        (Some((64, 64)), url("small")),
        (Some((300, 300)), url("default")),
    ])
    .expect("a cover set");

    for want in [36, SMALL_MAX_PX, 176, 224, 352, 512, 4000] {
        assert_eq!(forward.pick(want), backward.pick(want), "a {want}px box");
    }
    assert_eq!(forward, backward, "the same set, however it was built");
}
