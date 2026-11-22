//! The only part of the golden tests that names the date-time library.

use chrono::{DateTime, SecondsFormat, Utc};

pub type Instant = DateTime<Utc>;

pub fn instant(text: &str) -> Instant {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

pub fn show(at: Instant) -> String {
    at.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

pub fn plus_seconds(at: Instant, seconds: i64) -> Instant {
    at + chrono::Duration::seconds(seconds)
}

pub fn zone(name: &str) -> chrono_tz::Tz {
    name.parse().unwrap()
}
