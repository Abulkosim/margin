pub mod claude;
pub mod codex;

use super::model::{Account, Identity, Provider, Snapshot, UsageError};
use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::header::{HeaderMap, RETRY_AFTER};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::time::Duration;

const MAX_PRIVATE_FILE_BYTES: u64 = 1_048_576;
const MAX_RESPONSE_BYTES: usize = 1_048_576;
const MAX_RETRY_AFTER_SECONDS: u64 = 86_400;

fn history_key(provider: Provider, account: &str, scope: &str) -> String {
    let mut digest = Sha256::new();
    for part in [provider.key(), account, scope] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

pub fn http_client() -> Result<reqwest::Client, UsageError> {
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("Margin/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| UsageError::new("network", "The secure connection could not be initialized."))
}

pub async fn fetch(
    client: &reqwest::Client,
    account: &Account,
    profiles: &claude::ProfileCache,
    resets: &codex::ResetCreditCache,
) -> Result<(Snapshot, Identity), UsageError> {
    match account.provider {
        Provider::Claude => claude::fetch(client, account, profiles).await,
        Provider::Codex => codex::fetch(client, account, resets).await,
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn malformed_response() -> UsageError {
    UsageError::new(
        "invalidResponse",
        "The provider returned an unrecognized usage response.",
    )
}

fn malformed_credentials() -> UsageError {
    UsageError::new(
        "credentials",
        "The CLI credentials are incomplete. Finish signing in, then refresh.",
    )
}

fn credentials_changed() -> UsageError {
    UsageError::new(
        "credentialsChanged",
        "The CLI account changed during refresh. Refresh again to read the current account.",
    )
}

fn read_private_file(path: &Path) -> Result<Vec<u8>, UsageError> {
    let read_error = |error: std::io::Error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            UsageError::new(
                "credentials",
                "CLI credentials were not found. Sign in with the CLI, then refresh.",
            )
        } else {
            UsageError::new(
                "credentials",
                "CLI credentials could not be read. Check the folder permissions.",
            )
        }
    };
    let metadata = std::fs::metadata(path).map_err(read_error)?;
    if !metadata.is_file() || metadata.len() > MAX_PRIVATE_FILE_BYTES {
        return Err(malformed_credentials());
    }
    let file = std::fs::File::open(path).map_err(read_error)?;
    let metadata = file.metadata().map_err(|_| malformed_credentials())?;
    if !metadata.is_file() || metadata.len() > MAX_PRIVATE_FILE_BYTES {
        return Err(malformed_credentials());
    }
    let mut bytes = Vec::new();
    file.take(MAX_PRIVATE_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| malformed_credentials())?;
    if bytes.len() as u64 > MAX_PRIVATE_FILE_BYTES {
        return Err(malformed_credentials());
    }
    Ok(bytes)
}

async fn read_credentials<T>(
    path: &Path,
    parser: fn(&[u8]) -> Result<T, UsageError>,
    retries: u8,
) -> Result<T, UsageError> {
    let mut attempt = 0;
    loop {
        let result = read_private_file(path).and_then(|bytes| parser(&bytes));
        if result.is_ok()
            || attempt >= retries
            || result
                .as_ref()
                .err()
                .is_some_and(|error| error.kind != "credentials")
        {
            return result;
        }
        attempt += 1;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn response_json(mut response: reqwest::Response) -> Result<Value, UsageError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(malformed_response());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| {
        UsageError::new(
            "network",
            "The usage response was interrupted. Refresh to try again.",
        )
    })? {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(malformed_response());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| malformed_response())
}

fn signed_out(provider: Provider, directory: Option<&Path>, since: Option<String>) -> UsageError {
    let command = sign_in_command(provider, directory);
    let message = format!(
        "{} is signed out. Run {command}, then refresh.",
        provider.name()
    );
    UsageError {
        signed_out_since: since,
        sign_in_command: Some(command),
        ..UsageError::new("signedOut", &message)
    }
}

pub(crate) fn sign_in_command(provider: Provider, directory: Option<&Path>) -> String {
    let (variable, program, default) = match provider {
        Provider::Claude => ("CLAUDE_CONFIG_DIR", "claude", ".claude"),
        Provider::Codex => ("CODEX_HOME", "codex login", ".codex"),
    };
    let Some(directory) = directory.filter(|directory| !is_home_directory(directory, default))
    else {
        return program.into();
    };
    let path = directory.to_string_lossy();
    if cfg!(windows) {
        format!("$env:{variable}='{}'; {program}", path.replace('\'', "''"))
    } else if path
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"/._-+@%:,".contains(&byte))
    {
        format!("{variable}={path} {program}")
    } else {
        format!("{variable}='{}' {program}", path.replace('\'', "'\\''"))
    }
}

fn is_home_directory(path: &Path, name: &str) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let default = home.join(name);
    path == default
        || std::fs::canonicalize(path)
            .ok()
            .zip(std::fs::canonicalize(&default).ok())
            .is_some_and(|(path, default)| path == default)
}

fn request_error(error: reqwest::Error) -> UsageError {
    if error.is_timeout() {
        UsageError::new(
            "network",
            "The provider took too long to respond. Refresh to try again.",
        )
    } else if error.is_builder() {
        UsageError::new(
            "credentials",
            "The request could not be authenticated. Sign in with the CLI again.",
        )
    } else {
        UsageError::new(
            "network",
            "Could not reach the provider. Check your connection.",
        )
    }
}

fn http_error(provider: Provider, status: reqwest::StatusCode, headers: &HeaderMap) -> UsageError {
    match status.as_u16() {
        401 => UsageError::new("auth", match provider {
            Provider::Claude => "Claude sign-in expired. Open Claude Code in this account's configuration folder and sign in again.",
            Provider::Codex => "Codex sign-in expired. Run codex login with this account's CODEX_HOME, then refresh.",
        }),
        403 => UsageError::new("auth", match provider {
            Provider::Claude => "Claude usage access was denied. Sign in with a token that includes user:profile.",
            Provider::Codex => "Codex usage access was denied. Check this account's sign-in and subscription.",
        }),
        429 => UsageError {
            retry_after_seconds: retry_after(headers, Utc::now())
                .and_then(|seconds| u32::try_from(seconds).ok()),
            ..UsageError::new("rateLimited", "The provider asked us to wait. Retrying automatically.")
        },
        300..=399 => UsageError::new("redirect", "The provider redirected the request. Credentials were not forwarded."),
        500..=599 => UsageError::new("provider", "The provider is temporarily unavailable. Retrying automatically."),
        _ => UsageError::new("provider", "The provider could not return usage. Refresh to try again."),
    }
}

fn retry_after(headers: &HeaderMap, now: DateTime<Utc>) -> Option<u64> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds.clamp(1, MAX_RETRY_AFTER_SECONDS));
    }
    let target = DateTime::parse_from_rfc2822(value)
        .ok()?
        .with_timezone(&Utc);
    Some(
        (target.signed_duration_since(now).num_seconds().max(1) as u64)
            .min(MAX_RETRY_AFTER_SECONDS),
    )
}

fn percent(value: &Value) -> Result<f64, UsageError> {
    let value = value.as_f64().ok_or_else(malformed_response)?;
    if !value.is_finite() || value < 0.0 {
        return Err(malformed_response());
    }
    Ok(value.min(100.0))
}

fn timestamp(value: Option<&Value>) -> Result<Option<String>, UsageError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let parsed = if let Some(value) = value.as_str() {
        DateTime::parse_from_rfc3339(value)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|_| malformed_response())?
    } else if let Some(value) = value.as_i64() {
        DateTime::from_timestamp(value, 0).ok_or_else(malformed_response)?
    } else {
        return Err(malformed_response());
    };
    Ok(Some(parsed.to_rfc3339_opts(SecondsFormat::Secs, true)))
}

fn nonempty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| {
            !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
        })
        .map(str::to_owned)
}

fn token(value: Option<&Value>) -> Result<String, UsageError> {
    let token = value
        .and_then(Value::as_str)
        .ok_or_else(malformed_credentials)?;
    if token.is_empty()
        || token.len() > 32_768
        || !token.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(malformed_credentials());
    }
    Ok(token.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn percentages_reject_invalid_values_and_cap_overuse() {
        for value in [json!(null), json!("50"), json!(-1), json!(-0.5)] {
            assert!(percent(&value).is_err());
        }
        assert_eq!(percent(&json!(0)).unwrap(), 0.0);
        assert_eq!(percent(&json!(100)).unwrap(), 100.0);
        assert_eq!(percent(&json!(104.2)).unwrap(), 100.0);
        assert_eq!(percent(&json!(1e6)).unwrap(), 100.0);
    }

    #[test]
    fn reset_timestamp_is_normalized_and_missing_remains_unknown() {
        assert_eq!(
            timestamp(Some(&json!("2026-10-16T15:00:00+05:00"))).unwrap(),
            Some("2026-10-16T10:00:00Z".into())
        );
        assert_eq!(timestamp(None).unwrap(), None);
        assert_eq!(timestamp(Some(&Value::Null)).unwrap(), None);
        assert!(timestamp(Some(&json!("soon"))).is_err());
    }

    #[test]
    fn retry_after_accepts_seconds_and_http_dates() {
        let mut headers = HeaderMap::new();
        let now = DateTime::parse_from_rfc3339("2026-10-16T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        headers.insert(RETRY_AFTER, HeaderValue::from_static("120"));
        assert_eq!(retry_after(&headers, now), Some(120));
        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_static("Fri, 16 Oct 2026 10:02:00 GMT"),
        );
        assert_eq!(retry_after(&headers, now), Some(120));
        headers.insert(RETRY_AFTER, HeaderValue::from_static("garbage"));
        assert_eq!(retry_after(&headers, now), None);
        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_static("18446744073709551615"),
        );
        assert_eq!(retry_after(&headers, now), Some(86_400));
        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_static("Sat, 16 Oct 2027 10:00:00 GMT"),
        );
        assert_eq!(retry_after(&headers, now), Some(86_400));
    }

    #[test]
    fn status_errors_are_safe_and_rate_limits_carry_backoff() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("90"));
        let error = http_error(
            Provider::Codex,
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            &headers,
        );
        assert_eq!(error.kind, "rateLimited");
        assert_eq!(error.retry_after_seconds, Some(90));
        assert_eq!(
            http_error(
                Provider::Claude,
                reqwest::StatusCode::UNAUTHORIZED,
                &headers
            )
            .kind,
            "auth"
        );
        assert_eq!(
            http_error(Provider::Codex, reqwest::StatusCode::FOUND, &headers).kind,
            "redirect"
        );
        assert!(!error.message.contains("Bearer"));
    }

    #[test]
    fn sign_in_commands_target_the_account_folder() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(sign_in_command(Provider::Claude, None), "claude");
        assert_eq!(
            sign_in_command(Provider::Claude, Some(&home.join(".claude"))),
            "claude"
        );
        assert_eq!(
            sign_in_command(Provider::Codex, Some(&home.join(".codex"))),
            "codex login"
        );
        let error = signed_out(
            Provider::Codex,
            Some(Path::new("/example/.codex-work")),
            None,
        );
        assert_eq!(error.kind, "signedOut");
        if cfg!(windows) {
            assert_eq!(
                error.sign_in_command.as_deref(),
                Some("$env:CODEX_HOME='/example/.codex-work'; codex login")
            );
        } else {
            assert_eq!(
                error.sign_in_command.as_deref(),
                Some("CODEX_HOME=/example/.codex-work codex login")
            );
            assert_eq!(
                sign_in_command(Provider::Claude, Some(Path::new("/example/My Claude's"))),
                "CLAUDE_CONFIG_DIR='/example/My Claude'\\''s' claude"
            );
        }
    }

    #[test]
    fn private_reads_reject_directories_and_oversized_files() {
        let directory = tempfile::tempdir().unwrap();
        assert!(read_private_file(directory.path()).is_err());
        let path = directory.path().join("auth.json");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(MAX_PRIVATE_FILE_BYTES + 1)
            .unwrap();
        assert!(read_private_file(&path).is_err());
    }

    #[tokio::test]
    async fn recovers_from_partial_credential_writes() {
        let path = std::env::temp_dir().join(format!(
            "margin-credentials-{}-{}.json",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, "{\"token\":").unwrap();
        let writer_path = path.clone();
        let writer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            std::fs::write(writer_path, "{\"token\":\"synthetic\"}").unwrap();
        });
        fn parse(bytes: &[u8]) -> Result<Value, UsageError> {
            serde_json::from_slice(bytes).map_err(|_| malformed_credentials())
        }
        let value = read_credentials(&path, parse, 2).await.unwrap();
        writer.await.unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(value["token"], "synthetic");
    }
}
