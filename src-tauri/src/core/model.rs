use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    Claude,
    Codex,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub fn credential_filename(self) -> &'static str {
        match self {
            Self::Claude => ".credentials.json",
            Self::Codex => "auth.json",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AccountSource {
    ConfigDir {
        path: String,
        #[serde(
            default,
            rename = "keychainSuffix",
            skip_serializing_if = "Option::is_none"
        )]
        keychain_suffix: Option<String>,
    },
    Keychain,
}

impl AccountSource {
    pub fn is_valid_for(&self, provider: Provider) -> bool {
        match self {
            Self::ConfigDir {
                path,
                keychain_suffix,
            } => {
                path.len() <= 4096
                    && !path.chars().any(char::is_control)
                    && std::path::Path::new(path).is_absolute()
                    && keychain_suffix.as_deref().is_none_or(|suffix| {
                        provider == Provider::Claude && valid_keychain_suffix(suffix)
                    })
            }
            Self::Keychain => provider == Provider::Claude,
        }
    }
}

pub fn valid_keychain_suffix(suffix: &str) -> bool {
    suffix.len() == 8
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub email: Option<String>,
    pub plan: Option<String>,
    #[serde(default)]
    pub history_key: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowScope {
    Core,
    Model,
    #[default]
    Other,
}

impl WindowScope {
    pub fn infer(id: &str) -> Self {
        match id {
            "five_hour" | "seven_day" | "primary_window" | "secondary_window" => Self::Core,
            "seven_day_sonnet" | "seven_day_opus" => Self::Model,
            id if id.starts_with("weekly_scoped_") => Self::Model,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(from = "WireUsageWindow", into = "WireUsageWindow")]
pub struct UsageWindow {
    pub id: String,
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<String>,
    pub window_seconds: Option<u64>,
}

impl UsageWindow {
    pub fn scope(&self) -> WindowScope {
        WindowScope::infer(&self.id)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireUsageWindow {
    id: String,
    label: String,
    used_percent: f64,
    resets_at: Option<String>,
    window_seconds: Option<u64>,
    #[serde(default)]
    scope: WindowScope,
}

impl From<WireUsageWindow> for UsageWindow {
    fn from(wire: WireUsageWindow) -> Self {
        let label = match wire.label.strip_suffix(" weekly") {
            Some(model) if wire.id.starts_with("weekly_scoped_") && !model.is_empty() => {
                model.to_owned()
            }
            _ => wire.label,
        };
        Self {
            id: wire.id,
            label,
            used_percent: wire.used_percent,
            resets_at: wire.resets_at,
            window_seconds: wire.window_seconds,
        }
    }
}

impl From<UsageWindow> for WireUsageWindow {
    fn from(window: UsageWindow) -> Self {
        Self {
            scope: window.scope(),
            id: window.id,
            label: window.label,
            used_percent: window.used_percent,
            resets_at: window.resets_at,
            window_seconds: window.window_seconds,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraUsage {
    pub spent: f64,
    pub limit: Option<f64>,
    pub currency: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub windows: Vec<UsageWindow>,
    pub fetched_at: String,
    pub extra_usage: Option<ExtraUsage>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageError {
    pub kind: String,
    pub message: String,
    pub retry_after_seconds: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_out_since: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign_in_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_at: Option<i64>,
}

impl UsageError {
    pub fn new(kind: &str, message: &str) -> Self {
        Self {
            kind: kind.to_owned(),
            message: message.to_owned(),
            retry_after_seconds: None,
            signed_out_since: None,
            sign_in_command: None,
            retry_at: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub provider: Provider,
    pub label: String,
    pub color: String,
    pub source: AccountSource,
    pub identity: Identity,
    pub snapshot: Option<Snapshot>,
    pub error: Option<UsageError>,
    pub refreshing: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_shape_has_no_credentials() {
        let account = Account {
            id: "test".into(),
            provider: Provider::Codex,
            label: "Work".into(),
            color: "#ffffff".into(),
            source: AccountSource::ConfigDir {
                path: "/example".into(),
                keychain_suffix: None,
            },
            identity: Identity::default(),
            snapshot: Some(Snapshot {
                windows: vec![UsageWindow {
                    id: "primary_window".into(),
                    label: "Session".into(),
                    used_percent: 23.0,
                    resets_at: None,
                    window_seconds: Some(18_000),
                }],
                fetched_at: "2026-10-16T10:00:00Z".into(),
                extra_usage: None,
            }),
            error: None,
            refreshing: false,
        };
        let value = serde_json::to_value(account).unwrap();
        assert_eq!(value["provider"], "codex");
        assert_eq!(value["source"]["kind"], "configDir");
        assert!(value["source"].get("keychainSuffix").is_none());
        assert_eq!(value["snapshot"]["windows"][0]["usedPercent"], 23.0);
        assert_eq!(value["snapshot"]["windows"][0]["scope"], "core");
        assert!(value.get("accessToken").is_none());
        assert!(value.get("refreshToken").is_none());
    }

    #[test]
    fn stored_windows_without_scope_infer_it_from_the_id() {
        let window = |id: &str, label: &str| {
            serde_json::from_value::<UsageWindow>(serde_json::json!({
                "id": id, "label": label, "usedPercent": 10.0, "resetsAt": null, "windowSeconds": 604_800
            }))
            .unwrap()
        };
        assert_eq!(window("five_hour", "Session").scope(), WindowScope::Core);
        assert_eq!(
            window("secondary_window", "Weekly").scope(),
            WindowScope::Core
        );
        assert_eq!(
            window("seven_day_sonnet", "Sonnet").scope(),
            WindowScope::Model
        );
        let scoped = window("weekly_scoped_0123456789abcdef", "Fable weekly");
        assert_eq!(scoped.scope(), WindowScope::Model);
        assert_eq!(scoped.label, "Fable");
        assert_eq!(
            window("seven_day_cowork", "Cowork").scope(),
            WindowScope::Other
        );
        assert_eq!(
            window("extra_0123456789abcdef_primary_window", "Spark · session").scope(),
            WindowScope::Other
        );
        let restored: UsageWindow =
            serde_json::from_value(serde_json::to_value(scoped.clone()).unwrap()).unwrap();
        assert_eq!(restored, scoped);
    }
}
