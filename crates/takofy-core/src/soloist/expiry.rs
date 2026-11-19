//! When a Soloist build stops working.
//!
//! Builds expire 90 days after their build date. The `--version` format is not
//! documented, so anything unparseable is `Unknown` and never blocks play; the
//! exit code still does.

/// A build this old has expired.
pub const EXPIRY_DAYS: i64 = 90;
/// A build this close to expiry is reported as expiring.
pub const EXPIRING_WITHIN_DAYS: i64 = 14;
/// The exit code Soloist uses for an expired build.
pub const EXPIRED_EXIT_CODE: i32 = 10;

const DAY_SECONDS: u64 = 86_400;

/// How usable the found build is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Build {
    Fresh,
    Expiring { days_left: i64 },
    Expired,
    Unknown,
}

impl Build {
    /// Whether this build may be started.
    pub fn playable(&self) -> bool {
        !matches!(self, Self::Expired)
    }
}

/// The build time in `--version` output, as unix seconds.
///
/// The first token that is an RFC 3339 timestamp or a large integer wins.
pub fn parse_build(version_output: &str) -> Option<u64> {
    let tokens: Vec<&str> = version_output.split_whitespace().collect();
    for (index, token) in tokens.iter().enumerate() {
        if let Some(build) = parse_token(token) {
            return Some(build);
        }
        // A date and its time can be split by the whitespace above.
        if let Some(next) = tokens.get(index + 1)
            && let Some(build) = parse_token(&format!("{token} {next}"))
        {
            return Some(build);
        }
    }
    None
}

/// Classifies `--version` output against a unix time.
pub fn classify(version_output: &str, now_unix: u64) -> Build {
    match parse_build(version_output) {
        Some(build) => classify_build(build, now_unix),
        None => Build::Unknown,
    }
}

/// Classifies a known build time against a unix time.
pub fn classify_build(build_unix: u64, now_unix: u64) -> Build {
    let age_seconds = now_unix.saturating_sub(build_unix);
    let age_days = i64::try_from(age_seconds / DAY_SECONDS).unwrap_or(i64::MAX);
    let days_left = EXPIRY_DAYS - age_days;
    if days_left <= 0 {
        Build::Expired
    } else if days_left <= EXPIRING_WITHIN_DAYS {
        Build::Expiring { days_left }
    } else {
        Build::Fresh
    }
}

/// Whether the process exit code means the build has expired.
pub fn exit_code_means_expired(code: i32) -> bool {
    code == EXPIRED_EXIT_CODE
}

fn parse_token(token: &str) -> Option<u64> {
    let token = token.trim_matches(|character| {
        matches!(character, '(' | ')' | '[' | ']' | ',' | ';' | '"' | '\'')
    });
    if let Ok(number) = token.parse::<u64>() {
        return (number >= 1_000_000_000).then_some(number);
    }
    parse_datetime(token)
}

fn parse_datetime(text: &str) -> Option<u64> {
    let text = text
        .strip_suffix('Z')
        .or_else(|| text.strip_suffix('z'))
        .unwrap_or(text);
    let (date, time) = text.split_once(['T', ' '])?;

    let mut date = date.split('-');
    let year: i64 = date.next()?.parse().ok()?;
    let month: u32 = date.next()?.parse().ok()?;
    let day: u32 = date.next()?.parse().ok()?;
    if date.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let mut time = time.split(':');
    let hour: u32 = time.next()?.parse().ok()?;
    let minute: u32 = time.next()?.parse().ok()?;
    let second: u32 = time.next()?.split('.').next()?.parse().ok()?;
    if time.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let seconds =
        days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second);
    u64::try_from(seconds).ok()
}

/// Days since 1970-01-01, by Howard Hinnant's civil-calendar formula.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = i64::from((month + 9) % 12);
    let day_of_year = (153 * month_prime + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests;
