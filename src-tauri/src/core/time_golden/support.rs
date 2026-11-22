//! The only part of the golden tests that names the date-time library.

use crate::core::time::{self, SignedDuration, TimeZone};

pub type Instant = time::Timestamp;

pub fn instant(text: &str) -> Instant {
    time::parse(text).unwrap()
}

/// RFC 3339 in UTC with `Z`, fraction in groups of three digits when nonzero.
pub fn show(at: Instant) -> String {
    at.to_rfc3339().replace("+00:00", "Z")
}

pub fn plus_seconds(at: Instant, seconds: i64) -> Instant {
    at + SignedDuration::from_secs(seconds)
}

pub fn zone(name: &str) -> TimeZone {
    TimeZone::get(name).unwrap()
}
