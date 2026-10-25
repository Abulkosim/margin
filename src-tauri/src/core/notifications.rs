use super::model::{Snapshot, UsageWindow};
use chrono::{DateTime, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};

pub type AlertMemory = HashMap<String, WindowAlerts>;

const RESET_JITTER_SECONDS: i64 = 60;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowAlerts {
    reset: Option<String>,
    sent: BTreeSet<u8>,
}

impl WindowAlerts {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.reset.as_ref().is_some_and(|reset| {
            reset.len() > 64 || chrono::DateTime::parse_from_rfc3339(reset).is_err()
        }) || self
            .sent
            .iter()
            .any(|threshold| !(1..=100).contains(threshold))
        {
            return Err("Stored notification state is invalid.".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AlertKind {
    Threshold(u8),
    Reset,
}

#[derive(Clone, Debug)]
pub struct Alert {
    pub window_label: String,
    pub kind: AlertKind,
    pub resets_at: Option<String>,
    pub used_percent: f64,
}

impl Alert {
    pub fn title(&self, account_label: &str) -> String {
        let window = &self.window_label;
        match self.kind {
            AlertKind::Threshold(100) => format!("{account_label} · {window} limit reached"),
            AlertKind::Threshold(threshold) => {
                let left = (100.0 - self.used_percent)
                    .floor()
                    .clamp(0.0, f64::from(100 - threshold));
                format!("{account_label} · {window} {left}% left")
            }
            AlertKind::Reset => format!("{account_label} · {window} limit reset"),
        }
    }

    pub fn body(&self, now: DateTime<Utc>) -> Option<String> {
        self.body_in(&Local, now)
    }

    fn body_in<Tz: TimeZone>(&self, zone: &Tz, now: DateTime<Utc>) -> Option<String>
    where
        Tz::Offset: std::fmt::Display,
    {
        let reset = parse(self.resets_at.as_ref())?;
        let milliseconds = (reset - now).num_milliseconds();
        (milliseconds > 0).then(|| {
            format!(
                "Resets in {} · {}",
                short_duration(milliseconds),
                clock_in(zone, reset, now)
            )
        })
    }
}

pub fn reset_clock(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    clock_in(&Local, at, now)
}

fn clock_in<Tz: TimeZone>(zone: &Tz, at: DateTime<Utc>, now: DateTime<Utc>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let local = at.with_timezone(zone);
    let pattern = if local.date_naive() == now.with_timezone(zone).date_naive() {
        "%H:%M"
    } else if at - now < chrono::Duration::days(6) {
        "%a %H:%M"
    } else {
        "%b %-d %H:%M"
    };
    local.format(pattern).to_string()
}

fn short_duration(milliseconds: i64) -> String {
    if milliseconds < 60_000 {
        return "<1m".into();
    }
    let minutes = (milliseconds + 59_999) / 60_000;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h {}m", minutes % 60);
    }
    format!("{}d {}h", hours / 24, hours % 24)
}

fn parse(value: Option<&String>) -> Option<DateTime<Utc>> {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn same_period(previous: Option<&String>, current: Option<&String>) -> bool {
    match (parse(previous), parse(current)) {
        (Some(previous), Some(current)) => {
            (current - previous).num_seconds().abs() <= RESET_JITTER_SECONDS
        }
        _ => true,
    }
}

fn tracked(window: &UsageWindow) -> bool {
    matches!(window.label.as_str(), "Session" | "Weekly")
}

/// A window seen for the first time, or in a new reset period, alerts once for the highest
/// threshold it already passed. Windows remembered from before a restart only alert for
/// thresholds above the highest one already sent.
pub fn observe(
    memory: &mut AlertMemory,
    account_id: &str,
    snapshot: &Snapshot,
    thresholds: &[u8],
    now: DateTime<Utc>,
) -> Vec<Alert> {
    let prefix = format!("{account_id}/");
    let current: HashSet<_> = snapshot
        .windows
        .iter()
        .map(|window| window.id.as_str())
        .collect();
    memory.retain(|key, _| {
        key.strip_prefix(&prefix)
            .is_none_or(|id| current.contains(id))
    });
    let mut alerts = Vec::new();
    for window in &snapshot.windows {
        if window
            .resets_at
            .as_ref()
            .is_some_and(|reset| parse(Some(reset)).is_none_or(|reset| reset <= now))
        {
            continue;
        }
        let key = format!("{account_id}/{}", window.id);
        let reached: BTreeSet<u8> = thresholds
            .iter()
            .copied()
            .filter(|threshold| window.used_percent >= f64::from(*threshold))
            .collect();
        let alert = |kind| Alert {
            window_label: window.label.clone(),
            kind,
            resets_at: window.resets_at.clone(),
            used_percent: window.used_percent,
        };
        // A missing reset cannot prove a new period; percentage drops never rearm alerts.
        match memory.get_mut(&key) {
            Some(previous) if same_period(previous.reset.as_ref(), window.resets_at.as_ref()) => {
                let highest_sent = previous.sent.last().copied().unwrap_or(0);
                if let Some(threshold) = reached.last().copied().filter(|t| *t > highest_sent) {
                    alerts.push(alert(AlertKind::Threshold(threshold)));
                }
                previous.sent.extend(reached);
                if window.resets_at.is_some() {
                    previous.reset.clone_from(&window.resets_at);
                }
            }
            previous => {
                let reset = previous.is_some_and(|previous| {
                    !previous.sent.is_empty()
                        && tracked(window)
                        && parse(previous.reset.as_ref()) < parse(window.resets_at.as_ref())
                });
                if reset {
                    alerts.push(alert(AlertKind::Reset));
                }
                if let Some(threshold) = reached.last().copied() {
                    alerts.push(alert(AlertKind::Threshold(threshold)));
                }
                memory.insert(
                    key,
                    WindowAlerts {
                        reset: window.resets_at.clone(),
                        sent: reached,
                    },
                );
            }
        }
    }
    alerts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::UsageWindow;

    const RESET: &str = "2100-01-01T00:00:00Z";

    fn now() -> DateTime<Utc> {
        "2099-12-31T22:40:00Z".parse().unwrap()
    }

    fn reading(used: f64, reset: &str) -> Snapshot {
        labelled("Session", used, reset)
    }

    fn labelled(label: &str, used: f64, reset: &str) -> Snapshot {
        Snapshot {
            windows: vec![UsageWindow {
                id: "session".into(),
                label: label.into(),
                used_percent: used,
                resets_at: Some(reset.into()),
                window_seconds: Some(18000),
            }],
            fetched_at: "2026-10-15T12:00:00Z".into(),
            extra_usage: None,
            reset_grants: None,
        }
    }

    fn kinds(alerts: &[Alert]) -> Vec<AlertKind> {
        alerts.iter().map(|alert| alert.kind.clone()).collect()
    }

    fn check(memory: &mut AlertMemory, snapshot: &Snapshot) -> Vec<AlertKind> {
        kinds(&observe(memory, "a", snapshot, &[80, 95, 100], now()))
    }

    #[test]
    fn fresh_account_past_threshold_alerts_once_for_the_highest() {
        let mut memory = AlertMemory::new();
        assert_eq!(
            check(&mut memory, &reading(97.0, RESET)),
            vec![AlertKind::Threshold(95)]
        );
        assert!(check(&mut memory, &reading(97.0, RESET)).is_empty());
        assert!(check(&mut memory, &reading(10.0, RESET)).is_empty());
    }

    #[test]
    fn restart_with_persisted_alerts_has_no_burst() {
        let mut memory = AlertMemory::new();
        memory.insert(
            "a/session".into(),
            WindowAlerts {
                reset: Some(RESET.into()),
                sent: BTreeSet::from([95]),
            },
        );
        assert!(check(&mut memory, &reading(97.0, RESET)).is_empty());
        assert_eq!(
            check(&mut memory, &reading(100.0, RESET)),
            vec![AlertKind::Threshold(100)]
        );
    }

    #[test]
    fn each_threshold_once_per_reset() {
        let mut memory = AlertMemory::new();
        assert!(check(&mut memory, &reading(40.0, RESET)).is_empty());
        assert_eq!(
            check(&mut memory, &reading(81.0, RESET)),
            vec![AlertKind::Threshold(80)]
        );
        assert_eq!(
            check(&mut memory, &reading(96.0, RESET)),
            vec![AlertKind::Threshold(95)]
        );
        assert!(check(&mut memory, &reading(90.0, RESET)).is_empty());
        assert!(check(&mut memory, &reading(96.0, RESET)).is_empty());
        let next = "2100-01-01T05:00:00Z";
        assert_eq!(
            check(&mut memory, &reading(1.0, next)),
            vec![AlertKind::Reset]
        );
        assert!(check(&mut memory, &reading(2.0, next)).is_empty());
        assert_eq!(
            check(&mut memory, &reading(100.0, next)),
            vec![AlertKind::Threshold(100)]
        );
    }

    #[test]
    fn reset_jitter_within_a_minute_is_the_same_period() {
        let mut memory = AlertMemory::new();
        assert_eq!(
            check(&mut memory, &reading(96.0, RESET)),
            vec![AlertKind::Threshold(95)]
        );
        for reset in [
            "2100-01-01T00:00:30Z",
            "2099-12-31T23:59:45Z",
            "2100-01-01T00:00:40Z",
            "2100-01-01T00:01:35Z",
        ] {
            assert!(check(&mut memory, &reading(96.0, reset)).is_empty());
        }
        assert_eq!(
            check(&mut memory, &reading(96.0, "2100-01-01T00:03:00Z")),
            vec![AlertKind::Reset, AlertKind::Threshold(95)]
        );
    }

    #[test]
    fn reset_alert_needs_a_used_session_or_weekly_window() {
        let next = "2100-01-01T05:00:00Z";
        let mut memory = AlertMemory::new();
        check(&mut memory, &reading(50.0, RESET));
        assert!(check(&mut memory, &reading(0.0, next)).is_empty());
        let mut memory = AlertMemory::new();
        check(&mut memory, &labelled("Sonnet", 90.0, RESET));
        assert!(check(&mut memory, &labelled("Sonnet", 0.0, next)).is_empty());
        let mut memory = AlertMemory::new();
        check(&mut memory, &labelled("Weekly", 90.0, next));
        assert!(check(&mut memory, &labelled("Weekly", 0.0, RESET)).is_empty());
    }

    #[test]
    fn copy_is_terse() {
        let alert = Alert {
            window_label: "Session".into(),
            kind: AlertKind::Threshold(95),
            resets_at: Some(RESET.into()),
            used_percent: 95.0,
        };
        assert_eq!(alert.title("Personal"), "Personal · Session 5% left");
        let jumped = Alert {
            used_percent: 97.4,
            ..alert.clone()
        };
        assert_eq!(jumped.title("Personal"), "Personal · Session 2% left");
        assert_eq!(
            alert.body_in(&Utc, now()).as_deref(),
            Some("Resets in 1h 20m · Fri 00:00")
        );
        let early = Alert {
            kind: AlertKind::Threshold(80),
            used_percent: 80.0,
            ..alert.clone()
        };
        assert_eq!(early.title("Personal"), "Personal · Session 20% left");
        let full = Alert {
            kind: AlertKind::Threshold(100),
            ..alert.clone()
        };
        assert_eq!(full.title("Work"), "Work · Session limit reached");
        let reset = Alert {
            kind: AlertKind::Reset,
            resets_at: None,
            ..alert
        };
        assert_eq!(reset.title("Personal"), "Personal · Session limit reset");
        assert_eq!(reset.body(now()), None);
        assert_eq!(short_duration(30_000), "<1m");
        assert_eq!(reset.body_in(&Utc, now()), None);
        assert_eq!(short_duration(90 * 3_600_000), "3d 18h");
    }

    #[test]
    fn reset_clock_names_the_day_only_when_needed() {
        let at = |value: &str| value.parse::<DateTime<Utc>>().unwrap();
        let now = at("2099-12-31T10:00:00Z");
        assert_eq!(clock_in(&Utc, at("2099-12-31T16:30:00Z"), now), "16:30");
        assert_eq!(clock_in(&Utc, at("2100-01-01T09:05:00Z"), now), "Fri 09:05");
        assert_eq!(
            clock_in(&Utc, at("2100-01-08T09:05:00Z"), now),
            "Jan 8 09:05"
        );
        let east = chrono::FixedOffset::east_opt(9 * 3600).unwrap();
        assert_eq!(
            clock_in(&east, at("2099-12-31T16:30:00Z"), now),
            "Fri 01:30"
        );
    }

    #[test]
    fn accounts_do_not_share_threshold_memory() {
        let mut memory = AlertMemory::new();
        for account in ["a", "b"] {
            observe(&mut memory, account, &reading(10.0, RESET), &[80], now());
        }
        for account in ["a", "b"] {
            assert_eq!(
                observe(&mut memory, account, &reading(85.0, RESET), &[80], now()).len(),
                1
            );
        }
    }

    #[test]
    fn rejects_invalid_persisted_alert_dates_and_thresholds() {
        let valid = WindowAlerts {
            reset: Some(RESET.into()),
            sent: BTreeSet::from([80, 100]),
        };
        assert!(valid.validate().is_ok());
        assert!(WindowAlerts {
            reset: None,
            sent: BTreeSet::new()
        }
        .validate()
        .is_ok());
        assert!(WindowAlerts {
            reset: Some("tomorrow".into()),
            ..valid.clone()
        }
        .validate()
        .is_err());
        for threshold in [0, 101, 255] {
            assert!(WindowAlerts {
                sent: BTreeSet::from([threshold]),
                ..valid.clone()
            }
            .validate()
            .is_err());
        }
    }

    #[test]
    fn prunes_removed_windows_only_for_the_refreshed_account() {
        let mut memory = AlertMemory::new();
        let snapshot = reading(10.0, RESET);
        observe(&mut memory, "a", &snapshot, &[80], now());
        observe(&mut memory, "ab", &snapshot, &[80], now());
        let mut replacement = snapshot.clone();
        replacement.windows[0].id = "weekly".into();
        observe(&mut memory, "a", &replacement, &[80], now());
        assert!(!memory.contains_key("a/session"));
        assert!(memory.contains_key("a/weekly"));
        assert!(memory.contains_key("ab/session"));
        replacement.windows.clear();
        observe(&mut memory, "a", &replacement, &[80], now());
        assert!(!memory.contains_key("a/weekly"));
        assert!(memory.contains_key("ab/session"));
    }

    #[test]
    fn expired_windows_never_trigger_threshold_alerts() {
        let mut memory = AlertMemory::new();
        memory.insert(
            "a/session".into(),
            WindowAlerts {
                reset: Some("2000-01-01T00:00:00Z".into()),
                sent: BTreeSet::new(),
            },
        );
        assert!(observe(
            &mut memory,
            "a",
            &reading(100.0, "2000-01-01T00:00:00Z"),
            &[80, 100],
            now()
        )
        .is_empty());
        assert!(memory["a/session"].sent.is_empty());
    }

    #[test]
    fn unknown_reset_never_rearms_from_a_percentage_drop() {
        let mut memory = AlertMemory::new();
        let mut snapshot = reading(10.0, RESET);
        snapshot.windows[0].resets_at = None;
        assert!(check(&mut memory, &snapshot).is_empty());
        snapshot.windows[0].used_percent = 90.0;
        assert_eq!(check(&mut memory, &snapshot).len(), 1);
        snapshot.windows[0].used_percent = 10.0;
        assert!(check(&mut memory, &snapshot).is_empty());
        snapshot.windows[0].used_percent = 90.0;
        assert!(check(&mut memory, &snapshot).is_empty());
        snapshot.windows[0].resets_at = Some(RESET.into());
        assert!(check(&mut memory, &snapshot).is_empty());
    }
}
