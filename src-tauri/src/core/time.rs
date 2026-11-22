//! Instants, durations and local time, in the exact text formats Margin has
//! always read and written (with chrono until v0.11; `fixtures/golden` pins
//! them).
//!
//! jiff supplies the clock, time zones and calendar dates. A [`Timestamp`]
//! holds what chrono's `DateTime<Utc>` held: years far past 9999 and leap
//! seconds, which a `jiff::Timestamp` cannot. Providers and old files can send
//! both, and Margin compares, subtracts and writes them back as it always did.
//!
//! Text timestamps follow RFC 3339 as chrono read it: a four-digit year, `T`,
//! `t` or a space between date and time, an optional fraction of any length
//! (digits past the ninth ignored), a leap second `60`, and `Z`, `z` or
//! `±HH:MM` up to 23:59, with `−` (U+2212) accepted as minus.

pub use jiff::civil::{Date, DateTime, Weekday};
pub use jiff::tz::{Offset, TimeZone};
pub use jiff::SignedDuration;

use jiff::tz::AmbiguousOffset;
use std::ops::{Add, RangeInclusive, Sub};

const NANOS: u32 = 1_000_000_000;
const DAY: i64 = 86_400;
/// The years chrono's dates covered.
const YEARS: RangeInclusive<i64> = -262_143..=262_142;

/// A point in time to the nanosecond, as chrono's `DateTime<Utc>` held it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp {
    /// Seconds since 1970-01-01T00:00:00Z; a leap second counts as the second before it.
    secs: i64,
    /// Nanoseconds into `secs`, past 999,999,999 only during a leap second.
    frac: u32,
}

fn first_second() -> i64 {
    days_from_civil(*YEARS.start(), 1, 1) * DAY
}

fn last_second() -> i64 {
    days_from_civil(*YEARS.end(), 12, 31) * DAY + DAY - 1
}

impl Timestamp {
    fn new(secs: i64, frac: u32) -> Option<Self> {
        (first_second()..=last_second())
            .contains(&secs)
            .then_some(Self { secs, frac })
    }

    pub fn now() -> Self {
        Self::from(jiff::Timestamp::now())
    }

    pub fn from_unix_seconds(seconds: i64) -> Option<Self> {
        Self::new(seconds, 0)
    }

    pub fn from_unix_millis(millis: i64) -> Option<Self> {
        Self::new(
            millis.div_euclid(1000),
            u32::try_from(millis.rem_euclid(1000)).ok()? * 1_000_000,
        )
    }

    pub fn unix_nanos(self) -> i128 {
        i128::from(self.secs) * 1_000_000_000 + i128::from(self.frac)
    }

    pub fn unix_millis(self) -> i64 {
        self.secs * 1000 + i64::from(self.frac / 1_000_000)
    }

    /// `self + duration`, or `None` past chrono's range.
    pub fn checked_add(self, duration: SignedDuration) -> Option<Self> {
        let day = self.secs.div_euclid(DAY);
        let mut secs = self.secs.rem_euclid(DAY);
        let mut frac = i64::from(self.frac);
        let (add_secs, add_frac) = (duration.as_secs(), i64::from(duration.subsec_nanos()));
        // Leaving a leap second drops it, as chrono's `NaiveTime` does.
        if frac >= i64::from(NANOS) {
            if add_secs > 0 || (add_frac > 0 && frac + add_frac >= 2 * i64::from(NANOS)) {
                frac -= i64::from(NANOS);
            } else if add_secs < 0 {
                frac -= i64::from(NANOS);
                secs += 1;
            } else {
                return Some(Self {
                    secs: self.secs,
                    frac: u32::try_from(frac + add_frac).ok()?,
                });
            }
        }
        let mut secs = secs.checked_add(add_secs)?;
        frac += add_frac;
        if frac < 0 {
            frac += i64::from(NANOS);
            secs -= 1;
        } else if frac >= i64::from(NANOS) {
            frac -= i64::from(NANOS);
            secs += 1;
        }
        let day = day.checked_add(secs.div_euclid(DAY))?;
        Self::new(
            day.checked_mul(DAY)?.checked_add(secs.rem_euclid(DAY))?,
            u32::try_from(frac).ok()?,
        )
    }

    pub fn checked_sub(self, duration: SignedDuration) -> Option<Self> {
        self.checked_add(duration.checked_neg()?)
    }

    /// The span from `earlier` to `self`, counting leap seconds as chrono did.
    pub fn since(self, earlier: Self) -> SignedDuration {
        let days = self.secs.div_euclid(DAY) - earlier.secs.div_euclid(DAY);
        let (later_secs, earlier_secs) = (self.secs.rem_euclid(DAY), earlier.secs.rem_euclid(DAY));
        let mut secs = later_secs - earlier_secs;
        if later_secs > earlier_secs && earlier.frac >= NANOS {
            secs += 1;
        } else if later_secs < earlier_secs && self.frac >= NANOS {
            secs -= 1;
        }
        let frac = i64::from(self.frac) - i64::from(earlier.frac);
        SignedDuration::new(
            days * DAY + secs + frac.div_euclid(i64::from(NANOS)),
            i32::try_from(frac.rem_euclid(i64::from(NANOS))).unwrap_or_default(),
        )
    }

    /// The UTC year.
    pub fn year(self) -> i64 {
        Civil::new(self.secs, self.frac, 0).year
    }

    fn utc(self) -> Civil {
        Civil::new(self.secs, self.frac, 0)
    }

    /// Wall-clock fields of `self` in `zone`.
    pub fn civil(self, zone: &TimeZone) -> Civil {
        Civil::new(
            self.secs,
            self.frac,
            i64::from(zone.to_offset(self.timestamp()).seconds()),
        )
    }

    /// The calendar date of `self` in `zone`, held at jiff's range.
    pub fn date(self, zone: &TimeZone) -> Date {
        zone.to_datetime(self.timestamp()).date()
    }

    /// The nearest jiff timestamp, a leap second read as the nanosecond before it.
    fn timestamp(self) -> jiff::Timestamp {
        let (min, max) = (jiff::Timestamp::MIN, jiff::Timestamp::MAX);
        if self.secs < min.as_second() {
            return min;
        }
        if self.secs > max.as_second() {
            return max;
        }
        let frac = i32::try_from(self.frac.min(NANOS - 1)).unwrap_or_default();
        jiff::Timestamp::new(self.secs, frac).unwrap_or(if self.secs < 0 { min } else { max })
    }

    /// `2026-10-16T12:00:00.250+00:00`: fraction in groups of three digits, only when nonzero.
    pub fn to_rfc3339(self) -> String {
        let at = self.utc();
        let nanos = at.nanos;
        let fraction = if nanos == 0 {
            String::new()
        } else if nanos.is_multiple_of(1_000_000) {
            format!(".{:03}", nanos / 1_000_000)
        } else if nanos.is_multiple_of(1_000) {
            format!(".{:06}", nanos / 1_000)
        } else {
            format!(".{nanos:09}")
        };
        format!("{}{fraction}+00:00", at.clock())
    }

    /// [`Self::to_rfc3339`] at a fixed offset, such as `2026-10-16T17:30:00+05:30`.
    #[cfg(test)]
    pub fn to_rfc3339_at(self, offset_seconds: i32) -> String {
        let offset = i64::from(offset_seconds);
        let utc = self.to_rfc3339();
        let at = Civil::new(self.secs, self.frac, offset);
        let fraction = &utc[utc.find('.').unwrap_or(utc.len() - 6)..utc.len() - 6];
        let sign = if offset < 0 { '-' } else { '+' };
        let minutes = offset.abs() / 60;
        format!(
            "{}{fraction}{sign}{:02}:{:02}",
            at.clock(),
            minutes / 60,
            minutes % 60
        )
    }

    /// `2026-10-16T12:00:00Z`, fraction dropped.
    pub fn to_rfc3339_secs(self) -> String {
        format!("{}Z", self.utc().clock())
    }

    /// `20261016T120000Z`, for file names.
    pub fn compact(self) -> String {
        let at = self.utc();
        format!(
            "{}{:02}{:02}T{:02}{:02}{:02}Z",
            year(at.year),
            at.month,
            at.day,
            at.hour,
            at.minute,
            at.second
        )
    }

    /// `20261016T120000.250Z`, for file names.
    pub fn compact_millis(self) -> String {
        let mut stamp = self.compact();
        stamp.insert_str(
            stamp.len() - 1,
            &format!(".{:03}", self.utc().nanos / 1_000_000),
        );
        stamp
    }
}

/// RFC 3339, as [`parse`] reads it.
impl std::str::FromStr for Timestamp {
    type Err = InvalidTime;

    fn from_str(text: &str) -> Result<Self, InvalidTime> {
        parse(text).ok_or(InvalidTime)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidTime;

impl From<jiff::Timestamp> for Timestamp {
    fn from(at: jiff::Timestamp) -> Self {
        Self {
            secs: at.as_second(),
            frac: at.subsec_nanosecond().unsigned_abs(),
        }
        .normalized(at.subsec_nanosecond() < 0)
    }
}

impl Timestamp {
    /// jiff keeps a negative fraction before 1970; chrono counts it forward from the second.
    fn normalized(self, negative: bool) -> Self {
        if negative {
            Self {
                secs: self.secs - 1,
                frac: NANOS - self.frac,
            }
        } else {
            self
        }
    }
}

/// `self + duration`; panics past chrono's range, as chrono did.
impl Add<SignedDuration> for Timestamp {
    type Output = Self;

    fn add(self, duration: SignedDuration) -> Self {
        self.checked_add(duration)
            .expect("`Timestamp + SignedDuration` overflowed")
    }
}

impl Sub<SignedDuration> for Timestamp {
    type Output = Self;

    fn sub(self, duration: SignedDuration) -> Self {
        self.checked_sub(duration)
            .expect("`Timestamp - SignedDuration` overflowed")
    }
}

impl Sub for Timestamp {
    type Output = SignedDuration;

    fn sub(self, earlier: Self) -> SignedDuration {
        self.since(earlier)
    }
}

/// Wall-clock fields of an instant at some offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    pub month: i64,
    pub day: i64,
    pub hour: i64,
    pub minute: i64,
    /// 60 during a leap second.
    pub second: i64,
    nanos: u32,
    weekday: i64,
}

impl Civil {
    fn new(secs: i64, frac: u32, offset: i64) -> Self {
        let local = secs + offset;
        let days = local.div_euclid(DAY);
        let of_day = local.rem_euclid(DAY);
        let (year, month, day) = civil_from_days(days);
        let leap = frac >= NANOS;
        Self {
            year,
            month,
            day,
            hour: of_day / 3600,
            minute: of_day / 60 % 60,
            second: of_day % 60 + i64::from(leap),
            nanos: if leap { frac - NANOS } else { frac },
            weekday: (days + 3).rem_euclid(7),
        }
    }

    pub fn same_day(&self, other: &Self) -> bool {
        (self.year, self.month, self.day) == (other.year, other.month, other.day)
    }

    fn clock(&self) -> String {
        format!(
            "{}-{:02}-{:02}T{:02}:{:02}:{:02}",
            year(self.year),
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second
        )
    }

    /// `16:30` or `4:30 PM`.
    pub fn time_of_day(&self, twelve_hour: bool) -> String {
        if twelve_hour {
            let hour = (self.hour + 11) % 12 + 1;
            let half = if self.hour < 12 { "AM" } else { "PM" };
            format!("{hour}:{:02} {half}", self.minute)
        } else {
            format!("{:02}:{:02}", self.hour, self.minute)
        }
    }

    /// `Mon 16:30`.
    pub fn weekday_time(&self, twelve_hour: bool) -> String {
        format!(
            "{} {}",
            WEEKDAYS[usize::try_from(self.weekday).unwrap_or_default()],
            self.time_of_day(twelve_hour)
        )
    }

    /// `Oct 6 16:30`.
    pub fn date_time(&self, twelve_hour: bool) -> String {
        format!(
            "{} {} {}",
            MONTHS[usize::try_from(self.month - 1).unwrap_or_default()],
            self.day,
            self.time_of_day(twelve_hour)
        )
    }
}

fn year(year: i64) -> String {
    if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{year:+05}")
    }
}

pub fn now() -> Timestamp {
    Timestamp::now()
}

/// How far ahead of the clock a saved time may be and still count. A small step back of the
/// clock keeps cooldowns and sent slots; a time further ahead was written by a wrong clock.
pub const CLOCK_SLACK: SignedDuration = mins(5);

/// `at`, unless it is more than `CLOCK_SLACK` after `now`. A saved time from a clock that was
/// ahead is dropped, so it cannot hold back alerts or digests until the clock catches up.
pub fn not_ahead(at: Timestamp, now: Timestamp) -> Option<Timestamp> {
    now.checked_add(CLOCK_SLACK)
        .is_none_or(|limit| at <= limit)
        .then_some(at)
}

pub const fn secs(secs: i64) -> SignedDuration {
    SignedDuration::from_secs(secs)
}

pub const fn mins(mins: i64) -> SignedDuration {
    SignedDuration::from_mins(mins)
}

pub const fn hours(hours: i64) -> SignedDuration {
    SignedDuration::from_hours(hours)
}

/// Whole seconds as chrono's `TimeDelta::try_seconds` allowed them.
pub fn try_secs(secs: i64) -> Option<SignedDuration> {
    (secs.checked_abs()? <= i64::MAX / 1000).then(|| SignedDuration::from_secs(secs))
}

pub const fn days(days: i64) -> SignedDuration {
    SignedDuration::from_hours(days * 24)
}

/// Whole milliseconds, toward zero, like chrono's `num_milliseconds`.
pub fn millis(duration: SignedDuration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(if duration.is_negative() {
        i64::MIN
    } else {
        i64::MAX
    })
}

/// The OS time zone: `TZ`, then `/etc/localtime` on Unix, the system setting on Windows.
pub fn local() -> TimeZone {
    TimeZone::system()
}

/// Days from 1970-01-01 to a proleptic Gregorian date, for any `i64` year.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The proleptic Gregorian date `days` after 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn valid_date(year: i64, month: i64, day: i64) -> bool {
    YEARS.contains(&year)
        && (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
}

fn digits<const N: usize>(bytes: &[u8], start: usize) -> Option<i64> {
    bytes
        .get(start..start + N)?
        .iter()
        .try_fold(0, |value, byte| {
            byte.is_ascii_digit()
                .then(|| value * 10 + i64::from(byte - b'0'))
        })
}

/// An RFC 3339 timestamp as chrono's `DateTime::parse_from_rfc3339` read it.
pub fn parse(text: &str) -> Option<Timestamp> {
    let bytes = text.as_bytes();
    if bytes.len() < 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't' | b' ')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let (year, month, day) = (
        digits::<4>(bytes, 0)?,
        digits::<2>(bytes, 5)?,
        digits::<2>(bytes, 8)?,
    );
    let (hour, minute, second) = (
        digits::<2>(bytes, 11)?,
        digits::<2>(bytes, 14)?,
        digits::<2>(bytes, 17)?,
    );
    let mut rest = &bytes[19..];
    let mut frac = 0;
    if let Some(fraction) = rest.strip_prefix(b".") {
        let count = fraction
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if count == 0 {
            return None;
        }
        frac = fraction[..count.min(9)]
            .iter()
            .fold(0, |value, byte| value * 10 + u32::from(byte - b'0'))
            * 10_u32.pow(9 - count.min(9) as u32);
        rest = &fraction[count..];
    }
    let offset = match rest {
        [b'Z' | b'z'] => 0,
        _ => {
            let (negative, rest) = if let Some(rest) = rest.strip_prefix("\u{2212}".as_bytes()) {
                (true, rest)
            } else {
                match rest.split_first()? {
                    (b'+', rest) => (false, rest),
                    (b'-', rest) => (true, rest),
                    _ => return None,
                }
            };
            if rest.len() != 5 || rest[2] != b':' {
                return None;
            }
            let (hours, minutes) = (digits::<2>(rest, 0)?, digits::<2>(rest, 3)?);
            if hours > 23 || minutes > 59 {
                return None;
            }
            let seconds = hours * 3600 + minutes * 60;
            if negative {
                -seconds
            } else {
                seconds
            }
        }
    };
    if !valid_date(year, month, day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let leap = second == 60;
    let local =
        days_from_civil(year, month, day) * DAY + hour * 3600 + minute * 60 + second.min(59);
    Timestamp::new(local - offset, frac + if leap { NANOS } else { 0 })
}

/// Whether chrono's RFC 3339 reader accepts `text`.
pub fn is_rfc3339(text: &str) -> bool {
    parse(text).is_some()
}

/// Whether `stamp` is 16 bytes that chrono read as a real date and time with
/// `%Y%m%dT%H%M%SZ`, the pattern [`Timestamp::compact`] writes. Like chrono,
/// each number may follow whitespace and stop early at a non-digit, and the
/// year may carry a sign and any number of digits.
pub fn is_compact(stamp: &str) -> bool {
    fn field(text: &str, width: usize) -> Option<(&str, i64)> {
        let text = text.trim_start();
        match text.as_bytes().first() {
            Some(b'-') if width == 4 => {
                let (rest, value) = number(&text[1..], 1, usize::MAX)?;
                Some((rest, value.checked_neg()?))
            }
            Some(b'+') if width == 4 => number(&text[1..], 1, usize::MAX),
            _ => number(text, 1, width),
        }
    }
    let read = || {
        let (rest, year) = field(stamp, 4)?;
        let (rest, month) = field(rest, 2)?;
        let (rest, day) = field(rest, 2)?;
        let (rest, hour) = field(rest.strip_prefix('T')?, 2)?;
        let (rest, minute) = field(rest, 2)?;
        let (rest, second) = field(rest, 2)?;
        (rest == "Z").then_some((year, month, day, hour, minute, second))
    };
    stamp.len() == 16
        && read().is_some_and(|(year, month, day, hour, minute, second)| {
            i32::try_from(year).is_ok()
                && valid_date(year, month, day)
                && hour <= 23
                && minute <= 59
                && second <= 60
        })
}

/// Up to `max` ASCII digits, at least `min`.
fn number(text: &str, min: usize, max: usize) -> Option<(&str, i64)> {
    let bytes = text.as_bytes();
    if bytes.len() < min {
        return None;
    }
    let mut value: i64 = 0;
    for (index, byte) in bytes.iter().take(max).enumerate() {
        if !byte.is_ascii_digit() {
            return (index >= min).then(|| (&text[index..], value));
        }
        value = value.checked_mul(10)?.checked_add(i64::from(byte - b'0'))?;
    }
    Some((&text[max.min(bytes.len())..], value))
}

/// One or more whitespace characters.
fn space(text: &str) -> Option<&str> {
    let rest = text.trim_start();
    (rest.len() < text.len()).then_some(rest)
}

/// Index of the three-letter name `text` starts with, ignoring case.
fn short_name<'a>(text: &'a str, names: &[&str]) -> Option<(&'a str, usize)> {
    let head = text.as_bytes().get(..3)?;
    let index = names
        .iter()
        .position(|name| name.as_bytes().eq_ignore_ascii_case(head))?;
    Some((&text[3..], index))
}

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn zone_2822(text: &str) -> Option<(&str, i64)> {
    let letters = text
        .bytes()
        .position(|byte| !byte.is_ascii_alphabetic())
        .unwrap_or(text.len());
    if letters > 0 {
        let (name, rest) = text.split_at(letters);
        let hours = match name.to_ascii_lowercase().as_str() {
            "gmt" | "ut" | "z" => 0,
            "edt" => -4,
            "est" | "cdt" => -5,
            "cst" | "mdt" => -6,
            "mst" | "pdt" => -7,
            "pst" => -8,
            // Military zones are unreliable; RFC 2822 reads them as -0000.
            single if single.len() == 1 && single != "j" => 0,
            _ => return None,
        };
        return Some((rest, hours * 3600));
    }
    let (negative, rest) = match text.as_bytes().first()? {
        b'+' => (false, &text[1..]),
        b'-' => (true, &text[1..]),
        _ => return None,
    };
    let bytes = rest.as_bytes();
    if bytes.len() < 4 || !bytes[..4].iter().all(u8::is_ascii_digit) || bytes[2] > b'5' {
        return None;
    }
    let digit = |index: usize| i64::from(bytes[index] - b'0');
    let seconds = (digit(0) * 10 + digit(1)) * 3600 + (digit(2) * 10 + digit(3)) * 60;
    Some((&rest[4..], if negative { -seconds } else { seconds }))
}

/// Skips one `(comment)`, nested and with `\` escapes, after optional whitespace.
fn comment_2822(text: &str) -> Option<&str> {
    let text = text.trim_start().strip_prefix('(')?;
    let mut depth = 1_usize;
    let mut escaped = false;
    for (index, byte) in text.bytes().enumerate() {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(&text[index + 1..]);
            }
        }
    }
    None
}

/// An RFC 2822 date as chrono's `DateTime::parse_from_rfc2822` read it:
/// case-insensitive names, an optional weekday that must match, two- and
/// three-digit years, obsolete zone names, and trailing comments.
pub fn parse_rfc2822(text: &str) -> Option<Timestamp> {
    let mut text = text.trim_start();
    let mut weekday = None;
    if let Some((rest, index)) = short_name(text, &WEEKDAYS) {
        text = rest.strip_prefix(',')?;
        weekday = Some(index as i64);
    }
    let (rest, day) = number(text.trim_start(), 1, 2)?;
    let (rest, month) = short_name(space(rest)?, &MONTHS)?;
    let month = month as i64 + 1;
    let rest = space(rest)?;
    let (after, mut year) = number(rest, 2, usize::MAX)?;
    match (rest.len() - after.len(), year) {
        (2, 0..=49) => year += 2000,
        (2, 50..=99) | (3, _) => year += 1900,
        _ => {}
    }
    let (rest, hour) = number(space(after)?, 2, 2)?;
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let (mut rest, minute) = number(rest, 2, 2)?;
    let mut second = 0;
    if let Some(after) = rest.trim_start().strip_prefix(':') {
        (rest, second) = number(after, 2, 2)?;
    }
    let (mut rest, offset) = zone_2822(space(rest)?)?;
    while let Some(after) = comment_2822(rest) {
        rest = after;
    }
    if !rest.is_empty()
        || !valid_date(year, month, day)
        || hour > 23
        || minute > 59
        || second > 60
        || offset.abs() >= DAY
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if weekday.is_some_and(|weekday| weekday != (days + 3).rem_euclid(7)) {
        return None;
    }
    let local = days * DAY + hour * 3600 + minute * 60 + second.min(59);
    Timestamp::new(local - offset, if second == 60 { NANOS } else { 0 })
}

/// Unix seconds in UTC to the second, for four-digit years.
pub fn unix_seconds_text(seconds: i64) -> Option<String> {
    Timestamp::from_unix_seconds(seconds)
        .filter(|at| (0..=9999).contains(&at.year()))
        .map(Timestamp::to_rfc3339_secs)
}

/// The earliest instant showing `local` on the wall clock in `zone`, or
/// `None` when a clock change skips it.
pub fn earliest(zone: &TimeZone, local: DateTime) -> Option<Timestamp> {
    let offset = match zone.to_ambiguous_timestamp(local).offset() {
        AmbiguousOffset::Unambiguous { offset } => offset,
        // Clocks went back; the offset before the change gives the earlier instant.
        AmbiguousOffset::Fold { before, .. } => before,
        AmbiguousOffset::Gap { .. } => return None,
    };
    offset.to_timestamp(local).ok().map(Timestamp::from)
}

/// `local` read as UTC.
pub fn as_utc(local: DateTime) -> Option<Timestamp> {
    Offset::UTC.to_timestamp(local).ok().map(Timestamp::from)
}

/// `Mon`, `Tue`, …
pub fn weekday_name(day: Weekday) -> &'static str {
    WEEKDAYS[usize::try_from(day.to_monday_zero_offset()).unwrap_or_default()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Timestamp {
        parse(text).unwrap()
    }

    #[test]
    fn civil_dates_round_trip_across_chronos_range() {
        for days in [
            i64::from(i32::MIN) / 4,
            -719_528,
            -1,
            0,
            59,
            10_957,
            20_742,
            2_932_896,
            i64::from(i32::MAX) / 4,
        ] {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, month, day), days, "{days}");
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-719_528), (0, 1, 1));
        assert_eq!(civil_from_days(20_742), (2026, 10, 16));
    }

    #[test]
    fn instants_match_jiff_inside_its_range() {
        for text in [
            "1969-12-31T23:59:59.5Z",
            "1970-01-01T00:00:00Z",
            "2026-10-16T12:00:00.123456789+05:30",
            "0001-01-01T00:00:00Z",
            "9999-12-30T21:59:59Z",
        ] {
            let jiff: jiff::Timestamp = text.parse().unwrap();
            assert_eq!(Timestamp::from(jiff), at(text), "{text}");
            assert_eq!(at(text).timestamp(), jiff, "{text}");
        }
    }

    #[test]
    fn leap_seconds_order_and_subtract_like_chrono() {
        let leap = at("2026-10-16T23:59:60Z");
        assert!(leap > at("2026-10-16T23:59:59.999999999Z"));
        assert!(at("2026-10-16T23:59:60.5Z") < at("2026-10-17T00:00:00Z"));
        assert_eq!(at("2026-10-17T00:00:00Z") - leap, SignedDuration::ZERO);
        assert_eq!(leap - at("2026-10-16T23:59:59Z"), secs(1));
        assert_eq!(
            at("2026-10-16T23:59:60.5Z") - at("2026-10-16T23:59:00Z"),
            SignedDuration::from_millis(60_500)
        );
        assert_eq!(
            leap + SignedDuration::from_millis(500),
            at("2026-10-16T23:59:60.5Z")
        );
        assert_eq!(leap + secs(1), at("2026-10-17T00:00:00Z"));
        assert_eq!(leap - secs(1), at("2026-10-16T23:59:59Z"));
        assert_eq!(leap.to_rfc3339(), "2026-10-16T23:59:60+00:00");
        assert_eq!(leap.compact(), "20261016T235960Z");
    }

    #[test]
    fn years_past_jiffs_range_keep_their_value() {
        let late = at("9999-12-31T23:59:59-00:01");
        assert_eq!(late.to_rfc3339_secs(), "+10000-01-01T00:00:59Z");
        assert!(late > at("9999-12-31T23:59:59Z"));
        assert_eq!(late - at("9999-12-31T23:59:59Z"), secs(60));
        assert_eq!(
            unix_seconds_text(253_402_300_799),
            Some("9999-12-31T23:59:59Z".into())
        );
        assert_eq!(unix_seconds_text(253_402_300_800), None);
        assert_eq!(Timestamp::from_unix_seconds(i64::MAX), None);
        assert_eq!(
            Timestamp::from_unix_millis(-1).unwrap().to_rfc3339(),
            "1969-12-31T23:59:59.999+00:00"
        );
        let zone = TimeZone::get("Asia/Tashkent").unwrap();
        assert_eq!(late.civil(&zone).date_time(false), "Jan 1 05:00");
    }

    #[test]
    fn fractions_print_in_groups_of_three() {
        for (text, written) in [
            ("2026-10-16T12:00:00Z", "2026-10-16T12:00:00+00:00"),
            ("2026-10-16T12:00:00.5Z", "2026-10-16T12:00:00.500+00:00"),
            (
                "2026-10-16T12:00:00.0001Z",
                "2026-10-16T12:00:00.000100+00:00",
            ),
            (
                "2026-10-16T12:00:00.1234567Z",
                "2026-10-16T12:00:00.123456700+00:00",
            ),
        ] {
            assert_eq!(at(text).to_rfc3339(), written, "{text}");
        }
        assert_eq!(
            at("2026-10-16T12:00:00.0019Z").compact_millis(),
            "20261016T120000.001Z"
        );
    }

    #[test]
    fn clock_text_matches_strftime_patterns() {
        let zone = TimeZone::get("America/New_York").unwrap();
        let local = at("2026-03-08T07:05:00Z").civil(&zone);
        assert_eq!(local.time_of_day(false), "03:05");
        assert_eq!(local.time_of_day(true), "3:05 AM");
        assert_eq!(local.weekday_time(true), "Sun 3:05 AM");
        assert_eq!(local.date_time(false), "Mar 8 03:05");
        assert_eq!(
            at("2026-10-16T16:00:00Z").civil(&zone).time_of_day(true),
            "12:00 PM"
        );
        assert_eq!(
            at("2026-10-16T04:00:00Z").civil(&zone).time_of_day(true),
            "12:00 AM"
        );
    }

    /// Seconds from 0001-01-01 to 9999-12-30, inside jiff's range.
    const SECONDS: std::ops::Range<i64> = -62_135_596_800..253_402_214_399;

    proptest::proptest! {
        #[test]
        fn arithmetic_and_text_match_jiff_without_leap_seconds(
            (a, a_nanos) in (SECONDS, 0..1_000_000_000_i32),
            (b, b_nanos) in (SECONDS, 0..1_000_000_000_i32),
            (shift, shift_nanos) in (-400_000_000_000_i64..400_000_000_000, -999_999_999..=999_999_999_i32),
            zone in proptest::sample::select(vec!["America/New_York", "Asia/Tashkent", "UTC"]),
        ) {
            let (jiff_a, jiff_b) = (
                jiff::Timestamp::new(a, a_nanos).unwrap(),
                jiff::Timestamp::new(b, b_nanos).unwrap(),
            );
            let (ours_a, ours_b) = (Timestamp::from(jiff_a), Timestamp::from(jiff_b));
            proptest::prop_assert_eq!(ours_a.cmp(&ours_b), jiff_a.cmp(&jiff_b));
            proptest::prop_assert_eq!(ours_a - ours_b, jiff_a.duration_since(jiff_b));
            proptest::prop_assert_eq!(ours_a.timestamp(), jiff_a);
            proptest::prop_assert_eq!(parse(&ours_a.to_rfc3339()), Some(ours_a));
            proptest::prop_assert_eq!(
                ours_a.to_rfc3339_secs(),
                jiff_a.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()
            );
            proptest::prop_assert_eq!(
                ours_a.compact(),
                jiff_a.strftime("%Y%m%dT%H%M%SZ").to_string()
            );
            let shift = SignedDuration::new(shift, shift_nanos);
            if let Ok(moved) = jiff_a.checked_add(shift) {
                proptest::prop_assert_eq!(ours_a + shift, Timestamp::from(moved));
                proptest::prop_assert_eq!((ours_a + shift) - shift, ours_a);
            }
            let zone = TimeZone::get(zone).unwrap();
            let theirs = zone.to_datetime(jiff_a);
            let civil = ours_a.civil(&zone);
            proptest::prop_assert_eq!(
                (civil.year, civil.month, civil.day, civil.hour, civil.minute, civil.second),
                (
                    i64::from(theirs.year()),
                    i64::from(theirs.month()),
                    i64::from(theirs.day()),
                    i64::from(theirs.hour()),
                    i64::from(theirs.minute()),
                    i64::from(theirs.second()),
                )
            );
            proptest::prop_assert_eq!(ours_a.date(&zone), theirs.date());
            proptest::prop_assert_eq!(
                civil.weekday_time(false),
                theirs.strftime("%a %H:%M").to_string()
            );
        }
    }

    #[test]
    fn earliest_picks_the_first_of_a_repeated_hour_and_skips_a_gap() {
        let zone = TimeZone::get("America/New_York").unwrap();
        let repeated = Date::constant(2026, 11, 1).at(1, 30, 0, 0);
        assert_eq!(earliest(&zone, repeated), Some(at("2026-11-01T05:30:00Z")));
        let skipped = Date::constant(2026, 3, 8).at(2, 30, 0, 0);
        assert_eq!(earliest(&zone, skipped), None);
        // A zone whose clocks skip 09:00-10:00 on the first Monday of March.
        let monday = TimeZone::posix("STD-1DST,M3.1.1/9,M10.5.0").unwrap();
        let first = Date::constant(2026, 3, 2).at(9, 30, 0, 0);
        assert_eq!(earliest(&monday, first), None);
    }
}
