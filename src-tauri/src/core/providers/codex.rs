use super::{
    credentials_changed, http_error, malformed_credentials, malformed_response, nonempty_string,
    now, percent, read_credentials, read_private_file, request_error, response_json, signed_out,
    timestamp, token,
};
use crate::core::model::{
    Account, AccountSource, Identity, Provider, Snapshot, UsageError, UsageWindow,
};
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use reqwest::Url;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;

const DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api";
const AUTH_CLAIM: &str = "https://api.openai.com/auth";
const PROFILE_CLAIM: &str = "https://api.openai.com/profile";

struct Credentials {
    access_token: String,
    account_id: Option<String>,
    identity: Identity,
}

pub(super) async fn fetch(
    client: &reqwest::Client,
    account: &Account,
) -> Result<(Snapshot, Identity), UsageError> {
    fetch_from(client, account, configured_usage_url).await
}

async fn fetch_from(
    client: &reqwest::Client,
    account: &Account,
    resolve_url: impl Fn(&Path) -> Result<Url, UsageError>,
) -> Result<(Snapshot, Identity), UsageError> {
    let AccountSource::ConfigDir { path, .. } = &account.source else {
        return Err(UsageError::new(
            "credentials",
            "Add the configuration folder containing this Codex account's auth.json.",
        ));
    };
    let directory = Path::new(path);
    let credentials = read_credentials(&directory.join("auth.json"), parse_credentials, 2).await?;
    let url = resolve_url(directory)?;
    let mut request = client
        .get(url.clone())
        .bearer_auth(&credentials.access_token)
        .header("Accept", "application/json")
        .header("Cache-Control", "no-store");
    if let Some(account_id) = &credentials.account_id {
        let mut header = reqwest::header::HeaderValue::from_str(account_id)
            .map_err(|_| malformed_credentials())?;
        header.set_sensitive(true);
        request = request.header("ChatGPT-Account-Id", header);
    }
    let response = request.send().await.map_err(request_error)?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(signed_out(Provider::Codex, Some(directory), None));
    }
    if !response.status().is_success() {
        return Err(http_error(
            Provider::Codex,
            response.status(),
            response.headers(),
        ));
    }
    let value = response_json(response).await?;
    let snapshot = parse_usage(&value)?;
    let current = read_credentials(&directory.join("auth.json"), parse_credentials, 2).await?;
    ensure_same_credentials(&credentials, &current)?;
    if resolve_url(directory)? != url {
        return Err(credentials_changed());
    }
    let identity = identity_from_usage(&value, credentials.identity);
    Ok((snapshot, identity))
}

fn identity_from_usage(value: &Value, fallback: Identity) -> Identity {
    Identity {
        email: nonempty_string(value.get("email")).or(fallback.email),
        plan: nonempty_string(value.get("plan_type")).or(fallback.plan),
        history_key: fallback.history_key,
    }
}

fn ensure_same_credentials(
    previous: &Credentials,
    current: &Credentials,
) -> Result<(), UsageError> {
    if previous.access_token != current.access_token
        || previous.account_id != current.account_id
        || previous.identity != current.identity
    {
        return Err(credentials_changed());
    }
    Ok(())
}

fn parse_credentials(bytes: &[u8]) -> Result<Credentials, UsageError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| malformed_credentials())?;
    if value
        .get("auth_mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| mode == "apikey")
    {
        return Err(UsageError::new("auth", "This Codex folder uses an API key. Sign in with a ChatGPT account to read subscription usage."));
    }
    let tokens = value.get("tokens").and_then(Value::as_object).ok_or_else(|| {
        UsageError::new("credentials", "No Codex account token found. Run codex login with this folder's CODEX_HOME, then refresh.")
    })?;
    let access_token = token(tokens.get("access_token"))?;
    let access_claims = jwt_claims(&access_token);
    let id_claims = tokens
        .get("id_token")
        .and_then(Value::as_str)
        .and_then(jwt_claims);
    let mut identity = access_claims
        .as_ref()
        .map(identity_from_claims)
        .unwrap_or_default();
    if let Some(claims) = &id_claims {
        let id_identity = identity_from_claims(claims);
        identity.email = identity.email.or(id_identity.email);
        identity.plan = identity.plan.or(id_identity.plan);
    }
    let mut account_id = parse_account_id(tokens.get("account_id"))?;
    for claims in [&access_claims, &id_claims].into_iter().flatten() {
        if account_id.is_some() {
            break;
        }
        account_id = parse_account_id(
            claims
                .get(AUTH_CLAIM)
                .and_then(|auth| auth.get("chatgpt_account_id")),
        )?;
    }
    let user = [&access_claims, &id_claims]
        .into_iter()
        .flatten()
        .find_map(|claims| {
            nonempty_string(claims.pointer("/https:~1~1api.openai.com~1auth/chatgpt_user_id"))
                .or_else(|| nonempty_string(claims.get("sub")))
        })
        .or_else(|| identity.email.clone());
    identity.history_key = account_id
        .as_deref()
        .zip(user.as_deref())
        .map(|(account, user)| super::history_key(Provider::Codex, account, user));
    Ok(Credentials {
        access_token,
        account_id,
        identity,
    })
}

fn parse_account_id(value: Option<&Value>) -> Result<Option<String>, UsageError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let id = value.as_str().ok_or_else(malformed_credentials)?;
    if id.is_empty()
        || id.len() > 256
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
    {
        return Err(malformed_credentials());
    }
    Ok(Some(id.to_owned()))
}

fn jwt_claims(token: &str) -> Option<Value> {
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() != 3 || parts[1].len() > 32_768 {
        return None;
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .or_else(|_| URL_SAFE.decode(parts[1]))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn identity_from_claims(claims: &Value) -> Identity {
    let email = nonempty_string(claims.get("email")).or_else(|| {
        claims
            .get(PROFILE_CLAIM)
            .and_then(|profile| nonempty_string(profile.get("email")))
    });
    let plan = claims
        .get(AUTH_CLAIM)
        .and_then(|auth| nonempty_string(auth.get("chatgpt_plan_type")));
    Identity {
        email,
        plan,
        history_key: None,
    }
}

fn configured_usage_url(directory: &Path) -> Result<Url, UsageError> {
    let config = directory.join("config.toml");
    let base = match std::fs::metadata(&config) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => DEFAULT_BASE_URL.into(),
        Err(_) => return Err(config_error()),
        Ok(_) => {
            let bytes = read_private_file(&config).map_err(|_| config_error())?;
            let source = std::str::from_utf8(&bytes).map_err(|_| config_error())?;
            let parsed: toml::Value = source.parse().map_err(|_| config_error())?;
            match parsed.get("chatgpt_base_url") {
                None => DEFAULT_BASE_URL.into(),
                Some(value) => value
                    .as_str()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(config_error)?
                    .trim()
                    .to_owned(),
            }
        }
    };
    usage_url(&base)
}

fn config_error() -> UsageError {
    UsageError::new(
        "configuration",
        "Codex config.toml could not be read. Check its syntax and chatgpt_base_url setting.",
    )
}

fn usage_url(base: &str) -> Result<Url, UsageError> {
    let invalid = || {
        UsageError::new("configuration", "Codex chatgpt_base_url must be an HTTPS URL without credentials, a query, or a fragment.")
    };
    let mut url = Url::parse(base).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    let mut path = url.path().trim_end_matches('/').to_owned();
    let mut has_backend_api = path.split('/').any(|part| part == "backend-api");
    if matches!(url.host_str(), Some("chatgpt.com" | "chat.openai.com")) && !has_backend_api {
        path.push_str("/backend-api");
        has_backend_api = true;
    }
    path.push_str(if has_backend_api {
        "/wham/usage"
    } else {
        "/api/codex/usage"
    });
    url.set_path(&path);
    Ok(url)
}

pub fn parse_usage(value: &Value) -> Result<Snapshot, UsageError> {
    if !value.is_object() {
        return Err(malformed_response());
    }
    let mut windows = Vec::new();
    if let Some(rate_limit) = value.get("rate_limit").filter(|value| !value.is_null()) {
        append_windows(rate_limit, "", None, &mut windows)?;
    }
    let mut seen = HashSet::new();
    if let Some(additional) = value
        .get("additional_rate_limits")
        .filter(|value| !value.is_null())
    {
        for entry in additional.as_array().ok_or_else(malformed_response)? {
            if !entry.is_object() {
                return Err(malformed_response());
            }
            let Some(rate_limit) = entry.get("rate_limit").filter(|value| !value.is_null()) else {
                continue;
            };
            let name = nonempty_string(entry.get("limit_name"))
                .or_else(|| nonempty_string(entry.get("metered_feature")))
                .ok_or_else(malformed_response)?;
            if name.len() > 512 {
                return Err(malformed_response());
            }
            let key = nonempty_string(entry.get("metered_feature")).unwrap_or_else(|| name.clone());
            if !seen.insert(key.clone()) {
                continue;
            }
            let digest = Sha256::digest(key.as_bytes());
            let fingerprint = format!("{digest:x}");
            append_windows(
                rate_limit,
                &format!("extra_{}_", &fingerprint[..16]),
                Some(&name),
                &mut windows,
            )?;
        }
    }
    if windows.len() > 64 {
        return Err(malformed_response());
    }
    if windows.is_empty() {
        return Err(UsageError::new(
            "unavailable",
            "Codex did not return a measured usage window for this account.",
        ));
    }
    Ok(Snapshot {
        windows,
        fetched_at: now(),
        extra_usage: None,
    })
}

fn append_windows(
    rate_limit: &Value,
    prefix: &str,
    name: Option<&str>,
    windows: &mut Vec<UsageWindow>,
) -> Result<(), UsageError> {
    if !rate_limit.is_object() {
        return Err(malformed_response());
    }
    for (key, default_label) in [
        ("primary_window", "Session"),
        ("secondary_window", "Weekly"),
    ] {
        let Some(window) = rate_limit.get(key).filter(|value| !value.is_null()) else {
            continue;
        };
        if !window.is_object() {
            return Err(malformed_response());
        }
        let Some(utilization) = window.get("used_percent").filter(|value| !value.is_null()) else {
            continue;
        };
        let duration = match window
            .get("limit_window_seconds")
            .filter(|value| !value.is_null())
        {
            Some(value) => Some(
                value
                    .as_u64()
                    .filter(|seconds| *seconds > 0)
                    .ok_or_else(malformed_response)?,
            ),
            None => None,
        };
        let period = match duration {
            Some(604_800) => "Weekly".to_owned(),
            Some(2_592_000) => "Monthly".to_owned(),
            Some(18_000) => "Session".to_owned(),
            None => default_label.to_owned(),
            Some(seconds) if seconds % 86_400 == 0 => format!("{} days", seconds / 86_400),
            Some(seconds) if seconds % 3_600 == 0 => format!("{} hours", seconds / 3_600),
            Some(seconds) if seconds % 60 == 0 => format!("{} minutes", seconds / 60),
            Some(seconds) => format!("{seconds} seconds"),
        };
        windows.push(UsageWindow {
            id: format!("{prefix}{key}"),
            label: name.map_or_else(
                || period.clone(),
                |name| format!("{name} · {}", period.to_lowercase()),
            ),
            used_percent: percent(utilization)?,
            resets_at: timestamp(window.get("reset_at"))?,
            window_seconds: duration,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::WindowScope;
    use serde_json::json;

    fn jwt(claims: Value) -> String {
        format!(
            "synthetic.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        )
    }

    #[test]
    fn history_identity_separates_workspaces_and_survives_token_rotation() {
        let credentials = |workspace: &str, issued: u64| {
            parse_credentials(
                &serde_json::to_vec(&json!({
                    "tokens": {
                        "access_token": jwt(json!({"sub": "user-one", "iat": issued})),
                        "account_id": workspace
                    }
                }))
                .unwrap(),
            )
            .unwrap()
        };
        let original = credentials("workspace-one", 1);
        assert_eq!(original.identity.history_key.as_ref().unwrap().len(), 64);
        assert_eq!(
            original.identity.history_key,
            credentials("workspace-one", 2).identity.history_key
        );
        assert_ne!(
            original.identity.history_key,
            credentials("workspace-two", 1).identity.history_key
        );
    }

    #[test]
    fn live_primary_weekly_shape_does_not_invent_a_session_or_spend() {
        let value =
            serde_json::from_str(include_str!("../../../fixtures/codex-live-schema.json")).unwrap();
        let snapshot = parse_usage(&value).unwrap();
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].id, "primary_window");
        assert_eq!(snapshot.windows[0].label, "Weekly");
        assert_eq!(snapshot.windows[0].window_seconds, Some(604_800));
        assert_eq!(snapshot.windows[0].used_percent, 34.0);
        assert!(snapshot.windows[0].resets_at.is_some());
        assert!(snapshot.extra_usage.is_none());
    }

    #[test]
    fn response_identity_takes_precedence_and_claims_fill_missing_fields() {
        let value =
            serde_json::from_str(include_str!("../../../fixtures/codex-live-schema.json")).unwrap();
        let fallback = Identity {
            email: Some("previous@example.test".into()),
            plan: Some("plus".into()),
            history_key: None,
        };
        let identity = identity_from_usage(&value, fallback.clone());
        assert_eq!(identity.email.as_deref(), Some("person@example.test"));
        assert_eq!(identity.plan.as_deref(), Some("pro"));
        assert_eq!(identity_from_usage(&json!({}), fallback.clone()), fallback);
    }

    #[test]
    fn parses_primary_secondary_and_named_extra_windows() {
        let value =
            serde_json::from_str(include_str!("../../../fixtures/codex-all-windows.json")).unwrap();
        let snapshot = parse_usage(&value).unwrap();
        assert_eq!(snapshot.windows.len(), 4);
        assert_eq!(snapshot.windows[0].used_percent, 28.0);
        assert_eq!(snapshot.windows[1].label, "Weekly");
        assert_eq!(snapshot.windows[2].label, "Spark · session");
        assert_eq!(snapshot.windows[0].scope(), WindowScope::Core);
        assert!(snapshot
            .windows
            .iter()
            .skip(2)
            .all(|window| window.scope() == WindowScope::Other));
        assert_eq!(snapshot.windows[3].window_seconds, Some(604_800));
    }

    #[test]
    fn missing_windows_never_become_zero_and_secondary_stays_weekly() {
        for value in [
            json!({}),
            json!({"rate_limit": null}),
            json!({"rate_limit": {"primary_window": {}}}),
        ] {
            assert!(parse_usage(&value).is_err());
        }
        let value =
            serde_json::from_str(include_str!("../../../fixtures/codex-weekly-only.json")).unwrap();
        let snapshot = parse_usage(&value).unwrap();
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].id, "secondary_window");
        assert_eq!(snapshot.windows[0].used_percent, 91.0);
    }

    #[test]
    fn unknown_reset_remains_unknown_and_bad_percent_fails_closed() {
        let snapshot =
            parse_usage(&json!({"rate_limit": {"primary_window": {"used_percent": 0}}})).unwrap();
        assert_eq!(snapshot.windows[0].resets_at, None);
        assert_eq!(snapshot.windows[0].window_seconds, None);
        for value in [json!(-1), json!("50")] {
            assert!(parse_usage(
                &json!({"rate_limit": {"primary_window": {"used_percent": value}}})
            )
            .is_err());
        }
        let capped =
            parse_usage(&json!({"rate_limit": {"primary_window": {"used_percent": 104.2}}}))
                .unwrap();
        assert_eq!(capped.windows[0].used_percent, 100.0);
    }

    #[test]
    fn preserves_reported_nonstandard_window_durations() {
        let snapshot = parse_usage(&json!({"rate_limit": {"primary_window": {"used_percent": 50, "limit_window_seconds": 2_592_000}}})).unwrap();
        assert_eq!(snapshot.windows[0].label, "Monthly");
        assert_eq!(snapshot.windows[0].window_seconds, Some(2_592_000));
        let snapshot = parse_usage(&json!({"rate_limit": {"secondary_window": {"used_percent": 50, "limit_window_seconds": 18_000}}})).unwrap();
        assert_eq!(snapshot.windows[0].label, "Session");
    }

    #[test]
    fn parses_observed_auth_shape_with_identity_claims() {
        let id_token = jwt(
            json!({"email": "person@example.test", (AUTH_CLAIM): {"chatgpt_plan_type": "pro"}}),
        );
        let bytes = serde_json::to_vec(&json!({"auth_mode": "chatgpt", "OPENAI_API_KEY": null, "tokens": {"id_token": id_token, "access_token": "synthetic", "refresh_token": "ignored", "account_id": "example-account"}, "last_refresh": "2026-10-16T10:00:00Z"})).unwrap();
        let credentials = parse_credentials(&bytes).unwrap();
        assert_eq!(
            credentials.identity.email.as_deref(),
            Some("person@example.test")
        );
        assert_eq!(credentials.identity.plan.as_deref(), Some("pro"));
        assert_eq!(credentials.account_id.as_deref(), Some("example-account"));
    }

    #[test]
    fn access_claims_fill_missing_identity_and_account_id() {
        let access_token = jwt(
            json!({(PROFILE_CLAIM): {"email": "person@example.test"}, (AUTH_CLAIM): {"chatgpt_plan_type": "plus", "chatgpt_account_id": "account-2"}}),
        );
        let credentials = parse_credentials(
            &serde_json::to_vec(&json!({"tokens": {"access_token": access_token}})).unwrap(),
        )
        .unwrap();
        assert_eq!(credentials.identity.plan.as_deref(), Some("plus"));
        assert_eq!(credentials.account_id.as_deref(), Some("account-2"));
    }

    #[test]
    fn rejects_api_keys_partial_json_and_header_injection() {
        assert!(parse_credentials(br#"{"OPENAI_API_KEY":"synthetic"}"#).is_err());
        assert!(parse_credentials(br#"{"tokens": "#).is_err());
        assert!(parse_credentials(&serde_json::to_vec(&json!({"tokens": {"access_token": "synthetic", "account_id": "account\r\nX-Test: injection"}})).unwrap()).is_err());
        assert!(jwt_claims("not-a-jwt").is_none());
    }

    #[test]
    fn invalid_explicit_account_ids_do_not_silently_become_default_accounts() {
        for id in [
            json!(""),
            json!("  "),
            json!(42),
            json!("account\nname"),
            json!("x".repeat(257)),
        ] {
            assert!(parse_credentials(
                &serde_json::to_vec(
                    &json!({"tokens": {"access_token": "synthetic", "account_id": id}})
                )
                .unwrap()
            )
            .is_err());
        }
        assert_eq!(parse_account_id(Some(&Value::Null)).unwrap(), None);
    }

    #[test]
    fn usage_urls_follow_codex_base_path_semantics() {
        for base in [
            "https://chatgpt.com",
            "https://chatgpt.com/",
            "https://chatgpt.com/backend-api/",
        ] {
            assert_eq!(
                usage_url(base).unwrap().as_str(),
                "https://chatgpt.com/backend-api/wham/usage"
            );
        }
        assert_eq!(
            usage_url("https://proxy.example.test/backend-api")
                .unwrap()
                .as_str(),
            "https://proxy.example.test/backend-api/wham/usage"
        );
        assert_eq!(
            usage_url("https://proxy.example.test").unwrap().as_str(),
            "https://proxy.example.test/api/codex/usage"
        );
    }

    #[test]
    fn rejects_unsafe_custom_url_components() {
        for base in [
            "http://chatgpt.com",
            "file:///tmp/usage",
            "https://user:secret@example.test",
            "https://example.test?token=secret",
            "https://example.test#fragment",
            "/relative",
        ] {
            let error = usage_url(base).unwrap_err();
            assert_eq!(error.kind, "configuration");
            assert!(!error.message.contains("secret"));
        }
    }

    #[test]
    fn toml_parser_respects_comments_and_scoped_settings() {
        let config: toml::Value = "# chatgpt_base_url = 'https://wrong.test'\nchatgpt_base_url = 'https://proxy.example.test/backend-api' # chosen\n[profiles.other]\nchatgpt_base_url = 'https://other.test'\n".parse().unwrap();
        let url = usage_url(config["chatgpt_base_url"].as_str().unwrap()).unwrap();
        assert_eq!(url.host_str(), Some("proxy.example.test"));
    }

    #[test]
    fn credentials_or_workspace_changed_during_fetch_are_not_published() {
        let before = Credentials {
            access_token: "synthetic".into(),
            account_id: Some("one".into()),
            identity: Identity::default(),
        };
        let after = Credentials {
            access_token: "synthetic".into(),
            account_id: Some("two".into()),
            identity: Identity::default(),
        };
        assert_eq!(
            ensure_same_credentials(&before, &after).unwrap_err().kind,
            "credentialsChanged"
        );
        assert!(ensure_same_credentials(&before, &before).is_ok());
    }

    #[test]
    fn bearer_claims_take_precedence_over_older_id_token_metadata() {
        let access_token = jwt(json!({(PROFILE_CLAIM): {"email": "current@example.test"}}));
        let id_token = jwt(json!({"email": "previous@example.test"}));
        let credentials = parse_credentials(
            &serde_json::to_vec(
                &json!({"tokens": {"access_token": access_token, "id_token": id_token}}),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            credentials.identity.email.as_deref(),
            Some("current@example.test")
        );
    }

    fn config_account(directory: &Path) -> Account {
        std::fs::write(
            directory.join("auth.json"),
            serde_json::to_vec(&json!({"auth_mode": "chatgpt", "tokens": {"access_token": "synthetic", "account_id": "example-account"}})).unwrap(),
        )
        .unwrap();
        Account {
            id: "fixture-codex".into(),
            provider: Provider::Codex,
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

    async fn fetch_status(
        response: wiremock::ResponseTemplate,
    ) -> Result<(Snapshot, Identity), UsageError> {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path("/backend-api/wham/usage"))
            .and(wiremock::matchers::header(
                "authorization",
                "Bearer synthetic",
            ))
            .and(wiremock::matchers::header(
                "chatgpt-account-id",
                "example-account",
            ))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;
        let directory = tempfile::tempdir().unwrap();
        let account = config_account(directory.path());
        let url = Url::parse(&format!("{}/backend-api/wham/usage", server.uri())).unwrap();
        fetch_from(&reqwest::Client::new(), &account, |_| Ok(url.clone())).await
    }

    #[tokio::test]
    async fn fetch_reads_usage_with_account_header() {
        let (snapshot, _) = fetch_status(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(include_str!("../../../fixtures/codex-all-windows.json")),
        )
        .await
        .unwrap();
        assert!(!snapshot.windows.is_empty());
    }

    #[tokio::test]
    async fn fetch_rejected_token_reads_as_signed_out() {
        let error = fetch_status(wiremock::ResponseTemplate::new(401))
            .await
            .unwrap_err();
        assert_eq!(error.kind, "signedOut");
        assert!(error.sign_in_command.unwrap().contains("codex login"));
    }

    #[tokio::test]
    async fn fetch_rate_limit_honours_retry_after() {
        let error =
            fetch_status(wiremock::ResponseTemplate::new(429).insert_header("retry-after", "120"))
                .await
                .unwrap_err();
        assert_eq!(error.kind, "rateLimited");
        assert_eq!(error.retry_after_seconds, Some(120));
    }

    #[tokio::test]
    async fn fetch_server_errors_are_transient() {
        for status in [500, 502, 503] {
            let error = fetch_status(wiremock::ResponseTemplate::new(status))
                .await
                .unwrap_err();
            assert_eq!(error.kind, "provider");
            assert!(error.retry_after_seconds.is_none());
            assert!(error.sign_in_command.is_none());
        }
    }
}
