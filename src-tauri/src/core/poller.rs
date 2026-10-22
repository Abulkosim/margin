use super::{
    model::UsageError,
    notifications::{self, AlertKind},
    providers,
    settings::NotificationPermission,
    tray,
};
use crate::runtime::{publish, Runtime};
use std::{
    collections::HashMap,
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
        }
    }

    fn ready(&self, now: Instant) -> bool {
        !self.in_flight && now >= self.next && now >= self.not_before
    }
}

pub fn retry_delay(interval: u64, failures: u32, error: &UsageError) -> Duration {
    if error.kind == "signedOut" && error.signed_out_since.is_some() {
        return Duration::from_secs(interval);
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

fn bring_forward(schedules: &mut HashMap<String, Schedule>, after_wake: bool) {
    let now = Instant::now();
    for schedule in schedules.values_mut() {
        if after_wake {
            schedule.wake_retries = WAKE_RETRIES;
        }
        if !schedule.in_flight {
            schedule.next = now.max(schedule.not_before);
        }
    }
}

pub async fn refresh(runtime: &Runtime) {
    bring_forward(&mut runtime.inner.lock().await.schedules, false);
    runtime.wake.notify_one();
}

pub async fn refresh_after_wake(runtime: &Runtime) {
    bring_forward(&mut runtime.inner.lock().await.schedules, true);
    runtime.wake.notify_one();
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
    let mut inner = runtime.inner.lock().await;
    inner.view.settings.notification_permission = next;
    inner.view.notice = runtime.persist(&inner.view, &inner.alerts).err();
    inner.view.changed();
    let view = inner.view.clone();
    drop(inner);
    publish(app, &view);
}

fn post(app: &AppHandle, account_label: &str, alert: &notifications::Alert) {
    let mut builder = app
        .notification()
        .builder()
        .title(alert.title(account_label));
    if let Some(body) = alert.body(chrono::Utc::now()) {
        builder = builder.body(body);
    }
    let _ = builder.show();
}

pub fn start(app: AppHandle, runtime: Arc<Runtime>) {
    static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    tauri::async_runtime::spawn(async move {
        let mut last_tray_update = Instant::now();
        let mut last_tick = SystemTime::now();
        loop {
            let mut inner = runtime.inner.lock().await;
            let tick = SystemTime::now();
            let interval = Duration::from_secs(inner.view.settings.poll_interval_seconds);
            if resumed(last_tick, tick, interval) {
                bring_forward(&mut inner.schedules, true);
            }
            last_tick = tick;
            let ids: Vec<String> = inner
                .view
                .accounts
                .iter()
                .map(|account| account.id.clone())
                .collect();
            for (index, id) in ids.iter().enumerate() {
                inner
                    .schedules
                    .entry(id.clone())
                    .or_insert_with(|| Schedule::new(Duration::from_millis(index as u64 * 800)));
            }
            let now = Instant::now();
            let mut due = Vec::new();
            let running = inner
                .schedules
                .values()
                .filter(|schedule| schedule.in_flight)
                .count();
            for id in ids {
                if due.len() + running >= 3 {
                    break;
                }
                let ready = inner
                    .schedules
                    .get(&id)
                    .is_some_and(|schedule| schedule.ready(now));
                if ready {
                    let generation = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
                    if let Some(schedule) = inner.schedules.get_mut(&id) {
                        schedule.in_flight = true;
                        schedule.generation = generation;
                    }
                    if let Some(account) = inner
                        .view
                        .accounts
                        .iter_mut()
                        .find(|account| account.id == id)
                    {
                        account.refreshing = true;
                        due.push((account.clone(), generation));
                    }
                }
            }
            inner.view.refreshing = inner.view.accounts.iter().any(|account| account.refreshing);
            if !due.is_empty() {
                inner.view.changed();
            }
            let view = inner.view.clone();
            drop(inner);
            if !due.is_empty() {
                publish(&app, &view);
                last_tray_update = Instant::now();
                if view.settings.notifications {
                    sync_permission(&app, &runtime).await;
                }
            } else if last_tray_update.elapsed() >= Duration::from_secs(5) {
                tray::update(&app, &view);
                last_tray_update = Instant::now();
            }
            for (account, generation) in due {
                let runtime = runtime.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let result =
                        providers::fetch(&runtime.client, &account, &runtime.profiles).await;
                    let visible = app
                        .get_webview_window("main")
                        .is_some_and(|window| window.is_visible().unwrap_or(false));
                    let mut inner = runtime.inner.lock().await;
                    if !inner.schedules.get(&account.id).is_some_and(|schedule| {
                        schedule.in_flight && schedule.generation == generation
                    }) {
                        return;
                    }
                    let Some(index) = inner.view.accounts.iter().position(|item| {
                        item.id == account.id
                            && item.provider == account.provider
                            && item.source == account.source
                    }) else {
                        return;
                    };
                    let interval = if visible {
                        inner.view.settings.poll_interval_seconds
                    } else {
                        inner.view.settings.idle_poll_interval_seconds
                    };
                    let mut alerts = Vec::new();
                    let error = result.as_ref().err().cloned();
                    match result {
                        Ok((snapshot, identity)) => {
                            let thresholds = inner.view.settings.thresholds.clone();
                            if inner.view.accounts[index].identity != identity {
                                inner
                                    .alerts
                                    .retain(|key, _| !key.starts_with(&format!("{}/", account.id)));
                            }
                            alerts = notifications::observe(
                                &mut inner.alerts,
                                &account.id,
                                &snapshot,
                                &thresholds,
                                chrono::Utc::now(),
                            );
                            let target = &mut inner.view.accounts[index];
                            target.snapshot = Some(snapshot);
                            target.error = None;
                            target.identity = identity;
                        }
                        Err(error) => inner.view.accounts[index].error = Some(error),
                    }
                    let observed = inner.view.accounts[index].clone();
                    if error.is_some() {
                        inner.history.failed(&observed);
                    } else {
                        inner
                            .history
                            .record(&observed, interval + 7, chrono::Utc::now());
                    }
                    inner.view.accounts[index].refreshing = false;
                    let jitter = account
                        .id
                        .bytes()
                        .fold(0_u64, |sum, byte| sum + u64::from(byte))
                        % 7;
                    let mut retry_at = None;
                    if let Some(schedule) = inner.schedules.get_mut(&account.id) {
                        schedule.in_flight = false;
                        let now = Instant::now();
                        if let Some(error) = error {
                            if error.kind == "network" && schedule.wake_retries > 0 {
                                schedule.wake_retries -= 1;
                                schedule.next = now + WAKE_RETRY_DELAY;
                                schedule.not_before = schedule.next;
                            } else {
                                schedule.wake_retries = 0;
                                schedule.failures = schedule.failures.saturating_add(1);
                                let delay = retry_delay(interval, schedule.failures, &error);
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
                                    chrono::Utc::now().timestamp_millis()
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
                    inner.view.notice = runtime.persist(&inner.view, &inner.alerts).err();
                    inner.view.changed();
                    let view = inner.view.clone();
                    drop(inner);
                    publish(&app, &view);
                    let settings = &view.settings;
                    if settings.notifications
                        && settings.notification_permission == NotificationPermission::Granted
                    {
                        for alert in alerts.iter().filter(|alert| {
                            settings.notify_on_reset || alert.kind != AlertKind::Reset
                        }) {
                            post(&app, &account.label, alert);
                        }
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
        bring_forward(&mut schedules, true);
        let now = Instant::now();
        assert!(schedules["idle"].ready(now));
        assert!(!schedules["limited"].ready(now));
        assert!(schedules
            .values()
            .all(|schedule| schedule.wake_retries == WAKE_RETRIES));
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
