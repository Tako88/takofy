//! The library rail's filter: the query the head's field keeps, and the rows
//! it leaves.
//!
//! The query belongs to the rail, not to a page: it is unkeyed, so it survives
//! every page change and every reload of any library list, and a redraw while a
//! list is loading changes nothing. Only the field's own clear control, an
//! Escape while the field owns the keyboard, and the create field's opening
//! clear it.
//!
//! Matching is trimmed and case-insensitive over a row's name alone, because the
//! kind line would make "art" match every artist. The answer is the rows' true
//! indices, so a narrowed row still names the playlist it stands for.

/// The rail's query: one string, with no page to belong to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RailFilter {
    text: String,
}

impl RailFilter {
    /// The text the field edits.
    pub fn edit(&mut self) -> &mut String {
        &mut self.text
    }

    /// The trimmed query as it was typed, for the line that names it.
    pub fn query(&self) -> &str {
        self.text.trim()
    }

    /// The needle the rows match, when the query is not blank.
    pub fn needle(&self) -> Option<String> {
        let trimmed = self.text.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_lowercase())
    }

    /// Drops the query, so the whole list shows again.
    pub fn clear(&mut self) {
        self.text.clear();
    }
}

/// The ascending true indices whose name contains `needle`.
///
/// It takes names rather than rows, so it does not depend on any one list's
/// shape: the sidebar answers the four kinds with the same call.
pub fn keep<'a>(names: impl Iterator<Item = &'a str>, needle: &str) -> Vec<usize> {
    names
        .enumerate()
        .filter(|(_, name)| name.to_lowercase().contains(needle))
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests;
