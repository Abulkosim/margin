use super::model::{Account, Provider};
use serde::Serialize;
use std::collections::BTreeSet;

pub const ISSUE_URL: &str = "https://github.com/Abulkosim/margin/issues/new";

/// Kept well under the roughly 8 KB where GitHub starts rejecting long URLs.
pub const MAX_URL_LENGTH: usize = 2000;

/// Anything else is reported as `other`, so a kind can never carry free text.
const ERROR_KINDS: [&str; 12] = [
    "auth",
    "configuration",
    "credentials",
    "credentialsChanged",
    "invalidResponse",
    "network",
    "provider",
    "rateLimited",
    "redirect",
    "signedOut",
    "stale",
    "unavailable",
];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemReport {
    pub version: &'static str,
    pub system: String,
    pub diagnostics: String,
}

impl ProblemReport {
    pub fn build(os: &str, os_version: &str, accounts: &[Account]) -> Self {
        let lines: Vec<String> = [Provider::Claude, Provider::Codex]
            .into_iter()
            .flat_map(|provider| provider_lines(provider, accounts))
            .collect();
        Self {
            version: env!("CARGO_PKG_VERSION"),
            system: system_label(os, os_version),
            diagnostics: lines.join("\n"),
        }
    }

    /// Parameter names match the field ids in `.github/ISSUE_TEMPLATE/bug.yml`.
    pub fn url(&self) -> Result<String, String> {
        reqwest::Url::parse_with_params(
            ISSUE_URL,
            [
                ("template", "bug.yml"),
                ("version", self.version),
                ("system", &self.system),
                ("diagnostics", &self.diagnostics),
            ],
        )
        .map(String::from)
        .map_err(|_| "Could not prepare the report.".into())
    }
}

fn provider_lines(provider: Provider, accounts: &[Account]) -> [String; 2] {
    let accounts = accounts
        .iter()
        .filter(|account| account.provider == provider);
    let count = accounts.clone().count();
    let kinds: BTreeSet<&str> = accounts
        .filter_map(|account| account.error.as_ref())
        .map(|error| error_kind(&error.kind))
        .collect();
    let errors = if kinds.is_empty() {
        "none".to_owned()
    } else {
        kinds.into_iter().collect::<Vec<_>>().join(", ")
    };
    let name = provider.name();
    [
        format!("{name} accounts: {count}"),
        format!("{name} errors: {errors}"),
    ]
}

fn error_kind(kind: &str) -> &'static str {
    ERROR_KINDS
        .into_iter()
        .find(|known| *known == kind)
        .unwrap_or("other")
}

fn system_label(os: &str, version: &str) -> String {
    let version = version_number(version);
    match os {
        "macos" => version.map_or("macOS".into(), |version| format!("macOS {version}")),
        "windows" => version.map_or("Windows".into(), |version| {
            let build = version
                .split('.')
                .nth(2)
                .and_then(|build| build.parse::<u32>().ok());
            let name = match build {
                Some(22_000..) => "Windows 11",
                Some(_) => "Windows 10",
                None => "Windows",
            };
            format!("{name} ({version})")
        }),
        "linux" => "Linux".into(),
        _ => "Other".into(),
    }
}

fn version_number(version: &str) -> Option<&str> {
    let valid = version.len() <= 24
        && version.starts_with(|first: char| first.is_ascii_digit())
        && version
            .chars()
            .all(|part| part.is_ascii_digit() || part == '.');
    valid.then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{AccountSource, Identity, Snapshot, UsageError, UsageWindow};

    const EMAIL: &str = "sentinelperson@sentinelmail.example";
    const PATH: &str = "/Users/sentinelhome/.claude-sentinelfolder";
    const TOKEN: &str = "sk-ant-oat01-sentineltoken";
    const LABEL: &str = "Sentinel Label";
    const MESSAGE: &str = "sentinelmessage from provider";
    const RAW: &str = "{\"sentinelraw\":true}";

    fn account(provider: Provider, kind: Option<&str>) -> Account {
        Account {
            id: "sentinelid".into(),
            provider,
            label: LABEL.into(),
            color: "#ffffff".into(),
            source: AccountSource::ConfigDir {
                path: PATH.into(),
                keychain_suffix: Some("sentinelsuffix".into()),
            },
            identity: Identity {
                email: Some(EMAIL.into()),
                plan: Some("sentinelplan".into()),
                history_key: Some("sentinelhistory".into()),
            },
            snapshot: Some(Snapshot {
                windows: vec![UsageWindow {
                    id: "sentinelwindow".into(),
                    label: "sentinelwindowlabel".into(),
                    used_percent: 42.0,
                    resets_at: Some("2026-11-04T00:00:00Z".into()),
                    window_seconds: Some(18_000),
                }],
                fetched_at: "2026-11-04T00:00:00Z".into(),
                extra_usage: None,
                reset_grants: None,
                balance: None,
                incomplete: false,
            }),
            error: kind.map(|kind| UsageError {
                sign_in_command: Some(format!("CLAUDE_CONFIG_DIR={PATH} claude {TOKEN}")),
                signed_out_since: Some("sentinelsince".into()),
                ..UsageError::new(kind, &format!("{MESSAGE} {EMAIL} {PATH} {TOKEN} {RAW}"))
            }),
            refreshing: false,
        }
    }

    fn query(url: &str) -> Vec<(String, String)> {
        reqwest::Url::parse(url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect()
    }

    #[test]
    fn counts_accounts_per_provider_and_lists_each_error_kind_once() {
        let report = ProblemReport::build(
            "macos",
            "15.1.0",
            &[
                account(Provider::Claude, Some("signedOut")),
                account(Provider::Claude, Some("network")),
                account(Provider::Claude, Some("network")),
                account(Provider::Codex, None),
            ],
        );
        assert_eq!(report.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(report.system, "macOS 15.1.0");
        assert_eq!(
            report.diagnostics,
            "Claude accounts: 3\nClaude errors: network, signedOut\nCodex accounts: 1\nCodex errors: none"
        );
        let empty = ProblemReport::build("windows", "10.0.22631", &[]);
        assert_eq!(
            empty.diagnostics,
            "Claude accounts: 0\nClaude errors: none\nCodex accounts: 0\nCodex errors: none"
        );
    }

    #[test]
    fn the_url_carries_exactly_the_previewed_fields() {
        let report =
            ProblemReport::build("macos", "15.1", &[account(Provider::Codex, Some("auth"))]);
        let url = report.url().unwrap();
        assert!(url.starts_with("https://github.com/Abulkosim/margin/issues/new?"));
        assert_eq!(
            query(&url),
            vec![
                ("template".to_owned(), "bug.yml".to_owned()),
                ("version".to_owned(), report.version.to_owned()),
                ("system".to_owned(), report.system.clone()),
                ("diagnostics".to_owned(), report.diagnostics.clone()),
            ]
        );
    }

    #[test]
    fn no_account_data_reaches_the_report() {
        let accounts = [
            account(Provider::Claude, Some("network")),
            account(Provider::Codex, Some(TOKEN)),
            account(Provider::Claude, Some(EMAIL)),
            account(Provider::Codex, Some(PATH)),
            account(Provider::Claude, Some(MESSAGE)),
        ];
        let report = ProblemReport::build("macos", TOKEN, &accounts);
        let url = report.url().unwrap();
        let decoded: String = query(&url)
            .into_iter()
            .map(|(key, value)| format!("{key}={value}\n"))
            .collect();
        let preview = serde_json::to_string(&report).unwrap();
        for text in [url.as_str(), decoded.as_str(), preview.as_str()] {
            assert!(!text.to_lowercase().contains("sentinel"), "{text}");
            for secret in [EMAIL, PATH, TOKEN, LABEL, MESSAGE, RAW] {
                assert!(!text.contains(secret), "{text}");
            }
            assert!(!text.contains("sk-ant"), "{text}");
            assert!(!text.contains('@'), "{text}");
            assert!(!text.contains("%40"), "{text}");
        }
        assert!(report.diagnostics.contains("Claude errors: network, other"));
        assert!(report.diagnostics.contains("Codex errors: other"));
        assert_eq!(report.system, "macOS");
    }

    #[test]
    fn an_unexpected_os_or_version_never_passes_through() {
        for (os, version, label) in [
            ("macos", "15.1.0", "macOS 15.1.0"),
            ("macos", "Unknown", "macOS"),
            ("macos", "/Users/alex", "macOS"),
            ("macos", "alex@example.com", "macOS"),
            ("macos", "1.2.3.4.5.6.7.8.9.10.11.12.13", "macOS"),
            ("windows", "10.0.22631", "Windows 11 (10.0.22631)"),
            ("windows", "10.0.19045", "Windows 10 (10.0.19045)"),
            ("windows", "10", "Windows (10)"),
            ("windows", "C:\\Users\\alex", "Windows"),
            ("linux", "6.8.0", "Linux"),
            ("alex@example.com", "6.8.0", "Other"),
        ] {
            assert_eq!(system_label(os, version), label);
        }
    }

    #[test]
    fn the_longest_possible_url_stays_under_the_limit() {
        let mut accounts = Vec::new();
        for provider in [Provider::Claude, Provider::Codex] {
            for kind in ERROR_KINDS.iter().copied().chain(["unlisted"]) {
                accounts.push(account(provider, Some(kind)));
            }
            accounts.extend((0..100).map(|_| account(provider, None)));
        }
        let url = ProblemReport::build("windows", "10.0.65535.65535.65535", &accounts)
            .url()
            .unwrap();
        assert!(url.len() < MAX_URL_LENGTH, "{} characters", url.len());
    }
}
