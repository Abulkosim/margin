use super::{model::UsageError, notifications, providers, tray};
use crate::runtime::{publish, Runtime};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

pub struct Schedule {
    pub next: Instant,
    pub not_before: Instant,
    pub failures: u32,
    pub in_flight: bool,
    pub generation: u64,
}

impl Schedule {
    fn new(delay: Duration) -> Self {
        let now = Instant::now();
        Self {
            next: now + delay,
            not_before: now,
            failures: 0,
            in_flight: false,
            generation: 0,
        }
    }

    fn ready(&self, now: Instant) -> bool {
        !self.in_flight && now >= self.next && now >= self.not_before
    }
}

pub fn retry_delay(interval: u64, failures: u32, error: &UsageError) -> Duration {
    let backoff = interval
        .saturating_mul(1_u64 << failures.saturating_sub(1).min(6))
        .min(3600);
    let minimum = if error.kind == "auth" || error.kind == "credentials" {
        300
    } else {
        0
    };
    Duration::from_secs(
        backoff
            .max(minimum)
            .max(error.retry_after_seconds.unwrap_or(0).min(86400)),
    )
}

pub async fn refresh(runtime: &Runtime) {
    let mut inner = runtime.inner.lock().await;
    let now = Instant::now();
    for schedule in inner.schedules.values_mut() {
        if !schedule.in_flight {
            schedule.next = now.max(schedule.not_before);
        }
    }
    drop(inner);
    runtime.wake.notify_one();
}

pub fn start(app: AppHandle, runtime: Arc<Runtime>) {
    static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    tauri::async_runtime::spawn(async move {
        let mut last_tray_update = Instant::now();
        loop {
            let mut inner = runtime.inner.lock().await;
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
            } else if last_tray_update.elapsed() >= Duration::from_secs(5) {
                tray::update(&app, &view);
                last_tray_update = Instant::now();
            }
            for (account, generation) in due {
                let runtime = runtime.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let result = providers::fetch(&runtime.client, &account).await;
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
                    if let Some(schedule) = inner.schedules.get_mut(&account.id) {
                        schedule.in_flight = false;
                        let now = Instant::now();
                        if let Some(error) = error {
                            schedule.failures = schedule.failures.saturating_add(1);
                            let delay = retry_delay(interval, schedule.failures, &error);
                            schedule.next = now + delay + Duration::from_secs(jitter);
                            schedule.not_before = if error.kind == "rateLimited" {
                                schedule.next
                            } else {
                                now + Duration::from_secs(5)
                            };
                        } else {
                            schedule.failures = 0;
                            schedule.next = now + Duration::from_secs(interval + jitter);
                            schedule.not_before = now + Duration::from_secs(5);
                        }
                    }
                    inner.view.refreshing = inner.view.accounts.iter().any(|item| item.refreshing);
                    inner.view.notice = runtime.persist(&inner.view, &inner.alerts).err();
                    inner.view.changed();
                    let view = inner.view.clone();
                    drop(inner);
                    publish(&app, &view);
                    if view.settings.notifications {
                        for alert in alerts {
                            let body = if alert.threshold == 100 {
                                format!("{} · {} limit reached.", account.label, alert.window_label)
                            } else {
                                format!(
                                    "{} · {} usage reached {}%.",
                                    account.label, alert.window_label, alert.threshold
                                )
                            };
                            let _ = app
                                .notification()
                                .builder()
                                .title("Margin")
                                .body(body)
                                .show();
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
    fn rate_limit_backoff_respects_retry_after() {
        let error = UsageError {
            kind: "rateLimited".into(),
            message: "Slow down".into(),
            retry_after_seconds: Some(7200),
        };
        assert_eq!(retry_delay(60, 1, &error), Duration::from_secs(7200));
    }

    #[test]
    fn failure_backoff_is_bounded() {
        let error = UsageError {
            kind: "network".into(),
            message: "Offline".into(),
            retry_after_seconds: None,
        };
        assert_eq!(retry_delay(60, 1, &error), Duration::from_secs(60));
        assert_eq!(retry_delay(60, 3, &error), Duration::from_secs(240));
        assert_eq!(retry_delay(60, u32::MAX, &error), Duration::from_secs(3600));
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
