use super::{
    history::HistoryPoint,
    model::{Snapshot, UsageWindow},
    notifications::{self, Alert, AlertKind, AlertMemory},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;

pub const THRESHOLDS: [u8; 3] = [10, 20, 30];
const LOOKBACK_MINUTES: i64 = 60;
const MIN_SPAN_MINUTES: i64 = 50;
const COOLDOWN_MINUTES: i64 = 60;
const QUIET_MINUTES: i64 = 60;
const RESET_JITTER_SECONDS: i64 = 300;
const LATEST_SLACK_SECONDS: i64 = 90;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BurnState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    alerted_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quiet_since: Option<String>,
}

impl BurnState {
    fn valid(&self) -> bool {
        [&self.reset, &self.alerted_at, &self.quiet_since]
            .into_iter()
            .flatten()
            .all(|value| timestamp(value).is_some())
    }
}

/// Unreadable burn state is dropped on its own so threshold alert memory survives.
pub(super) fn lenient<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<BurnState>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value::<BurnState>(value)
        .ok()
        .filter(BurnState::valid))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burn {
    pub increase: f64,
    pub span_seconds: i64,
}

impl Burn {
    fn runs_out(&self, used: f64, at: DateTime<Utc>) -> Option<DateTime<Utc>> {
        if self.increase <= 0.0 || self.span_seconds <= 0 {
            return None;
        }
        let seconds = (100.0 - used).max(0.0) * self.span_seconds as f64 / self.increase;
        at.checked_add_signed(Duration::try_seconds(seconds.ceil() as i64)?)
    }
}

fn timestamp(value: &str) -> Option<DateTime<Utc>> {
    (value.len() <= 64)
        .then(|| DateTime::parse_from_rfc3339(value).ok())
        .flatten()
        .map(|at| at.with_timezone(&Utc))
}

fn same_reset(a: Option<&String>, b: Option<&String>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => match (timestamp(a), timestamp(b)) {
            (Some(a), Some(b)) => (a - b).num_seconds().abs() <= RESET_JITTER_SECONDS,
            _ => false,
        },
        _ => false,
    }
}

fn continues(
    earlier: &HistoryPoint,
    used: f64,
    resets_at: Option<&String>,
    window_seconds: Option<u64>,
) -> bool {
    used >= earlier.used_percent
        && earlier.window_seconds == window_seconds
        && same_reset(earlier.resets_at.as_ref(), resets_at)
}

/// Increase in used % over the last hour of the current period, read step-wise from the history.
/// Needs a reading at least 50 minutes old with no gap, reset, or period change since.
pub fn hourly_increase(
    points: &[HistoryPoint],
    current: &UsageWindow,
    at: DateTime<Utc>,
) -> Option<Burn> {
    let readings = points
        .iter()
        .filter(|point| point.window_id == current.id)
        .map(|point| Some((timestamp(&point.at)?, point)))
        .collect::<Option<Vec<_>>>()?;
    let &(latest_at, latest) = readings.last()?;
    if at < latest_at
        || (at - latest_at).num_seconds() > LATEST_SLACK_SECONDS
        || !continues(
            latest,
            current.used_percent,
            current.resets_at.as_ref(),
            current.window_seconds,
        )
    {
        return None;
    }
    let mut start = readings.len() - 1;
    while start > 0 {
        let (previous_at, previous) = readings[start - 1];
        let (this_at, this) = readings[start];
        if this.gap_before
            || this_at <= previous_at
            || !continues(
                previous,
                this.used_percent,
                this.resets_at.as_ref(),
                this.window_seconds,
            )
        {
            break;
        }
        start -= 1;
    }
    let segment = &readings[start..];
    let (first_at, first) = segment[0];
    if first_at > at - Duration::minutes(MIN_SPAN_MINUTES) {
        return None;
    }
    let target = at - Duration::minutes(LOOKBACK_MINUTES);
    let (from, baseline) = segment
        .iter()
        .rev()
        .find(|(point_at, _)| *point_at <= target)
        .map_or((first_at, first.used_percent), |(_, point)| {
            (target, point.used_percent)
        });
    Some(Burn {
        increase: (current.used_percent - baseline).max(0.0),
        span_seconds: (at - from).num_seconds(),
    })
}

/// Fires once when the rate crosses the threshold, then waits for both a 60 minute cooldown and,
/// within the same period, a quiet hour below the threshold before it can fire again.
fn decide(
    state: &mut BurnState,
    burn: Option<Burn>,
    resets_at: Option<&String>,
    threshold: u8,
    now: DateTime<Utc>,
) -> bool {
    let alerted = state.alerted_at.as_deref().and_then(timestamp);
    let in_period = alerted.is_some() && same_reset(state.reset.as_ref(), resets_at);
    let cooled = alerted.is_none_or(|at| now - at >= Duration::minutes(COOLDOWN_MINUTES));
    let quiet = state
        .quiet_since
        .as_deref()
        .and_then(timestamp)
        .is_some_and(|since| now - since >= Duration::minutes(QUIET_MINUTES));
    let fast = burn.is_some_and(|burn| burn.increase >= f64::from(threshold));
    if fast && cooled && (!in_period || quiet) {
        *state = BurnState {
            reset: resets_at.cloned(),
            alerted_at: Some(now.to_rfc3339()),
            quiet_since: None,
        };
        return true;
    }
    if in_period && burn.is_some() && !fast {
        state.quiet_since.get_or_insert_with(|| now.to_rfc3339());
    } else {
        state.quiet_since = None;
    }
    false
}

/// Runs after the reading is recorded. Windows that already alerted in this poll are skipped,
/// but still count as alerted so the same climb does not alert twice.
pub fn observe(
    memory: &mut AlertMemory,
    account_id: &str,
    snapshot: &Snapshot,
    threshold: u8,
    suppressed: &HashSet<String>,
    mut history: impl FnMut(&str, DateTime<Utc>) -> Vec<HistoryPoint>,
    now: DateTime<Utc>,
) -> Vec<Alert> {
    let Some(at) = timestamp(&snapshot.fetched_at) else {
        return Vec::new();
    };
    let mut alerts = Vec::new();
    for window in snapshot
        .windows
        .iter()
        .filter(|window| notifications::tracked(window))
    {
        let Some(entry) = memory.get_mut(&format!("{account_id}/{}", window.id)) else {
            continue;
        };
        let reset = window.resets_at.as_deref().map(timestamp);
        if window.used_percent >= 100.0 || reset.is_some_and(|reset| reset.is_none_or(|r| r <= now))
        {
            continue;
        }
        let points = history(&window.id, at - Duration::minutes(LOOKBACK_MINUTES));
        let burn = hourly_increase(&points, window, at);
        let mut state = entry.burn.clone().unwrap_or_default();
        let fired = decide(&mut state, burn, window.resets_at.as_ref(), threshold, now);
        entry.burn = (state != BurnState::default()).then_some(state);
        let Some(burn) = burn.filter(|_| fired && !suppressed.contains(&window.id)) else {
            continue;
        };
        let runs_out = burn
            .runs_out(window.used_percent, at)
            .filter(|out| reset.flatten().is_some_and(|reset| *out < reset));
        alerts.push(Alert {
            window_id: window.id.clone(),
            window_label: window.label.clone(),
            kind: AlertKind::Burn {
                increase: burn.increase.floor().clamp(0.0, 100.0) as u8,
                runs_out,
            },
            resets_at: window.resets_at.clone(),
            used_percent: window.used_percent,
        });
    }
    alerts
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESET: &str = "2099-12-31T14:00:00Z";

    fn at() -> DateTime<Utc> {
        "2099-12-31T12:00:00Z".parse().unwrap()
    }

    fn minutes(offset: i64) -> DateTime<Utc> {
        at() + Duration::minutes(offset)
    }

    fn point(offset: i64, used: f64) -> HistoryPoint {
        HistoryPoint {
            window_id: "five_hour".into(),
            at: minutes(offset).to_rfc3339(),
            used_percent: used,
            resets_at: Some(RESET.into()),
            window_seconds: Some(18_000),
            gap_before: false,
        }
    }

    fn window(used: f64) -> UsageWindow {
        UsageWindow {
            id: "five_hour".into(),
            label: "Session".into(),
            used_percent: used,
            resets_at: Some(RESET.into()),
            window_seconds: Some(18_000),
        }
    }

    fn increase(points: &[HistoryPoint], used: f64) -> Option<f64> {
        hourly_increase(points, &window(used), at()).map(|burn| burn.increase)
    }

    #[test]
    fn dense_series_measures_the_last_hour() {
        let points: Vec<_> = (-90..=0)
            .map(|offset| point(offset, 10.0 + 0.4 * (offset + 90) as f64))
            .collect();
        let burn = hourly_increase(&points, &window(46.0), at()).unwrap();
        assert!((burn.increase - 24.0).abs() < 1e-9);
        assert_eq!(burn.span_seconds, 3600);
    }

    #[test]
    fn run_length_series_holds_flat_runs() {
        let points = [
            point(-180, 10.0),
            point(-30, 10.0),
            point(-29, 35.0),
            point(0, 35.0),
        ];
        assert_eq!(increase(&points, 35.0), Some(25.0));
        let flat = [point(-180, 10.0), point(0, 10.0)];
        assert_eq!(increase(&flat, 10.0), Some(0.0));
        let latest_skipped = [point(-120, 10.0), point(-1, 30.0)];
        assert_eq!(increase(&latest_skipped, 31.0), Some(21.0));
    }

    #[test]
    fn needs_a_reading_at_least_fifty_minutes_old() {
        assert_eq!(increase(&[point(-49, 5.0), point(0, 30.0)], 30.0), None);
        let burn = hourly_increase(&[point(-50, 5.0), point(0, 30.0)], &window(30.0), at());
        assert_eq!(
            burn,
            Some(Burn {
                increase: 25.0,
                span_seconds: 3000
            })
        );
        assert_eq!(increase(&[], 30.0), None);
    }

    #[test]
    fn never_reads_across_a_gap() {
        let mut after = point(-20, 30.0);
        after.gap_before = true;
        assert_eq!(
            increase(&[point(-70, 10.0), after, point(0, 40.0)], 40.0),
            None
        );
        let mut latest = point(0, 40.0);
        latest.gap_before = true;
        assert_eq!(increase(&[point(-70, 10.0), latest], 40.0), None);
        assert_eq!(increase(&[point(-70, 10.0), point(-3, 30.0)], 40.0), None);
    }

    #[test]
    fn never_reads_across_a_reset_or_period_change() {
        let mut old = point(-70, 80.0);
        old.resets_at = Some("2099-12-31T09:00:00Z".into());
        assert_eq!(
            increase(&[old.clone(), point(-40, 5.0), point(0, 30.0)], 30.0),
            None
        );
        old.used_percent = 1.0;
        assert_eq!(
            increase(&[old, point(-40, 5.0), point(0, 30.0)], 30.0),
            None
        );
        let dropped = [point(-70, 50.0), point(-40, 5.0), point(0, 30.0)];
        assert_eq!(increase(&dropped, 30.0), None);
        let mut resized = point(-70, 5.0);
        resized.window_seconds = Some(604_800);
        assert_eq!(increase(&[resized, point(0, 30.0)], 30.0), None);
        let mut current = window(30.0);
        current.resets_at = Some("2099-12-31T19:00:00Z".into());
        assert!(hourly_increase(&[point(-70, 5.0), point(0, 30.0)], &current, at()).is_none());
        let mut jitter = point(-70, 5.0);
        jitter.resets_at = Some("2099-12-31T14:02:00Z".into());
        assert_eq!(increase(&[jitter, point(0, 30.0)], 30.0), Some(25.0));
    }

    fn fast() -> Option<Burn> {
        Some(Burn {
            increase: 25.0,
            span_seconds: 3600,
        })
    }

    fn slow() -> Option<Burn> {
        Some(Burn {
            increase: 5.0,
            span_seconds: 3600,
        })
    }

    #[test]
    fn alerts_once_until_a_quiet_hour_passes() {
        let reset = Some(RESET.to_string());
        let reset = reset.as_ref();
        let mut state = BurnState::default();
        let mut step = |burn, offset| decide(&mut state, burn, reset, 20, minutes(offset));
        assert!(step(fast(), 0));
        assert!(!step(fast(), 10));
        assert!(!step(fast(), 90));
        assert!(!step(slow(), 100));
        assert!(!step(fast(), 130));
        assert!(!step(slow(), 140));
        assert!(!step(None, 150));
        assert!(!step(slow(), 160));
        assert!(!step(slow(), 219));
        assert!(step(fast(), 221));
        assert!(!step(fast(), 222));
    }

    #[test]
    fn a_new_period_rearms_after_the_cooldown() {
        let first = Some(RESET.to_string());
        let next = Some("2099-12-31T19:00:00Z".to_string());
        let mut state = BurnState::default();
        assert!(decide(&mut state, fast(), first.as_ref(), 20, minutes(0)));
        assert!(!decide(&mut state, fast(), next.as_ref(), 20, minutes(59)));
        assert!(decide(&mut state, fast(), next.as_ref(), 20, minutes(60)));
        assert!(!decide(&mut state, slow(), next.as_ref(), 10, minutes(200)));
        let below = Some(Burn {
            increase: 9.9,
            span_seconds: 3600,
        });
        assert!(!decide(
            &mut BurnState::default(),
            below,
            next.as_ref(),
            10,
            at()
        ));
    }

    fn snapshot(used: f64, label: &str) -> Snapshot {
        Snapshot {
            windows: vec![UsageWindow {
                label: label.into(),
                ..window(used)
            }],
            fetched_at: at().to_rfc3339(),
            extra_usage: None,
            reset_grants: None,
            balance: None,
        }
    }

    fn run(
        memory: &mut AlertMemory,
        used: f64,
        label: &str,
        earlier: f64,
    ) -> (Vec<Alert>, Vec<Alert>) {
        let snapshot = snapshot(used, label);
        let thresholds = notifications::observe(memory, "a", &snapshot, &[80, 95, 100], at());
        let suppressed = thresholds
            .iter()
            .map(|alert| alert.window_id.clone())
            .collect();
        let points = [point(-120, earlier), point(0, used)];
        let burns = observe(
            memory,
            "a",
            &snapshot,
            20,
            &suppressed,
            |_, _| points.to_vec(),
            at(),
        );
        (thresholds, burns)
    }

    #[test]
    fn alert_carries_the_increase_and_when_it_runs_out() {
        let mut memory = AlertMemory::new();
        let (thresholds, burns) = run(&mut memory, 60.0, "Session", 36.0);
        assert!(thresholds.is_empty());
        assert_eq!(burns.len(), 1);
        assert_eq!(
            burns[0].kind,
            AlertKind::Burn {
                increase: 24,
                runs_out: Some(minutes(100))
            }
        );
        assert_eq!(
            burns[0].title("Personal"),
            "Personal · Session: 24% used in the last hour"
        );
        let saved = serde_json::to_value(&memory).unwrap();
        assert_eq!(saved["a/five_hour"]["burn"]["reset"], RESET);
        let (_, burns) = run(&mut memory, 61.0, "Session", 36.0);
        assert!(burns.is_empty());
        let mut memory = AlertMemory::new();
        let (_, burns) = run(&mut memory, 30.0, "Session", 9.0);
        assert_eq!(
            burns[0].kind,
            AlertKind::Burn {
                increase: 21,
                runs_out: None
            }
        );
    }

    #[test]
    fn skips_when_a_usage_alert_fired_for_the_same_reading() {
        let mut memory = AlertMemory::new();
        let (thresholds, burns) = run(&mut memory, 85.0, "Session", 50.0);
        assert_eq!(thresholds.len(), 1);
        assert!(burns.is_empty());
        let (thresholds, burns) = run(&mut memory, 86.0, "Session", 50.0);
        assert!(thresholds.is_empty());
        assert!(burns.is_empty());
    }

    #[test]
    fn skips_full_and_untracked_windows() {
        let mut memory = AlertMemory::new();
        let (_, burns) = run(&mut memory, 100.0, "Session", 50.0);
        assert!(burns.is_empty());
        let mut memory = AlertMemory::new();
        let (_, burns) = run(&mut memory, 60.0, "Sonnet", 20.0);
        assert!(burns.is_empty());
        let mut memory = AlertMemory::new();
        let (_, burns) = run(&mut memory, 50.0, "Session", 31.0);
        assert!(burns.is_empty());
        assert!(serde_json::to_value(&memory).unwrap()["a/five_hour"]
            .get("burn")
            .is_none());
    }
}
