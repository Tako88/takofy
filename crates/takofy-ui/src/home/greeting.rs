//! The clock's greeting, in the reference's four buckets.

/// The greeting for a local hour, 0 through 23.
///
/// Any hour past 23 is an evening too, so a clock that answers something else
/// still has a word.
pub fn greeting(hour: u8) -> &'static str {
    match hour {
        0..=4 => "Still up",
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}

#[cfg(test)]
mod tests;
