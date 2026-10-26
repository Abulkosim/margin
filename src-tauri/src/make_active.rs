use crate::{
    core::{
        model::{Account, Provider},
        switch::{
            self, Context, DiskJournal, Journal, PlanKind, ProperLockfile, RecordAccounts,
            RecordState, RestoreOffer, SlotSpec, SwitchError, SystemStore,
        },
    },
    runtime::{publish, AppState, Runtime, RuntimeState},
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{atomic::Ordering, Arc};
use std::time::{Duration, Instant};
use tauri::{AppHandle, State};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub account_id: String,
    pub provider: Provider,
    pub kind: PlanKind,
    pub fingerprint: String,
    pub target: String,
    pub displaced: Option<String>,
    pub folder: String,
    pub running: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activated {
    pub state: AppState,
    pub target: String,
    pub previous_account_id: Option<String>,
}

struct Target {
    account: Account,
    previous: Option<Account>,
    home: PathBuf,
}

impl Target {
    fn folder(&self) -> PathBuf {
        match &self.account.source {
            crate::core::model::AccountSource::ConfigDir { path, .. } => PathBuf::from(path),
            crate::core::model::AccountSource::Keychain => {
                switch::default_dir(self.account.provider, &self.home)
            }
        }
    }

    fn message(&self, error: &SwitchError) -> String {
        switch::message(
            error,
            self.account.provider,
            &self.account.label,
            &self.folder(),
        )
    }

    fn default_source(&self) -> crate::core::model::AccountSource {
        self.previous
            .as_ref()
            .map(|account| account.source.clone())
            .unwrap_or_else(|| switch::default_source(self.account.provider, &self.home))
    }

    fn specs(&self) -> Result<(SlotSpec, SlotSpec), SwitchError> {
        let store = SystemStore;
        let context = Context {
            home: &self.home,
            keychain: cfg!(target_os = "macos"),
            store: &store,
        };
        Ok((
            switch::slot_spec(self.account.provider, &self.default_source(), &context)?,
            switch::slot_spec(self.account.provider, &self.account.source, &context)?,
        ))
    }
}

async fn target(runtime: &Runtime, id: &str) -> Result<Target, String> {
    let home = dirs::home_dir().ok_or("Your home folder could not be found.")?;
    let inner = runtime.inner.lock().await;
    let account = inner
        .view
        .accounts
        .iter()
        .find(|account| account.id == id)
        .cloned()
        .ok_or("Account no longer exists.")?;
    if switch::is_default_source(account.provider, &account.source, &home) {
        return Err("This account is already active.".into());
    }
    let previous = inner
        .view
        .accounts
        .iter()
        .find(|other| {
            other.provider == account.provider
                && switch::is_default_source(other.provider, &other.source, &home)
        })
        .cloned();
    Ok(Target {
        account,
        previous,
        home,
    })
}

fn preview(target: &Target) -> Result<Preview, String> {
    let fail = |error: SwitchError| target.message(&error);
    let (default, source) = target.specs().map_err(fail)?;
    let store = SystemStore;
    let plan = switch::plan(
        &switch::read_slot(&default, &store).map_err(fail)?,
        &switch::read_slot(&source, &store).map_err(fail)?,
    )
    .map_err(|refusal| fail(SwitchError::Refused(refusal)))?;
    Ok(Preview {
        account_id: target.account.id.clone(),
        provider: plan.provider,
        target: plan
            .target_email
            .unwrap_or_else(|| target.account.label.clone()),
        displaced: (plan.kind == PlanKind::Swap).then(|| {
            plan.displaced_email
                .or_else(|| {
                    target
                        .previous
                        .as_ref()
                        .map(|account| account.label.clone())
                })
                .unwrap_or_else(|| "The current account".into())
        }),
        folder: switch::display_path(&source.dir, &target.home),
        kind: plan.kind,
        fingerprint: plan.fingerprint,
        running: switch::running(plan.provider),
    })
}

#[tauri::command]
pub async fn preview_make_active(
    runtime: State<'_, Arc<Runtime>>,
    id: String,
) -> Result<Preview, String> {
    let target = target(&runtime, &id).await?;
    tauri::async_runtime::spawn_blocking(move || preview(&target))
        .await
        .map_err(|_| "Could not check that account.".to_owned())?
}

struct Switching<'a>(&'a Runtime);

impl<'a> Switching<'a> {
    fn start(runtime: &'a Runtime) -> Result<Self, String> {
        if runtime.switching.swap(true, Ordering::SeqCst) {
            return Err("Another switch is still running.".into());
        }
        Ok(Self(runtime))
    }
}

impl Drop for Switching<'_> {
    fn drop(&mut self) {
        self.0.switching.store(false, Ordering::SeqCst);
    }
}

/// Stops polling both accounts so no fetch reads half-swapped files; in-flight
/// results are dropped because their generation no longer matches.
async fn pause(app: &AppHandle, runtime: &Runtime, ids: &[String]) {
    let mut inner = runtime.inner.lock().await;
    let later = Instant::now() + Duration::from_secs(120);
    for id in ids {
        if let Some(schedule) = inner.schedules.get_mut(id) {
            schedule.in_flight = false;
            schedule.generation = 0;
            schedule.next = later;
            schedule.not_before = later;
        }
    }
    for account in inner
        .view
        .accounts
        .iter_mut()
        .filter(|account| ids.contains(&account.id))
    {
        account.refreshing = false;
    }
    inner.view.refreshing = inner.view.accounts.iter().any(|account| account.refreshing);
    inner.view.changed();
    let view = inner.view.clone();
    drop(inner);
    publish(app, &view);
}

fn resume(inner: &mut RuntimeState, ids: &[String]) {
    let now = Instant::now();
    for id in ids {
        if let Some(schedule) = inner.schedules.get_mut(id) {
            schedule.next = now;
            schedule.not_before = now;
            schedule.failures = 0;
        }
    }
}

async fn finish(
    app: &AppHandle,
    runtime: &Runtime,
    ids: &[String],
    change: impl FnOnce(&mut RuntimeState),
) -> AppState {
    let mut inner = runtime.inner.lock().await;
    change(&mut inner);
    resume(&mut inner, ids);
    inner.view.switching.active_account_ids = switch::active_ids(&inner.view.accounts);
    inner.view.notice = runtime.persist(&inner.view, &inner.alerts).err();
    inner.view.changed();
    let view = inner.view.clone();
    drop(inner);
    publish(app, &view);
    runtime.wake.notify_one();
    view
}

fn offer(journal: &DiskJournal, id: &str) -> Option<RestoreOffer> {
    journal
        .records()
        .iter()
        .find(|record| record.id == id)
        .map(RestoreOffer::new)
}

#[tauri::command]
pub async fn make_active(
    app: AppHandle,
    runtime: State<'_, Arc<Runtime>>,
    id: String,
    fingerprint: String,
) -> Result<Activated, String> {
    let _switching = Switching::start(&runtime)?;
    let target = Arc::new(target(&runtime, &id).await?);
    let ids: Vec<String> = std::iter::once(target.account.id.clone())
        .chain(target.previous.as_ref().map(|account| account.id.clone()))
        .collect();
    pause(&app, &runtime, &ids).await;
    let moved = RecordAccounts {
        target: target.account.id.clone(),
        previous: target.previous.as_ref().map(|account| account.id.clone()),
        target_source: target.account.source.clone(),
        default_source: target.default_source(),
    };
    let mut journal = runtime.journal();
    let job = target.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let (default, source) = job.specs()?;
        let outcome = switch::execute(
            &fingerprint,
            &default,
            &source,
            Some(moved),
            &SystemStore,
            &ProperLockfile::default(),
            &mut journal,
        )?;
        if outcome.plan.provider == Provider::Codex && switch::running(Provider::Codex) {
            std::thread::sleep(Duration::from_secs(2));
            if !switch::verify(&SystemStore, &outcome.record) {
                let _ = journal.mark(&outcome.record.id, RecordState::VerifyFailed);
                return Err(SwitchError::VerifyFailed {
                    id: outcome.record.id,
                });
            }
        }
        Ok(outcome)
    })
    .await
    .map_err(|_| SwitchError::Io)
    .and_then(|result| result);
    let journal = runtime.journal();
    match result {
        Ok(outcome) => {
            let record = outcome.record.clone();
            let view = finish(&app, &runtime, &ids, |inner| {
                if let Some(moved) = &record.accounts {
                    apply(inner, moved);
                }
            })
            .await;
            let mut journal = journal;
            let _ = journal.mark(&record.id, RecordState::Done);
            Ok(Activated {
                state: view,
                target: outcome
                    .plan
                    .target_email
                    .unwrap_or_else(|| target.account.label.clone()),
                previous_account_id: (outcome.plan.kind == PlanKind::Swap)
                    .then(|| record.accounts.and_then(|moved| moved.previous))
                    .flatten(),
            })
        }
        Err(error) => {
            let restore = match &error {
                SwitchError::VerifyFailed { id } | SwitchError::RollbackFailed { id } => {
                    offer(&journal, id)
                }
                _ => None,
            };
            finish(&app, &runtime, &ids, |inner| {
                if restore.is_some() {
                    inner.view.switching.restore = restore;
                }
            })
            .await;
            Err(target.message(&error))
        }
    }
}

fn apply(inner: &mut RuntimeState, moved: &RecordAccounts) -> bool {
    if !switch::apply_to_accounts(&mut inner.view.accounts, moved) {
        return false;
    }
    for id in std::iter::once(&moved.target).chain(moved.previous.as_ref()) {
        if let Some(account) = inner.view.accounts.iter().find(|account| &account.id == id) {
            inner.history.relocate(account);
        }
    }
    true
}

/// At launch: finish switches whose files were written but whose account move
/// wasn't saved, and surface a restore for any that stopped part way.
pub fn recover(inner: &mut RuntimeState, journal: &mut DiskJournal) {
    let recovery = switch::recover(&SystemStore, journal);
    for record in recovery.apply {
        let applied = record
            .accounts
            .as_ref()
            .is_some_and(|moved| apply(inner, moved));
        if !applied {
            let _ = journal.mark(&record.id, RecordState::Done);
        }
    }
    inner.view.switching.restore = recovery.offer.as_ref().map(RestoreOffer::new);
}

#[tauri::command]
pub async fn restore_switch_backup(
    app: AppHandle,
    runtime: State<'_, Arc<Runtime>>,
    id: String,
) -> Result<AppState, String> {
    let _switching = Switching::start(&runtime)?;
    let (provider, ids) = {
        let inner = runtime.inner.lock().await;
        let offer = inner
            .view
            .switching
            .restore
            .clone()
            .filter(|offer| offer.id == id)
            .ok_or("That backup is no longer offered.")?;
        let ids: Vec<String> = inner
            .view
            .accounts
            .iter()
            .filter(|account| account.provider == offer.provider)
            .map(|account| account.id.clone())
            .collect();
        (offer.provider, ids)
    };
    pause(&app, &runtime, &ids).await;
    let mut journal = runtime.journal();
    let backup = id.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        switch::restore(
            &backup,
            &SystemStore,
            &ProperLockfile::default(),
            &mut journal,
        )
    })
    .await
    .map_err(|_| SwitchError::Io)
    .and_then(|result| result);
    let failed = result.as_ref().err().cloned();
    let view = finish(&app, &runtime, &ids, |inner| {
        if failed.is_none() {
            inner.view.switching.restore = None;
        }
    })
    .await;
    match failed {
        None => Ok(view),
        Some(error) => Err(switch::message(
            &error,
            provider,
            provider.name(),
            Path::new(""),
        )),
    }
}

#[tauri::command]
pub async fn dismiss_switch_restore(
    app: AppHandle,
    runtime: State<'_, Arc<Runtime>>,
    id: String,
) -> Result<AppState, String> {
    let _ = runtime.journal().mark(&id, RecordState::Dismissed);
    let mut inner = runtime.inner.lock().await;
    if inner
        .view
        .switching
        .restore
        .as_ref()
        .is_some_and(|offer| offer.id == id)
    {
        inner.view.switching.restore = None;
    }
    inner.view.changed();
    let view = inner.view.clone();
    drop(inner);
    publish(&app, &view);
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_shape_has_no_secrets_or_paths_beyond_the_folder() {
        let preview = Preview {
            account_id: "claude-b".into(),
            provider: Provider::Claude,
            kind: PlanKind::Swap,
            fingerprint: "f".repeat(64),
            target: "work@example.com".into(),
            displaced: Some("personal@example.com".into()),
            folder: "~/.claude-work".into(),
            running: false,
        };
        let value = serde_json::to_value(&preview).unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            [
                "accountId",
                "displaced",
                "fingerprint",
                "folder",
                "kind",
                "provider",
                "running",
                "target"
            ]
        );
        assert_eq!(value["kind"], "swap");
    }
}
