use super::{
    model::{Account, AccountSource, Provider, Snapshot},
    notifications::AlertMemory,
    settings::Settings,
};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::Path,
};

const MAX_STORE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ACCOUNTS: usize = 100;
const MAX_WINDOWS: usize = 64;

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredState {
    pub version: u32,
    pub accounts: Vec<Account>,
    pub settings: Settings,
    #[serde(default)]
    pub alerts: AlertMemory,
}

pub fn load(path: &Path) -> Result<StoredState, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StoredState {
                version: 1,
                ..StoredState::default()
            });
        }
        Err(_) => return Err("Margin could not read its settings file.".into()),
    };
    if !metadata.is_file() || metadata.len() > MAX_STORE_BYTES {
        return Err(invalid_store());
    }
    let file = fs::File::open(path).map_err(|_| "Margin could not read its settings file.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Margin could not inspect its settings file.")?;
    if !metadata.is_file() || metadata.len() > MAX_STORE_BYTES {
        return Err(invalid_store());
    }
    let mut bytes = Vec::new();
    file.take(MAX_STORE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Margin could not read its settings file.")?;
    if bytes.len() as u64 > MAX_STORE_BYTES {
        return Err(invalid_store());
    }
    let mut state: StoredState = serde_json::from_slice(&bytes).map_err(|_| invalid_store())?;
    validate(&state)?;
    for account in &mut state.accounts {
        account.refreshing = false;
        if account.snapshot.is_some() {
            account.error = Some(super::model::UsageError {
                kind: "stale".into(),
                message: "Last saved reading. Waiting for a fresh update.".into(),
                retry_after_seconds: None,
            });
        }
    }
    Ok(state)
}

pub fn save(path: &Path, state: &StoredState) -> Result<(), String> {
    validate(state)?;
    let parent = path.parent().ok_or("Invalid settings location.")?;
    fs::create_dir_all(parent).map_err(|_| "Margin could not create its settings folder.")?;
    let mut file =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "Margin could not save settings.")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| "Margin could not protect its settings file.")?;
    }
    serde_json::to_writer_pretty(&mut file, state).map_err(|_| "Could not encode settings.")?;
    file.write_all(b"\n")
        .map_err(|_| "Could not write settings.")?;
    if file
        .as_file()
        .metadata()
        .map_err(|_| "Could not inspect saved settings.")?
        .len()
        > MAX_STORE_BYTES
    {
        return Err("Margin's settings exceed the supported file size.".into());
    }
    file.as_file()
        .sync_all()
        .map_err(|_| "Could not flush settings.")?;
    file.persist(path)
        .map_err(|_| "Could not replace settings atomically.")?;
    Ok(())
}

fn invalid_store() -> String {
    "Margin's settings file is damaged or unsupported. Preserve it before restoring a backup."
        .into()
}

fn validate(state: &StoredState) -> Result<(), String> {
    if state.version != 1 {
        return Err("This settings file needs a newer version of Margin.".into());
    }
    state.settings.validate()?;
    if state.accounts.len() > MAX_ACCOUNTS || state.alerts.len() > MAX_ACCOUNTS * MAX_WINDOWS {
        return Err(invalid_store());
    }
    let mut ids = HashSet::new();
    for account in &state.accounts {
        if !valid_id(&account.id)
            || !ids.insert(account.id.as_str())
            || !valid_text(&account.label, 192)
            || account.label.chars().count() > 48
            || account.color.len() != 7
            || !account.color.starts_with('#')
            || !account.color[1..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(invalid_store());
        }
        match &account.source {
            AccountSource::ConfigDir { path }
                if path.len() <= 4096
                    && !path.chars().any(char::is_control)
                    && Path::new(path).is_absolute() => {}
            AccountSource::Keychain if account.provider == Provider::Claude => {}
            _ => return Err(invalid_store()),
        }
        for text in [&account.identity.email, &account.identity.plan]
            .into_iter()
            .flatten()
        {
            if !valid_text(text, 4096) {
                return Err(invalid_store());
            }
        }
        if let Some(snapshot) = &account.snapshot {
            validate_snapshot(snapshot)?;
        }
        if let Some(error) = &account.error {
            if !valid_text(&error.kind, 128) || !valid_text(&error.message, 4096) {
                return Err(invalid_store());
            }
        }
    }
    if state
        .settings
        .pinned_account_id
        .as_ref()
        .is_some_and(|id| !ids.contains(id.as_str()))
    {
        return Err(invalid_store());
    }
    if state.alerts.keys().any(|key| {
        key.split_once('/')
            .is_none_or(|(id, window)| !ids.contains(id) || !valid_id(window))
    }) {
        return Err(invalid_store());
    }
    for alert in state.alerts.values() {
        alert.validate().map_err(|_| invalid_store())?;
    }
    Ok(())
}

fn validate_snapshot(snapshot: &Snapshot) -> Result<(), String> {
    if snapshot.windows.len() > MAX_WINDOWS
        || (snapshot.windows.is_empty() && snapshot.extra_usage.is_none())
        || !valid_timestamp(&snapshot.fetched_at)
    {
        return Err(invalid_store());
    }
    let mut ids = HashSet::new();
    for window in &snapshot.windows {
        if !valid_id(&window.id)
            || !ids.insert(window.id.as_str())
            || !valid_text(&window.label, 1024)
            || !window.used_percent.is_finite()
            || !(0.0..=100.0).contains(&window.used_percent)
            || window
                .resets_at
                .as_ref()
                .is_some_and(|value| !valid_timestamp(value))
            || window.window_seconds == Some(0)
        {
            return Err(invalid_store());
        }
    }
    if let Some(extra) = &snapshot.extra_usage {
        if !extra.spent.is_finite()
            || extra.spent < 0.0
            || extra
                .limit
                .is_some_and(|value| !value.is_finite() || value < 0.0)
            || extra.currency.len() != 3
            || !extra.currency.bytes().all(|byte| byte.is_ascii_uppercase())
        {
            return Err(invalid_store());
        }
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn valid_timestamp(value: &str) -> bool {
    value.len() <= 64 && DateTime::parse_from_rfc3339(value).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{ExtraUsage, Identity, UsageWindow};

    fn sample_account() -> Account {
        Account {
            id: "codex-example".into(),
            provider: Provider::Codex,
            label: "Example".into(),
            color: "#123456".into(),
            source: AccountSource::ConfigDir {
                path: std::env::temp_dir().to_string_lossy().into_owned(),
            },
            identity: Identity::default(),
            snapshot: Some(Snapshot {
                windows: vec![UsageWindow {
                    id: "primary_window".into(),
                    label: "Session".into(),
                    used_percent: 25.0,
                    resets_at: Some("2026-10-16T15:00:00Z".into()),
                    window_seconds: Some(18_000),
                }],
                fetched_at: "2026-10-16T10:00:00Z".into(),
                extra_usage: None,
            }),
            error: None,
            refreshing: true,
        }
    }

    fn sample_state() -> StoredState {
        StoredState {
            version: 1,
            accounts: vec![sample_account()],
            ..StoredState::default()
        }
    }

    #[test]
    fn corrupt_store_is_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        fs::write(&path, "{broken").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "{broken");
    }

    #[test]
    fn saves_and_replaces_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        let mut state = load(&path).unwrap();
        save(&path, &state).unwrap();
        state.settings.poll_interval_seconds = 120;
        save(&path, &state).unwrap();
        assert_eq!(load(&path).unwrap().settings.poll_interval_seconds, 120);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn rejects_duplicate_account_and_window_ids() {
        let mut state = sample_state();
        state.accounts.push(state.accounts[0].clone());
        assert!(validate(&state).is_err());
        state.accounts.pop();
        let windows = &mut state.accounts[0].snapshot.as_mut().unwrap().windows;
        windows.push(windows[0].clone());
        assert!(validate(&state).is_err());
    }

    #[test]
    fn rejects_relative_sources_and_invalid_provider_pairing() {
        let mut state = sample_state();
        state.accounts[0].source = AccountSource::ConfigDir {
            path: "../.codex".into(),
        };
        assert!(validate(&state).is_err());
        state.accounts[0].source = AccountSource::Keychain;
        assert!(validate(&state).is_err());
        state.accounts[0].provider = Provider::Claude;
        assert!(validate(&state).is_ok());
    }

    #[test]
    fn labels_use_the_same_character_limit_as_account_renaming() {
        let mut state = sample_state();
        state.accounts[0].label = "界".repeat(48);
        assert!(validate(&state).is_ok());
        state.accounts[0].label.push('界');
        assert!(validate(&state).is_err());
    }

    #[test]
    fn rejects_corrupt_cached_measurements_and_dates() {
        let snapshot = sample_account().snapshot.unwrap();
        for percent in [-1.0, 101.0, f64::NAN, f64::INFINITY] {
            let mut invalid = snapshot.clone();
            invalid.windows[0].used_percent = percent;
            assert!(validate_snapshot(&invalid).is_err());
        }
        let mut invalid = snapshot.clone();
        invalid.fetched_at = "not a date".into();
        assert!(validate_snapshot(&invalid).is_err());
        let mut invalid = snapshot;
        invalid.windows[0].resets_at = Some("soon".into());
        assert!(validate_snapshot(&invalid).is_err());
    }

    #[test]
    fn accepts_spend_only_snapshots_and_rejects_invalid_spend() {
        let mut snapshot = sample_account().snapshot.unwrap();
        snapshot.windows.clear();
        assert!(validate_snapshot(&snapshot).is_err());
        snapshot.extra_usage = Some(ExtraUsage {
            spent: 5.0,
            limit: None,
            currency: "USD".into(),
        });
        assert!(validate_snapshot(&snapshot).is_ok());
        snapshot.extra_usage.as_mut().unwrap().spent = f64::INFINITY;
        assert!(validate_snapshot(&snapshot).is_err());
    }

    #[test]
    fn restored_snapshots_are_stale_and_not_refreshing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        save(&path, &sample_state()).unwrap();
        let restored = load(&path).unwrap();
        assert!(!restored.accounts[0].refreshing);
        assert_eq!(restored.accounts[0].error.as_ref().unwrap().kind, "stale");
        assert_eq!(
            restored.accounts[0].snapshot.as_ref().unwrap().windows[0].used_percent,
            25.0
        );
    }

    #[test]
    fn oversized_store_is_rejected_without_replacing_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_STORE_BYTES + 1)
            .unwrap();
        assert!(load(&path).is_err());
        assert_eq!(fs::metadata(path).unwrap().len(), MAX_STORE_BYTES + 1);
    }

    #[test]
    fn invalid_state_cannot_replace_a_valid_store() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        let mut state = sample_state();
        save(&path, &state).unwrap();
        let original = fs::read(&path).unwrap();
        state.accounts[0].id = "bad/id".into();
        assert!(save(&path, &state).is_err());
        assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn corrupt_persisted_alerts_are_rejected_without_replacing_the_store() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        for alert in [
            serde_json::json!({"reset": "tomorrow", "sent": [80]}),
            serde_json::json!({"reset": null, "sent": [101]}),
        ] {
            let mut value = serde_json::to_value(sample_state()).unwrap();
            value["alerts"] = serde_json::json!({"codex-example/primary_window": alert});
            let bytes = serde_json::to_vec(&value).unwrap();
            fs::write(&path, &bytes).unwrap();
            assert!(load(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
}
