use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrayMode {
    Pinned,
    #[default]
    Lowest,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TrayMetric {
    #[default]
    Core,
    All,
    Session,
    Weekly,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OverviewLayout {
    #[default]
    Cards,
    Compact,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NotificationPermission {
    #[default]
    Unasked,
    Granted,
    Denied,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: Theme,
    pub overview_layout: OverviewLayout,
    pub poll_interval_seconds: u64,
    pub idle_poll_interval_seconds: u64,
    pub notifications: bool,
    pub thresholds: Vec<u8>,
    pub notify_on_reset: bool,
    pub notification_permission: NotificationPermission,
    pub tray_mode: TrayMode,
    pub tray_metric: TrayMetric,
    pub tray_show_percent: bool,
    pub pinned_account_id: Option<String>,
    pub autostart: bool,
    pub auto_check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            overview_layout: OverviewLayout::Cards,
            poll_interval_seconds: 60,
            idle_poll_interval_seconds: 300,
            notifications: true,
            thresholds: vec![80, 95, 100],
            notify_on_reset: false,
            notification_permission: NotificationPermission::Unasked,
            tray_mode: TrayMode::Lowest,
            tray_metric: TrayMetric::Core,
            tray_show_percent: false,
            pinned_account_id: None,
            autostart: false,
            auto_check_updates: true,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(30..=3600).contains(&self.poll_interval_seconds) {
            return Err("Refresh interval must be between 30 seconds and 1 hour.".into());
        }
        if !(self.poll_interval_seconds..=86400).contains(&self.idle_poll_interval_seconds) {
            return Err(
                "Idle refresh must be at least the active interval, up to 24 hours.".into(),
            );
        }
        if self.thresholds.is_empty()
            || self.thresholds.len() > 10
            || self
                .thresholds
                .iter()
                .any(|value| !(1..=100).contains(value))
            || self.thresholds.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err("Choose distinct, increasing thresholds between 1 and 100%.".into());
        }
        Ok(())
    }

    pub fn recover(value: Option<Value>, notes: &mut Vec<String>) -> Self {
        let defaults = Self::default();
        let map = match value {
            None | Some(Value::Null) => return defaults,
            Some(Value::Object(map)) => map,
            Some(_) => {
                notes.push("Settings were reset to defaults.".into());
                return defaults;
            }
        };
        let mut settings = Self {
            theme: field(&map, "theme", defaults.theme, "Theme", notes),
            overview_layout: field(
                &map,
                "overviewLayout",
                defaults.overview_layout,
                "Overview layout",
                notes,
            ),
            poll_interval_seconds: field(
                &map,
                "pollIntervalSeconds",
                defaults.poll_interval_seconds,
                "Refresh interval",
                notes,
            ),
            idle_poll_interval_seconds: field(
                &map,
                "idlePollIntervalSeconds",
                defaults.idle_poll_interval_seconds,
                "Background refresh",
                notes,
            ),
            notifications: field(
                &map,
                "notifications",
                defaults.notifications,
                "Usage alerts",
                notes,
            ),
            thresholds: field(
                &map,
                "thresholds",
                defaults.thresholds.clone(),
                "Alert thresholds",
                notes,
            ),
            notify_on_reset: field(
                &map,
                "notifyOnReset",
                defaults.notify_on_reset,
                "Reset alerts",
                notes,
            ),
            notification_permission: field(
                &map,
                "notificationPermission",
                defaults.notification_permission,
                "Notification permission",
                notes,
            ),
            tray_mode: field(&map, "trayMode", defaults.tray_mode, "Tray account", notes),
            tray_metric: field(
                &map,
                "trayMetric",
                defaults.tray_metric,
                "Tray shows",
                notes,
            ),
            tray_show_percent: field(
                &map,
                "trayShowPercent",
                defaults.tray_show_percent,
                "Show percentage",
                notes,
            ),
            pinned_account_id: field(
                &map,
                "pinnedAccountId",
                None::<String>,
                "Tray account",
                notes,
            ),
            autostart: field(
                &map,
                "autostart",
                defaults.autostart,
                "Launch at login",
                notes,
            ),
            auto_check_updates: field(
                &map,
                "autoCheckUpdates",
                defaults.auto_check_updates,
                "Check for updates daily",
                notes,
            ),
        };
        if !(30..=3600).contains(&settings.poll_interval_seconds) {
            settings.poll_interval_seconds = defaults.poll_interval_seconds;
            reset_note("Refresh interval", notes);
        }
        if !(settings.poll_interval_seconds..=86400).contains(&settings.idle_poll_interval_seconds)
        {
            settings.idle_poll_interval_seconds = defaults
                .idle_poll_interval_seconds
                .max(settings.poll_interval_seconds);
            reset_note("Background refresh", notes);
        }
        if settings.validate().is_err() {
            settings.thresholds = defaults.thresholds;
            reset_note("Alert thresholds", notes);
        }
        settings
    }
}

fn field<T: DeserializeOwned>(
    map: &Map<String, Value>,
    key: &str,
    fallback: T,
    label: &str,
    notes: &mut Vec<String>,
) -> T {
    match map.get(key) {
        None => fallback,
        Some(value) => serde_json::from_value(value.clone()).unwrap_or_else(|_| {
            reset_note(label, notes);
            fallback
        }),
    }
}

fn reset_note(label: &str, notes: &mut Vec<String>) {
    let note = format!("{label} was reset to its default.");
    if !notes.contains(&note) {
        notes.push(note);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_intervals_that_can_hammer_providers() {
        let settings = Settings {
            poll_interval_seconds: 1,
            ..Settings::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_notification_thresholds() {
        let settings = Settings {
            thresholds: vec![80, 80, 100],
            ..Settings::default()
        };
        assert!(settings.validate().is_err());
    }

    #[test]
    fn invalid_settings_fall_back_individually() {
        let mut notes = Vec::new();
        let settings = Settings::recover(
            Some(serde_json::json!({
                "theme": "dark",
                "pollIntervalSeconds": 5,
                "notifications": "yes",
                "thresholds": [95, 80],
                "autostart": true,
                "futureOption": 1
            })),
            &mut notes,
        );
        assert_eq!(settings.theme, Theme::Dark);
        assert!(settings.autostart);
        assert_eq!(settings.poll_interval_seconds, 60);
        assert!(settings.notifications);
        assert_eq!(settings.thresholds, vec![80, 95, 100]);
        assert!(settings.validate().is_ok());
        assert_eq!(notes.len(), 3);
    }

    #[test]
    fn new_installs_follow_the_lowest_account_and_saved_modes_win() {
        let mut notes = Vec::new();
        for value in [None, Some(serde_json::json!({}))] {
            assert_eq!(
                Settings::recover(value, &mut notes).tray_mode,
                TrayMode::Lowest
            );
        }
        let settings = Settings::recover(
            Some(serde_json::json!({ "trayMode": "pinned", "pinnedAccountId": "claude-a" })),
            &mut notes,
        );
        assert_eq!(settings.tray_mode, TrayMode::Pinned);
        assert_eq!(settings.pinned_account_id.as_deref(), Some("claude-a"));
        assert!(notes.is_empty());
    }

    #[test]
    fn tray_metric_defaults_to_core_and_recovers_unknown_values() {
        let mut notes = Vec::new();
        let settings = Settings::recover(
            Some(serde_json::json!({ "trayMode": "lowest" })),
            &mut notes,
        );
        assert_eq!(settings.tray_metric, TrayMetric::Core);
        assert!(notes.is_empty());
        let settings = Settings::recover(
            Some(serde_json::json!({ "trayMetric": "weekly" })),
            &mut notes,
        );
        assert_eq!(settings.tray_metric, TrayMetric::Weekly);
        let settings = Settings::recover(
            Some(serde_json::json!({ "trayMetric": "opus" })),
            &mut notes,
        );
        assert_eq!(settings.tray_metric, TrayMetric::Core);
        assert_eq!(notes, vec!["Tray shows was reset to its default."]);
    }

    #[test]
    fn tray_show_percent_defaults_off_and_recovers_bad_values() {
        let mut notes = Vec::new();
        let settings = Settings::recover(
            Some(serde_json::json!({ "trayMode": "lowest" })),
            &mut notes,
        );
        assert!(!settings.tray_show_percent);
        assert!(notes.is_empty());
        let settings = Settings::recover(
            Some(serde_json::json!({ "trayShowPercent": true })),
            &mut notes,
        );
        assert!(settings.tray_show_percent);
        let settings = Settings::recover(
            Some(serde_json::json!({ "trayShowPercent": "yes" })),
            &mut notes,
        );
        assert!(!settings.tray_show_percent);
        assert_eq!(notes, vec!["Show percentage was reset to its default."]);
        let saved = serde_json::to_value(Settings {
            tray_show_percent: true,
            ..Settings::default()
        })
        .unwrap();
        assert_eq!(saved["trayShowPercent"], true);
    }

    #[test]
    fn notifications_default_on_only_for_new_stores() {
        let mut notes = Vec::new();
        for value in [None, Some(serde_json::json!({}))] {
            let settings = Settings::recover(value, &mut notes);
            assert!(settings.notifications);
            assert!(!settings.notify_on_reset);
            assert_eq!(settings.thresholds, vec![80, 95, 100]);
            assert_eq!(
                settings.notification_permission,
                NotificationPermission::Unasked
            );
        }
        let settings = Settings::recover(
            Some(serde_json::json!({ "notifications": false, "thresholds": [50, 90] })),
            &mut notes,
        );
        assert!(!settings.notifications);
        assert_eq!(settings.thresholds, vec![50, 90]);
        assert!(notes.is_empty());
    }

    #[test]
    fn notification_fields_recover_from_bad_values() {
        let mut notes = Vec::new();
        let settings = Settings::recover(
            Some(serde_json::json!({
                "notifyOnReset": true,
                "notificationPermission": "denied"
            })),
            &mut notes,
        );
        assert!(settings.notify_on_reset);
        assert_eq!(
            settings.notification_permission,
            NotificationPermission::Denied
        );
        let settings = Settings::recover(
            Some(serde_json::json!({
                "notifyOnReset": "sometimes",
                "notificationPermission": "maybe",
                "thresholds": [0, 101]
            })),
            &mut notes,
        );
        assert!(!settings.notify_on_reset);
        assert_eq!(
            settings.notification_permission,
            NotificationPermission::Unasked
        );
        assert_eq!(settings.thresholds, vec![80, 95, 100]);
        assert_eq!(
            notes,
            vec![
                "Reset alerts was reset to its default.",
                "Notification permission was reset to its default.",
                "Alert thresholds was reset to its default.",
            ]
        );
        let saved = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(saved["notifyOnReset"], false);
        assert_eq!(saved["notificationPermission"], "unasked");
    }

    #[test]
    fn overview_layout_defaults_to_cards_and_recovers_unknown_values() {
        let mut notes = Vec::new();
        for value in [None, Some(serde_json::json!({}))] {
            assert_eq!(
                Settings::recover(value, &mut notes).overview_layout,
                OverviewLayout::Cards
            );
        }
        let settings = Settings::recover(
            Some(serde_json::json!({ "overviewLayout": "compact" })),
            &mut notes,
        );
        assert_eq!(settings.overview_layout, OverviewLayout::Compact);
        assert!(notes.is_empty());
        let settings = Settings::recover(
            Some(serde_json::json!({ "overviewLayout": "grid", "theme": "dark" })),
            &mut notes,
        );
        assert_eq!(settings.overview_layout, OverviewLayout::Cards);
        assert_eq!(settings.theme, Theme::Dark);
        assert_eq!(notes, vec!["Overview layout was reset to its default."]);
        let saved = serde_json::to_value(Settings {
            overview_layout: OverviewLayout::Compact,
            ..Settings::default()
        })
        .unwrap();
        assert_eq!(saved["overviewLayout"], "compact");
    }

    #[test]
    fn daily_update_checks_default_on_and_recover_bad_values() {
        let mut notes = Vec::new();
        for value in [None, Some(serde_json::json!({}))] {
            assert!(Settings::recover(value, &mut notes).auto_check_updates);
        }
        let settings = Settings::recover(
            Some(serde_json::json!({ "autoCheckUpdates": false })),
            &mut notes,
        );
        assert!(!settings.auto_check_updates);
        assert!(notes.is_empty());
        let settings = Settings::recover(
            Some(serde_json::json!({ "autoCheckUpdates": "daily" })),
            &mut notes,
        );
        assert!(settings.auto_check_updates);
        assert_eq!(
            notes,
            vec!["Check for updates daily was reset to its default."]
        );
        let saved = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(saved["autoCheckUpdates"], true);
    }
}
