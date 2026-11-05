use super::{
    burn::{same_reset, timestamp},
    history::HistoryPoint,
    model::{Snapshot, UsageWindow, WindowScope},
    notifications::{Alert, AlertKind, AlertMemory},
};
use chrono::{DateTime, Duration, Utc};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Forecast {
    pub runs_out: DateTime<Utc>,
    pub resets_at: DateTime<Utc>,
}

/// How far back the rate is read, and how far ahead a run-out warns: a fifth of the window,
/// one hour for a 5-hour session and about 34 hours for a week.
pub fn lookback(window: &UsageWindow) -> Option<Duration> {
    length(window).map(|seconds| Duration::seconds(seconds / 5))
}

fn length(window: &UsageWindow) -> Option<i64> {
    window
        .window_seconds
        .and_then(|seconds| i64::try_from(seconds).ok())
        .filter(|seconds| *seconds > 0)
}

/// When `current` reaches its limit at the average rate of the last fifth of the window, if that
/// is before the reset. Needs readings spanning a tenth of the window in the current period.
/// Gaps between readings are fine: used % only grows within a period, so two readings give the
/// exact average between them.
pub fn project(
    points: &[HistoryPoint],
    current: &UsageWindow,
    at: DateTime<Utc>,
) -> Option<Forecast> {
    let length = length(current)?;
    let resets_at = timestamp(current.resets_at.as_deref()?)?;
    if resets_at <= at || !(0.0..100.0).contains(&current.used_percent) {
        return None;
    }
    let mut readings: Vec<_> = points
        .iter()
        .filter(|point| {
            point.window_id == current.id
                && point.window_seconds == current.window_seconds
                && same_reset(point.resets_at.as_ref(), current.resets_at.as_ref())
        })
        .filter_map(|point| Some((timestamp(&point.at)?, point)))
        .filter(|(point_at, _)| *point_at <= at)
        .collect();
    readings.sort_by_key(|(point_at, _)| *point_at);
    let target = at - Duration::seconds(length / 5);
    let (from, baseline) = match readings
        .iter()
        .rposition(|(point_at, _)| *point_at <= target)
    {
        None => readings
            .first()
            .map(|(point_at, point)| (*point_at, point.used_percent))?,
        Some(index) => {
            let (before_at, before) = readings[index];
            match readings.get(index + 1) {
                Some(&(after_at, after)) if !after.gap_before && after_at > before_at => {
                    let share = (target - before_at).num_milliseconds() as f64
                        / (after_at - before_at).num_milliseconds() as f64;
                    let used =
                        before.used_percent + (after.used_percent - before.used_percent) * share;
                    (target, used)
                }
                _ => (before_at, before.used_percent),
            }
        }
    };
    let span = (at - from).num_seconds();
    let increase = current.used_percent - baseline;
    if span < length / 10 || increase <= 0.0 {
        return None;
    }
    let seconds = (100.0 - current.used_percent) * span as f64 / increase;
    let runs_out = at.checked_add_signed(Duration::try_seconds(seconds.ceil() as i64)?)?;
    let margin = Duration::seconds(length / 60);
    (runs_out + margin <= resets_at).then_some(Forecast {
        runs_out,
        resets_at,
    })
}

/// Warns once per period when a session, weekly, or model limit will run out before it resets
/// and within the look-back from now. A usage alert for the same window in this poll carries the
/// run-out time instead of sending a second notification.
pub fn observe(
    memory: &mut AlertMemory,
    account_id: &str,
    snapshot: &Snapshot,
    alerts: &mut Vec<Alert>,
    mut history: impl FnMut(&str, DateTime<Utc>) -> Vec<HistoryPoint>,
    now: DateTime<Utc>,
) {
    let Some(at) = timestamp(&snapshot.fetched_at) else {
        return;
    };
    for window in snapshot
        .windows
        .iter()
        .filter(|window| window.scope() != WindowScope::Other)
    {
        let Some(lookback) = lookback(window) else {
            continue;
        };
        let Some(entry) = memory.get_mut(&format!("{account_id}/{}", window.id)) else {
            continue;
        };
        let Some(forecast) = project(&history(&window.id, at - lookback), window, at)
            .filter(|forecast| forecast.runs_out > now)
        else {
            continue;
        };
        let soon = forecast.runs_out - at <= lookback;
        let mut same_poll = alerts
            .iter_mut()
            .filter(|alert| alert.window_id == window.id)
            .peekable();
        if same_poll.peek().is_some() {
            for alert in same_poll {
                if matches!(alert.kind, AlertKind::Threshold(threshold) if threshold < 100) {
                    alert.runs_out = Some(forecast.runs_out);
                    entry.forecast |= soon;
                }
            }
            continue;
        }
        if soon && !entry.forecast {
            entry.forecast = true;
            alerts.push(Alert {
                window_id: window.id.clone(),
                window_label: window.label.clone(),
                kind: AlertKind::RunsOut,
                resets_at: window.resets_at.clone(),
                used_percent: window.used_percent,
                runs_out: Some(forecast.runs_out),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::notifications;

    const RESET: &str = "2099-12-31T14:00:00Z";
    const WEEK_RESET: &str = "2100-01-03T12:00:00Z";

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

    fn session(used: f64) -> UsageWindow {
        UsageWindow {
            id: "five_hour".into(),
            label: "Session".into(),
            used_percent: used,
            resets_at: Some(RESET.into()),
            window_seconds: Some(18_000),
        }
    }

    fn runs_out(points: &[HistoryPoint], used: f64) -> Option<DateTime<Utc>> {
        project(points, &session(used), at()).map(|forecast| forecast.runs_out)
    }

    fn ramp(from: i64, to: i64, base: f64, per_minute: f64) -> Vec<HistoryPoint> {
        (from..=to)
            .step_by(5)
            .map(|offset| point(offset, base + (offset - from) as f64 * per_minute))
            .collect()
    }

    #[test]
    fn projects_from_the_last_hour_of_a_session() {
        let points = ramp(-90, 0, 0.0, 0.5);
        assert_eq!(runs_out(&points, 45.0), Some(minutes(110)));
        let forecast = project(&points, &session(45.0), at()).unwrap();
        assert_eq!(forecast.resets_at, RESET.parse::<DateTime<Utc>>().unwrap());
    }

    #[test]
    fn needs_half_the_look_back_of_readings() {
        assert_eq!(runs_out(&ramp(-25, 0, 50.0, 1.0), 75.0), None);
        assert_eq!(runs_out(&ramp(-30, 0, 50.0, 1.0), 80.0), Some(minutes(20)));
        assert_eq!(
            runs_out(&[point(-40, 10.0), point(0, 50.0)], 50.0),
            Some(minutes(50))
        );
    }

    #[test]
    fn slow_flat_or_falling_usage_has_no_run_out() {
        assert_eq!(runs_out(&ramp(-60, 0, 0.0, 0.1), 6.0), None);
        assert_eq!(runs_out(&ramp(-60, 0, 40.0, 0.0), 40.0), None);
        assert_eq!(runs_out(&[point(-60, 41.0), point(0, 40.0)], 40.0), None);
        assert_eq!(runs_out(&ramp(-60, 0, 0.0, 1.0), 100.0), None);
    }

    #[test]
    fn a_run_out_just_before_the_reset_is_not_worth_a_warning() {
        let points = [point(-60, 0.0), point(0, 40.0)];
        assert_eq!(runs_out(&points, 40.0), Some(minutes(90)));
        let late = [point(-60, 0.0), point(0, 34.0)];
        assert_eq!(runs_out(&late, 34.0), None);
    }

    #[test]
    fn interpolates_between_readings_but_not_across_a_gap() {
        let run_length = [point(-180, 10.0), point(-30, 10.0), point(0, 50.0)];
        assert_eq!(runs_out(&run_length, 50.0), Some(minutes(75)));
        let mut gap = vec![point(-180, 10.0), point(0, 50.0)];
        gap[1].gap_before = true;
        assert_eq!(runs_out(&gap, 50.0), None);
        let mut bridged = vec![point(-100, 10.0), point(0, 60.0)];
        bridged[1].gap_before = true;
        assert_eq!(runs_out(&bridged, 60.0), Some(minutes(80)));
    }

    #[test]
    fn earlier_periods_and_other_windows_never_count() {
        let mut old = ramp(-120, -65, 0.0, 1.0);
        for point in &mut old {
            point.resets_at = Some("2099-12-31T09:00:00Z".into());
        }
        let mut weekly = ramp(-120, 0, 0.0, 1.0);
        for point in &mut weekly {
            point.window_id = "seven_day".into();
        }
        let current = ramp(-30, 0, 0.0, 1.0);
        let points: Vec<_> = old.into_iter().chain(weekly).chain(current).collect();
        assert_eq!(runs_out(&points, 30.0), Some(minutes(70)));
        assert_eq!(runs_out(&points[..12], 30.0), None);
    }

    #[test]
    fn reset_jitter_is_the_same_period_and_order_does_not_matter() {
        let reset: DateTime<Utc> = RESET.parse().unwrap();
        let mut points = ramp(-60, 0, 0.0, 0.8);
        for (index, point) in points.iter_mut().enumerate() {
            let shift = (index as i64 % 3 - 1) * 30;
            point.resets_at = Some((reset + Duration::seconds(shift)).to_rfc3339());
        }
        points.reverse();
        assert_eq!(runs_out(&points, 48.0), Some(minutes(65)));
    }

    #[test]
    fn a_week_reads_the_last_day_and_a_half_across_gaps() {
        let window = UsageWindow {
            id: "seven_day".into(),
            label: "Weekly".into(),
            used_percent: 70.0,
            resets_at: Some(WEEK_RESET.into()),
            window_seconds: Some(604_800),
        };
        let reading = |offset: i64, used: f64, gap_before: bool| HistoryPoint {
            window_id: "seven_day".into(),
            at: minutes(offset).to_rfc3339(),
            used_percent: used,
            resets_at: Some(WEEK_RESET.into()),
            window_seconds: Some(604_800),
            gap_before,
        };
        let short = [reading(-16 * 60, 50.0, false), reading(0, 70.0, false)];
        assert_eq!(project(&short, &window, at()), None);
        let overnight = [
            reading(-2592, 10.0, false),
            reading(-1440, 40.0, false),
            reading(-240, 50.0, true),
            reading(0, 70.0, false),
        ];
        let forecast = project(&overnight, &window, at()).unwrap();
        assert_eq!(forecast.runs_out, at() + Duration::seconds(80_640));
        assert_eq!(lookback(&window), Some(Duration::seconds(120_960)));
    }

    fn snapshot(used: f64) -> Snapshot {
        Snapshot {
            windows: vec![session(used)],
            fetched_at: at().to_rfc3339(),
            extra_usage: None,
            reset_grants: None,
            balance: None,
            incomplete: false,
        }
    }

    fn run(memory: &mut AlertMemory, used: f64, points: &[HistoryPoint]) -> Vec<Alert> {
        let snapshot = snapshot(used);
        let mut alerts = notifications::observe(memory, "a", &snapshot, &[80, 95, 100], at());
        observe(
            memory,
            "a",
            &snapshot,
            &mut alerts,
            |_, _| points.to_vec(),
            at(),
        );
        alerts
    }

    #[test]
    fn warns_once_per_period_when_the_run_out_is_within_the_hour() {
        let mut memory = AlertMemory::new();
        assert!(run(&mut memory, 40.0, &[point(-60, 0.0), point(0, 40.0)]).is_empty());
        let alerts = run(&mut memory, 60.0, &[point(-60, 0.0), point(0, 60.0)]);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::RunsOut);
        assert_eq!(alerts[0].runs_out, Some(minutes(40)));
        assert_eq!(
            serde_json::to_value(&memory).unwrap()["a/five_hour"]["forecast"],
            true
        );
        assert!(run(&mut memory, 70.0, &[point(-60, 0.0), point(0, 70.0)]).is_empty());
        let mut next = memory.clone();
        let mut snapshot = snapshot(60.0);
        snapshot.windows[0].resets_at = Some("2099-12-31T16:00:00Z".into());
        let mut alerts = notifications::observe(&mut next, "a", &snapshot, &[80], at());
        let mut fresh = vec![point(-60, 0.0), point(0, 60.0)];
        for point in &mut fresh {
            point.resets_at = snapshot.windows[0].resets_at.clone();
        }
        observe(
            &mut next,
            "a",
            &snapshot,
            &mut alerts,
            |_, _| fresh.clone(),
            at(),
        );
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::RunsOut);
    }

    #[test]
    fn a_usage_alert_in_the_same_poll_carries_the_run_out() {
        let mut memory = AlertMemory::new();
        run(&mut memory, 10.0, &[]);
        let alerts = run(&mut memory, 82.0, &[point(-60, 22.0), point(0, 82.0)]);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].kind, AlertKind::Threshold(80));
        assert_eq!(alerts[0].runs_out, Some(minutes(18)));
        assert!(run(&mut memory, 83.0, &[point(-60, 22.0), point(0, 83.0)]).is_empty());
    }

    #[test]
    fn skips_other_windows_and_passed_projections() {
        let mut memory = AlertMemory::new();
        let mut snapshot = snapshot(60.0);
        snapshot.windows[0].id = "seven_day_routines".into();
        let mut alerts = notifications::observe(&mut memory, "a", &snapshot, &[80], at());
        let mut points = vec![point(-60, 0.0), point(0, 60.0)];
        for point in &mut points {
            point.window_id = "seven_day_routines".into();
        }
        observe(
            &mut memory,
            "a",
            &snapshot,
            &mut alerts,
            |_, _| points.clone(),
            at(),
        );
        assert!(alerts.is_empty());
        let mut memory = AlertMemory::new();
        let snapshot = self::snapshot(60.0);
        let mut alerts = notifications::observe(&mut memory, "a", &snapshot, &[80], at());
        let points = [point(-60, 0.0), point(0, 60.0)];
        observe(
            &mut memory,
            "a",
            &snapshot,
            &mut alerts,
            |_, _| points.to_vec(),
            minutes(45),
        );
        assert!(alerts.is_empty());
    }
}
