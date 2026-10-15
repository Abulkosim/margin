use crate::core::{
    model::Account,
    notifications::AlertMemory,
    poller::Schedule,
    providers,
    settings::Settings,
    store::{self, StoredState},
    tray,
};
use serde::Serialize;
use std::{collections::HashMap, path::PathBuf};
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
}

pub struct Runtime {
    pub inner: Mutex<RuntimeState>,
    pub client: reqwest::Client,
    pub wake: Notify,
    pub first_run: bool,
    store_path: PathBuf,
    recovery_error: Option<String>,
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
                Some(format!(
                    "{error} Settings file: {}. Restore it and reopen Margin.",
                    store_path.display()
                )),
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
                },
                alerts: saved.alerts,
                schedules: HashMap::new(),
            }),
            client: providers::http_client().map_err(|error| error.message)?,
            wake: Notify::new(),
            first_run: first_run && recovery_error.is_none(),
            store_path,
            recovery_error,
        })
    }

    pub fn ensure_store_available(&self) -> Result<(), String> {
        match &self.recovery_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    pub fn persist(&self, view: &AppState, alerts: &AlertMemory) -> Result<(), String> {
        self.ensure_store_available()?;
        store::save(
            &self.store_path,
            &StoredState {
                version: 1,
                accounts: view.accounts.clone(),
                settings: view.settings.clone(),
                alerts: alerts.clone(),
            },
        )
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
    async fn damaged_settings_remain_untouched_and_visible_for_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        std::fs::write(&path, "{damaged").unwrap();
        let runtime = Runtime::new(path.clone()).unwrap();
        let inner = runtime.inner.lock().await;
        assert!(!runtime.first_run);
        assert!(inner.view.accounts.is_empty());
        assert!(inner.view.notice.as_ref().unwrap().contains("state.json"));
        assert!(runtime.persist(&inner.view, &inner.alerts).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "{damaged");
    }
}
