use super::super::*;
use super::test_support::*;

#[test]
fn tab_order_is_play_shuffle_heart_more() {
    let ctx = context();
    let (_, expected) = frame_probed(&ctx, VIEWPORT, 0.0, Vec::new(), None, |ui| {
        vec![
            controls::slot_id(ui, Control::HeroPlay, ()),
            controls::slot_id(ui, Control::HeroShuffle, ()),
            controls::slot_id(ui, Control::HeroLike, ()),
            controls::slot_id(ui, Control::HeroMore, ()),
        ]
    });

    let [press, release] = crate::view::test_support::tab();
    let mut reached = Vec::new();
    for _ in 0..expected.len() {
        let _ = frame(&ctx, 0.0, vec![press.clone()]);
        reached.push(ctx.memory(|memory| memory.focused()));
        let _ = frame(&ctx, 0.0, vec![release.clone()]);
    }
    let reached: Vec<&str> = reached
        .iter()
        .map(|id| match *id {
            None => "nothing",
            Some(id) if id == expected[0] => "play",
            Some(id) if id == expected[1] => "shuffle",
            Some(id) if id == expected[2] => "heart",
            Some(id) if id == expected[3] => "more",
            Some(_) => "something else",
        })
        .collect();
    assert_eq!(reached, ["play", "shuffle", "heart", "more"]);
}
#[test]
fn tab_order_with_a_slot_is_play_follow_shuffle_more() {
    let ctx = context();
    let text = text_bar(kind(VIEWPORT));
    let (_, expected) = frame_of_probed(&ctx, &text, VIEWPORT, 0.0, Vec::new(), None, |ui| {
        vec![
            controls::slot_id(ui, Control::HeroPlay, ()),
            controls::slot_id(ui, Control::Follow, ()),
            controls::slot_id(ui, Control::HeroShuffle, ()),
            controls::slot_id(ui, Control::HeroMore, ()),
        ]
    });

    let [press, release] = crate::view::test_support::tab();
    let mut reached = Vec::new();
    for _ in 0..expected.len() {
        let _ = frame_of(&ctx, &text, VIEWPORT, 0.0, vec![press.clone()], None);
        reached.push(ctx.memory(|memory| memory.focused()));
        let _ = frame_of(&ctx, &text, VIEWPORT, 0.0, vec![release.clone()], None);
    }
    let reached: Vec<&str> = reached
        .iter()
        .map(|id| match *id {
            None => "nothing",
            Some(id) if id == expected[0] => "play",
            Some(id) if id == expected[1] => "follow",
            Some(id) if id == expected[2] => "shuffle",
            Some(id) if id == expected[3] => "more",
            Some(_) => "something else",
        })
        .collect();
    assert_eq!(
        reached,
        ["play", "follow", "shuffle", "more"],
        "the word is a Tab stop between the play tile and the tools"
    );
}
