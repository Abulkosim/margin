use crate::core::{
    history::HistoryStore,
    model::Account,
    notifications::AlertMemory,
    poller::Schedule,
    providers,
    settings::Settings,
    store::{self, StoredState},
    tray,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, Notify};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub revision: u64,
    pub accounts: Vec<Account>,
    pub settings: Settings,
    pub refreshing: bool,
    pub notice: Option<String>,
    pub recovery: bool,
    pub recovered: Vec<String>,
    pub ignored_sources: Vec<String>,
}

impl AppState {
    pub fn changed(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
}

pub struct RuntimeState {
    pub view: AppState,
    pub alerts: AlertMemory,
    pub schedules: HashMap<String, Schedule>,
    pub history: HistoryStore,
}

pub struct Runtime {
    pub inner: Mutex<RuntimeState>,
    pub client: reqwest::Client,
    pub profiles: providers::claude::ProfileCache,
    pub resets: providers::codex::ResetCreditCache,
    pub wake: Notify,
    pub update_wake: Notify,
    pub first_run: bool,
    store_path: PathBuf,
    recovery_error: std::sync::Mutex<Option<String>>,
    backup_pending: AtomicBool,
    last_update_check_at: std::sync::Mutex<Option<String>>,
}

impl Runtime {
    pub fn new(store_path: PathBuf) -> Result<Self, String> {
        let first_run = !store_path.exists();
        let (saved, recovery_error) = match store::load(&store_path) {
            Ok(saved) => (saved, None),
            Err(error) => (
                StoredState {
                    version: 1,
                    ..StoredState::default()
                },
                Some(format!("{error} Settings file: {}.", store_path.display())),
            ),
        };
        Ok(Self {
            inner: Mutex::new(RuntimeState {
                view: AppState {
                    revision: 0,
                    accounts: saved.accounts,
                    settings: saved.settings,
                    refreshing: false,
                    notice: recovery_error.clone(),
                    recovery: recovery_error.is_some(),
                    recovered: saved.recovered.clone(),
                    ignored_sources: saved.ignored_sources,
                },
                alerts: saved.alerts,
                schedules: HashMap::new(),
                history: HistoryStore::new(store_path.with_file_name("history")),
            }),
            client: providers::http_client().map_err(|error| error.message)?,
            profiles: Default::default(),
            resets: Default::default(),
            wake: Notify::new(),
            update_wake: Notify::new(),
            first_run: first_run && recovery_error.is_none(),
            store_path,
            recovery_error: std::sync::Mutex::new(recovery_error),
            backup_pending: AtomicBool::new(!saved.recovered.is_empty()),
            last_update_check_at: std::sync::Mutex::new(saved.last_update_check_at),
        })
    }

    pub fn ensure_store_available(&self) -> Result<(), String> {
        match &*self
            .recovery_error
            .lock()
            .map_err(|_| "Settings are unavailable.")?
        {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    pub async fn reset_store(&self) -> Result<AppState, String> {
        let mut inner = self.inner.lock().await;
        let mut next = AppState {
            revision: inner.view.revision,
            accounts: Vec::new(),
            settings: Settings {
                autostart: inner.view.settings.autostart,
                ..Settings::default()
            },
            refreshing: false,
            notice: None,
            recovery: false,
            recovered: Vec::new(),
            ignored_sources: Vec::new(),
        };
        next.changed();
        store::reset(
            &self.store_path,
            &StoredState {
                version: 1,
                settings: next.settings.clone(),
                ..StoredState::default()
            },
        )?;
        *self
            .recovery_error
            .lock()
            .map_err(|_| "Settings are unavailable.")? = None;
        self.backup_pending.store(false, Ordering::SeqCst);
        inner.view = next.clone();
        inner.alerts.clear();
        inner.schedules.clear();
        Ok(next)
    }

    pub fn persist(&self, view: &AppState, alerts: &AlertMemory) -> Result<(), String> {
        self.ensure_store_available()?;
        if self.backup_pending.swap(false, Ordering::SeqCst) {
            if let Err(error) = store::backup(&self.store_path) {
                self.backup_pending.store(true, Ordering::SeqCst);
                return Err(error);
            }
        }
        store::save(
            &self.store_path,
            &StoredState {
                version: 1,
                accounts: view.accounts.clone(),
                settings: view.settings.clone(),
                alerts: alerts.clone(),
                ignored_sources: view.ignored_sources.clone(),
                last_update_check_at: self.last_update_check_at(),
                ..StoredState::default()
            },
        )
    }

    pub fn last_update_check_at(&self) -> Option<String> {
        self.last_update_check_at
            .lock()
            .ok()
            .and_then(|value| value.clone())
    }

    pub async fn record_update_check(&self, at: String) -> Result<(), String> {
        *self
            .last_update_check_at
            .lock()
            .map_err(|_| "Update check time unavailable.")? = Some(at);
        let inner = self.inner.lock().await;
        self.persist(&inner.view, &inner.alerts)
    }

    pub async fn change<F>(&self, app: &AppHandle, change: F) -> Result<AppState, String>
    where
        F: FnOnce(&mut AppState) -> Result<(), String>,
    {
        let mut inner = self.inner.lock().await;
        let mut next = inner.view.clone();
        change(&mut next)?;
        next.refreshing = next.accounts.iter().any(|account| account.refreshing);
        next.notice = None;
        next.changed();
        let mut alerts = inner.alerts.clone();
        alerts.retain(|key, _| {
            next.accounts
                .iter()
                .any(|account| key.starts_with(&format!("{}/", account.id)))
        });
        self.ensure_store_available()?;
        let changed_ids: Vec<String> = inner
            .view
            .accounts
            .iter()
            .filter(|account| !next.accounts.iter().any(|next| next.id == account.id))
            .chain(
                next.accounts
                    .iter()
                    .filter(|account| !inner.view.accounts.iter().any(|old| old.id == account.id)),
            )
            .map(|account| account.id.clone())
            .collect();
        for id in changed_ids {
            inner.history.remove(&id)?;
        }
        self.persist(&next, &alerts)?;
        inner.view = next.clone();
        inner.alerts = alerts;
        inner
            .schedules
            .retain(|id, _| next.accounts.iter().any(|account| &account.id == id));
        drop(inner);
        publish(app, &next);
        self.wake.notify_one();
        Ok(next)
    }
}

pub fn publish(app: &AppHandle, state: &AppState) {
    tray::update(app, state);
    let _ = app.emit("state_changed", state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn existing_settings_remain_usable_when_separate_history_is_corrupt() {
        use crate::core::model::{AccountSource, Identity, Provider};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        let account = Account {
            id: "codex-test".into(),
            provider: Provider::Codex,
            label: "Work".into(),
            color: "#ffffff".into(),
            source: AccountSource::ConfigDir {
                path: directory.path().to_string_lossy().into_owned(),
                keychain_suffix: None,
            },
            identity: Identity::default(),
            snapshot: None,
            error: None,
            refreshing: false,
        };
        let saved = StoredState {
            version: 1,
            accounts: vec![account.clone()],
            ..StoredState::default()
        };
        store::save(&path, &saved).unwrap();
        std::fs::create_dir(directory.path().join("history")).unwrap();
        let history_path = directory.path().join("history/codex-test.json");
        std::fs::write(&history_path, "{damaged").unwrap();
        let runtime = Runtime::new(path).unwrap();
        let mut inner = runtime.inner.lock().await;
        assert_eq!(inner.view.accounts.len(), 1);
        assert!(inner.view.notice.is_none());
        assert!(inner
            .history
            .get(&account, chrono::Utc::now())
            .error
            .is_some());
        assert!(runtime.persist(&inner.view, &inner.alerts).is_ok());
        assert_eq!(std::fs::read_to_string(history_path).unwrap(), "{damaged");
    }

    #[tokio::test]
    async fn damaged_settings_remain_untouched_and_visible_for_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        std::fs::write(&path, "{damaged").unwrap();
        let runtime = Runtime::new(path.clone()).unwrap();
        let inner = runtime.inner.lock().await;
        assert!(!runtime.first_run);
        assert!(inner.view.accounts.is_empty());
        assert!(inner.view.notice.as_ref().unwrap().contains("state.json"));
        assert!(inner.view.recovery);
        assert!(runtime.persist(&inner.view, &inner.alerts).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{damaged");
        drop(inner);
        let view = runtime.reset_store().await.unwrap();
        assert!(!view.recovery && view.notice.is_none());
        assert!(runtime.ensure_store_available().is_ok());
        assert!(store::load(&path).unwrap().accounts.is_empty());
        assert_eq!(backups(directory.path()), vec!["{damaged".to_string()]);
    }

    #[tokio::test]
    async fn dropping_a_corrupt_account_backs_up_once_before_writing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        let original = serde_json::json!({
            "version": 1,
            "accounts": [{"id": "broken"}],
            "settings": {}
        })
        .to_string();
        std::fs::write(&path, &original).unwrap();
        let runtime = Runtime::new(path.clone()).unwrap();
        let inner = runtime.inner.lock().await;
        assert!(!inner.view.recovery);
        assert_eq!(inner.view.recovered.len(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        runtime.persist(&inner.view, &inner.alerts).unwrap();
        runtime.persist(&inner.view, &inner.alerts).unwrap();
        assert_eq!(backups(directory.path()), vec![original]);
        assert!(store::load(&path).unwrap().recovered.is_empty());
    }

    fn backups(directory: &std::path::Path) -> Vec<String> {
        std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.to_string_lossy().contains("state.json.bak-"))
            .map(|path| std::fs::read_to_string(path).unwrap())
            .collect()
    }
}
