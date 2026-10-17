use super::{
    credentials_changed, http_error, malformed_credentials, malformed_response, nonempty_string,
    now, percent, read_credentials, request_error, response_json, timestamp, token,
};
use crate::core::model::{
    Account, AccountSource, ExtraUsage, Identity, Provider, Snapshot, UsageError, UsageWindow,
};
use reqwest::StatusCode;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";

struct Credentials {
    access_token: String,
    plan: Option<String>,
}

pub(super) async fn fetch(
    client: &reqwest::Client,
    account: &Account,
) -> Result<(Snapshot, Identity), UsageError> {
    let credentials = load_credentials(&account.source).await?;
    let mut response = usage_request(client, &credentials.access_token, true).await?;
    if needs_query_fallback(response.status()) {
        response = usage_request(client, &credentials.access_token, false).await?;
    }
    if !response.status().is_success() {
        return Err(http_error(
            Provider::Claude,
            response.status(),
            response.headers(),
        ));
    }
    let snapshot = parse_usage(&response_json(response).await?)?;
    let mut identity = Identity {
        email: None,
        plan: credentials.plan.clone(),
        history_key: None,
    };
    let profile = client
        .get(PROFILE_URL)
        .bearer_auth(&credentials.access_token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Accept", "application/json")
        .header("Cache-Control", "no-store")
        .timeout(Duration::from_secs(3))
        .send()
        .await;
    if let Ok(response) = profile {
        if response.status().is_success() {
            if let Ok(value) = response_json(response).await {
                let profile = parse_profile(&value);
                identity.email = profile.email;
                identity.plan = profile.plan.or(identity.plan);
                identity.history_key = profile.history_key;
            }
        }
    }
    let current = load_credentials(&account.source).await?;
    ensure_same_credentials(&credentials, &current)?;
    Ok((snapshot, identity))
}

fn ensure_same_credentials(
    previous: &Credentials,
    current: &Credentials,
) -> Result<(), UsageError> {
    if previous.access_token != current.access_token || previous.plan != current.plan {
        return Err(credentials_changed());
    }
    Ok(())
}

async fn usage_request(
    client: &reqwest::Client,
    access_token: &str,
    include_query: bool,
) -> Result<reqwest::Response, UsageError> {
    let mut request = client
        .get(USAGE_URL)
        .bearer_auth(access_token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Accept", "application/json")
        .header("Cache-Control", "no-store");
    if include_query {
        request = request.query(&[("cedar_ember", "1")]);
    }
    request.send().await.map_err(request_error)
}

fn needs_query_fallback(status: StatusCode) -> bool {
    matches!(status, StatusCode::BAD_REQUEST | StatusCode::FORBIDDEN)
}

async fn load_credentials(source: &AccountSource) -> Result<Credentials, UsageError> {
    match source {
        AccountSource::ConfigDir { path } => {
            let result = read_credentials(
                &Path::new(path).join(".credentials.json"),
                parse_credentials,
                2,
            )
            .await;
            #[cfg(target_os = "macos")]
            if result
                .as_ref()
                .err()
                .is_some_and(|error| error.kind == "credentials")
                && is_default_directory(Path::new(path))
            {
                return read_keychain().await;
            }
            result
        }
        AccountSource::Keychain => read_keychain().await,
    }
}

fn parse_credentials(bytes: &[u8]) -> Result<Credentials, UsageError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| malformed_credentials())?;
    let oauth = value
        .get("claudeAiOauth")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            UsageError::new(
                "credentials",
                "No Claude account token found. Sign in with Claude Code, then refresh.",
            )
        })?;
    if let Some(scopes) = oauth.get("scopes").filter(|value| !value.is_null()) {
        let has_profile = if let Some(scopes) = scopes.as_array() {
            if scopes.iter().any(|scope| !scope.is_string()) {
                return Err(malformed_credentials());
            }
            scopes
                .iter()
                .any(|scope| scope.as_str() == Some("user:profile"))
        } else if let Some(scopes) = scopes.as_str() {
            scopes
                .split_whitespace()
                .any(|scope| scope == "user:profile")
        } else {
            return Err(malformed_credentials());
        };
        if !has_profile {
            return Err(UsageError::new("auth", "This Claude token cannot read usage. Sign in with Claude Code to grant user:profile access."));
        }
    }
    let plan = nonempty_string(oauth.get("subscriptionType"))
        .or_else(|| nonempty_string(oauth.get("rate_limit_tier")))
        .or_else(|| nonempty_string(oauth.get("rateLimitTier")));
    Ok(Credentials {
        access_token: token(oauth.get("accessToken"))?,
        plan: plan.map(normalize_plan),
    })
}

#[cfg(target_os = "macos")]
fn is_default_directory(path: &Path) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let Ok(default) = std::fs::canonicalize(home.join(".claude")) else {
        return false;
    };
    std::fs::canonicalize(path).is_ok_and(|path| path == default)
}

async fn read_keychain() -> Result<Credentials, UsageError> {
    #[cfg(target_os = "macos")]
    {
        tokio::task::spawn_blocking(|| {
            read_keychain_bytes().and_then(|bytes| parse_credentials(&bytes))
        })
        .await
        .map_err(|_| keychain_error())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(UsageError::new("credentials", "Keychain accounts are only available on macOS. Add this account's configuration folder."))
    }
}

#[cfg(target_os = "macos")]
fn keychain_error() -> UsageError {
    UsageError::new("credentials", "Claude Code's account token is unavailable in Keychain. Sign in with Claude Code, then refresh.")
}

#[cfg(target_os = "macos")]
pub(crate) fn keychain_available() -> bool {
    read_keychain_bytes()
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .is_some_and(|value| {
            value
                .pointer("/claudeAiOauth/accessToken")
                .and_then(Value::as_str)
                .is_some_and(|token| !token.is_empty())
        })
}

#[cfg(target_os = "macos")]
fn read_keychain_bytes() -> Result<Vec<u8>, UsageError> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut child = Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| keychain_error())?;
    let stdout = child.stdout.take().ok_or_else(keychain_error)?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(super::MAX_PRIVATE_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let succeeded = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| keychain_error())?
        .map_err(|_| keychain_error())?;
    if !succeeded || bytes.len() as u64 > super::MAX_PRIVATE_FILE_BYTES {
        return Err(keychain_error());
    }
    Ok(bytes)
}

pub fn parse_usage(value: &Value) -> Result<Snapshot, UsageError> {
    if !value.is_object() {
        return Err(malformed_response());
    }
    let mut windows = Vec::new();
    for (id, label, seconds) in [
        ("five_hour", "Session", 18_000),
        ("seven_day", "Weekly", 604_800),
        ("seven_day_sonnet", "Sonnet", 604_800),
        ("seven_day_opus", "Opus", 604_800),
        ("seven_day_routines", "Routines", 604_800),
        ("seven_day_cowork", "Cowork", 604_800),
    ] {
        let Some(window) = value.get(id).filter(|value| !value.is_null()) else {
            continue;
        };
        if !window.is_object() {
            return Err(malformed_response());
        }
        let Some(utilization) = window.get("utilization").filter(|value| !value.is_null()) else {
            continue;
        };
        windows.push(UsageWindow {
            id: id.into(),
            label: label.into(),
            used_percent: percent(utilization)?,
            resets_at: timestamp(window.get("resets_at"))?,
            window_seconds: Some(seconds),
        });
    }
    let mut scoped_ids = HashSet::new();
    if let Some(limits) = value.get("limits").filter(|value| !value.is_null()) {
        for limit in limits.as_array().ok_or_else(malformed_response)? {
            if limit.get("kind").and_then(Value::as_str) != Some("weekly_scoped") {
                continue;
            }
            let Some(model_name) = nonempty_string(limit.pointer("/scope/model/display_name"))
            else {
                continue;
            };
            if model_name.len() > 512 {
                return Err(malformed_response());
            }
            if model_name.eq_ignore_ascii_case("all models") {
                continue;
            }
            let Some(utilization) = limit.get("percent").filter(|value| !value.is_null()) else {
                continue;
            };
            let model_id = nonempty_string(limit.pointer("/scope/model/id"))
                .unwrap_or_else(|| model_name.clone());
            let digest = Sha256::digest(model_id.as_bytes());
            let fingerprint = format!("{digest:x}");
            let id = format!("weekly_scoped_{}", &fingerprint[..16]);
            if !scoped_ids.insert(id.clone()) {
                continue;
            }
            windows.push(UsageWindow {
                id,
                label: format!("{model_name} weekly"),
                used_percent: percent(utilization)?,
                resets_at: timestamp(limit.get("resets_at"))?,
                window_seconds: Some(604_800),
            });
        }
    }
    let extra_usage = parse_extra_usage(value.get("extra_usage"))?;
    if windows.len() > 64 {
        return Err(malformed_response());
    }
    if windows.is_empty() && extra_usage.is_none() {
        return Err(UsageError::new(
            "unavailable",
            "Claude did not return a measured usage window for this account.",
        ));
    }
    Ok(Snapshot {
        windows,
        fetched_at: now(),
        extra_usage,
    })
}

fn parse_extra_usage(value: Option<&Value>) -> Result<Option<ExtraUsage>, UsageError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    if !value.is_object() {
        return Err(malformed_response());
    }
    if value.get("is_enabled").and_then(Value::as_bool) == Some(false) {
        return Ok(None);
    }
    let Some(spent) = value.get("used_credits").filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let spent = nonnegative_amount(spent)?;
    let limit = value
        .get("monthly_limit")
        .filter(|value| !value.is_null())
        .map(nonnegative_amount)
        .transpose()?;
    let currency = match value.get("currency").filter(|value| !value.is_null()) {
        Some(value) => value
            .as_str()
            .ok_or_else(malformed_response)?
            .trim()
            .to_uppercase(),
        None => "USD".into(),
    };
    if currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Err(malformed_response());
    }
    let decimal_places = match value.get("decimal_places").filter(|value| !value.is_null()) {
        Some(value) => value
            .as_u64()
            .filter(|places| *places <= 6)
            .ok_or_else(malformed_response)? as i32,
        None => 2,
    };
    let divisor = 10f64.powi(decimal_places);
    Ok(Some(ExtraUsage {
        spent: spent / divisor,
        limit: limit.map(|limit| limit / divisor),
        currency,
    }))
}

fn nonnegative_amount(value: &Value) -> Result<f64, UsageError> {
    value
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(malformed_response)
}

fn parse_profile(value: &Value) -> Identity {
    let email = [
        "/account/email",
        "/account/email_address",
        "/account/emailAddress",
        "/email",
        "/email_address",
        "/emailAddress",
    ]
    .iter()
    .find_map(|path| nonempty_string(value.pointer(path)));
    let plan = [
        "/subscriptionType",
        "/account/subscriptionType",
        "/organization/subscriptionType",
        "/organization/subscription_type",
        "/rate_limit_tier",
        "/organization/rate_limit_tier",
    ]
    .iter()
    .find_map(|path| nonempty_string(value.pointer(path)))
    .map(normalize_plan);
    let history_key = nonempty_string(value.pointer("/account/uuid")).and_then(|account| {
        nonempty_string(value.pointer("/organization/uuid"))
            .map(|organization| super::history_key(Provider::Claude, &account, &organization))
    });
    Identity {
        email,
        plan,
        history_key,
    }
}

fn normalize_plan(value: String) -> String {
    match value.to_lowercase().as_str() {
        "max" | "claude_max" | "default_claude_max" => "Max".into(),
        "max_5x" | "default_claude_max_5x" => "Max 5x".into(),
        "max_20x" | "default_claude_max_20x" => "Max 20x".into(),
        "enterprise" | "claude_enterprise" | "default_claude_enterprise" => "Enterprise".into(),
        "team" | "claude_team" | "default_claude_team" => "Team".into(),
        "pro" | "claude_pro" | "default_claude_pro" => "Pro".into(),
        _ => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_identity_is_stable_and_separates_organizations() {
        let profile = serde_json::json!({
            "account": {"uuid": "account-one", "email": "example@example.test"},
            "organization": {"uuid": "organization-one"}
        });
        let identity = parse_profile(&profile);
        assert_eq!(identity.history_key.as_ref().unwrap().len(), 64);
        let mut renamed = profile.clone();
        renamed["account"]["email"] = serde_json::json!("renamed@example.test");
        assert_eq!(identity.history_key, parse_profile(&renamed).history_key);
        renamed["organization"]["uuid"] = serde_json::json!("organization-two");
        assert_ne!(identity.history_key, parse_profile(&renamed).history_key);
        assert!(parse_profile(&serde_json::json!({})).history_key.is_none());
    }
    use serde_json::json;

    #[test]
    fn live_usage_shape_preserves_scoped_window_with_missing_model_id() {
        let value = serde_json::from_str(include_str!("../../../fixtures/claude-live-schema.json"))
            .unwrap();
        let snapshot = parse_usage(&value).unwrap();
        assert_eq!(snapshot.windows.len(), 3);
        assert_eq!(snapshot.windows[0].label, "Session");
        assert_eq!(snapshot.windows[1].label, "Weekly");
        assert!(snapshot.windows[2].id.starts_with("weekly_scoped_"));
        assert_eq!(snapshot.windows[2].window_seconds, Some(604_800));
        assert_eq!(snapshot.windows[2].used_percent, 34.0);
        assert!(snapshot.extra_usage.is_none());
    }

    #[test]
    fn live_profile_shape_reads_organization_tier_and_preserves_plan_precedence() {
        let mut value: Value = serde_json::from_str(include_str!(
            "../../../fixtures/claude-profile-live-schema.json"
        ))
        .unwrap();
        let identity = parse_profile(&value);
        assert_eq!(identity.email.as_deref(), Some("person@example.test"));
        assert_eq!(identity.plan.as_deref(), Some("Pro"));
        value["organization"]["rate_limit_tier"] = json!("default_claude_max_20x");
        assert_eq!(parse_profile(&value).plan.as_deref(), Some("Max 20x"));
        value["subscriptionType"] = json!("team");
        assert_eq!(parse_profile(&value).plan.as_deref(), Some("Team"));
    }

    #[test]
    fn parses_all_documented_windows_and_spend() {
        let value = serde_json::from_str(include_str!("../../../fixtures/claude-all-windows.json"))
            .unwrap();
        let snapshot = parse_usage(&value).unwrap();
        assert_eq!(snapshot.windows.len(), 7);
        assert_eq!(snapshot.windows[0].used_percent, 34.0);
        assert_eq!(snapshot.windows[6].label, "Example model weekly");
        assert_eq!(snapshot.extra_usage.as_ref().unwrap().spent, 12.34);
        assert_eq!(snapshot.extra_usage.unwrap().limit, Some(50.0));
    }

    #[test]
    fn weekly_fallback_keeps_correct_identity_and_duration() {
        let value = serde_json::from_str(include_str!("../../../fixtures/claude-weekly-only.json"))
            .unwrap();
        let snapshot = parse_usage(&value).unwrap();
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].id, "seven_day");
        assert_eq!(snapshot.windows[0].label, "Weekly");
        assert_eq!(snapshot.windows[0].window_seconds, Some(604_800));
    }

    #[test]
    fn omitted_measurements_never_turn_into_zero_usage() {
        for value in [
            json!({}),
            json!({"five_hour": null}),
            json!({"five_hour": {"resets_at": null}}),
        ] {
            assert!(parse_usage(&value).is_err());
        }
        let snapshot = parse_usage(
            &json!({"five_hour": {"utilization": null}, "seven_day": {"utilization": 24}}),
        )
        .unwrap();
        assert_eq!(snapshot.windows[0].label, "Weekly");
    }

    #[test]
    fn invalid_measured_values_fail_closed() {
        for utilization in [json!(-2), json!(101), json!("22")] {
            assert!(parse_usage(&json!({"five_hour": {"utilization": utilization}})).is_err());
        }
        assert!(
            parse_usage(&json!({"five_hour": {"utilization": 22, "resets_at": "invalid"}}))
                .is_err()
        );
    }

    #[test]
    fn spend_only_accounts_do_not_get_synthetic_quota_windows() {
        let value =
            serde_json::from_str(include_str!("../../../fixtures/claude-spend-only.json")).unwrap();
        let snapshot = parse_usage(&value).unwrap();
        assert!(snapshot.windows.is_empty());
        assert_eq!(snapshot.extra_usage.unwrap().spent, 43.21);
    }

    #[test]
    fn invalid_explicit_currencies_do_not_default_to_usd() {
        for currency in [json!(""), json!(42), json!("US\nD"), json!("US dollars")] {
            assert!(
                parse_extra_usage(Some(&json!({"used_credits": 100, "currency": currency})))
                    .is_err()
            );
        }
    }

    #[test]
    fn rejects_mcp_only_and_inference_only_credentials() {
        assert!(parse_credentials(br#"{"mcpOAuth":{}}"#).is_err());
        let error = parse_credentials(
            br#"{"claudeAiOauth":{"accessToken":"synthetic","scopes":["user:inference"]}}"#,
        )
        .err()
        .unwrap();
        assert_eq!(error.kind, "auth");
        assert!(error.message.contains("user:profile"));
        let credentials = parse_credentials(br#"{"claudeAiOauth":{"accessToken":"synthetic","scopes":["user:profile"],"subscriptionType":"max"}}"#).unwrap();
        assert_eq!(credentials.plan.as_deref(), Some("Max"));
    }

    #[test]
    fn fallback_is_limited_to_optional_query_rejection() {
        assert!(needs_query_fallback(StatusCode::BAD_REQUEST));
        assert!(needs_query_fallback(StatusCode::FORBIDDEN));
        for status in [
            StatusCode::OK,
            StatusCode::UNAUTHORIZED,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            assert!(!needs_query_fallback(status));
        }
    }

    #[test]
    fn inactive_scoped_limits_are_preserved_and_duplicates_collapse() {
        let entry = json!({"kind": "weekly_scoped", "group": "weekly", "percent": 15, "is_active": false, "scope": {"model": {"id": "example", "display_name": "Example"}}});
        let snapshot = parse_usage(&json!({"limits": [entry.clone(), entry]})).unwrap();
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].used_percent, 15.0);
    }

    #[test]
    fn profile_supports_nested_identity_and_preferred_plan() {
        let profile = parse_profile(
            &json!({"account": {"email_address": "person@example.test"}, "subscriptionType": "pro", "rate_limit_tier": "default_claude_max_20x"}),
        );
        assert_eq!(profile.email.as_deref(), Some("person@example.test"));
        assert_eq!(profile.plan.as_deref(), Some("Pro"));
    }

    #[test]
    fn credentials_changed_during_fetch_are_not_published() {
        let before = Credentials {
            access_token: "synthetic-before".into(),
            plan: Some("Pro".into()),
        };
        let after = Credentials {
            access_token: "synthetic-after".into(),
            plan: Some("Pro".into()),
        };
        assert_eq!(
            ensure_same_credentials(&before, &after).unwrap_err().kind,
            "credentialsChanged"
        );
        assert!(ensure_same_credentials(&before, &before).is_ok());
    }

    #[test]
    fn unknown_plan_names_are_not_guessed() {
        assert_eq!(normalize_plan("maximum_trial".into()), "maximum_trial");
        assert_eq!(normalize_plan("default_claude_max_20x".into()), "Max 20x");
    }
}
