use super::{
    burn,
    delivery::Message,
    digest, forecast,
    model::{Account, AccountSource, Identity, Snapshot, UsageError},
    notifications::{self, Alert, AlertMemory, Clock12h},
    outage::{self, Outage},
    providers,
    settings::NotificationPermission,
    tray,
};
use crate::runtime::{publish, AppState, Runtime, RuntimeState};
use chrono::{DateTime, Utc};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime},
};
use tauri::{plugin::PermissionState, AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

pub struct Schedule {
    pub next: Instant,
    pub not_before: Instant,
    pub failures: u32,
    pub in_flight: bool,
    pub generation: u64,
    pub wake_retries: u8,
    pub fetched: Option<Instant>,
    pub outage: Outage,
}

const WAKE_RETRIES: u8 = 6;
const WAKE_RETRY_DELAY: Duration = Duration::from_secs(5);

impl Schedule {
    fn new(delay: Duration) -> Self {
        let now = Instant::now();
        Self {
            next: now + delay,
            not_before: now,
            failures: 0,
            in_flight: false,
            generation: 0,
            wake_retries: 0,
            fetched: None,
            outage: Outage::default(),
        }
    }

    fn ready(&self, now: Instant) -> bool {
        !self.in_flight && now >= self.next && now >= self.not_before
    }
}

/// A credential rotated mid-fetch is read again soon; the new token is already on disk.
const CREDENTIALS_CHANGED_RETRY: Duration = Duration::from_secs(5);

/// Rechecking an expired token reads local credentials and makes no request, so it can run at
/// the active interval. A Keychain read spawns `security` and may prompt, so those stay as they
/// are.
fn recheck_interval(error: &UsageError, source: &AccountSource, active: u64, current: u64) -> u64 {
    if error.token_expired() && !providers::claude::reads_keychain(source) {
        active
    } else {
        current
    }
}

pub fn retry_delay(interval: u64, failures: u32, error: &UsageError) -> Duration {
    if error.token_expired() {
        return Duration::from_secs(interval);
    }
    if error.kind == "credentialsChanged" && failures <= 1 {
        return CREDENTIALS_CHANGED_RETRY;
    }
    let backoff = interval
        .saturating_mul(1_u64 << failures.saturating_sub(1).min(6))
        .min(3600);
    let minimum = if matches!(error.kind.as_str(), "auth" | "credentials" | "signedOut") {
        300
    } else {
        0
    };
    Duration::from_secs(
        backoff
            .max(minimum)
            .max(u64::from(error.retry_after_seconds.unwrap_or(0)).min(86400)),
    )
}

pub fn resumed(last_tick: SystemTime, now: SystemTime, interval: Duration) -> bool {
    now.duration_since(last_tick)
        .is_ok_and(|gap| gap > interval.saturating_mul(2))
}

const OPEN_REFRESH_AGE: Duration = Duration::from_secs(30);
const DIGEST_CHECK_INTERVAL: Duration = Duration::from_secs(60);

fn bring_forward(
    schedules: &mut HashMap<String, Schedule>,
    after_wake: bool,
    older_than: Option<Duration>,
) {
    let now = Instant::now();
    for schedule in schedules.values_mut() {
        if older_than.is_some_and(|age| {
            schedule
                .fetched
                .is_some_and(|fetched| now.saturating_duration_since(fetched) < age)
        }) {
            continue;
        }
        if after_wake {
            schedule.wake_retries = WAKE_RETRIES;
        }
        if !schedule.in_flight {
            schedule.next = now.max(schedule.not_before);
        }
    }
}

fn release(view: &mut AppState, schedules: &mut HashMap<String, Schedule>, id: &str) -> bool {
    let Some(schedule) = schedules.get_mut(id) else {
        return false;
    };
    let now = Instant::now();
    schedule.in_flight = false;
    schedule.next = now.max(schedule.not_before);
    for account in view.accounts.iter_mut().filter(|account| account.id == id) {
        account.refreshing = false;
    }
    view.refreshing = view.accounts.iter().any(|account| account.refreshing);
    view.changed();
    true
}

pub async fn refresh(runtime: &Runtime) {
    bring_forward(&mut runtime.inner.lock().await.schedules, false, None);
    runtime.wake.notify_one();
}

pub async fn refresh_on_open(runtime: &Runtime) {
    bring_forward(
        &mut runtime.inner.lock().await.schedules,
        false,
        Some(OPEN_REFRESH_AGE),
    );
    runtime.wake.notify_one();
}

pub async fn refresh_after_wake(runtime: &Runtime) {
    bring_forward(&mut runtime.inner.lock().await.schedules, true, None);
    runtime.wake.notify_one();
    runtime.update_wake.notify_one();
}

#[cfg(target_os = "macos")]
pub fn watch_wake(runtime: Arc<Runtime>) {
    use objc2_app_kit::{NSWorkspace, NSWorkspaceDidWakeNotification};
    use objc2_foundation::NSNotification;
    use std::ptr::NonNull;

    let block = block2::RcBlock::new(move |_: NonNull<NSNotification>| {
        let runtime = runtime.clone();
        tauri::async_runtime::spawn(async move { refresh_after_wake(&runtime).await });
    });
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidWakeNotification),
            None,
            None,
            &block,
        )
    };
    std::mem::forget(observer);
}

fn permission(
    state: Option<PermissionState>,
    previous: NotificationPermission,
) -> NotificationPermission {
    match state {
        Some(PermissionState::Granted) => NotificationPermission::Granted,
        Some(PermissionState::Denied) => NotificationPermission::Denied,
        _ => previous,
    }
}

async fn sync_permission(app: &AppHandle, runtime: &Runtime) {
    let previous = runtime
        .inner
        .lock()
        .await
        .view
        .settings
        .notification_permission;
    let state = if previous == NotificationPermission::Unasked {
        app.notification().request_permission()
    } else {
        app.notification().permission_state()
    };
    let next = permission(state.ok(), previous);
    if next == previous {
        return;
    }
    runtime
        .update_and_publish(app, |inner| {
            inner.view.settings.notification_permission = next;
            inner.view.notice = runtime
                .persist(&inner.view, &inner.alerts, &inner.delivery)
                .err();
        })
        .await;
}

/// Whether the notification reached the plugin's background thread. The plugin drops errors from
/// the OS call on that thread, so `true` does not mean the OS accepted or displayed it.
pub fn submit(app: &AppHandle, title: &str, body: Option<&str>) -> bool {
    let mut builder = app.notification().builder().title(title);
    if let Some(body) = body {
        builder = builder.body(body);
    }
    builder.show().is_ok()
}

fn submit_pending(
    app: &AppHandle,
    runtime: &Runtime,
    inner: &mut crate::runtime::RuntimeState,
) -> bool {
    let crate::runtime::RuntimeState {
        view,
        alerts,
        delivery,
        ..
    } = inner;
    if delivery.is_idle() {
        return false;
    }
    let attempt = delivery.attempt(
        &mut view.notification_failed_at,
        &view.settings,
        &view.accounts,
        chrono::Utc::now(),
        Clock12h::system(),
        |title, body| submit(app, title, body),
    );
    if attempt.save {
        view.notice = runtime.persist(view, alerts, delivery).err();
    }
    attempt.failure_changed || attempt.save
}

/// Queues an alert for accounts that just crossed 30 minutes without a reading.
fn not_updating(inner: &mut crate::runtime::RuntimeState, now: chrono::DateTime<chrono::Utc>) {
    let crate::runtime::RuntimeState {
        view,
        schedules,
        delivery,
        ..
    } = inner;
    for account in &view.accounts {
        let Some(schedule) = schedules.get_mut(&account.id) else {
            continue;
        };
        let last_reading = account
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.fetched_at.as_str());
        let proof = outage::connection_proof(&view.accounts, account);
        if let (Some(last), Some(error)) = (
            schedule
                .outage
                .observe(account.error.as_ref(), last_reading, proof, now),
            account.error.as_ref(),
        ) {
            delivery.enqueue(
                Some(&account.id),
                Message::NotUpdating {
                    label: account.label.clone(),
                    error: error.clone(),
                    last,
                },
                now,
            );
        }
    }
}

/// What the fetch task does after `apply_result`, once the lock is released.
#[derive(Debug)]
enum Applied {
    /// A newer fetch replaced this one, or the account was removed. Nothing changed.
    Dropped,
    /// The account's provider or source changed mid-fetch. Publish and wake the loop.
    Released,
    /// The reading or error was applied and `alerts` were queued for delivery. Save it (`urgent`
    /// skips the reading save interval), publish, and wake the loop if anything was queued.
    Reading { urgent: bool, alerts: Vec<Alert> },
}

fn jitter(account_id: &str) -> u64 {
    account_id
        .bytes()
        .fold(0_u64, |sum, byte| sum + u64::from(byte))
        % 7
}

/// Applies a finished fetch to the state: account, alert memory, history, queued alerts and the
/// next schedule. Saving and publishing are left to the caller.
fn apply_result(
    inner: &mut RuntimeState,
    account: &Account,
    generation: u64,
    result: Result<(Snapshot, Identity), UsageError>,
    visible: bool,
    now: Instant,
    wall: DateTime<Utc>,
) -> Applied {
    if !inner
        .schedules
        .get(&account.id)
        .is_some_and(|schedule| schedule.in_flight && schedule.generation == generation)
    {
        return Applied::Dropped;
    }
    let Some(index) = inner.view.accounts.iter().position(|item| {
        item.id == account.id && item.provider == account.provider && item.source == account.source
    }) else {
        return if release(&mut inner.view, &mut inner.schedules, &account.id) {
            Applied::Released
        } else {
            Applied::Dropped
        };
    };
    let interval = if visible {
        inner.view.settings.poll_interval_seconds
    } else {
        inner.view.settings.idle_poll_interval_seconds
    };
    let active_interval = inner.view.settings.poll_interval_seconds;
    let mut alerts = Vec::new();
    let mut before = AlertMemory::new();
    let mut identity_changed = false;
    let error = result.as_ref().err().cloned();
    match result {
        Ok((snapshot, identity)) => {
            let prefix = format!("{}/", account.id);
            if !inner.view.accounts[index].identity.same_person(&identity) {
                inner.alerts.retain(|key, _| !key.starts_with(&prefix));
                inner.delivery.forget(&account.id);
            }
            before = inner
                .alerts
                .iter()
                .filter(|(key, _)| key.starts_with(&prefix))
                .map(|(key, memory)| (key.clone(), memory.clone()))
                .collect();
            alerts = notifications::observe(
                &mut inner.alerts,
                &account.id,
                &snapshot,
                &inner.view.settings.thresholds,
                wall,
            );
            let target = &mut inner.view.accounts[index];
            target.snapshot = Some(snapshot);
            target.error = None;
            identity_changed = target.identity != identity;
            target.identity = identity;
        }
        Err(error) => inner.view.accounts[index].error = Some(error),
    }
    let observed = inner.view.accounts[index].clone();
    if error.is_some() {
        inner.history.failed(&observed);
    } else {
        inner.history.record(&observed, interval + 7, wall);
        let settings = &inner.view.settings;
        if let Some(snapshot) = observed.snapshot.as_ref().filter(|_| settings.pace_alerts) {
            forecast::observe(
                &mut inner.alerts,
                &account.id,
                snapshot,
                &mut alerts,
                |window, since| inner.history.recent(&observed, window, since),
                wall,
            );
        }
        let settings = &inner.view.settings;
        if let Some(snapshot) = observed.snapshot.as_ref().filter(|_| settings.burn_alerts) {
            let suppressed: HashSet<String> =
                alerts.iter().map(|alert| alert.window_id.clone()).collect();
            alerts.extend(burn::observe(
                &mut inner.alerts,
                &account.id,
                snapshot,
                settings.burn_threshold,
                &suppressed,
                |window, since| inner.history.recent(&observed, window, since),
                wall,
            ));
        }
    }
    inner.view.accounts[index].refreshing = false;
    let jitter = jitter(&account.id);
    let mut retry_at = None;
    if let Some(schedule) = inner.schedules.get_mut(&account.id) {
        schedule.in_flight = false;
        schedule.fetched = Some(now);
        if let Some(error) = error {
            schedule.outage.failed(wall);
            if error.kind == "network" && schedule.wake_retries > 0 {
                schedule.wake_retries -= 1;
                schedule.next = now + WAKE_RETRY_DELAY;
                schedule.not_before = schedule.next;
            } else {
                schedule.wake_retries = 0;
                schedule.failures = schedule.failures.saturating_add(1);
                let wait = recheck_interval(&error, &account.source, active_interval, interval);
                let delay = retry_delay(wait, schedule.failures, &error);
                schedule.next = now + delay + Duration::from_secs(jitter);
                schedule.not_before = if error.kind == "rateLimited" {
                    schedule.next
                } else {
                    now + Duration::from_secs(5)
                };
            }
            if error.kind == "rateLimited" {
                let delay = schedule.next.saturating_duration_since(now);
                retry_at = Some(
                    wall.timestamp_millis()
                        + i64::try_from(delay.as_millis()).unwrap_or(i64::MAX / 2),
                );
            }
        } else {
            schedule.wake_retries = 0;
            schedule.failures = 0;
            schedule.next = now + Duration::from_secs(interval + jitter);
            schedule.not_before = now + Duration::from_secs(5);
        }
    }
    if let Some(error) = inner.view.accounts[index].error.as_mut() {
        error.retry_at = retry_at;
    }
    inner.view.refreshing = inner.view.accounts.iter().any(|item| item.refreshing);
    let urgent = !alerts.is_empty() || identity_changed;
    let mut queued = Vec::with_capacity(alerts.len());
    for alert in alerts {
        let before = before
            .get(&format!("{}/{}", account.id, alert.window_id))
            .cloned();
        queued.push(alert.clone());
        inner.delivery.enqueue(
            Some(&account.id),
            Message::Alert {
                label: account.label.clone(),
                alert,
                before,
            },
            wall,
        );
    }
    Applied::Reading {
        urgent,
        alerts: queued,
    }
}

pub fn start(app: AppHandle, runtime: Arc<Runtime>) {
    static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    tauri::async_runtime::spawn(async move {
        let mut last_tray_update = Instant::now();
        let mut last_tick = SystemTime::now();
        let mut last_digest_check: Option<Instant> = None;
        loop {
            let mut inner = runtime.inner.lock().await;
            if runtime.flush_due() {
                inner.view.notice = runtime
                    .persist(&inner.view, &inner.alerts, &inner.delivery)
                    .err();
            }
            let tick = SystemTime::now();
            let interval = Duration::from_secs(inner.view.settings.poll_interval_seconds);
            if resumed(last_tick, tick, interval) {
                bring_forward(&mut inner.schedules, true, None);
                for schedule in inner.schedules.values_mut() {
                    schedule.outage.restart();
                }
                runtime.update_wake.notify_one();
                last_digest_check = None;
            }
            last_tick = tick;
            let state = &mut *inner;
            for (index, account) in state.view.accounts.iter().enumerate() {
                if !state.schedules.contains_key(&account.id) {
                    state.schedules.insert(
                        account.id.clone(),
                        Schedule::new(Duration::from_millis(index as u64 * 800)),
                    );
                }
            }
            not_updating(&mut inner, chrono::Utc::now());
            let failure_changed = submit_pending(&app, &runtime, &mut inner);
            let now = Instant::now();
            let mut due = Vec::new();
            let state = &mut *inner;
            let running = state
                .schedules
                .values()
                .filter(|schedule| schedule.in_flight)
                .count();
            for account in &mut state.view.accounts {
                if due.len() + running >= 3 {
                    break;
                }
                let Some(schedule) = state
                    .schedules
                    .get_mut(&account.id)
                    .filter(|schedule| schedule.ready(now))
                else {
                    continue;
                };
                let generation = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
                schedule.in_flight = true;
                schedule.generation = generation;
                account.refreshing = true;
                due.push((account.clone(), generation));
            }
            inner.view.refreshing = inner.view.accounts.iter().any(|account| account.refreshing);
            let publish_now = !due.is_empty() || failure_changed;
            let tray_due = last_tray_update.elapsed() >= Duration::from_secs(5);
            if publish_now {
                inner.view.changed();
            }
            let view = (publish_now || tray_due).then(|| inner.view.clone());
            drop(inner);
            if let Some(view) = view {
                if publish_now {
                    publish(&app, &view);
                } else {
                    tray::update(&app, &view);
                }
                last_tray_update = Instant::now();
                if publish_now && view.settings.notifications {
                    sync_permission(&app, &runtime).await;
                }
            }
            if last_digest_check.is_none_or(|at| at.elapsed() >= DIGEST_CHECK_INTERVAL) {
                last_digest_check = Some(Instant::now());
                digest::deliver(&runtime).await;
            }
            for (account, generation) in due {
                let runtime = runtime.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let result = providers::fetch(
                        &runtime.client,
                        &account,
                        &runtime.profiles,
                        &runtime.resets,
                    )
                    .await;
                    let visible = app
                        .get_webview_window("main")
                        .is_some_and(|window| window.is_visible().unwrap_or(false));
                    let mut inner = runtime.inner.lock().await;
                    let applied = apply_result(
                        &mut inner,
                        &account,
                        generation,
                        result,
                        visible,
                        Instant::now(),
                        chrono::Utc::now(),
                    );
                    let wake = match applied {
                        Applied::Dropped => return,
                        Applied::Released => true,
                        Applied::Reading { urgent, alerts } => {
                            if let Some(saved) = runtime.persist_reading(
                                &inner.view,
                                &inner.alerts,
                                &inner.delivery,
                                urgent,
                            ) {
                                inner.view.notice = saved.err();
                            }
                            inner.view.changed();
                            !alerts.is_empty()
                        }
                    };
                    let view = inner.view.clone();
                    drop(inner);
                    publish(&app, &view);
                    if wake {
                        runtime.wake.notify_one();
                    }
                });
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(1)) => {},
                _ = runtime.wake.notified() => {},
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        delivery::Delivery,
        history::HistoryStore,
        model::{Provider, UsageWindow},
        notifications::AlertKind,
        settings::Settings,
    };

    #[test]
    fn permission_answer_is_kept_until_the_system_decides() {
        use NotificationPermission::*;
        assert_eq!(permission(Some(PermissionState::Granted), Unasked), Granted);
        assert_eq!(permission(Some(PermissionState::Denied), Granted), Denied);
        assert_eq!(permission(Some(PermissionState::Granted), Denied), Granted);
        assert_eq!(permission(Some(PermissionState::Prompt), Unasked), Unasked);
        assert_eq!(permission(None, Denied), Denied);
    }

    #[test]
    fn rate_limit_backoff_respects_retry_after() {
        let error = UsageError {
            retry_after_seconds: Some(7200),
            ..UsageError::new("rateLimited", "Slow down")
        };
        assert_eq!(retry_delay(60, 1, &error), Duration::from_secs(7200));
    }

    #[test]
    fn failure_backoff_is_bounded() {
        let error = UsageError::new("network", "Offline");
        assert_eq!(retry_delay(60, 1, &error), Duration::from_secs(60));
        assert_eq!(retry_delay(60, 3, &error), Duration::from_secs(240));
        assert_eq!(retry_delay(60, u32::MAX, &error), Duration::from_secs(3600));
    }

    #[test]
    fn local_sign_out_is_rechecked_without_backoff() {
        let local = UsageError {
            signed_out_since: Some("2026-10-16T10:00:00Z".into()),
            ..UsageError::new("signedOut", "Signed out")
        };
        assert_eq!(retry_delay(60, 5, &local), Duration::from_secs(60));
        let rejected = UsageError::new("signedOut", "Signed out");
        assert_eq!(retry_delay(60, 1, &rejected), Duration::from_secs(300));
        let changed = UsageError::new("credentialsChanged", "Changed");
        assert_eq!(retry_delay(60, 1, &changed), CREDENTIALS_CHANGED_RETRY);
        assert_eq!(retry_delay(60, 2, &changed), Duration::from_secs(120));
    }

    #[test]
    fn expired_tokens_recheck_at_the_active_interval_unless_the_keychain_is_involved() {
        let expired = UsageError {
            signed_out_since: Some("2026-10-16T10:00:00Z".into()),
            ..UsageError::new("signedOut", "Expired")
        };
        let folder = AccountSource::ConfigDir {
            path: "/example/.claude-work".into(),
            keychain_suffix: None,
        };
        assert_eq!(recheck_interval(&expired, &folder, 60, 300), 60);
        let rejected = UsageError::new("signedOut", "Rejected");
        assert_eq!(recheck_interval(&rejected, &folder, 60, 300), 300);
        let offline = UsageError::new("network", "Offline");
        assert_eq!(recheck_interval(&offline, &folder, 60, 300), 300);
        let suffixed = AccountSource::ConfigDir {
            path: "/example/.claude-work".into(),
            keychain_suffix: Some("0123abcd".into()),
        };
        let keychain_wait = if cfg!(target_os = "macos") { 300 } else { 60 };
        for source in [AccountSource::Keychain, suffixed] {
            assert_eq!(recheck_interval(&expired, &source, 60, 300), keychain_wait);
        }
    }

    #[test]
    fn wall_clock_gap_beyond_two_intervals_is_a_wake() {
        let last = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let interval = Duration::from_secs(60);
        assert!(!resumed(last, last + Duration::from_secs(1), interval));
        assert!(!resumed(last, last + Duration::from_secs(120), interval));
        assert!(resumed(last, last + Duration::from_secs(121), interval));
        assert!(resumed(
            last,
            last + Duration::from_secs(8 * 3600),
            interval
        ));
        assert!(!resumed(last, last - Duration::from_secs(3600), interval));
    }

    #[test]
    fn wake_refresh_keeps_rate_limit_cooldown() {
        let mut schedules = HashMap::new();
        let mut limited = Schedule::new(Duration::from_secs(600));
        limited.not_before = Instant::now() + Duration::from_secs(600);
        schedules.insert("limited".to_owned(), limited);
        schedules.insert("idle".to_owned(), Schedule::new(Duration::from_secs(600)));
        bring_forward(&mut schedules, true, None);
        let now = Instant::now();
        assert!(schedules["idle"].ready(now));
        assert!(!schedules["limited"].ready(now));
        assert!(schedules
            .values()
            .all(|schedule| schedule.wake_retries == WAKE_RETRIES));
    }

    const ID: &str = "claude-a";

    fn view(account: Account, settings: Settings) -> AppState {
        AppState {
            revision: 0,
            accounts: vec![account],
            settings,
            refreshing: true,
            notice: None,
            recovery: false,
            recovered: Vec::new(),
            ignored_sources: Vec::new(),
            switching: Default::default(),
            notification_failed_at: None,
        }
    }

    fn identity(email: &str) -> Identity {
        Identity {
            email: Some(email.into()),
            ..Identity::default()
        }
    }

    fn account() -> Account {
        Account {
            id: ID.into(),
            provider: Provider::Claude,
            label: "Work".into(),
            color: "#ffffff".into(),
            source: AccountSource::ConfigDir {
                path: "/example/.claude-work".into(),
                keychain_suffix: None,
            },
            identity: identity("a@example.test"),
            snapshot: None,
            error: None,
            refreshing: true,
        }
    }

    fn wall(minutes: i64) -> DateTime<Utc> {
        "2099-12-31T12:00:00Z".parse::<DateTime<Utc>>().unwrap()
            + chrono::Duration::minutes(minutes)
    }

    fn reading(used: f64, minutes: i64) -> Snapshot {
        Snapshot {
            windows: vec![UsageWindow {
                id: "five_hour".into(),
                label: "Session".into(),
                used_percent: used,
                resets_at: Some(wall(300).to_rfc3339()),
                window_seconds: Some(18_000),
            }],
            fetched_at: wall(minutes).to_rfc3339(),
            extra_usage: None,
            reset_grants: None,
            balance: None,
            incomplete: false,
        }
    }

    fn runtime_state(settings: Settings) -> (RuntimeState, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let state = RuntimeState {
            view: view(account(), settings),
            alerts: AlertMemory::new(),
            delivery: Delivery::default(),
            schedules: HashMap::new(),
            history: HistoryStore::new(directory.path().into()),
        };
        (state, directory)
    }

    /// Marks the account in flight as the poll loop does before a fetch.
    fn dispatch(state: &mut RuntimeState, generation: u64) {
        let schedule = state
            .schedules
            .entry(ID.into())
            .or_insert_with(|| Schedule::new(Duration::ZERO));
        schedule.in_flight = true;
        schedule.generation = generation;
        state.view.accounts[0].refreshing = true;
    }

    fn apply(
        state: &mut RuntimeState,
        generation: u64,
        result: Result<(Snapshot, Identity), UsageError>,
        now: Instant,
        minutes: i64,
    ) -> Applied {
        apply_result(
            state,
            &account(),
            generation,
            result,
            true,
            now,
            wall(minutes),
        )
    }

    fn reading_of(applied: &Applied) -> (bool, Vec<AlertKind>) {
        match applied {
            Applied::Reading { urgent, alerts } => (
                *urgent,
                alerts.iter().map(|alert| alert.kind.clone()).collect(),
            ),
            other => panic!("expected a reading, got {other:?}"),
        }
    }

    #[test]
    fn source_change_mid_fetch_releases_the_account() {
        let account = Account {
            source: AccountSource::Keychain,
            ..account()
        };
        let mut view = view(account, Settings::default());
        let mut schedules = HashMap::new();
        let mut schedule = Schedule::new(Duration::from_secs(600));
        schedule.in_flight = true;
        schedules.insert("claude-a".to_owned(), schedule);
        assert!(!release(&mut view, &mut schedules, "missing"));
        assert!(release(&mut view, &mut schedules, "claude-a"));
        assert!(!view.accounts[0].refreshing);
        assert!(!view.refreshing);
        assert_eq!(view.revision, 1);
        assert!(schedules["claude-a"].ready(Instant::now()));
    }

    #[test]
    fn a_superseded_fetch_changes_nothing() {
        let (mut state, _history) = runtime_state(Settings::default());
        dispatch(&mut state, 2);
        let result = || Ok((reading(90.0, 0), identity("a@example.test")));
        let now = Instant::now();
        assert!(matches!(
            apply(&mut state, 1, result(), now, 0),
            Applied::Dropped
        ));
        assert!(state.view.accounts[0].snapshot.is_none());
        assert!(state.view.accounts[0].refreshing);
        assert!(state.schedules[ID].in_flight);
        assert!(state.alerts.is_empty());
        assert!(state.delivery.is_idle());
        assert_eq!(state.view.revision, 0);
        state.schedules.get_mut(ID).unwrap().in_flight = false;
        assert!(matches!(
            apply(&mut state, 2, result(), now, 0),
            Applied::Dropped
        ));
        state.schedules.clear();
        assert!(matches!(
            apply(&mut state, 2, result(), now, 0),
            Applied::Dropped
        ));
        assert!(state.view.accounts[0].snapshot.is_none());
    }

    #[test]
    fn a_moved_account_is_released_instead_of_updated() {
        let (mut state, _history) = runtime_state(Settings::default());
        dispatch(&mut state, 1);
        state.view.accounts[0].source = AccountSource::Keychain;
        let result = Ok((reading(90.0, 0), identity("a@example.test")));
        assert!(matches!(
            apply(&mut state, 1, result, Instant::now(), 0),
            Applied::Released
        ));
        assert!(state.view.accounts[0].snapshot.is_none());
        assert!(!state.view.accounts[0].refreshing);
        assert!(!state.schedules[ID].in_flight);
        assert!(state.alerts.is_empty());
    }

    #[test]
    fn a_new_person_behind_the_account_starts_alert_memory_over() {
        let (mut state, _history) = runtime_state(Settings::default());
        let now = Instant::now();
        let fetch = |state: &mut RuntimeState, generation, used, who: Identity| {
            dispatch(state, generation);
            reading_of(&apply(
                state,
                generation,
                Ok((reading(used, 0), who)),
                now,
                0,
            ))
        };
        let a = identity("a@example.test");
        assert_eq!(
            fetch(&mut state, 1, 85.0, a.clone()),
            (true, vec![AlertKind::Threshold(80)])
        );
        assert_eq!(fetch(&mut state, 2, 85.0, a.clone()), (false, vec![]));
        let plan = Identity {
            plan: Some("max".into()),
            ..a
        };
        assert_eq!(fetch(&mut state, 3, 85.0, plan), (true, vec![]));
        assert!(!state.delivery.is_idle());
        state.delivery.enqueue(
            Some(ID),
            Message::NotUpdating {
                label: "Work".into(),
                error: UsageError::new("network", "Offline"),
                last: wall(0),
            },
            wall(0),
        );
        let b = identity("b@example.test");
        assert_eq!(fetch(&mut state, 4, 50.0, b.clone()), (true, vec![]));
        assert!(state.delivery.is_idle());
        assert_eq!(state.view.accounts[0].identity, b);
        assert_eq!(
            fetch(&mut state, 5, 85.0, b),
            (true, vec![AlertKind::Threshold(80)])
        );
    }

    #[test]
    fn a_rate_limit_holds_the_next_fetch_until_the_server_allows_it() {
        let (mut state, _history) = runtime_state(Settings::default());
        dispatch(&mut state, 1);
        let now = Instant::now();
        let limited = UsageError {
            retry_after_seconds: Some(600),
            ..UsageError::new("rateLimited", "Slow down")
        };
        assert_eq!(
            reading_of(&apply(&mut state, 1, Err(limited), now, 0)),
            (false, vec![])
        );
        let wait = Duration::from_secs(600 + jitter(ID));
        let schedule = &state.schedules[ID];
        assert!(!schedule.in_flight);
        assert_eq!(schedule.fetched, Some(now));
        assert_eq!(schedule.failures, 1);
        assert_eq!(schedule.next, now + wait);
        assert_eq!(schedule.not_before, schedule.next);
        let error = state.view.accounts[0].error.as_ref().unwrap();
        assert_eq!(
            error.retry_at,
            Some(wall(0).timestamp_millis() + i64::try_from(wait.as_millis()).unwrap())
        );
        assert!(!state.view.accounts[0].refreshing);
        assert!(!state.view.refreshing);

        dispatch(&mut state, 2);
        let offline = UsageError::new("network", "Offline");
        apply(&mut state, 2, Err(offline), now, 0);
        let schedule = &state.schedules[ID];
        assert_eq!(schedule.failures, 2);
        assert_eq!(schedule.next, now + Duration::from_secs(120 + jitter(ID)));
        assert_eq!(schedule.not_before, now + Duration::from_secs(5));
        assert_eq!(
            state.view.accounts[0].error.as_ref().unwrap().retry_at,
            None
        );
    }

    #[test]
    fn wake_retries_retry_network_errors_quickly_without_backoff() {
        let (mut state, _history) = runtime_state(Settings::default());
        dispatch(&mut state, 1);
        state.schedules.get_mut(ID).unwrap().wake_retries = 2;
        let now = Instant::now();
        apply(
            &mut state,
            1,
            Err(UsageError::new("network", "Offline")),
            now,
            0,
        );
        let schedule = &state.schedules[ID];
        assert_eq!(schedule.wake_retries, 1);
        assert_eq!(schedule.failures, 0);
        assert_eq!(schedule.next, now + WAKE_RETRY_DELAY);
        assert_eq!(schedule.not_before, schedule.next);

        dispatch(&mut state, 2);
        apply(
            &mut state,
            2,
            Err(UsageError::new("auth", "Rejected")),
            now,
            0,
        );
        let schedule = &state.schedules[ID];
        assert_eq!(schedule.wake_retries, 0);
        assert_eq!(schedule.failures, 1);
        assert_eq!(schedule.next, now + Duration::from_secs(300 + jitter(ID)));

        dispatch(&mut state, 3);
        state.schedules.get_mut(ID).unwrap().wake_retries = 1;
        let result = Ok((reading(10.0, 0), identity("a@example.test")));
        apply(&mut state, 3, result, now, 0);
        let schedule = &state.schedules[ID];
        assert_eq!(schedule.wake_retries, 0);
        assert_eq!(schedule.failures, 0);
        assert_eq!(schedule.next, now + Duration::from_secs(60 + jitter(ID)));
        assert_eq!(schedule.not_before, now + Duration::from_secs(5));
        assert!(state.view.accounts[0].error.is_none());
    }

    /// Readings every 10 minutes climbing from 10% to 20%, then `last` at minute 60.
    fn climb(last: f64) -> (RuntimeState, tempfile::TempDir, Vec<AlertKind>) {
        let (mut state, history) = runtime_state(Settings {
            poll_interval_seconds: 600,
            pace_alerts: false,
            burn_alerts: true,
            ..Settings::default()
        });
        let now = Instant::now();
        let fetch = |state: &mut RuntimeState, step: i64, used| {
            dispatch(state, step as u64 + 1);
            let result = Ok((reading(used, step * 10), identity("a@example.test")));
            reading_of(&apply(state, step as u64 + 1, result, now, step * 10)).1
        };
        for step in 0..=5 {
            assert_eq!(fetch(&mut state, step, 10.0 + 2.0 * step as f64), vec![]);
        }
        let alerts = fetch(&mut state, 6, last);
        (state, history, alerts)
    }

    #[test]
    fn a_burn_alert_is_held_back_when_another_alert_fired_for_the_window() {
        assert_eq!(climb(60.0).2, vec![AlertKind::Burn { increase: 50 }]);
        let (mut state, _history, alerts) = climb(85.0);
        assert_eq!(alerts, vec![AlertKind::Threshold(80)]);
        dispatch(&mut state, 8);
        let result = Ok((reading(90.0, 70), identity("a@example.test")));
        let next = reading_of(&apply(&mut state, 8, result, Instant::now(), 70));
        assert_eq!(next, (false, vec![]));
    }

    #[test]
    fn an_expired_token_is_rechecked_at_the_active_interval_while_hidden() {
        let expired = UsageError {
            signed_out_since: Some("2099-12-31T11:00:00Z".into()),
            ..UsageError::new("signedOut", "Expired")
        };
        let offline = UsageError::new("network", "Offline");
        for (error, wait) in [(expired, 60), (offline, 300)] {
            let (mut state, _history) = runtime_state(Settings::default());
            dispatch(&mut state, 1);
            let now = Instant::now();
            apply_result(&mut state, &account(), 1, Err(error), false, now, wall(0));
            assert_eq!(
                state.schedules[ID].next,
                now + Duration::from_secs(wait + jitter(ID))
            );
        }
    }

    #[test]
    fn opening_the_popover_refreshes_only_old_readings() {
        let mut schedules = HashMap::new();
        let mut recent = Schedule::new(Duration::from_secs(600));
        recent.fetched = Some(Instant::now() - Duration::from_secs(10));
        let mut old = Schedule::new(Duration::from_secs(600));
        old.fetched = Some(Instant::now() - Duration::from_secs(31));
        schedules.insert("recent".to_owned(), recent);
        schedules.insert("old".to_owned(), old);
        schedules.insert("never".to_owned(), Schedule::new(Duration::from_secs(600)));
        bring_forward(&mut schedules, false, Some(OPEN_REFRESH_AGE));
        let now = Instant::now();
        assert!(!schedules["recent"].ready(now));
        assert!(schedules["old"].ready(now));
        assert!(schedules["never"].ready(now));
        bring_forward(&mut schedules, false, None);
        assert!(schedules["recent"].ready(Instant::now()));
    }

    #[test]
    fn in_flight_and_server_cooldown_block_refresh() {
        let mut schedule = Schedule::new(Duration::ZERO);
        schedule.not_before = Instant::now() + Duration::from_secs(60);
        assert!(!schedule.ready(Instant::now()));
        schedule.not_before = Instant::now();
        schedule.in_flight = true;
        assert!(!schedule.ready(Instant::now()));
    }
}
