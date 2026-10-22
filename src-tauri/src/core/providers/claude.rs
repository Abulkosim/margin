use super::{
    credentials_changed, http_error, malformed_credentials, malformed_response, nonempty_string,
    now, percent, read_credentials, request_error, response_json, signed_out, timestamp, token,
};
use crate::core::model::{
    valid_keychain_suffix, Account, AccountSource, ExtraUsage, Identity, Provider, Snapshot,
    UsageError, UsageWindow,
};
use reqwest::StatusCode;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const API_BASE_URL: &str = "https://api.anthropic.com";
const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
const PROFILE_TTL: Duration = Duration::from_secs(3600);
const EXPIRY_SKEW_MS: i64 = 60_000;

struct Credentials {
    access_token: String,
    plan: Option<String>,
    expires_at: Option<i64>,
}

#[derive(Default)]
pub struct ProfileCache(Mutex<HashMap<String, CachedProfile>>);

struct CachedProfile {
    token: [u8; 32],
    identity: Identity,
    fetched: Instant,
}

impl ProfileCache {
    fn fresh(&self, account: &str, token: &[u8; 32]) -> Option<Identity> {
        let cache = self.0.lock().ok()?;
        cache
            .get(account)
            .filter(|entry| &entry.token == token && entry.fetched.elapsed() < PROFILE_TTL)
            .map(|entry| entry.identity.clone())
    }

    fn last(&self, account: &str) -> Option<Identity> {
        let cache = self.0.lock().ok()?;
        cache.get(account).map(|entry| entry.identity.clone())
    }

    fn store(&self, account: &str, token: [u8; 32], identity: Identity) {
        if let Ok(mut cache) = self.0.lock() {
            cache.insert(
                account.to_owned(),
                CachedProfile {
                    token,
                    identity,
                    fetched: Instant::now(),
                },
            );
        }
    }
}

pub(super) async fn fetch(
    client: &reqwest::Client,
    account: &Account,
    profiles: &ProfileCache,
) -> Result<(Snapshot, Identity), UsageError> {
    fetch_from(client, account, profiles, API_BASE_URL).await
}

async fn fetch_from(
    client: &reqwest::Client,
    account: &Account,
    profiles: &ProfileCache,
    base_url: &str,
) -> Result<(Snapshot, Identity), UsageError> {
    let credentials = load_credentials(&account.source).await?;
    if let Some(expires_at) = credentials
        .expires_at
        .filter(|expires_at| *expires_at <= chrono::Utc::now().timestamp_millis() + EXPIRY_SKEW_MS)
    {
        let since = chrono::DateTime::from_timestamp_millis(expires_at)
            .map(|since| since.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        return Err(signed_out(
            Provider::Claude,
            sign_in_directory(&account.source),
            since,
        ));
    }
    let mut response = usage_request(client, base_url, &credentials.access_token, true).await?;
    if needs_query_fallback(response.status()) {
        response = usage_request(client, base_url, &credentials.access_token, false).await?;
    }
    if response.status() == StatusCode::UNAUTHORIZED {
        return Err(signed_out(
            Provider::Claude,
            sign_in_directory(&account.source),
            None,
        ));
    }
    if !response.status().is_success() {
        return Err(http_error(
            Provider::Claude,
            response.status(),
            response.headers(),
        ));
    }
    let snapshot = parse_usage(&response_json(response).await?)?;
    let token_hash: [u8; 32] = Sha256::digest(credentials.access_token.as_bytes()).into();
    let profile = match profiles.fresh(&account.id, &token_hash) {
        Some(profile) => profile,
        None => match profile_request(client, base_url, &credentials.access_token).await {
            Some(profile) => {
                profiles.store(&account.id, token_hash, profile.clone());
                profile
            }
            None => profiles
                .last(&account.id)
                .unwrap_or_else(|| account.identity.clone()),
        },
    };
    let identity = Identity {
        email: profile.email,
        plan: profile.plan.or(credentials.plan.clone()),
        history_key: profile.history_key,
    };
    let current = load_credentials(&account.source).await?;
    ensure_same_credentials(&credentials, &current)?;
    Ok((snapshot, identity))
}

async fn profile_request(
    client: &reqwest::Client,
    base_url: &str,
    access_token: &str,
) -> Option<Identity> {
    let response = client
        .get(format!("{base_url}/api/oauth/profile"))
        .bearer_auth(access_token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Accept", "application/json")
        .header("Cache-Control", "no-store")
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .ok()
        .filter(|response| response.status().is_success())?;
    Some(parse_profile(&response_json(response).await.ok()?))
}

fn sign_in_directory(source: &AccountSource) -> Option<&Path> {
    match source {
        AccountSource::ConfigDir { path, .. } => Some(Path::new(path)),
        AccountSource::Keychain => None,
    }
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
    base_url: &str,
    access_token: &str,
    include_query: bool,
) -> Result<reqwest::Response, UsageError> {
    let mut request = client
        .get(format!("{base_url}/api/oauth/usage"))
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
        AccountSource::ConfigDir {
            path,
            keychain_suffix,
        } => {
            let file = Path::new(path).join(".credentials.json");
            #[cfg(target_os = "macos")]
            if keychain_suffix.is_some() && !file.exists() {
                return read_keychain(keychain_service(keychain_suffix.as_deref())).await;
            }
            let result = read_credentials(&file, parse_credentials, 2).await;
            #[cfg(target_os = "macos")]
            if result
                .as_ref()
                .err()
                .is_some_and(|error| error.kind == "credentials")
            {
                if keychain_suffix.is_some() {
                    return read_keychain(keychain_service(keychain_suffix.as_deref())).await;
                }
                if is_default_directory(Path::new(path)) {
                    return read_keychain(keychain_service(None)).await;
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = keychain_suffix;
            result
        }
        AccountSource::Keychain => read_keychain(keychain_service(None)).await,
    }
}

pub(crate) fn keychain_service(suffix: Option<&str>) -> String {
    match suffix {
        Some(suffix) => format!("{KEYCHAIN_SERVICE}-{suffix}"),
        None => KEYCHAIN_SERVICE.into(),
    }
}

pub fn keychain_suffix(config_dir: &str) -> String {
    let digest = Sha256::digest(config_dir.as_bytes());
    format!("{digest:x}")[..8].to_owned()
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn parse_keychain_suffixes(dump: &str) -> Vec<String> {
    let prefix = format!("\"svce\"<blob>=\"{KEYCHAIN_SERVICE}-");
    let mut suffixes = Vec::new();
    for line in dump.lines() {
        let Some(rest) = line.trim().strip_prefix(&prefix) else {
            continue;
        };
        let Some(suffix) = rest.strip_suffix('"') else {
            continue;
        };
        if valid_keychain_suffix(suffix) && !suffixes.iter().any(|known| known == suffix) {
            suffixes.push(suffix.to_owned());
        }
    }
    suffixes
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
        expires_at: oauth.get("expiresAt").and_then(Value::as_i64),
    })
}

#[cfg(target_os = "macos")]
fn is_default_directory(path: &Path) -> bool {
    super::is_home_directory(path, ".claude")
}

async fn read_keychain(service: String) -> Result<Credentials, UsageError> {
    #[cfg(target_os = "macos")]
    {
        tokio::task::spawn_blocking(move || {
            read_keychain_bytes(&service).and_then(|bytes| parse_credentials(&bytes))
        })
        .await
        .map_err(|_| keychain_error())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = service;
        Err(UsageError::new("credentials", "Keychain accounts are only available on macOS. Add this account's configuration folder."))
    }
}

#[cfg(target_os = "macos")]
fn keychain_error() -> UsageError {
    UsageError::new("credentials", "Claude Code's account token is unavailable in Keychain. Sign in with Claude Code, then refresh.")
}

#[cfg(target_os = "macos")]
pub(crate) fn keychain_available() -> bool {
    read_keychain_bytes(KEYCHAIN_SERVICE)
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
pub(crate) fn keychain_suffixes() -> Vec<String> {
    run_security(&["dump-keychain"], 32 * 1024 * 1024)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|dump| parse_keychain_suffixes(&dump))
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn read_keychain_bytes(service: &str) -> Result<Vec<u8>, UsageError> {
    run_security(
        &["find-generic-password", "-s", service, "-w"],
        super::MAX_PRIVATE_FILE_BYTES,
    )
}

#[cfg(target_os = "macos")]
fn run_security(args: &[&str], limit: u64) -> Result<Vec<u8>, UsageError> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut child = Command::new("/usr/bin/security")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| keychain_error())?;
    let stdout = child.stdout.take().ok_or_else(keychain_error)?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(limit + 1)
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
    if !succeeded || bytes.len() as u64 > limit {
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
            expires_at: None,
        };
        let after = Credentials {
            access_token: "synthetic-after".into(),
            plan: Some("Pro".into()),
            expires_at: None,
        };
        assert_eq!(
            ensure_same_credentials(&before, &after).unwrap_err().kind,
            "credentialsChanged"
        );
        assert!(ensure_same_credentials(&before, &before).is_ok());
    }

    #[test]
    fn keychain_suffix_matches_claude_code_scheme() {
        assert_eq!(
            keychain_suffix("/var/folders/57/qqyts3xn2494yf95yb3r_n200000gn/T/claude-hostloop-plugins/e4062e16ea6c1eaf"),
            "96e73013"
        );
        assert_eq!(keychain_suffix("/Users/example/.claude-work"), "dd1118a7");
        assert_ne!(
            keychain_suffix("/Users/example/.claude-work"),
            keychain_suffix("/Users/example/.claude-work/")
        );
        assert_eq!(
            keychain_service(Some("96e73013")),
            "Claude Code-credentials-96e73013"
        );
        assert_eq!(keychain_service(None), "Claude Code-credentials");
    }

    #[test]
    fn keychain_listing_yields_only_valid_unique_suffixes() {
        let dump = r#"keychain: "/Users/example/Library/Keychains/login.keychain-db"
class: "genp"
attributes:
    "acct"<blob>="example"
    "svce"<blob>="Claude Code-credentials"
    "svce"<blob>="Claude Code-credentials-52eda417"
    "svce"<blob>="Claude Code-credentials-52eda417"
    "svce"<blob>="Claude Code-credentials-ac386099"
    "svce"<blob>="Claude Code-credentials-NOTHEX00"
    "svce"<blob>="Claude Code-credentials-ac38609"
    "svce"<blob>="Other Code-credentials-8389e6a0"
    "svce"<blob>=<NULL>
"#;
        assert_eq!(parse_keychain_suffixes(dump), vec!["52eda417", "ac386099"]);
    }

    fn config_account(directory: &Path) -> Account {
        Account {
            id: "fixture-claude".into(),
            provider: Provider::Claude,
            label: "Work".into(),
            color: "#ffffff".into(),
            source: AccountSource::ConfigDir {
                path: directory.to_string_lossy().into_owned(),
                keychain_suffix: None,
            },
            identity: Identity::default(),
            snapshot: None,
            error: None,
            refreshing: false,
        }
    }

    fn write_token(directory: &Path, token: &str) {
        let expires_at = chrono::Utc::now().timestamp_millis() + 3_600_000;
        std::fs::write(
            directory.join(".credentials.json"),
            format!(r#"{{"claudeAiOauth":{{"accessToken":"{token}","expiresAt":{expires_at},"scopes":["user:profile"]}}}}"#),
        )
        .unwrap();
    }

    async fn profile_calls(server: &wiremock::MockServer) -> usize {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() == "/api/oauth/profile")
            .count()
    }

    #[tokio::test]
    async fn expired_credentials_never_reach_the_network() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::any())
            .respond_with(wiremock::ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join(".credentials.json"),
            include_str!("../../../fixtures/claude-credentials-expired.json"),
        )
        .unwrap();
        let error = fetch_from(
            &reqwest::Client::new(),
            &config_account(directory.path()),
            &ProfileCache::default(),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.kind, "signedOut");
        assert_eq!(
            error.signed_out_since.as_deref(),
            Some("2025-10-16T10:00:00Z")
        );
        let command = error.sign_in_command.unwrap();
        assert!(
            command.starts_with("CLAUDE_CONFIG_DIR=")
                || command.starts_with("$env:CLAUDE_CONFIG_DIR=")
        );
        assert_eq!(
            command,
            super::super::sign_in_command(Provider::Claude, Some(directory.path()))
        );
    }

    #[tokio::test]
    async fn profile_is_requested_once_per_token_and_survives_failures() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/api/oauth/usage"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string(include_str!("../../../fixtures/claude-all-windows.json")),
            )
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::path("/api/oauth/profile"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_string(include_str!(
                    "../../../fixtures/claude-profile-live-schema.json"
                )),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::path("/api/oauth/profile"))
            .respond_with(wiremock::ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let account = config_account(directory.path());
        let client = reqwest::Client::new();
        let profiles = ProfileCache::default();
        write_token(directory.path(), "synthetic-one");
        for _ in 0..5 {
            let (_, identity) = fetch_from(&client, &account, &profiles, &server.uri())
                .await
                .unwrap();
            assert_eq!(identity.email.as_deref(), Some("person@example.test"));
        }
        assert_eq!(profile_calls(&server).await, 1);
        write_token(directory.path(), "synthetic-two");
        let (_, identity) = fetch_from(&client, &account, &profiles, &server.uri())
            .await
            .unwrap();
        assert_eq!(profile_calls(&server).await, 2);
        assert_eq!(identity.email.as_deref(), Some("person@example.test"));
        assert_eq!(identity.plan.as_deref(), Some("Pro"));
    }

    #[tokio::test]
    async fn rejected_token_reads_as_signed_out() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/api/oauth/usage"))
            .respond_with(wiremock::ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        write_token(directory.path(), "synthetic-revoked");
        let error = fetch_from(
            &reqwest::Client::new(),
            &config_account(directory.path()),
            &ProfileCache::default(),
            &server.uri(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.kind, "signedOut");
        assert!(error.signed_out_since.is_none());
        assert!(error.sign_in_command.is_some());
    }

    #[test]
    fn unknown_plan_names_are_not_guessed() {
        assert_eq!(normalize_plan("maximum_trial".into()), "maximum_trial");
        assert_eq!(normalize_plan("default_claude_max_20x".into()), "Max 20x");
    }
}
