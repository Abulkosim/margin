use super::model::{Account, AccountSource, Identity, Provider};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn discover(existing: &[Account]) -> Vec<Account> {
    let mut accounts = existing.to_vec();
    let mut candidates = Vec::new();
    if let Some(home) = dirs::home_dir() {
        candidates.push((Provider::Claude, home.join(".claude")));
        candidates.push((Provider::Codex, home.join(".codex")));
        if let Ok(entries) = std::fs::read_dir(&home) {
            let mut paths: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
            paths.sort();
            for path in paths {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if !name.starts_with('.') || !path.is_dir() {
                    continue;
                }
                for provider in [Provider::Claude, Provider::Codex] {
                    if name.contains(provider.key()) {
                        candidates.push((provider, path.clone()));
                    }
                }
            }
        }
    }
    for (provider, variable) in [
        (Provider::Claude, "CLAUDE_CONFIG_DIR"),
        (Provider::Codex, "CODEX_HOME"),
    ] {
        if let Some(path) = std::env::var_os(variable).filter(|value| !value.is_empty()) {
            candidates.insert(0, (provider, PathBuf::from(path)));
        }
    }
    merge_candidates(&mut accounts, candidates);
    #[cfg(target_os = "macos")]
    if !has_default_claude_account(&accounts)
        && !accounts.iter().any(|account| {
            account.provider == Provider::Claude && account.source == AccountSource::Keychain
        })
        && super::providers::claude::keychain_available()
    {
        accounts.push(new_account(
            Provider::Claude,
            AccountSource::Keychain,
            "Claude".into(),
        ));
    }
    accounts
}

pub fn add_config_dir(provider: Provider, path: &str) -> Result<Account, String> {
    let path = expand_path(path)?;
    if !path.is_absolute() {
        return Err("Choose an absolute configuration folder path.".into());
    }
    let canonical = std::fs::canonicalize(&path)
        .map_err(|_| "That configuration folder could not be opened.".to_owned())?;
    if !canonical.is_dir() {
        return Err("Choose a configuration folder, not a file.".into());
    }
    if !canonical.join(provider.credential_filename()).is_file() {
        return Err(format!(
            "No {} credentials found in that folder. Sign in with the CLI first.",
            provider.name()
        ));
    }
    let path = canonical
        .to_str()
        .ok_or_else(|| "The configuration folder path must contain valid text.".to_owned())?
        .to_owned();
    if path.len() > 4096 || path.chars().any(char::is_control) {
        return Err("The resolved configuration folder path contains unsupported text.".into());
    }
    let label = default_label(provider, &canonical);
    Ok(new_account(
        provider,
        AccountSource::ConfigDir { path },
        label,
    ))
}

fn merge_candidates(accounts: &mut Vec<Account>, candidates: Vec<(Provider, PathBuf)>) {
    let mut known: HashSet<String> = accounts.iter().map(source_key).collect();
    for (provider, path) in candidates {
        let Some(path) = path.to_str() else { continue };
        if let Ok(account) = add_config_dir(provider, path) {
            if known.insert(source_key(&account)) {
                accounts.push(account);
            }
        }
    }
}

fn source_key(account: &Account) -> String {
    match &account.source {
        AccountSource::ConfigDir { path } => {
            let normalized = std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));
            format!(
                "{}:{}",
                account.provider.key(),
                normalized.to_string_lossy()
            )
        }
        AccountSource::Keychain => format!("{}:keychain", account.provider.key()),
    }
}

fn expand_path(path: &str) -> Result<PathBuf, String> {
    let path = path.trim();
    if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err("Enter a configuration folder path.".into());
    }
    if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
        let home =
            dirs::home_dir().ok_or_else(|| "Your home folder could not be found.".to_owned())?;
        return Ok(if path == "~" {
            home
        } else {
            home.join(&path[2..])
        });
    }
    Ok(PathBuf::from(path))
}

fn default_label(provider: Provider, path: &Path) -> String {
    let folder = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(provider.key());
    if folder == format!(".{}", provider.key()) || folder == provider.key() {
        return provider.name().into();
    }
    let label = folder.trim_start_matches('.').trim();
    if label.is_empty() {
        provider.name().into()
    } else {
        label.chars().take(48).collect()
    }
}

fn new_account(provider: Provider, source: AccountSource, label: String) -> Account {
    let mut hasher = Sha256::new();
    hasher.update(provider.key().as_bytes());
    hasher.update([0]);
    match &source {
        AccountSource::ConfigDir { path } => hasher.update(path.as_bytes()),
        AccountSource::Keychain => hasher.update(b"keychain"),
    }
    let digest = hasher.finalize();
    let fingerprint = format!("{digest:x}");
    Account {
        id: format!("{}-{}", provider.key(), &fingerprint[..24]),
        provider,
        label,
        color: match provider {
            Provider::Claude => "#b48265",
            Provider::Codex => "#8e91a8",
        }
        .into(),
        source,
        identity: Identity::default(),
        snapshot: None,
        error: None,
        refreshing: false,
    }
}

#[cfg(target_os = "macos")]
fn has_default_claude_account(accounts: &[Account]) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let default = std::fs::canonicalize(home.join(".claude")).ok();
    accounts.iter().any(|account| {
        account.provider == Provider::Claude
            && match &account.source {
                AccountSource::ConfigDir { path } => {
                    default.is_some() && std::fs::canonicalize(path).ok() == default
                }
                AccountSource::Keychain => false,
            }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "margin-accounts-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn duplicate_discovery_preserves_user_metadata() {
        let dir = TestDir::new();
        std::fs::write(dir.0.join("auth.json"), "{}").unwrap();
        let mut account = add_config_dir(Provider::Codex, dir.0.to_str().unwrap()).unwrap();
        account.label = "Personal".into();
        account.color = "#123456".into();
        account.identity.email = Some("person@example.test".into());
        let mut accounts = vec![account.clone()];
        merge_candidates(
            &mut accounts,
            vec![
                (Provider::Codex, dir.0.clone()),
                (Provider::Codex, dir.0.join(".")),
            ],
        );
        assert_eq!(accounts, vec![account]);
    }

    #[test]
    fn ids_distinguish_provider_and_path_without_exposing_paths() {
        let a = new_account(
            Provider::Codex,
            AccountSource::ConfigDir {
                path: "/private/a".into(),
            },
            "A".into(),
        );
        let b = new_account(Provider::Claude, a.source.clone(), "B".into());
        let c = new_account(
            Provider::Codex,
            AccountSource::ConfigDir {
                path: "/private/b".into(),
            },
            "C".into(),
        );
        assert_ne!(a.id, b.id);
        assert_ne!(a.id, c.id);
        assert!(!a.id.contains("private"));
        assert_eq!(a.id.len(), 30);
    }

    #[test]
    fn rejects_missing_credentials_and_relative_paths() {
        let dir = TestDir::new();
        assert!(add_config_dir(Provider::Codex, dir.0.to_str().unwrap()).is_err());
        assert!(add_config_dir(Provider::Codex, ".codex").is_err());
        assert!(expand_path("").is_err());
    }

    #[test]
    fn default_labels_fit_rename_limits_without_splitting_unicode() {
        let directory = PathBuf::from(format!(".codex-{}", "界".repeat(60)));
        let label = default_label(Provider::Codex, &directory);
        assert_eq!(label.chars().count(), 48);
        assert!(label.starts_with("codex-"));
        assert_eq!(default_label(Provider::Codex, Path::new(".codex")), "Codex");
        assert_eq!(default_label(Provider::Claude, Path::new(".   ")), "Claude");
    }
}
