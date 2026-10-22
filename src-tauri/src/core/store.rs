use super::{
    model::{Account, Snapshot},
    notifications::AlertMemory,
    settings::Settings,
};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
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
    #[serde(skip)]
    pub recovered: Vec<String>,
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
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid_store())?;
    let mut state = decode(value)?;
    for account in &mut state.accounts {
        account.refreshing = false;
        if account.snapshot.is_some() {
            account.error = Some(super::model::UsageError::new(
                "stale",
                "Last saved reading. Waiting for a fresh update.",
            ));
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

pub fn backup(path: &Path) -> Result<PathBuf, String> {
    let target = backup_path(path)?;
    fs::copy(path, &target).map_err(|_| "Margin could not back up its settings file.")?;
    Ok(target)
}

pub fn reset(path: &Path, state: &StoredState) -> Result<Option<PathBuf>, String> {
    let target = match fs::symlink_metadata(path) {
        Ok(_) => {
            let target = backup_path(path)?;
            fs::rename(path, &target).map_err(|_| "Margin could not back up its settings file.")?;
            Some(target)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("Margin could not read its settings file.".into()),
    };
    save(path, state)?;
    Ok(target)
}

fn backup_path(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Invalid settings location.")?;
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    (0..100)
        .map(|attempt| match attempt {
            0 => path.with_file_name(format!("{name}.bak-{stamp}")),
            _ => path.with_file_name(format!("{name}.bak-{stamp}-{attempt}")),
        })
        .find(|candidate| fs::symlink_metadata(candidate).is_err())
        .ok_or_else(|| "Margin could not choose a backup file name.".into())
}

fn decode(value: Value) -> Result<StoredState, String> {
    let Value::Object(mut root) = value else {
        return Err(invalid_store());
    };
    match root.get("version").and_then(Value::as_u64) {
        Some(1) => {}
        Some(version) if version > 1 => return Err(newer_store()),
        _ => return Err(invalid_store()),
    }
    let mut recovered = Vec::new();
    let mut accounts: Vec<Account> = Vec::new();
    match root.remove("accounts") {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            for (index, item) in items.into_iter().enumerate() {
                let name = item
                    .get("label")
                    .and_then(Value::as_str)
                    .filter(|label| valid_text(label, 192) && label.chars().count() <= 48)
                    .map(|label| format!("“{label}”"))
                    .unwrap_or_else(|| (index + 1).to_string());
                if accounts.len() >= MAX_ACCOUNTS {
                    recovered.push(format!(
                        "Skipped account {name}. Margin supports up to {MAX_ACCOUNTS} accounts."
                    ));
                    continue;
                }
                match serde_json::from_value::<Account>(item) {
                    Ok(account)
                        if validate_account(&account).is_ok()
                            && !accounts.iter().any(|existing| existing.id == account.id) =>
                    {
                        accounts.push(account);
                    }
                    _ => recovered.push(format!(
                        "Skipped account {name} because its saved data couldn’t be read."
                    )),
                }
            }
        }
        Some(_) => recovered.push("Saved accounts couldn’t be read and were skipped.".into()),
    }
    let mut settings = Settings::recover(root.remove("settings"), &mut recovered);
    if settings
        .pinned_account_id
        .as_ref()
        .is_some_and(|id| !accounts.iter().any(|account| &account.id == id))
    {
        settings.pinned_account_id = None;
    }
    let mut alerts = AlertMemory::new();
    let mut dropped_alerts = false;
    match root.remove("alerts") {
        None | Some(Value::Null) => {}
        Some(Value::Object(entries)) => {
            for (key, value) in entries {
                let Some((id, window)) = key.split_once('/') else {
                    dropped_alerts = true;
                    continue;
                };
                if !accounts.iter().any(|account| account.id == id) {
                    continue;
                }
                match serde_json::from_value::<super::notifications::WindowAlerts>(value) {
                    Ok(alert)
                        if valid_id(window)
                            && alert.validate().is_ok()
                            && alerts.len() < MAX_ACCOUNTS * MAX_WINDOWS =>
                    {
                        alerts.insert(key, alert);
                    }
                    _ => dropped_alerts = true,
                }
            }
        }
        Some(_) => dropped_alerts = true,
    }
    if dropped_alerts {
        recovered.push("Some alert history couldn’t be read and was cleared.".into());
    }
    Ok(StoredState {
        version: 1,
        accounts,
        settings,
        alerts,
        recovered,
    })
}

fn newer_store() -> String {
    "This settings file needs a newer version of Margin.".into()
}

fn invalid_store() -> String {
    "Margin's settings file is damaged or unsupported.".into()
}

fn validate(state: &StoredState) -> Result<(), String> {
    if state.version != 1 {
        return Err(newer_store());
    }
    state.settings.validate()?;
    if state.accounts.len() > MAX_ACCOUNTS || state.alerts.len() > MAX_ACCOUNTS * MAX_WINDOWS {
        return Err(invalid_store());
    }
    let mut ids = HashSet::new();
    for account in &state.accounts {
        validate_account(account)?;
        if !ids.insert(account.id.as_str()) {
            return Err(invalid_store());
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

fn validate_account(account: &Account) -> Result<(), String> {
    if !valid_id(&account.id)
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
    if !account.source.is_valid_for(account.provider) {
        return Err(invalid_store());
    }
    for text in [&account.identity.email, &account.identity.plan]
        .into_iter()
        .flatten()
    {
        if !valid_text(text, 4096) {
            return Err(invalid_store());
        }
    }
    if account
        .identity
        .history_key
        .as_ref()
        .is_some_and(|key| key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(invalid_store());
    }
    if let Some(snapshot) = &account.snapshot {
        validate_snapshot(snapshot)?;
    }
    if let Some(error) = &account.error {
        if !valid_text(&error.kind, 128)
            || !valid_text(&error.message, 4096)
            || error
                .signed_out_since
                .as_deref()
                .is_some_and(|since| !valid_timestamp(since))
            || error
                .sign_in_command
                .as_deref()
                .is_some_and(|command| !valid_text(command, 8192))
        {
            return Err(invalid_store());
        }
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
    use crate::core::model::{AccountSource, ExtraUsage, Identity, Provider, UsageWindow};

    fn sample_account() -> Account {
        Account {
            id: "codex-example".into(),
            provider: Provider::Codex,
            label: "Example".into(),
            color: "#123456".into(),
            source: AccountSource::ConfigDir {
                path: std::env::temp_dir().to_string_lossy().into_owned(),
                keychain_suffix: None,
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
            keychain_suffix: None,
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
    fn corrupt_persisted_alerts_are_dropped_without_replacing_the_store() {
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
            let state = load(&path).unwrap();
            assert!(state.alerts.is_empty());
            assert_eq!(state.accounts.len(), 1);
            assert_eq!(state.recovered.len(), 1);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn one_corrupt_account_keeps_the_others() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        let mut state = sample_state();
        let mut second = sample_account();
        second.id = "codex-second".into();
        second.label = "Second".into();
        state.accounts.push(second);
        state.settings.pinned_account_id = Some("codex-example".into());
        let mut value = serde_json::to_value(&state).unwrap();
        value["accounts"][0]["color"] = serde_json::json!("red");
        value["accounts"][1]["futureField"] = serde_json::json!(true);
        value["accounts"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!(42));
        value["futureSection"] = serde_json::json!({});
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].id, "codex-second");
        assert_eq!(loaded.settings.pinned_account_id, None);
        assert_eq!(
            loaded.recovered,
            vec![
                "Skipped account “Example” because its saved data couldn’t be read.",
                "Skipped account 3 because its saved data couldn’t be read.",
            ]
        );
        assert!(validate(&loaded).is_ok());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn invalid_setting_falls_back_without_dropping_accounts() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        let mut value = serde_json::to_value(sample_state()).unwrap();
        value["settings"]["pollIntervalSeconds"] = serde_json::json!(1);
        value["settings"]["theme"] = serde_json::json!("dark");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.settings.poll_interval_seconds, 60);
        assert_eq!(loaded.settings.theme, crate::core::settings::Theme::Dark);
        assert_eq!(
            loaded.recovered,
            vec!["Refresh interval was reset to its default."]
        );
    }

    #[test]
    fn newer_or_missing_version_needs_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        let mut value = serde_json::to_value(sample_state()).unwrap();
        value["version"] = serde_json::json!(2);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(load(&path).err().unwrap(), newer_store());
        value.as_object_mut().unwrap().remove("version");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(load(&path).err().unwrap(), invalid_store());
    }

    #[test]
    fn reset_moves_the_file_to_a_backup_and_writes_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        fs::write(&path, "{broken").unwrap();
        let defaults = StoredState {
            version: 1,
            ..StoredState::default()
        };
        let backup = reset(&path, &defaults).unwrap().unwrap();
        assert!(backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("state.json.bak-"));
        assert_eq!(fs::read_to_string(&backup).unwrap(), "{broken");
        let loaded = load(&path).unwrap();
        assert!(loaded.accounts.is_empty());
        assert_eq!(loaded.settings.poll_interval_seconds, 60);
        let second = reset(&path, &defaults).unwrap().unwrap();
        assert_ne!(second, backup);
        assert!(reset(&directory.path().join("missing.json"), &defaults)
            .unwrap()
            .is_none());
    }

    #[test]
    fn backup_copies_without_touching_the_original() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        fs::write(&path, "original").unwrap();
        let copy = backup(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        assert_eq!(fs::read_to_string(copy).unwrap(), "original");
    }

    #[test]
    fn version_one_sources_load_and_keychain_links_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        let folder = std::env::temp_dir().to_string_lossy().into_owned();
        let legacy = serde_json::json!({
            "version": 1,
            "accounts": [
                {"id": "claude-legacy", "provider": "claude", "label": "Claude", "color": "#b48265", "source": {"kind": "keychain"}, "identity": {"email": null, "plan": null}, "snapshot": null, "error": null, "refreshing": false},
                {"id": "claude-folder", "provider": "claude", "label": "Work", "color": "#b48265", "source": {"kind": "configDir", "path": folder}, "identity": {"email": null, "plan": null}, "snapshot": null, "error": null, "refreshing": false}
            ],
            "settings": serde_json::to_value(Settings::default()).unwrap()
        });
        fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut state = load(&path).unwrap();
        assert_eq!(state.accounts[0].source, AccountSource::Keychain);
        assert_eq!(
            state.accounts[1].source,
            AccountSource::ConfigDir {
                path: folder.clone(),
                keychain_suffix: None
            }
        );
        state.accounts[1].source = AccountSource::ConfigDir {
            path: folder,
            keychain_suffix: Some("ac386099".into()),
        };
        save(&path, &state).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["accounts"][0]["source"]["kind"], "keychain");
        assert_eq!(saved["accounts"][1]["source"]["keychainSuffix"], "ac386099");
        let restored = load(&path).unwrap();
        assert_eq!(restored.accounts[0].source, state.accounts[0].source);
        assert_eq!(restored.accounts[1].source, state.accounts[1].source);
    }

    #[test]
    fn rejects_invalid_keychain_suffixes_and_codex_keychain_links() {
        let mut state = sample_state();
        let path = std::env::temp_dir().to_string_lossy().into_owned();
        state.accounts[0].provider = Provider::Claude;
        for suffix in ["52EDA417", "52eda41", "52eda4170", "../../x", ""] {
            state.accounts[0].source = AccountSource::ConfigDir {
                path: path.clone(),
                keychain_suffix: Some(suffix.into()),
            };
            assert!(validate(&state).is_err());
        }
        state.accounts[0].source = AccountSource::ConfigDir {
            path,
            keychain_suffix: Some("52eda417".into()),
        };
        assert!(validate(&state).is_ok());
        state.accounts[0].provider = Provider::Codex;
        assert!(validate(&state).is_err());
    }
}
