use serde::{Deserialize, Serialize};

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
    #[default]
    Pinned,
    Lowest,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: Theme,
    pub poll_interval_seconds: u64,
    pub idle_poll_interval_seconds: u64,
    pub notifications: bool,
    pub thresholds: Vec<u8>,
    pub tray_mode: TrayMode,
    pub pinned_account_id: Option<String>,
    pub autostart: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            poll_interval_seconds: 60,
            idle_poll_interval_seconds: 300,
            notifications: false,
            thresholds: vec![80, 95, 100],
            tray_mode: TrayMode::Pinned,
            pinned_account_id: None,
            autostart: false,
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
}
