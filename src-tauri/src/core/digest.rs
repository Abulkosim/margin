use super::{history::HistoryPoint, model::Provider};
use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, TimeZone, Utc, Weekday};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const RESET_JITTER_SECONDS: i64 = 60;
const MIN_COVERAGE_HOURS: f64 = 24.0;
const FULL_WEEK_HOURS: f64 = 144.0;
const TITLE_LIMIT: usize = 40;
const SHOWN_ACCOUNTS: usize = 2;
const MAX_SUMMARY_ACCOUNTS: usize = 100;
const MAX_PERIODS: u32 = 10_000;
const MIN_SEND_GAP_DAYS: i64 = 6;
const COMPARISON_MAX_AGE_DAYS: i64 = 8;

pub const SESSION_IDS: [&str; 2] = ["five_hour", "primary_window"];
pub const WEEKLY_IDS: [&str; 2] = ["seven_day", "secondary_window"];

pub struct AccountInput<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub provider: Provider,
    pub points: &'a [HistoryPoint],
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowWeek {
    pub peak_used: f64,
    pub periods: u32,
    pub limit_hits: u32,
    /// Sum of positive deltas, in percent points.
    pub consumed: f64,
    pub limit_hit_day: Option<Weekday>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccountWeek {
    pub id: String,
    pub label: String,
    pub provider: Provider,
    pub session: Option<WindowWeek>,
    pub weekly: Option<WindowWeek>,
    pub busiest_day: Option<Weekday>,
    pub coverage_hours: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Digest {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub accounts: Vec<AccountWeek>,
    pub previous: Option<WeeklySummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SummaryRow {
    pub id: String,
    pub weekly_peak: Option<f64>,
    pub session_hits: u32,
    pub session_periods: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeeklySummary {
    pub from: String,
    pub to: String,
    pub accounts: Vec<SummaryRow>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DigestState {
    pub last_sent_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<WeeklySummary>,
}

impl DigestState {
    /// Enabling starts the clock now, so the first digest is the first Monday
    /// 09:00 at least six days out.
    pub fn enabled_at(now: DateTime<Utc>) -> Self {
        Self {
            last_sent_at: now.to_rfc3339(),
            previous: None,
        }
    }

    /// Re-enabling restarts the clock but keeps the last summary, so the
    /// next digest can still say "(last week …)".
    pub fn re_enabled(previous: Option<Self>, now: DateTime<Utc>) -> Self {
        Self {
            previous: previous.and_then(|state| state.previous),
            ..Self::enabled_at(now)
        }
    }

    pub fn last_sent_at(&self) -> Option<DateTime<Utc>> {
        timestamp(&self.last_sent_at)
    }

    pub fn validate(&self) -> Result<(), String> {
        let invalid = || "Stored digest state is invalid.".to_string();
        timestamp(&self.last_sent_at).ok_or_else(invalid)?;
        let Some(previous) = &self.previous else {
            return Ok(());
        };
        if timestamp(&previous.from).is_none()
            || timestamp(&previous.to).is_none()
            || previous.accounts.len() > MAX_SUMMARY_ACCOUNTS
            || previous.accounts.iter().any(|row| {
                row.id.is_empty()
                    || row.id.len() > 128
                    || row
                        .weekly_peak
                        .is_some_and(|peak| !peak.is_finite() || !(0.0..=100.0).contains(&peak))
                    || row.session_periods > MAX_PERIODS
                    || row.session_hits > row.session_periods
            })
        {
            return Err(invalid());
        }
        Ok(())
    }
}

struct WindowStats {
    week: WindowWeek,
    by_day: BTreeMap<NaiveDate, f64>,
    days: BTreeSet<NaiveDate>,
    /// Time between consecutive readings with no gap between them; a
    /// compacted flat run counts in full.
    observed: Duration,
}

fn timestamp(value: &str) -> Option<DateTime<Utc>> {
    (value.len() <= 64)
        .then(|| DateTime::parse_from_rfc3339(value).ok())
        .flatten()
        .map(|at| at.with_timezone(&Utc))
}

/// A period changes with the reset time; without reset times, a drop or a gap starts one.
fn new_period(previous: &HistoryPoint, point: &HistoryPoint) -> bool {
    match (
        previous.resets_at.as_deref().and_then(timestamp),
        point.resets_at.as_deref().and_then(timestamp),
    ) {
        (Some(before), Some(now)) => (now - before).num_seconds().abs() > RESET_JITTER_SECONDS,
        _ => {
            point.gap_before
                || point.used_percent < previous.used_percent
                || point.window_seconds != previous.window_seconds
        }
    }
}

fn analyze<Tz: TimeZone>(
    points: &[HistoryPoint],
    ids: &[&str],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    zone: &Tz,
) -> Option<WindowStats> {
    let mut stats: Option<WindowStats> = None;
    let mut previous: Option<(&HistoryPoint, DateTime<Utc>)> = None;
    let mut period_hit = false;
    for point in points
        .iter()
        .filter(|point| ids.contains(&point.window_id.as_str()))
    {
        let Some(at) = timestamp(&point.at).filter(|at| (from..=to).contains(at)) else {
            continue;
        };
        if previous.is_some_and(|(_, previous_at)| at <= previous_at) {
            continue;
        }
        let day = at.with_timezone(zone).date_naive();
        let used = point.used_percent;
        let current = stats.get_or_insert_with(|| WindowStats {
            week: WindowWeek {
                peak_used: 0.0,
                periods: 0,
                limit_hits: 0,
                consumed: 0.0,
                limit_hit_day: None,
            },
            by_day: BTreeMap::new(),
            days: BTreeSet::new(),
            observed: Duration::zero(),
        });
        current.week.peak_used = current.week.peak_used.max(used);
        let starts_period = previous.is_none_or(|(earlier, _)| new_period(earlier, point));
        if starts_period {
            current.week.periods = current.week.periods.saturating_add(1);
            period_hit = false;
        }
        if used >= 100.0 && !period_hit {
            period_hit = true;
            current.week.limit_hits = current.week.limit_hits.saturating_add(1);
            current.week.limit_hit_day.get_or_insert(day.weekday());
        }
        if let Some((earlier, earlier_at)) = previous {
            if !point.gap_before {
                current.observed += at - earlier_at;
                let mut date = earlier_at.with_timezone(zone).date_naive();
                while date < day {
                    current.days.insert(date);
                    date = date.succ_opt()?;
                }
                // A confirmed reset starts the new period at `used`, all of it spent.
                let delta = if starts_period {
                    used
                } else {
                    (used - earlier.used_percent).max(0.0)
                };
                current.week.consumed += delta;
                *current.by_day.entry(day).or_insert(0.0) += delta;
            }
        }
        current.days.insert(day);
        previous = Some((point, at));
    }
    stats
}

fn hours(span: Duration) -> f64 {
    span.num_seconds().max(0) as f64 / 3600.0
}

/// None when no account has at least 24 hours of readings inside the last 7 days.
pub fn digest<Tz: TimeZone>(
    history: &[AccountInput<'_>],
    previous: Option<&WeeklySummary>,
    now: DateTime<Utc>,
    zone: &Tz,
) -> Option<Digest> {
    let from = now - Duration::days(7);
    let mut accounts = Vec::new();
    for input in history {
        let session = analyze(input.points, &SESSION_IDS, from, now, zone);
        let weekly = analyze(input.points, &WEEKLY_IDS, from, now, zone);
        let coverage_hours = [&session, &weekly]
            .into_iter()
            .flatten()
            .map(|stats| hours(stats.observed))
            .fold(0.0, f64::max);
        if coverage_hours < MIN_COVERAGE_HOURS {
            continue;
        }
        let busiest_day = session
            .as_ref()
            .filter(|stats| stats.days.len() >= 2)
            .and_then(|stats| {
                stats
                    .by_day
                    .iter()
                    .filter(|(_, consumed)| **consumed > 0.0)
                    .fold(None, |best: Option<(&NaiveDate, &f64)>, entry| match best {
                        Some(best) if best.1 >= entry.1 => Some(best),
                        _ => Some(entry),
                    })
                    .map(|(date, _)| date.weekday())
            });
        accounts.push(AccountWeek {
            id: input.id.to_string(),
            label: input.label.to_string(),
            provider: input.provider,
            session: session.map(|stats| stats.week),
            weekly: weekly.map(|stats| stats.week),
            busiest_day,
            coverage_hours,
        });
    }
    if accounts.is_empty() {
        return None;
    }
    accounts.sort_by(|left, right| {
        let peak = |account: &AccountWeek| {
            (
                account.weekly.as_ref().map(|week| week.peak_used),
                account.session.as_ref().map(|week| week.peak_used),
            )
        };
        let (left_weekly, left_session) = peak(left);
        let (right_weekly, right_session) = peak(right);
        right_weekly
            .partial_cmp(&left_weekly)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                right_session
                    .partial_cmp(&left_session)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then_with(|| left.label.cmp(&right.label))
    });
    Some(Digest {
        from,
        to: now,
        accounts,
        previous: previous.cloned(),
    })
}

fn percent(value: f64) -> u32 {
    value.round().clamp(0.0, 100.0) as u32
}

fn fit(text: String, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text;
    }
    let mut short: String = text.chars().take(limit - 1).collect();
    short.push('…');
    short
}

impl Digest {
    pub fn title(&self) -> String {
        let weekly_hits: Vec<&AccountWeek> = self
            .accounts
            .iter()
            .filter(|account| {
                account
                    .weekly
                    .as_ref()
                    .is_some_and(|week| week.limit_hits > 0)
            })
            .collect();
        let title = match weekly_hits.as_slice() {
            [] => {
                let hits: u32 = self
                    .accounts
                    .iter()
                    .filter_map(|account| account.session.as_ref())
                    .map(|week| week.limit_hits)
                    .sum();
                match hits {
                    0 => "Last 7 days · no limits hit".to_string(),
                    1 => "Last 7 days · 1 session hit the limit".to_string(),
                    hits => format!("Last 7 days · {hits} sessions hit the limit"),
                }
            }
            [account] => {
                let named = format!("Last 7 days · {} weekly limit hit", account.label);
                if named.chars().count() <= TITLE_LIMIT {
                    named
                } else {
                    "Last 7 days · weekly limit hit".to_string()
                }
            }
            hits => format!("Last 7 days · {} weekly limits hit", hits.len()),
        };
        fit(title, TITLE_LIMIT)
    }

    pub fn body(&self) -> String {
        let mut lines: Vec<String> = self
            .accounts
            .iter()
            .take(SHOWN_ACCOUNTS)
            .map(|account| self.line(account))
            .collect();
        let more = self.accounts.len().saturating_sub(SHOWN_ACCOUNTS);
        if more > 0 {
            if let Some(last) = lines.last_mut() {
                last.push_str(&format!(" · +{more} more in Margin"));
            }
        }
        lines.join("\n")
    }

    fn line(&self, account: &AccountWeek) -> String {
        let mut parts = Vec::new();
        if let Some(weekly) = &account.weekly {
            if weekly.limit_hits > 0 {
                parts.push(match weekly.limit_hit_day {
                    Some(day) => format!("weekly limit reached {day}"),
                    None => "weekly limit reached".to_string(),
                });
            } else {
                let mut part = format!("weekly low {}% left", percent(100.0 - weekly.peak_used));
                let last_week = self
                    .previous
                    .as_ref()
                    .filter(|previous| self.follows(previous))
                    .and_then(|previous| {
                        previous
                            .accounts
                            .iter()
                            .find(|row| row.id == account.id)
                            .and_then(|row| row.weekly_peak)
                    });
                if let Some(last_week) = last_week {
                    part.push_str(&format!(
                        " (last week {}% left)",
                        percent(100.0 - last_week)
                    ));
                }
                parts.push(part);
            }
        }
        if let Some(session) = &account.session {
            let count = session.periods;
            let noun = if count == 1 { "session" } else { "sessions" };
            parts.push(if session.limit_hits > 0 {
                format!("{} of {count} {noun} hit 0%", session.limit_hits)
            } else {
                format!("{count} {noun}")
            });
        }
        if let Some(day) = account.busiest_day {
            parts.push(format!("busiest {day}"));
        }
        let mut line = format!("{}: {}", account.label, parts.join(", "));
        if account.coverage_hours < FULL_WEEK_HOURS {
            let days = (account.coverage_hours / 24.0).round().max(1.0) as u32;
            let noun = if days == 1 { "day" } else { "days" };
            line.push_str(&format!(" ({days} {noun} of data)"));
        }
        line
    }

    /// A stored summary only reads as "last week" when it ended just before this span.
    fn follows(&self, previous: &WeeklySummary) -> bool {
        timestamp(&previous.to)
            .is_some_and(|ended| self.to - ended <= Duration::days(COMPARISON_MAX_AGE_DAYS))
    }

    pub fn summary(&self) -> WeeklySummary {
        WeeklySummary {
            from: self.from.to_rfc3339(),
            to: self.to.to_rfc3339(),
            accounts: self
                .accounts
                .iter()
                .take(MAX_SUMMARY_ACCOUNTS)
                .map(|account| SummaryRow {
                    id: account.id.clone(),
                    weekly_peak: account.weekly.as_ref().map(|week| week.peak_used),
                    session_hits: account
                        .session
                        .as_ref()
                        .map_or(0, |week| week.limit_hits.min(MAX_PERIODS)),
                    session_periods: account
                        .session
                        .as_ref()
                        .map_or(0, |week| week.periods.min(MAX_PERIODS)),
                })
                .collect(),
        }
    }
}

fn nine_oclock<Tz: TimeZone>(zone: &Tz, date: NaiveDate) -> DateTime<Utc> {
    let naive = date.and_hms_opt(9, 0, 0).expect("09:00 is a valid time");
    match zone.from_local_datetime(&naive) {
        LocalResult::Single(at) | LocalResult::Ambiguous(at, _) => at.with_timezone(&Utc),
        LocalResult::None => zone
            .from_local_datetime(&(naive + Duration::hours(1)))
            .earliest()
            .map_or_else(
                || Utc.from_utc_datetime(&naive),
                |at| at.with_timezone(&Utc),
            ),
    }
}

/// Most recent Monday 09:00 in `zone` at or before `now`.
pub fn due_at<Tz: TimeZone>(now: DateTime<Utc>, zone: &Tz) -> DateTime<Utc> {
    let date = now.with_timezone(zone).date_naive();
    let monday = date - Duration::days(i64::from(date.weekday().num_days_from_monday()));
    let candidate = nine_oclock(zone, monday);
    if candidate <= now {
        candidate
    } else {
        nine_oclock(zone, monday - Duration::days(7))
    }
}

/// Some(due) once per Monday 09:00 slot at least six days after the last send;
/// a long sleep yields one. The floor keeps a westward clock change from
/// reopening a sent slot and makes enabling just before 09:00 wait a week.
pub fn should_send<Tz: TimeZone>(
    last_sent_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    zone: &Tz,
) -> Option<DateTime<Utc>> {
    let due = due_at(now, zone);
    last_sent_at
        .is_none_or(|last| due - last >= Duration::days(MIN_SEND_GAP_DAYS))
        .then_some(due)
}

/// Marks `next` as the sent slot through `persist` before anything is shown, so
/// a restart never repeats a digest. A failed save restores the old state and
/// leaves the slot for the next tick.
fn mark_sent(
    runtime: &crate::runtime::Runtime,
    next: DigestState,
    persist: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let before = runtime.digest_state();
    runtime.set_digest_state(Some(next));
    persist().inspect_err(|_| runtime.set_digest_state(before))
}

/// Sends the digest when a Monday 09:00 slot is due, after every account's first poll.
/// The slot is marked sent even when nothing is worth sending.
pub async fn deliver(app: &tauri::AppHandle, runtime: &crate::runtime::Runtime) {
    use super::settings::NotificationPermission;
    use tauri_plugin_notification::NotificationExt;

    let now = Utc::now();
    let mut inner = runtime.inner.lock().await;
    let settings = &inner.view.settings;
    if !settings.weekly_digest
        || !settings.notifications
        || settings.notification_permission != NotificationPermission::Granted
    {
        return;
    }
    let state = runtime.digest_state();
    let last_sent_at = state.as_ref().and_then(DigestState::last_sent_at);
    let Some(due) = should_send(last_sent_at, now, &chrono::Local) else {
        return;
    };
    let accounts = inner.view.accounts.clone();
    if accounts.iter().any(|account| {
        inner
            .schedules
            .get(&account.id)
            .is_none_or(|schedule| schedule.fetched.is_none())
    }) {
        return;
    }
    let histories: Vec<Vec<HistoryPoint>> = accounts
        .iter()
        .map(|account| inner.history.get(account, now).points)
        .collect();
    let inputs: Vec<AccountInput<'_>> = accounts
        .iter()
        .zip(&histories)
        .map(|(account, points)| AccountInput {
            id: &account.id,
            label: &account.label,
            provider: account.provider,
            points,
        })
        .collect();
    let previous = state.and_then(|state| state.previous);
    let digest = digest(&inputs, previous.as_ref(), now, &chrono::Local);
    let next = DigestState {
        last_sent_at: due.to_rfc3339(),
        previous: digest.as_ref().map(Digest::summary),
    };
    let persisted = mark_sent(runtime, next, || {
        runtime.persist(&inner.view, &inner.alerts)
    });
    inner.view.notice = persisted.clone().err();
    drop(inner);
    if persisted.is_err() {
        return;
    }
    if let Some(digest) = digest {
        if let Err(error) = app
            .notification()
            .builder()
            .title(digest.title())
            .body(digest.body())
            .show()
        {
            eprintln!("weekly digest could not be shown: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, NaiveDateTime};

    const SESSION: &str = "five_hour";
    const WEEKLY: &str = "seven_day";

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }

    /// Monday 2026-10-26 09:00 UTC.
    fn now() -> DateTime<Utc> {
        at("2026-10-26T09:00:00Z")
    }

    fn point(window: &str, at: DateTime<Utc>, used: f64, reset: &str, gap: bool) -> HistoryPoint {
        HistoryPoint {
            window_id: window.into(),
            at: at.to_rfc3339(),
            used_percent: used,
            resets_at: Some(reset.into()),
            window_seconds: Some(if window == SESSION { 18_000 } else { 604_800 }),
            gap_before: gap,
        }
    }

    fn session(minutes: i64, used: f64, reset: &str) -> HistoryPoint {
        point(
            SESSION,
            at("2026-10-21T10:00:00Z") + Duration::minutes(minutes),
            used,
            reset,
            false,
        )
    }

    fn input<'a>(id: &'a str, points: &'a [HistoryPoint]) -> AccountInput<'a> {
        AccountInput {
            id,
            label: id,
            provider: Provider::Claude,
            points,
        }
    }

    /// Two days of steady weekly readings so the account clears the coverage floor.
    fn weekly_floor(reset: &str) -> Vec<HistoryPoint> {
        vec![
            point(WEEKLY, at("2026-10-20T10:00:00Z"), 10.0, reset, false),
            point(WEEKLY, at("2026-10-24T10:00:00Z"), 10.0, reset, false),
        ]
    }

    fn with_floor(mut points: Vec<HistoryPoint>) -> Vec<HistoryPoint> {
        points.extend(weekly_floor("2026-10-28T00:00:00Z"));
        points
    }

    fn only(points: &[HistoryPoint]) -> Digest {
        digest(&[input("a", points)], None, now(), &Utc).unwrap()
    }

    fn week(digest: &Digest, index: usize) -> &AccountWeek {
        &digest.accounts[index]
    }

    #[test]
    fn dense_minute_readings_count_one_period_and_one_hit() {
        const RESET: &str = "2026-10-21T15:00:00Z";
        let points: Vec<HistoryPoint> = (0..=100_i32)
            .map(|minute| session(i64::from(minute), f64::from(minute), RESET))
            .collect();
        let digest = only(&with_floor(points));
        let session = week(&digest, 0).session.as_ref().unwrap();
        assert_eq!(session.periods, 1);
        assert_eq!(session.limit_hits, 1);
        assert_eq!(session.consumed, 100.0);
        assert_eq!(session.peak_used, 100.0);
        assert_eq!(session.limit_hit_day, Some(Weekday::Wed));
    }

    #[test]
    fn run_length_storage_gives_the_same_result_as_dense_readings() {
        const RESET: &str = "2026-10-21T15:00:00Z";
        let levels = [
            0.0, 0.0, 0.0, 10.0, 10.0, 10.0, 10.0, 50.0, 50.0, 100.0, 100.0, 100.0,
        ];
        let dense: Vec<HistoryPoint> = levels
            .iter()
            .enumerate()
            .map(|(minute, used)| session(minute as i64, *used, RESET))
            .collect();
        let mut compacted = dense.clone();
        assert!(crate::core::history::compact(&mut compacted));
        assert_eq!(compacted.len(), 8);
        let dense = only(&with_floor(dense));
        let compacted = only(&with_floor(compacted));
        assert_eq!(dense.accounts, compacted.accounts);
        let session = week(&dense, 0).session.as_ref().unwrap();
        assert_eq!(session.consumed, 100.0);
        assert_eq!(session.periods, 1);
    }

    #[test]
    fn gap_skips_the_delta_but_keeps_peak_and_hits() {
        const RESET: &str = "2026-10-21T15:00:00Z";
        let mut points = vec![
            session(0, 10.0, RESET),
            session(10, 40.0, RESET),
            session(40, 90.0, RESET),
            session(50, 100.0, RESET),
        ];
        points[2].gap_before = true;
        let digest = only(&with_floor(points));
        let session = week(&digest, 0).session.as_ref().unwrap();
        assert_eq!(session.consumed, 40.0);
        assert_eq!(session.peak_used, 100.0);
        assert_eq!(session.limit_hits, 1);
        assert_eq!(session.periods, 1);
    }

    #[test]
    fn reset_without_gap_starts_a_new_period_without_negative_delta() {
        let points = vec![
            session(0, 90.0, "2026-10-21T15:00:00Z"),
            session(10, 95.0, "2026-10-21T15:00:00Z"),
            session(20, 3.0, "2026-10-21T20:00:00Z"),
            session(30, 8.0, "2026-10-21T20:00:00Z"),
        ];
        let digest = only(&with_floor(points));
        let session = week(&digest, 0).session.as_ref().unwrap();
        assert_eq!(session.periods, 2);
        assert_eq!(session.limit_hits, 0);
        assert_eq!(session.consumed, 13.0, "5 before the reset, then 3 and 5");
        assert_eq!(session.peak_used, 95.0);
    }

    #[test]
    fn an_unobserved_reset_counts_nothing() {
        let mut gapped = vec![
            session(0, 90.0, "2026-10-21T15:00:00Z"),
            session(20, 3.0, "2026-10-21T20:00:00Z"),
            session(30, 8.0, "2026-10-21T20:00:00Z"),
        ];
        gapped[1].gap_before = true;
        let digest = only(&with_floor(gapped));
        let stats = week(&digest, 0).session.as_ref().unwrap();
        assert_eq!(stats.consumed, 5.0);
        assert_eq!(stats.periods, 2);
    }

    #[test]
    fn coverage_counts_only_observed_intervals() {
        const RESET: &str = "2026-10-29T00:00:00Z";
        let mut points = vec![
            point(WEEKLY, at("2026-10-19T10:00:00Z"), 10.0, RESET, false),
            point(WEEKLY, at("2026-10-25T10:00:00Z"), 12.0, RESET, true),
        ];
        assert!(
            super::digest(&[input("a", &points)], None, now(), &Utc).is_none(),
            "two readings across a gap are not six days of data"
        );
        points.insert(
            1,
            point(WEEKLY, at("2026-10-20T10:00:00Z"), 11.0, RESET, false),
        );
        let digest = only(&points);
        assert_eq!(week(&digest, 0).coverage_hours, 24.0);
        assert_eq!(digest.body(), "a: weekly low 88% left (1 day of data)");
    }

    #[test]
    fn reset_jitter_within_a_minute_is_one_period() {
        let points = vec![
            session(0, 10.0, "2026-10-21T15:00:00Z"),
            session(10, 20.0, "2026-10-21T15:00:30Z"),
            session(20, 30.0, "2026-10-21T14:59:40Z"),
            session(30, 40.0, "2026-10-21T15:00:30Z"),
        ];
        let digest = only(&with_floor(points));
        let session = week(&digest, 0).session.as_ref().unwrap();
        assert_eq!(session.periods, 1);
        assert_eq!(session.consumed, 30.0);
    }

    #[test]
    fn weekly_reset_inside_the_span_reports_the_higher_peak() {
        let points = vec![
            point(
                WEEKLY,
                at("2026-10-20T10:00:00Z"),
                60.0,
                "2026-10-22T00:00:00Z",
                false,
            ),
            point(
                WEEKLY,
                at("2026-10-21T10:00:00Z"),
                88.0,
                "2026-10-22T00:00:00Z",
                false,
            ),
            point(
                WEEKLY,
                at("2026-10-22T10:00:00Z"),
                5.0,
                "2026-10-29T00:00:00Z",
                false,
            ),
            point(
                WEEKLY,
                at("2026-10-25T10:00:00Z"),
                20.0,
                "2026-10-29T00:00:00Z",
                false,
            ),
        ];
        let digest = only(&points);
        let weekly = week(&digest, 0).weekly.as_ref().unwrap();
        assert_eq!(weekly.peak_used, 88.0);
        assert_eq!(weekly.limit_hits, 0);
        assert_eq!(weekly.periods, 2);
        assert!(week(&digest, 0).session.is_none());
        assert_eq!(digest.body(), "a: weekly low 12% left (5 days of data)");
    }

    #[test]
    fn accounts_added_mid_week_are_labelled_or_dropped() {
        const RESET: &str = "2026-10-29T00:00:00Z";
        let thursday = vec![
            point(WEEKLY, at("2026-10-23T09:00:00Z"), 5.0, RESET, false),
            point(WEEKLY, at("2026-10-26T09:00:00Z"), 41.0, RESET, false),
        ];
        let fresh = vec![
            point(WEEKLY, at("2026-10-26T01:00:00Z"), 1.0, RESET, false),
            point(WEEKLY, at("2026-10-26T08:00:00Z"), 2.0, RESET, false),
        ];
        let digest = digest(
            &[input("fresh", &fresh), input("thu", &thursday)],
            None,
            now(),
            &Utc,
        )
        .unwrap();
        assert_eq!(digest.accounts.len(), 1);
        assert_eq!(week(&digest, 0).id, "thu");
        assert_eq!(week(&digest, 0).coverage_hours, 72.0);
        assert_eq!(digest.body(), "thu: weekly low 59% left (3 days of data)");
        assert!(super::digest(&[input("fresh", &fresh)], None, now(), &Utc).is_none());
        assert!(super::digest(&[], None, now(), &Utc).is_none());
    }

    #[test]
    fn retention_edge_includes_the_clamped_first_reading_only() {
        const RESET: &str = "2026-10-29T00:00:00Z";
        let from = now() - Duration::days(7);
        let before = from - Duration::seconds(1);
        let points = vec![
            point(WEEKLY, before, 99.0, RESET, false),
            point(WEEKLY, from, 30.0, RESET, false),
            point(WEEKLY, from + Duration::days(2), 50.0, RESET, false),
        ];
        let digest = only(&points);
        let weekly = week(&digest, 0).weekly.as_ref().unwrap();
        assert_eq!(weekly.peak_used, 50.0);
        assert_eq!(weekly.consumed, 20.0);
        assert_eq!(week(&digest, 0).coverage_hours, 48.0);
        assert_eq!(digest.from, from);
    }

    #[test]
    fn busiest_day_follows_the_local_day_of_the_later_reading() {
        const RESET: &str = "2026-10-21T00:00:00Z";
        let east = FixedOffset::east_opt(5 * 3600).unwrap();
        let points = with_floor(vec![
            point(SESSION, at("2026-10-20T16:00:00Z"), 10.0, RESET, false),
            point(SESSION, at("2026-10-20T18:00:00Z"), 20.0, RESET, false),
            point(SESSION, at("2026-10-20T20:00:00Z"), 60.0, RESET, false),
        ]);
        let local = digest(&[input("a", &points)], None, now(), &east).unwrap();
        assert_eq!(week(&local, 0).busiest_day, Some(Weekday::Wed));
        let utc = only(&points);
        assert_eq!(week(&utc, 0).busiest_day, None);
        let two_days = with_floor(vec![
            point(SESSION, at("2026-10-20T10:00:00Z"), 10.0, RESET, false),
            point(SESSION, at("2026-10-20T11:00:00Z"), 15.0, RESET, false),
            point(SESSION, at("2026-10-21T10:00:00Z"), 15.0, RESET, false),
        ]);
        let utc = only(&two_days);
        assert_eq!(week(&utc, 0).busiest_day, Some(Weekday::Tue));
    }

    #[test]
    fn previous_summary_compares_by_account_id() {
        const RESET: &str = "2026-10-29T00:00:00Z";
        let points = vec![
            point(WEEKLY, at("2026-10-19T10:00:00Z"), 10.0, RESET, false),
            point(WEEKLY, at("2026-10-25T10:00:00Z"), 86.0, RESET, false),
        ];
        let previous = WeeklySummary {
            from: "2026-10-12T09:00:00+00:00".into(),
            to: "2026-10-19T09:00:00+00:00".into(),
            accounts: vec![SummaryRow {
                id: "a".into(),
                weekly_peak: Some(71.4),
                session_hits: 0,
                session_periods: 3,
            }],
        };
        let digest = super::digest(&[input("a", &points)], Some(&previous), now(), &Utc).unwrap();
        assert_eq!(digest.body(), "a: weekly low 14% left (last week 29% left)");
        let renamed = super::digest(&[input("b", &points)], Some(&previous), now(), &Utc).unwrap();
        assert_eq!(renamed.body(), "b: weekly low 14% left");
        let mut stale = previous.clone();
        stale.to = "2026-10-05T09:00:00+00:00".into();
        let stale = super::digest(&[input("a", &points)], Some(&stale), now(), &Utc).unwrap();
        assert_eq!(
            stale.body(),
            "a: weekly low 14% left",
            "a summary from weeks ago is not last week"
        );
        let mut edge = previous.clone();
        edge.to = "2026-10-18T09:00:00+00:00".into();
        let edge = super::digest(&[input("a", &points)], Some(&edge), now(), &Utc).unwrap();
        assert!(edge.body().contains("(last week 29% left)"));
        let summary = digest.summary();
        assert_eq!(summary.accounts[0].weekly_peak, Some(86.0));
        assert_eq!(summary.from, digest.from.to_rfc3339());
        assert!(DigestState {
            last_sent_at: now().to_rfc3339(),
            previous: Some(summary),
        }
        .validate()
        .is_ok());
    }

    /// America/New_York for 2026: DST from Mar 8 07:00Z to Nov 1 06:00Z.
    #[derive(Clone, Copy, Debug)]
    struct NewYork;

    impl NewYork {
        fn offset_at_utc(utc: &NaiveDateTime) -> FixedOffset {
            let start = at("2026-03-08T07:00:00Z").naive_utc();
            let end = at("2026-11-01T06:00:00Z").naive_utc();
            let hours = if (start..end).contains(utc) { -4 } else { -5 };
            FixedOffset::east_opt(hours * 3600).unwrap()
        }
    }

    impl TimeZone for NewYork {
        type Offset = FixedOffset;

        fn from_offset(_: &FixedOffset) -> Self {
            NewYork
        }

        fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<FixedOffset> {
            let candidates: Vec<FixedOffset> = [-4, -5]
                .into_iter()
                .map(|hours| FixedOffset::east_opt(hours * 3600).unwrap())
                .filter(|offset| {
                    let utc = *local - Duration::seconds(i64::from(offset.local_minus_utc()));
                    Self::offset_at_utc(&utc) == *offset
                })
                .collect();
            match candidates.as_slice() {
                [single] => LocalResult::Single(*single),
                [first, second] => LocalResult::Ambiguous(*first, *second),
                _ => LocalResult::None,
            }
        }

        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
            Self::offset_at_utc(utc)
        }

        fn offset_from_local_date(&self, local: &NaiveDate) -> LocalResult<FixedOffset> {
            self.offset_from_local_datetime(&local.and_hms_opt(12, 0, 0).unwrap())
        }

        fn offset_from_utc_date(&self, utc: &NaiveDate) -> FixedOffset {
            Self::offset_at_utc(&utc.and_hms_opt(12, 0, 0).unwrap())
        }
    }

    #[test]
    fn schedule_fires_once_per_monday_morning() {
        let monday = at("2026-10-26T09:00:00Z");
        assert_eq!(
            should_send(None, at("2026-10-25T23:59:00Z"), &Utc),
            Some(at("2026-10-19T09:00:00Z"))
        );
        assert_eq!(
            should_send(
                Some(at("2026-10-19T09:00:00Z")),
                at("2026-10-25T23:59:00Z"),
                &Utc
            ),
            None
        );
        assert_eq!(
            should_send(Some(at("2026-10-19T09:00:00Z")), monday, &Utc),
            Some(monday)
        );
        assert_eq!(should_send(Some(monday), monday, &Utc), None);
        assert_eq!(
            should_send(Some(monday), at("2026-10-26T09:30:00Z"), &Utc),
            None
        );
        let tuesday = at("2026-10-27T10:00:00Z");
        assert_eq!(
            should_send(Some(at("2026-10-19T09:00:00Z")), tuesday, &Utc),
            Some(monday)
        );
        assert_eq!(should_send(Some(monday), tuesday, &Utc), None);
        let three_weeks = at("2026-10-05T09:00:00Z");
        assert_eq!(should_send(Some(three_weeks), tuesday, &Utc), Some(monday));
        assert_eq!(
            should_send(Some(monday), at("2026-11-02T08:59:59Z"), &Utc),
            None
        );
        assert_eq!(
            should_send(Some(monday), at("2026-11-02T09:00:00Z"), &Utc),
            Some(at("2026-11-02T09:00:00Z"))
        );
        let wednesday = at("2026-10-28T12:00:00Z");
        let enabled = DigestState::enabled_at(wednesday);
        assert_eq!(
            should_send(enabled.last_sent_at(), at("2026-10-30T09:00:00Z"), &Utc),
            None
        );
        assert_eq!(
            should_send(enabled.last_sent_at(), at("2026-11-02T09:00:00Z"), &Utc),
            None,
            "fewer than six days of data"
        );
        assert_eq!(
            should_send(enabled.last_sent_at(), at("2026-11-09T09:00:00Z"), &Utc),
            Some(at("2026-11-09T09:00:00Z"))
        );
    }

    #[test]
    fn enabling_just_before_nine_waits_a_week() {
        let enabled = DigestState::enabled_at(at("2026-10-26T08:59:00Z"));
        let monday = at("2026-10-26T09:00:00Z");
        assert_eq!(should_send(enabled.last_sent_at(), monday, &Utc), None);
        assert_eq!(
            should_send(enabled.last_sent_at(), at("2026-10-27T09:00:00Z"), &Utc),
            None
        );
        assert_eq!(
            should_send(enabled.last_sent_at(), at("2026-11-02T09:00:00Z"), &Utc),
            Some(at("2026-11-02T09:00:00Z"))
        );
    }

    #[test]
    fn a_westward_clock_change_does_not_reopen_a_sent_slot() {
        let east = FixedOffset::east_opt(5 * 3600).unwrap();
        let west = FixedOffset::west_opt(5 * 3600).unwrap();
        let sent = should_send(None, at("2026-10-26T04:00:00Z"), &east).unwrap();
        assert_eq!(sent, at("2026-10-26T04:00:00Z"));
        let later_that_monday = at("2026-10-26T15:00:00Z");
        assert_eq!(due_at(later_that_monday, &west), at("2026-10-26T14:00:00Z"));
        assert_eq!(should_send(Some(sent), later_that_monday, &west), None);
        assert_eq!(
            should_send(Some(sent), at("2026-11-02T14:00:00Z"), &west),
            Some(at("2026-11-02T14:00:00Z"))
        );
    }

    #[test]
    fn re_enabling_keeps_the_last_summary() {
        let summary = WeeklySummary {
            from: "2026-10-12T09:00:00+00:00".into(),
            to: "2026-10-19T09:00:00+00:00".into(),
            accounts: vec![],
        };
        let before = DigestState {
            last_sent_at: "2026-10-19T09:00:00+00:00".into(),
            previous: Some(summary.clone()),
        };
        let again = DigestState::re_enabled(Some(before), now());
        assert_eq!(again.last_sent_at, now().to_rfc3339());
        assert_eq!(again.previous, Some(summary));
        assert_eq!(
            DigestState::re_enabled(None, now()),
            DigestState::enabled_at(now())
        );
    }

    #[test]
    fn a_failed_save_leaves_the_slot_unsent() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = crate::runtime::Runtime::new(directory.path().join("state.json")).unwrap();
        let next = DigestState::enabled_at(now());
        let mut attempted = false;
        let result = mark_sent(&runtime, next.clone(), || {
            attempted = true;
            Err("disk full".into())
        });
        assert_eq!(result, Err("disk full".into()));
        assert!(attempted);
        assert_eq!(runtime.digest_state(), None, "retried next tick");
        assert_eq!(mark_sent(&runtime, next.clone(), || Ok(())), Ok(()));
        assert_eq!(runtime.digest_state(), Some(next));
    }

    #[test]
    fn due_slot_stays_at_nine_local_across_dst() {
        let east = FixedOffset::east_opt(5 * 3600).unwrap();
        assert_eq!(
            due_at(at("2026-10-26T03:59:00Z"), &east),
            at("2026-10-19T04:00:00Z")
        );
        assert_eq!(
            due_at(at("2026-10-26T04:00:00Z"), &east),
            at("2026-10-26T04:00:00Z")
        );
        // Spring forward on Sunday Mar 8: Monday Mar 9 09:00 EDT = 13:00Z, the week before 14:00Z.
        assert_eq!(
            due_at(at("2026-03-09T13:00:00Z"), &NewYork),
            at("2026-03-09T13:00:00Z")
        );
        assert_eq!(
            due_at(at("2026-03-09T12:59:00Z"), &NewYork),
            at("2026-03-02T14:00:00Z")
        );
        assert_eq!(
            due_at(at("2026-03-09T13:00:00Z"), &NewYork)
                .with_timezone(&NewYork)
                .format("%a %H:%M")
                .to_string(),
            "Mon 09:00"
        );
        // Fall back on Sunday Nov 1: Monday Nov 2 09:00 EST = 14:00Z, the week before 13:00Z.
        assert_eq!(
            due_at(at("2026-11-02T14:00:00Z"), &NewYork),
            at("2026-11-02T14:00:00Z")
        );
        assert_eq!(
            due_at(at("2026-11-02T13:30:00Z"), &NewYork),
            at("2026-10-26T13:00:00Z")
        );
        assert_eq!(
            due_at(at("2026-11-02T14:00:00Z"), &NewYork)
                .with_timezone(&NewYork)
                .format("%a %H:%M")
                .to_string(),
            "Mon 09:00"
        );
        assert_eq!(
            should_send(
                Some(at("2026-10-26T13:00:00Z")),
                at("2026-11-02T14:00:00Z"),
                &NewYork
            ),
            Some(at("2026-11-02T14:00:00Z"))
        );
    }

    fn account_week(
        id: &str,
        weekly_peak: f64,
        weekly_hits: u32,
        periods: u32,
        hits: u32,
    ) -> AccountWeek {
        AccountWeek {
            id: id.into(),
            label: id.into(),
            provider: Provider::Codex,
            session: Some(WindowWeek {
                peak_used: 100.0,
                periods,
                limit_hits: hits,
                consumed: 0.0,
                limit_hit_day: None,
            }),
            weekly: Some(WindowWeek {
                peak_used: weekly_peak,
                periods: 1,
                limit_hits: weekly_hits,
                consumed: 0.0,
                limit_hit_day: (weekly_hits > 0).then_some(Weekday::Thu),
            }),
            busiest_day: None,
            coverage_hours: 168.0,
        }
    }

    fn sample(accounts: Vec<AccountWeek>) -> Digest {
        Digest {
            from: now() - Duration::days(7),
            to: now(),
            accounts,
            previous: None,
        }
    }

    #[test]
    fn copy_stays_within_notification_limits() {
        let quiet = sample(vec![account_week("Personal", 38.0, 0, 11, 0)]);
        assert_eq!(quiet.title(), "Last 7 days · no limits hit");
        assert_eq!(quiet.body(), "Personal: weekly low 62% left, 11 sessions");
        let one = sample(vec![account_week("Personal", 86.0, 0, 14, 1)]);
        assert_eq!(one.title(), "Last 7 days · 1 session hit the limit");
        assert_eq!(
            one.body(),
            "Personal: weekly low 14% left, 1 of 14 sessions hit 0%"
        );
        let mut many = sample(vec![
            account_week("Personal", 86.0, 0, 14, 2),
            account_week("Codex", 60.0, 0, 9, 1),
        ]);
        many.accounts[1].busiest_day = Some(Weekday::Wed);
        many.previous = Some(WeeklySummary {
            from: (now() - Duration::days(14)).to_rfc3339(),
            to: (now() - Duration::days(7)).to_rfc3339(),
            accounts: vec![SummaryRow {
                id: "Personal".into(),
                weekly_peak: Some(71.0),
                session_hits: 0,
                session_periods: 0,
            }],
        });
        assert_eq!(many.title(), "Last 7 days · 3 sessions hit the limit");
        assert_eq!(
            many.body(),
            "Personal: weekly low 14% left (last week 29% left), 2 of 14 sessions hit 0%\nCodex: weekly low 40% left, 1 of 9 sessions hit 0%, busiest Wed"
        );
        let weekly = sample(vec![
            account_week("Personal", 100.0, 1, 19, 4),
            account_week("Work", 54.0, 0, 7, 0),
        ]);
        assert_eq!(weekly.title(), "Last 7 days · Personal weekly limit hit");
        assert_eq!(
            weekly.body(),
            "Personal: weekly limit reached Thu, 4 of 19 sessions hit 0%\nWork: weekly low 46% left, 7 sessions"
        );
        let long = sample(vec![account_week(
            "Client work for Northwind Studio",
            100.0,
            1,
            1,
            1,
        )]);
        assert_eq!(long.title(), "Last 7 days · weekly limit hit");
        assert_eq!(
            long.body(),
            "Client work for Northwind Studio: weekly limit reached Thu, 1 of 1 session hit 0%"
        );
        let two_weekly = sample(vec![
            account_week("A", 100.0, 1, 1, 0),
            account_week("B", 100.0, 1, 1, 0),
        ]);
        assert_eq!(two_weekly.title(), "Last 7 days · 2 weekly limits hit");
        let mut five = sample(
            (0..5)
                .map(|index| {
                    account_week(&format!("Acct {index}"), 90.0 - f64::from(index), 0, 10, 0)
                })
                .collect(),
        );
        five.accounts[0].session.as_mut().unwrap().limit_hits = 1;
        assert_eq!(five.title(), "Last 7 days · 1 session hit the limit");
        assert_eq!(
            five.body(),
            "Acct 0: weekly low 10% left, 1 of 10 sessions hit 0%\nAcct 1: weekly low 11% left, 10 sessions · +3 more in Margin"
        );
        let huge = sample(vec![account_week("A", 10.0, 0, 9999, 999)]);
        for digest in [
            &quiet,
            &one,
            &many,
            &weekly,
            &long,
            &two_weekly,
            &five,
            &huge,
        ] {
            assert!(
                digest.title().chars().count() <= TITLE_LIMIT,
                "{}",
                digest.title()
            );
            assert!(digest.body().lines().count() <= 2);
        }
    }

    #[test]
    fn stored_digest_state_rejects_bad_dates_and_rows() {
        let valid = DigestState {
            last_sent_at: "2026-10-26T09:00:00+00:00".into(),
            previous: Some(WeeklySummary {
                from: "2026-10-19T09:00:00+00:00".into(),
                to: "2026-10-26T09:00:00+00:00".into(),
                accounts: vec![SummaryRow {
                    id: "claude-a".into(),
                    weekly_peak: None,
                    session_hits: 1,
                    session_periods: 2,
                }],
            }),
        };
        assert!(valid.validate().is_ok());
        assert!(DigestState {
            last_sent_at: "monday".into(),
            ..valid.clone()
        }
        .validate()
        .is_err());
        let mut bad_peak = valid.clone();
        bad_peak.previous.as_mut().unwrap().accounts[0].weekly_peak = Some(101.0);
        assert!(bad_peak.validate().is_err());
        let mut bad_hits = valid.clone();
        bad_hits.previous.as_mut().unwrap().accounts[0].session_hits = 3;
        assert!(bad_hits.validate().is_err());
        let mut bad_from = valid;
        bad_from.previous.as_mut().unwrap().from = "last week".into();
        assert!(bad_from.validate().is_err());
    }
}
