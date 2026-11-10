use super::{
    burn::BurnState,
    model::{Snapshot, UsageWindow, WindowScope},
};
use chrono::{DateTime, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;

pub type AlertMemory = HashMap<String, WindowAlerts>;

const RESET_JITTER_SECONDS: i64 = 60;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowAlerts {
    reset: Option<String>,
    sent: BTreeSet<u8>,
    #[serde(
        default,
        deserialize_with = "super::burn::lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub(super) burn: Option<BurnState>,
    /// A run-out warning was sent in this period.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) forecast: bool,
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
    Burn { increase: u8 },
    RunsOut,
}

#[derive(Clone, Debug)]
pub struct Alert {
    pub window_id: String,
    pub window_label: String,
    pub kind: AlertKind,
    pub resets_at: Option<String>,
    pub used_percent: f64,
    /// When the window is projected to reach its limit, if that is before it resets.
    pub runs_out: Option<DateTime<Utc>>,
}

impl Alert {
    pub fn title(&self, account_label: &str, now: DateTime<Utc>, clock: Clock12h) -> String {
        self.title_in(&Local, clock, account_label, now)
    }

    fn title_in<Tz: TimeZone>(
        &self,
        zone: &Tz,
        clock: Clock12h,
        account_label: &str,
        now: DateTime<Utc>,
    ) -> String
    where
        Tz::Offset: std::fmt::Display,
    {
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
            AlertKind::Burn { increase } => {
                format!("{account_label} · {window}: {increase}% used in the last hour")
            }
            AlertKind::RunsOut => match self.runs_out {
                Some(out) => format!(
                    "{account_label} · {window} runs out around {}",
                    clock_in(zone, clock, out, now)
                ),
                None => format!("{account_label} · {window} runs out before reset"),
            },
        }
    }

    pub fn body(&self, now: DateTime<Utc>, clock: Clock12h) -> Option<String> {
        self.body_in(&Local, clock, now)
    }

    fn body_in<Tz: TimeZone>(
        &self,
        zone: &Tz,
        clock: Clock12h,
        now: DateTime<Utc>,
    ) -> Option<String>
    where
        Tz::Offset: std::fmt::Display,
    {
        let reset = parse(self.resets_at.as_ref()).filter(|reset| *reset > now);
        if self.kind == AlertKind::RunsOut {
            let left = (100.0 - self.used_percent).floor().clamp(0.0, 100.0);
            return Some(match reset {
                Some(reset) => format!(
                    "{left}% left · Resets in {} · {}",
                    short_duration((reset - now).num_milliseconds()),
                    clock_in(zone, clock, reset, now)
                ),
                None => format!("{left}% left"),
            });
        }
        if let Some(out) = self.runs_out.filter(|out| *out > now) {
            let out = clock_in(zone, clock, out, now);
            return Some(match reset {
                Some(reset) => format!(
                    "At this rate it runs out around {out}, before the {} reset",
                    clock_in(zone, clock, reset, now)
                ),
                None => format!("At this rate it runs out around {out}"),
            });
        }
        let reset = reset?;
        Some(format!(
            "Resets in {} · {}",
            short_duration((reset - now).num_milliseconds()),
            clock_in(zone, clock, reset, now)
        ))
    }
}

/// Whether clock times read `4:30 PM` (true) or `16:30`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Clock12h(pub bool);

impl Clock12h {
    /// The OS preference, read once per process.
    pub fn system() -> Self {
        static SYSTEM: OnceLock<Clock12h> = OnceLock::new();
        *SYSTEM.get_or_init(|| Clock12h(system_clock::prefers_12h()))
    }
}

pub fn reset_clock(at: DateTime<Utc>, now: DateTime<Utc>, clock: Clock12h) -> String {
    clock_in(&Local, clock, at, now)
}

fn clock_in<Tz: TimeZone>(
    zone: &Tz,
    clock: Clock12h,
    at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let local = at.with_timezone(zone);
    let time = if clock.0 { "%-I:%M %p" } else { "%H:%M" };
    let pattern = if local.date_naive() == now.with_timezone(zone).date_naive() {
        time.to_owned()
    } else if (at - now).abs() < chrono::Duration::days(6) {
        format!("%a {time}")
    } else {
        format!("%b %-d {time}")
    };
    local.format(&pattern).to_string()
}

/// Reads the OS clock preference. Nothing here runs in tests: the formatters
/// take a `Clock12h` instead.
mod system_clock {
    /// Locales whose default short time is 12-hour; most others use 24.
    const TWELVE_HOUR_REGIONS: [&str; 14] = [
        "US", "CA", "AU", "NZ", "IN", "PH", "PK", "BD", "EG", "SA", "MX", "CO", "KR", "TW",
    ];

    /// `en_US`, `en-US`, `en_US@rg=gbzzzz`, `en_US.UTF-8` and a bare `en` all read
    /// as 12-hour; `de_DE` and `en_GB` as 24.
    pub(super) fn locale_prefers_12h(locale: &str) -> bool {
        let tag = locale.split(['@', '.']).next().unwrap_or_default().trim();
        let mut parts = tag.split(['_', '-']);
        let language = parts.next().unwrap_or_default();
        match parts.find(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic())) {
            Some(region) => TWELVE_HOUR_REGIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(region)),
            None => language.eq_ignore_ascii_case("en"),
        }
    }

    /// Windows `sShortTime`: `h:mm tt` is 12-hour, `HH:mm` is 24.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(super) fn short_time_prefers_12h(pattern: &str) -> bool {
        pattern.contains('h') && !pattern.contains('H')
    }

    #[cfg(target_os = "macos")]
    pub(super) fn prefers_12h() -> bool {
        let read = |key: &str| {
            std::process::Command::new("defaults")
                .args(["read", "-g", key])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        };
        if read("AppleICUForce12HourTime").as_deref() == Some("1") {
            return true;
        }
        if read("AppleICUForce24HourTime").as_deref() == Some("1") {
            return false;
        }
        locale_prefers_12h(&read("AppleLocale").unwrap_or_else(env_locale))
    }

    #[cfg(windows)]
    pub(super) fn prefers_12h() -> bool {
        let output = std::process::Command::new("reg")
            .args([
                "query",
                r"HKCU\Control Panel\International",
                "/v",
                "sShortTime",
            ])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned());
        let pattern = output.as_deref().and_then(|text| {
            text.lines()
                .find(|line| line.trim_start().starts_with("sShortTime"))
                .and_then(|line| line.split("REG_SZ").nth(1))
                .map(str::trim)
        });
        match pattern {
            Some(pattern) => short_time_prefers_12h(pattern),
            None => locale_prefers_12h(&env_locale()),
        }
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    pub(super) fn prefers_12h() -> bool {
        locale_prefers_12h(&env_locale())
    }

    fn env_locale() -> String {
        ["LC_ALL", "LC_TIME", "LANG"]
            .iter()
            .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
            .unwrap_or_default()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn locales_pick_the_clock() {
            for locale in ["en_US", "en-US", "en_US.UTF-8", "en", "es_MX"] {
                assert!(locale_prefers_12h(locale), "{locale}");
            }
            for locale in ["en_GB", "de_DE", "fr_FR", "ja_JP", "ru_RU", ""] {
                assert!(!locale_prefers_12h(locale), "{locale:?}");
            }
        }

        #[test]
        fn windows_short_time_patterns_pick_the_clock() {
            assert!(short_time_prefers_12h("h:mm tt"));
            assert!(short_time_prefers_12h("hh:mm tt"));
            assert!(!short_time_prefers_12h("HH:mm"));
            assert!(!short_time_prefers_12h("H:mm"));
            assert!(!short_time_prefers_12h(""));
        }
    }
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

/// Session and weekly windows, by id: Codex labels a core window by its length,
/// so a 30-day or 3-day plan would never match a "Session"/"Weekly" label.
pub(super) fn tracked(window: &UsageWindow) -> bool {
    window.scope() == WindowScope::Core
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
    // A window missing from an incomplete reading may still exist.
    if !snapshot.incomplete {
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
    }
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
            window_id: window.id.clone(),
            window_label: window.label.clone(),
            kind,
            resets_at: window.resets_at.clone(),
            used_percent: window.used_percent,
            runs_out: None,
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
                let burn = previous.as_ref().and_then(|previous| previous.burn.clone());
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
                        burn,
                        forecast: false,
                    },
                );
            }
        }
    }
    alerts
}

/// Takes back what `alert` recorded, so the first reading after a restart raises it again.
/// `before` is the window's memory from before the alert. A reset is taken back only while
/// nothing else in the new period was shown: with no thresholds recorded, or with a threshold
/// alert for the window also waiting, which outranks any shown one.
pub(super) fn unrecord(
    memory: &mut AlertMemory,
    account_id: &str,
    alert: &Alert,
    before: Option<&WindowAlerts>,
    threshold_waiting: bool,
) {
    let Some(entry) = memory.get_mut(&format!("{account_id}/{}", alert.window_id)) else {
        return;
    };
    if !same_period(entry.reset.as_ref(), alert.resets_at.as_ref()) {
        return;
    }
    match alert.kind {
        AlertKind::Threshold(threshold) => entry.sent.retain(|sent| *sent < threshold),
        AlertKind::RunsOut => entry.forecast = false,
        AlertKind::Burn { .. } => entry.burn = before.and_then(|before| before.burn.clone()),
        AlertKind::Reset => {
            if let Some(before) = before.filter(|_| threshold_waiting || entry.sent.is_empty()) {
                entry.reset.clone_from(&before.reset);
                entry.sent.clone_from(&before.sent);
            }
        }
    }
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

    #[test]
    fn an_incomplete_reading_keeps_alert_memory_for_windows_it_dropped() {
        let mut memory = AlertMemory::new();
        let mut both = reading(90.0, RESET);
        both.windows
            .push(labelled("Sonnet", 90.0, RESET).windows.remove(0));
        observe(&mut memory, "a", &both, &[80], now());
        assert_eq!(memory.len(), 2);
        let mut partial = reading(90.0, RESET);
        partial.incomplete = true;
        assert!(observe(&mut memory, "a", &partial, &[80], now()).is_empty());
        assert_eq!(memory.len(), 2);
        assert!(observe(&mut memory, "a", &both, &[80], now()).is_empty());
        observe(&mut memory, "a", &reading(90.0, RESET), &[80], now());
        assert_eq!(memory.len(), 1);
    }

    fn labelled(label: &str, used: f64, reset: &str) -> Snapshot {
        let id = match label {
            "Session" => "five_hour",
            "Weekly" => "seven_day",
            "Monthly" => "primary_window",
            _ => "seven_day_sonnet",
        };
        Snapshot {
            windows: vec![UsageWindow {
                id: id.into(),
                label: label.into(),
                used_percent: used,
                resets_at: Some(reset.into()),
                window_seconds: Some(18000),
            }],
            fetched_at: "2026-10-15T12:00:00Z".into(),
            extra_usage: None,
            reset_grants: None,
            balance: None,
            incomplete: false,
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
            "a/five_hour".into(),
            WindowAlerts {
                reset: Some(RESET.into()),
                sent: BTreeSet::from([95]),
                burn: None,
                forecast: false,
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
        let mut memory = AlertMemory::new();
        check(&mut memory, &labelled("Monthly", 90.0, RESET));
        assert_eq!(
            check(&mut memory, &labelled("Monthly", 0.0, next)),
            vec![AlertKind::Reset]
        );
    }

    #[test]
    fn copy_is_terse() {
        let alert = Alert {
            window_id: "session".into(),
            window_label: "Session".into(),
            kind: AlertKind::Threshold(95),
            resets_at: Some(RESET.into()),
            used_percent: 95.0,
            runs_out: None,
        };
        assert_eq!(
            alert.title_in(&Utc, Clock12h(false), "Personal", now()),
            "Personal · Session 5% left"
        );
        let jumped = Alert {
            used_percent: 97.4,
            ..alert.clone()
        };
        assert_eq!(
            jumped.title_in(&Utc, Clock12h(false), "Personal", now()),
            "Personal · Session 2% left"
        );
        assert_eq!(
            alert.body_in(&Utc, Clock12h(false), now()).as_deref(),
            Some("Resets in 1h 20m · Fri 00:00")
        );
        let early = Alert {
            kind: AlertKind::Threshold(80),
            used_percent: 80.0,
            ..alert.clone()
        };
        assert_eq!(
            early.title_in(&Utc, Clock12h(false), "Personal", now()),
            "Personal · Session 20% left"
        );
        let full = Alert {
            kind: AlertKind::Threshold(100),
            ..alert.clone()
        };
        assert_eq!(
            full.title_in(&Utc, Clock12h(false), "Work", now()),
            "Work · Session limit reached"
        );
        let reset = Alert {
            kind: AlertKind::Reset,
            resets_at: None,
            ..alert
        };
        assert_eq!(
            reset.title_in(&Utc, Clock12h(false), "Personal", now()),
            "Personal · Session limit reset"
        );
        assert_eq!(reset.body(now(), Clock12h(false)), None);
        assert_eq!(short_duration(30_000), "<1m");
        assert_eq!(reset.body_in(&Utc, Clock12h(false), now()), None);
        assert_eq!(short_duration(90 * 3_600_000), "3d 18h");
    }

    #[test]
    fn run_out_copy_names_the_time_and_the_reset() {
        let reset: DateTime<Utc> = RESET.parse().unwrap();
        let alert = Alert {
            window_id: "five_hour".into(),
            window_label: "Session".into(),
            kind: AlertKind::Burn { increase: 20 },
            resets_at: Some(RESET.into()),
            used_percent: 64.0,
            runs_out: Some(reset - chrono::Duration::minutes(30)),
        };
        let title = |alert: &Alert| alert.title_in(&Utc, Clock12h(false), "Personal", now());
        let body = |alert: &Alert| alert.body_in(&Utc, Clock12h(false), now());
        assert_eq!(
            title(&alert),
            "Personal · Session: 20% used in the last hour"
        );
        assert_eq!(
            body(&alert).as_deref(),
            Some("At this rate it runs out around 23:30, before the Fri 00:00 reset")
        );
        let steady = Alert {
            runs_out: None,
            ..alert.clone()
        };
        assert_eq!(
            body(&steady).as_deref(),
            Some("Resets in 1h 20m · Fri 00:00")
        );
        let passed = Alert {
            runs_out: Some(now() - chrono::Duration::minutes(1)),
            ..alert.clone()
        };
        assert_eq!(
            body(&passed).as_deref(),
            Some("Resets in 1h 20m · Fri 00:00")
        );
        let threshold = Alert {
            kind: AlertKind::Threshold(80),
            used_percent: 81.0,
            ..alert.clone()
        };
        assert_eq!(title(&threshold), "Personal · Session 19% left");
        assert_eq!(
            body(&threshold).as_deref(),
            Some("At this rate it runs out around 23:30, before the Fri 00:00 reset")
        );
        let warning = Alert {
            kind: AlertKind::RunsOut,
            used_percent: 63.4,
            ..alert.clone()
        };
        assert_eq!(title(&warning), "Personal · Session runs out around 23:30");
        assert_eq!(
            body(&warning).as_deref(),
            Some("36% left · Resets in 1h 20m · Fri 00:00")
        );
        let unknown = Alert {
            runs_out: None,
            resets_at: None,
            ..warning
        };
        assert_eq!(title(&unknown), "Personal · Session runs out before reset");
        assert_eq!(body(&unknown).as_deref(), Some("36% left"));
    }

    #[test]
    fn reset_clock_names_the_day_only_when_needed() {
        let at = |value: &str| value.parse::<DateTime<Utc>>().unwrap();
        let now = at("2099-12-31T10:00:00Z");
        assert_eq!(
            clock_in(&Utc, Clock12h(false), at("2099-12-31T16:30:00Z"), now),
            "16:30"
        );
        assert_eq!(
            clock_in(&Utc, Clock12h(false), at("2100-01-01T09:05:00Z"), now),
            "Fri 09:05"
        );
        assert_eq!(
            clock_in(&Utc, Clock12h(false), at("2100-01-08T09:05:00Z"), now),
            "Jan 8 09:05"
        );
        let east = chrono::FixedOffset::east_opt(9 * 3600).unwrap();
        assert_eq!(
            clock_in(&east, Clock12h(false), at("2099-12-31T16:30:00Z"), now),
            "Fri 01:30"
        );
    }

    #[test]
    fn twelve_hour_clock_drops_the_leading_zero_and_names_the_half() {
        let at = |value: &str| value.parse::<DateTime<Utc>>().unwrap();
        let today = at("2099-12-31T10:00:00Z");
        let twelve = Clock12h(true);
        assert_eq!(
            clock_in(&Utc, twelve, at("2099-12-31T16:30:00Z"), today),
            "4:30 PM"
        );
        assert_eq!(
            clock_in(&Utc, twelve, at("2099-12-31T00:05:00Z"), today),
            "12:05 AM"
        );
        assert_eq!(
            clock_in(&Utc, twelve, at("2100-01-01T16:30:00Z"), today),
            "Fri 4:30 PM"
        );
        assert_eq!(
            clock_in(&Utc, twelve, at("2100-01-08T09:05:00Z"), today),
            "Jan 8 9:05 AM"
        );
        let alert = Alert {
            window_id: "session".into(),
            window_label: "Session".into(),
            kind: AlertKind::Threshold(95),
            resets_at: Some(RESET.into()),
            used_percent: 95.0,
            runs_out: None,
        };
        assert_eq!(
            alert.body_in(&Utc, twelve, now()).as_deref(),
            Some("Resets in 1h 20m · Fri 12:00 AM")
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
            burn: None,
            forecast: false,
        };
        assert!(valid.validate().is_ok());
        assert!(WindowAlerts {
            reset: None,
            sent: BTreeSet::new(),
            burn: None,
            forecast: false,
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
    fn unrecord_takes_back_only_what_the_alert_recorded() {
        let burn: BurnState =
            serde_json::from_value(serde_json::json!({"alertedAt": "2099-12-31T22:00:00Z"}))
                .unwrap();
        let mut memory = AlertMemory::new();
        memory.insert(
            "a/five_hour".into(),
            WindowAlerts {
                reset: Some(RESET.into()),
                sent: BTreeSet::from([80, 95]),
                burn: Some(burn),
                forecast: true,
            },
        );
        let alert = |kind| Alert {
            window_id: "five_hour".into(),
            window_label: "Session".into(),
            kind,
            resets_at: Some(RESET.into()),
            used_percent: 96.0,
            runs_out: None,
        };
        let entry = |memory: &AlertMemory| memory["a/five_hour"].clone();
        unrecord(&mut memory, "a", &alert(AlertKind::RunsOut), None, false);
        assert!(!entry(&memory).forecast);
        assert!(entry(&memory).burn.is_some());
        unrecord(
            &mut memory,
            "a",
            &alert(AlertKind::Burn { increase: 20 }),
            None,
            false,
        );
        assert!(entry(&memory).burn.is_none());
        unrecord(
            &mut memory,
            "a",
            &alert(AlertKind::Threshold(95)),
            None,
            false,
        );
        assert_eq!(entry(&memory).sent, BTreeSet::from([80]));
        let earlier_period = Alert {
            resets_at: Some("2099-12-31T17:00:00Z".into()),
            ..alert(AlertKind::Threshold(80))
        };
        unrecord(&mut memory, "a", &earlier_period, None, false);
        assert_eq!(entry(&memory).sent, BTreeSet::from([80]));
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
        assert!(!memory.contains_key("a/five_hour"));
        assert!(memory.contains_key("a/weekly"));
        assert!(memory.contains_key("ab/five_hour"));
        replacement.windows.clear();
        observe(&mut memory, "a", &replacement, &[80], now());
        assert!(!memory.contains_key("a/weekly"));
        assert!(memory.contains_key("ab/five_hour"));
    }

    #[test]
    fn expired_windows_never_trigger_threshold_alerts() {
        let mut memory = AlertMemory::new();
        memory.insert(
            "a/five_hour".into(),
            WindowAlerts {
                reset: Some("2000-01-01T00:00:00Z".into()),
                sent: BTreeSet::new(),
                burn: None,
                forecast: false,
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
        assert!(memory["a/five_hour"].sent.is_empty());
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
