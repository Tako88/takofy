//! The order a show page draws its episodes in.
//!
//! The wire sends a show's own order, and that is what the page draws until the
//! user asks for another one. Both dated orders keep the wire's order wherever
//! the dates cannot decide, so an undated show is never shuffled behind the
//! user's back.

use std::cmp::Reverse;

use takofy_model::{Episode, PublishedAt};

/// How a show page orders the episodes it already holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShowOrder {
    /// The order the wire sent, which is the show's own.
    #[default]
    Wire,
    /// Most recently published first.
    Newest,
    /// Earliest published first.
    Oldest,
}

/// The episodes in `order`, borrowed from the page's own list.
///
/// Undated episodes come last whichever direction is chosen, in the wire's
/// order among themselves: a date the wire did not send is not an early date.
/// The sort is stable, so two episodes published in the same millisecond keep
/// the order the wire gave them.
pub fn ordered(episodes: &[Episode], order: ShowOrder) -> Vec<&Episode> {
    if order == ShowOrder::Wire {
        return episodes.iter().collect();
    }
    let stamp = |index: usize| episodes[index].published.map(PublishedAt::as_millis);
    let (mut dated, undated): (Vec<usize>, Vec<usize>) =
        (0..episodes.len()).partition(|&index| stamp(index).is_some());
    if order == ShowOrder::Newest {
        dated.sort_by_key(|&index| Reverse(stamp(index)));
    } else {
        dated.sort_by_key(|&index| stamp(index));
    }
    dated
        .into_iter()
        .chain(undated)
        .map(|index| &episodes[index])
        .collect()
}

#[cfg(test)]
mod tests {
    use takofy_model::{Episode, PublishedAt};

    use super::*;

    fn episode(uri: &str, published: Option<i64>) -> Episode {
        Episode {
            uri: uri.to_owned(),
            title: uri.to_owned(),
            show: "Lateral".to_owned(),
            show_uri: Some("spotify:show:a".to_owned()),
            description: None,
            published: published.and_then(PublishedAt::from_millis),
            duration_ms: None,
            cover: None,
            named: true,
        }
    }

    /// The URIs `ordered` hands back, in the order it hands them back.
    fn uris(episodes: &[Episode], order: ShowOrder) -> Vec<String> {
        ordered(episodes, order)
            .iter()
            .map(|episode| episode.uri.clone())
            .collect()
    }

    #[test]
    fn wire_order_is_the_list_untouched() {
        let episodes = vec![
            episode("third", None),
            episode("first", Some(300)),
            episode("second", Some(100)),
        ];

        assert_eq!(
            uris(&episodes, ShowOrder::Wire),
            vec!["third", "first", "second"]
        );
    }

    #[test]
    fn newest_first_sorts_by_date_descending() {
        let episodes = vec![
            episode("middle", Some(200)),
            episode("newest", Some(300)),
            episode("oldest", Some(100)),
        ];

        assert_eq!(
            uris(&episodes, ShowOrder::Newest),
            vec!["newest", "middle", "oldest"]
        );
    }

    #[test]
    fn oldest_first_sorts_by_date_ascending() {
        let episodes = vec![
            episode("middle", Some(200)),
            episode("newest", Some(300)),
            episode("oldest", Some(100)),
        ];

        assert_eq!(
            uris(&episodes, ShowOrder::Oldest),
            vec!["oldest", "middle", "newest"]
        );
    }

    #[test]
    fn undated_episodes_keep_their_order_and_come_last_in_both() {
        let episodes = vec![
            episode("undated-second", None),
            episode("dated-old", Some(100)),
            episode("undated-first", None),
            episode("dated-new", Some(300)),
        ];

        assert_eq!(
            uris(&episodes, ShowOrder::Newest),
            vec!["dated-new", "dated-old", "undated-second", "undated-first"]
        );
        assert_eq!(
            uris(&episodes, ShowOrder::Oldest),
            vec!["dated-old", "dated-new", "undated-second", "undated-first"]
        );
    }

    #[test]
    fn two_episodes_at_the_same_instant_keep_their_wire_order() {
        // A long run of ties with other dates scattered through it: short
        // slices are insertion-sorted in place, so only a slice this size shows
        // whether the sort is stable.
        let mut episodes: Vec<Episode> = (0..25)
            .map(|index| episode(&format!("tie-{index:02}"), Some(200)))
            .collect();
        for (at, uri, millis) in [
            (3, "brand-new", 500),
            (9, "later", 300),
            (16, "ancient", 100),
            (22, "earlier", 50),
        ] {
            episodes.insert(at, episode(uri, Some(millis)));
        }

        let ties: Vec<String> = (0..25).map(|index| format!("tie-{index:02}")).collect();
        let mut newest = vec!["brand-new".to_owned(), "later".to_owned()];
        newest.extend(ties.iter().cloned());
        newest.extend(["ancient".to_owned(), "earlier".to_owned()]);
        assert_eq!(uris(&episodes, ShowOrder::Newest), newest);

        let mut oldest = vec!["earlier".to_owned(), "ancient".to_owned()];
        oldest.extend(ties);
        oldest.extend(["later".to_owned(), "brand-new".to_owned()]);
        assert_eq!(uris(&episodes, ShowOrder::Oldest), oldest);
    }

    #[test]
    fn a_one_episode_show_is_unchanged() {
        let dated = vec![episode("only", Some(100))];
        let undated = vec![episode("only", None)];

        for order in [ShowOrder::Wire, ShowOrder::Newest, ShowOrder::Oldest] {
            assert_eq!(uris(&dated, order), vec!["only"]);
            assert_eq!(uris(&undated, order), vec!["only"]);
        }
    }
}
