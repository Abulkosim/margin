use chrono::{DateTime, Utc};
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

impl Identity {
    /// False only when both sides name a person and the names differ; a plan
    /// change or a key appearing for the first time is the same person.
    pub fn same_person(&self, other: &Self) -> bool {
        match (&self.history_key, &other.history_key) {
            (Some(a), Some(b)) => a == b,
            _ => match (&self.email, &other.email) {
                (Some(a), Some(b)) => same_email(a, b),
                _ => true,
            },
        }
    }
}

/// Providers may change an address's letter case between readings.
pub fn same_email(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
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
            id if id.starts_with("extra_") && id.ends_with("_secondary_window") => Self::Model,
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

/// Prepaid credits; present only when the provider reports some.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Credits {
    pub amount: Option<f64>,
    pub unlimited: bool,
}

/// A per-person credit cap set by a workspace, with its own reset.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpendLimit {
    pub used: f64,
    pub limit: f64,
    pub used_percent: f64,
    pub resets_at: Option<String>,
    pub reached: bool,
    /// What `used` and `limit` count, e.g. `credit`; absent when the API omits it.
    #[serde(default)]
    pub unit: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Balance {
    #[serde(default)]
    pub credits: Option<Credits>,
    #[serde(default)]
    pub spend_limit: Option<SpendLimit>,
}

impl Balance {
    pub fn is_empty(&self) -> bool {
        self.credits.is_none() && self.spend_limit.is_none()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub windows: Vec<UsageWindow>,
    pub fetched_at: String,
    pub extra_usage: Option<ExtraUsage>,
    #[serde(default)]
    pub reset_grants: Option<ResetGrants>,
    #[serde(default)]
    pub balance: Option<Balance>,
    /// An optional limit in the response could not be read, so `windows` may be missing some.
    #[serde(default)]
    pub incomplete: bool,
}

impl Snapshot {
    /// True once a window's reset has passed with no reading since: its percentage describes
    /// the previous period, and the current value is unknown.
    pub fn unread_since_reset(&self, window: &UsageWindow, now: DateTime<Utc>) -> bool {
        let parse = |value: &str| DateTime::parse_from_rfc3339(value).ok();
        let Some(reset) = window.resets_at.as_deref().and_then(parse) else {
            return false;
        };
        reset <= now && parse(&self.fetched_at).is_none_or(|fetched| fetched < reset)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetGrants {
    pub available: u32,
    pub kind: Option<String>,
    pub expires_at: Option<String>,
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

    /// The CLI's token expired on disk. Nothing rejected it, and the CLI refreshes it on its
    /// next run, so the account is idle rather than signed out.
    pub fn token_expired(&self) -> bool {
        self.kind == "signedOut" && self.signed_out_since.is_some()
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
                reset_grants: None,
                balance: None,
                incomplete: false,
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
        assert!(value["snapshot"]["resetGrants"].is_null());
        assert!(value["snapshot"]["balance"].is_null());
    }

    #[test]
    fn identity_changes_only_count_when_the_person_differs() {
        let identity = |email: Option<&str>, plan: &str, key: Option<&str>| Identity {
            email: email.map(Into::into),
            plan: Some(plan.into()),
            history_key: key.map(Into::into),
        };
        let before = identity(Some("a@example.com"), "Pro", None);
        assert!(before.same_person(&identity(Some("a@example.com"), "Max 5×", None)));
        assert!(before.same_person(&identity(Some("A@example.com"), "Pro", Some("k1"))));
        assert!(before.same_person(&identity(None, "Pro", None)));
        assert!(!before.same_person(&identity(Some("b@example.com"), "Pro", None)));
        let keyed = identity(Some("a@example.com"), "Team", Some("k1"));
        assert!(!keyed.same_person(&identity(Some("a@example.com"), "Pro", Some("k2"))));
        assert!(keyed.same_person(&identity(Some("b@example.com"), "Team", Some("k1"))));
    }

    #[test]
    fn stored_snapshots_without_a_balance_still_parse_and_balances_round_trip() {
        let snapshot: Snapshot = serde_json::from_value(serde_json::json!({
            "windows": [], "fetchedAt": "2026-10-16T10:00:00Z", "extraUsage": null, "resetGrants": null
        }))
        .unwrap();
        assert!(snapshot.balance.is_none());
        let balance = Balance {
            credits: Some(Credits {
                amount: Some(1250.0),
                unlimited: false,
            }),
            spend_limit: Some(SpendLimit {
                used: 0.0,
                limit: 1.0,
                used_percent: 0.0,
                resets_at: Some("2026-11-01T00:00:00Z".into()),
                reached: false,
                unit: Some("credit".into()),
            }),
        };
        let value = serde_json::to_value(&balance).unwrap();
        assert_eq!(value["credits"]["amount"], 1250.0);
        assert_eq!(value["spendLimit"]["usedPercent"], 0.0);
        assert_eq!(value["spendLimit"]["resetsAt"], "2026-11-01T00:00:00Z");
        assert_eq!(value["spendLimit"]["unit"], "credit");
        let restored: Balance = serde_json::from_value(value).unwrap();
        assert_eq!(restored, balance);
        assert!(Balance::default().is_empty());
        let partial: Balance =
            serde_json::from_value(serde_json::json!({"credits": null})).unwrap();
        assert!(partial.is_empty());
    }

    #[test]
    fn stored_snapshots_without_incomplete_read_as_complete() {
        let snapshot: Snapshot = serde_json::from_value(serde_json::json!({
            "windows": [], "fetchedAt": "2026-10-16T10:00:00Z", "extraUsage": null
        }))
        .unwrap();
        assert!(!snapshot.incomplete);
        assert_eq!(
            serde_json::to_value(&snapshot).unwrap()["incomplete"],
            false
        );
    }

    #[test]
    fn only_a_reset_after_the_last_reading_counts_as_unread() {
        let at = |value: &str| value.parse::<DateTime<Utc>>().unwrap();
        let window = |reset: Option<&str>| UsageWindow {
            id: "five_hour".into(),
            label: "Session".into(),
            used_percent: 40.0,
            resets_at: reset.map(Into::into),
            window_seconds: None,
        };
        let snapshot = Snapshot {
            windows: Vec::new(),
            fetched_at: "2026-10-16T10:00:00Z".into(),
            extra_usage: None,
            reset_grants: None,
            balance: None,
            incomplete: false,
        };
        let now = at("2026-10-16T12:00:00Z");
        assert!(snapshot.unread_since_reset(&window(Some("2026-10-16T11:00:00Z")), now));
        assert!(snapshot.unread_since_reset(&window(Some("2026-10-16T12:00:00Z")), now));
        assert!(!snapshot.unread_since_reset(&window(Some("2026-10-16T13:00:00Z")), now));
        assert!(!snapshot.unread_since_reset(&window(Some("2026-10-16T09:00:00Z")), now));
        assert!(!snapshot.unread_since_reset(&window(None), now));
    }

    #[test]
    fn only_a_timed_sign_out_is_an_expired_token() {
        let expired = UsageError {
            signed_out_since: Some("2026-10-16T10:00:00Z".into()),
            ..UsageError::new("signedOut", "Expired")
        };
        assert!(expired.token_expired());
        assert!(!UsageError::new("signedOut", "Rejected").token_expired());
        assert!(!UsageError::new("network", "Offline").token_expired());
    }

    #[test]
    fn stored_snapshots_without_reset_grants_still_parse() {
        let snapshot: Snapshot = serde_json::from_value(serde_json::json!({
            "windows": [], "fetchedAt": "2026-10-16T10:00:00Z", "extraUsage": null
        }))
        .unwrap();
        assert!(snapshot.reset_grants.is_none());
        let grants = ResetGrants {
            available: 2,
            kind: None,
            expires_at: Some("2026-10-19T10:00:00Z".into()),
        };
        let value = serde_json::to_value(&grants).unwrap();
        assert_eq!(value["expiresAt"], "2026-10-19T10:00:00Z");
        assert_eq!(value["available"], 2);
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
        assert_eq!(
            window("extra_0123456789abcdef_secondary_window", "Spark").scope(),
            WindowScope::Model
        );
        let restored: UsageWindow =
            serde_json::from_value(serde_json::to_value(scoped.clone()).unwrap()).unwrap();
        assert_eq!(restored, scoped);
    }
}
