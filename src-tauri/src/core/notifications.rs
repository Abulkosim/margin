use super::model::Snapshot;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};

pub type AlertMemory = HashMap<String, WindowAlerts>;

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

pub struct Alert {
    pub window_label: String,
    pub threshold: u8,
}

pub fn observe(
    memory: &mut AlertMemory,
    account_id: &str,
    snapshot: &Snapshot,
    thresholds: &[u8],
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
        if window.resets_at.as_ref().is_some_and(|reset| {
            chrono::DateTime::parse_from_rfc3339(reset)
                .map_or(true, |reset| reset <= chrono::Utc::now())
        }) {
            continue;
        }
        let key = format!("{account_id}/{}", window.id);
        let reached: BTreeSet<u8> = thresholds
            .iter()
            .copied()
            .filter(|threshold| window.used_percent >= f64::from(*threshold))
            .collect();
        // A missing reset cannot prove a new period; percentage drops never rearm alerts.
        match memory.get_mut(&key) {
            Some(previous) if previous.reset == window.resets_at => {
                if let Some(threshold) = reached.difference(&previous.sent).last().copied() {
                    alerts.push(Alert {
                        window_label: window.label.clone(),
                        threshold,
                    });
                }
                previous.sent.extend(reached);
            }
            _ => {
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

    fn reading(used: f64, reset: &str) -> Snapshot {
        Snapshot {
            windows: vec![UsageWindow {
                id: "session".into(),
                label: "Session".into(),
                used_percent: used,
                resets_at: Some(reset.into()),
                window_seconds: Some(18000),
            }],
            fetched_at: "2026-10-15T12:00:00Z".into(),
            extra_usage: None,
        }
    }

    #[test]
    fn no_startup_burst_and_each_threshold_once_per_reset() {
        let mut memory = AlertMemory::new();
        let thresholds = [80, 95, 100];
        assert!(observe(
            &mut memory,
            "a",
            &reading(81.0, "2100-01-01T00:00:00Z"),
            &thresholds
        )
        .is_empty());
        let alerts = observe(
            &mut memory,
            "a",
            &reading(96.0, "2100-01-01T00:00:00Z"),
            &thresholds,
        );
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].threshold, 95);
        observe(
            &mut memory,
            "a",
            &reading(90.0, "2100-01-01T00:00:00Z"),
            &thresholds,
        );
        assert!(observe(
            &mut memory,
            "a",
            &reading(96.0, "2100-01-01T00:00:00Z"),
            &thresholds
        )
        .is_empty());
        assert!(observe(
            &mut memory,
            "a",
            &reading(1.0, "2100-01-02T00:00:00Z"),
            &thresholds
        )
        .is_empty());
        assert_eq!(
            observe(
                &mut memory,
                "a",
                &reading(100.0, "2100-01-02T00:00:00Z"),
                &thresholds
            )[0]
            .threshold,
            100
        );
    }

    #[test]
    fn accounts_do_not_share_threshold_memory() {
        let mut memory = AlertMemory::new();
        observe(
            &mut memory,
            "a",
            &reading(10.0, "2100-01-01T00:00:00Z"),
            &[80],
        );
        observe(
            &mut memory,
            "b",
            &reading(10.0, "2100-01-01T00:00:00Z"),
            &[80],
        );
        assert_eq!(
            observe(
                &mut memory,
                "a",
                &reading(85.0, "2100-01-01T00:00:00Z"),
                &[80]
            )
            .len(),
            1
        );
        assert_eq!(
            observe(
                &mut memory,
                "b",
                &reading(85.0, "2100-01-01T00:00:00Z"),
                &[80]
            )
            .len(),
            1
        );
    }

    #[test]
    fn rejects_invalid_persisted_alert_dates_and_thresholds() {
        let valid = WindowAlerts {
            reset: Some("2100-01-01T00:00:00Z".into()),
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
        let snapshot = reading(10.0, "2100-01-01T00:00:00Z");
        observe(&mut memory, "a", &snapshot, &[80]);
        observe(&mut memory, "ab", &snapshot, &[80]);
        let mut replacement = snapshot.clone();
        replacement.windows[0].id = "weekly".into();
        observe(&mut memory, "a", &replacement, &[80]);
        assert!(!memory.contains_key("a/session"));
        assert!(memory.contains_key("a/weekly"));
        assert!(memory.contains_key("ab/session"));
        replacement.windows.clear();
        observe(&mut memory, "a", &replacement, &[80]);
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
            &[80, 100]
        )
        .is_empty());
        assert!(memory["a/session"].sent.is_empty());
    }

    #[test]
    fn unknown_reset_never_rearms_from_a_percentage_drop() {
        let mut memory = AlertMemory::new();
        let mut snapshot = reading(10.0, "2100-01-01T00:00:00Z");
        snapshot.windows[0].resets_at = None;
        assert!(observe(&mut memory, "a", &snapshot, &[80]).is_empty());
        snapshot.windows[0].used_percent = 90.0;
        assert_eq!(observe(&mut memory, "a", &snapshot, &[80]).len(), 1);
        snapshot.windows[0].used_percent = 10.0;
        assert!(observe(&mut memory, "a", &snapshot, &[80]).is_empty());
        snapshot.windows[0].used_percent = 90.0;
        assert!(observe(&mut memory, "a", &snapshot, &[80]).is_empty());
    }
}
