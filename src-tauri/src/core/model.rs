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
    ConfigDir { path: String },
    Keychain,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub email: Option<String>,
    pub plan: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub id: String,
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<String>,
    pub window_seconds: Option<u64>,
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
    pub retry_after_seconds: Option<u64>,
}

impl UsageError {
    pub fn new(kind: &str, message: &str) -> Self {
        Self {
            kind: kind.to_owned(),
            message: message.to_owned(),
            retry_after_seconds: None,
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
            },
            identity: Identity::default(),
            snapshot: Some(Snapshot {
                windows: vec![UsageWindow {
                    id: "primary".into(),
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
        assert_eq!(value["snapshot"]["windows"][0]["usedPercent"], 23.0);
        assert!(value.get("accessToken").is_none());
        assert!(value.get("refreshToken").is_none());
    }
}
