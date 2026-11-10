use super::{
    digest::DigestState,
    model::{Account, UsageError},
    notifications::{self, Alert, AlertKind, AlertMemory, Clock12h, WindowAlerts},
    outage,
    settings::{NotificationPermission, Settings},
};
use chrono::{DateTime, Duration, Utc};

/// Waits before each retry of a notification that could not be submitted.
const RETRY_DELAYS_SECONDS: [i64; 3] = [60, 300, 900];

#[derive(Clone, Debug)]
pub enum Message {
    Alert {
        label: String,
        alert: Alert,
        /// The window's alert memory from before this alert was raised.
        before: Option<WindowAlerts>,
    },
    NotUpdating {
        label: String,
        error: UsageError,
        last: DateTime<Utc>,
    },
    Digest {
        title: String,
        body: String,
        /// The digest state from before this digest was marked sent.
        before: Option<DigestState>,
    },
}

impl Message {
    fn render(&self, now: DateTime<Utc>, clock: Clock12h) -> (String, Option<String>) {
        match self {
            Self::Alert { label, alert, .. } => {
                (alert.title(label, now, clock), alert.body(now, clock))
            }
            Self::NotUpdating { label, error, last } => (
                outage::title(label),
                Some(outage::body(error, *last, now, clock)),
            ),
            Self::Digest { title, body, .. } => (title.clone(), Some(body.clone())),
        }
    }

    /// A newer message for the same slot replaces one still waiting, so 95% does not follow a
    /// delayed 80%.
    fn same_slot(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Alert { alert: a, .. }, Self::Alert { alert: b, .. }) => {
                a.window_id == b.window_id
                    && std::mem::discriminant(&a.kind) == std::mem::discriminant(&b.kind)
            }
            (Self::NotUpdating { .. }, Self::NotUpdating { .. })
            | (Self::Digest { .. }, Self::Digest { .. }) => true,
            _ => false,
        }
    }

    fn still_wanted(
        &self,
        settings: &Settings,
        account: Option<&Account>,
        now: DateTime<Utc>,
    ) -> bool {
        match self {
            Self::Alert { alert, .. } => {
                let enabled = match alert.kind {
                    AlertKind::Threshold(threshold) => settings.thresholds.contains(&threshold),
                    AlertKind::Reset => settings.notify_on_reset,
                    AlertKind::Burn { .. } => settings.burn_alerts,
                    AlertKind::RunsOut => settings.pace_alerts,
                };
                let open = alert.kind == AlertKind::Reset
                    || alert.resets_at.as_deref().is_none_or(|reset| {
                        DateTime::parse_from_rfc3339(reset).is_ok_and(|reset| reset > now)
                    });
                enabled && open && account.is_some_and(|account| account.error.is_none())
            }
            Self::NotUpdating { error, .. } => {
                settings.stale_alerts
                    && account
                        .and_then(|account| account.error.as_ref())
                        .is_some_and(|now| now.kind == error.kind && outage::hides_usage(now))
            }
            Self::Digest { .. } => settings.weekly_digest,
        }
    }
}

#[derive(Debug)]
struct Pending {
    account_id: Option<String>,
    message: Message,
    retries: usize,
    due: DateTime<Utc>,
}

/// Notifications waiting to be submitted, in memory. Alert memory and the digest state record a
/// notification when it is raised, so it is never raised twice; this queue is what gets it to the
/// OS. While a notification waits for a retry, the saved copy of that state leaves it out, so the
/// first reading after a restart raises it again from current usage and settings.
#[derive(Debug, Default)]
pub struct Delivery {
    pending: Vec<Pending>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Attempt {
    /// `failed_at` changed.
    pub failure_changed: bool,
    /// A notification started or stopped waiting for a retry, so the saved state changed.
    pub save: bool,
}

impl Delivery {
    pub fn enqueue(&mut self, account_id: Option<&str>, message: Message, now: DateTime<Utc>) {
        self.pending.retain(|pending| {
            pending.account_id.as_deref() != account_id || !pending.message.same_slot(&message)
        });
        self.pending.push(Pending {
            account_id: account_id.map(str::to_owned),
            message,
            retries: 0,
            due: now,
        });
    }

    pub fn is_idle(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn forget(&mut self, account_id: &str) {
        self.pending
            .retain(|pending| pending.account_id.as_deref() != Some(account_id));
    }

    /// Alerts waiting for a retry, with their account and the memory from before them.
    fn retrying_alerts(&self) -> impl Iterator<Item = (&str, &Alert, Option<&WindowAlerts>)> {
        self.pending
            .iter()
            .filter(|pending| pending.retries > 0)
            .filter_map(|pending| match (&pending.account_id, &pending.message) {
                (Some(account_id), Message::Alert { alert, before, .. }) => {
                    Some((account_id.as_str(), alert, before.as_ref()))
                }
                _ => None,
            })
    }

    /// Alert memory as it should be saved.
    pub fn saved_alerts(&self, memory: &AlertMemory) -> AlertMemory {
        let mut saved = memory.clone();
        for (account_id, alert, before) in self.retrying_alerts() {
            let threshold_waiting = self.retrying_alerts().any(|(id, other, _)| {
                id == account_id
                    && other.window_id == alert.window_id
                    && matches!(other.kind, AlertKind::Threshold(_))
            });
            notifications::unrecord(&mut saved, account_id, alert, before, threshold_waiting);
        }
        saved
    }

    /// Digest state as it should be saved.
    pub fn saved_digest(&self, current: Option<DigestState>) -> Option<DigestState> {
        self.pending
            .iter()
            .filter(|pending| pending.retries > 0)
            .find_map(|pending| match &pending.message {
                Message::Digest { before, .. } => Some(before.clone()),
                _ => None,
            })
            .unwrap_or(current)
    }

    /// Submits what is due. `failed_at` holds the latest failure until the next success.
    pub fn attempt(
        &mut self,
        failed_at: &mut Option<String>,
        settings: &Settings,
        accounts: &[Account],
        now: DateTime<Utc>,
        clock: Clock12h,
        mut submit: impl FnMut(&str, Option<&str>) -> bool,
    ) -> Attempt {
        let allowed = settings.notifications
            && settings.notification_permission == NotificationPermission::Granted;
        let mut save = false;
        self.pending.retain(|pending| {
            let account = pending
                .account_id
                .as_deref()
                .and_then(|id| accounts.iter().find(|account| account.id == id));
            let wanted = allowed && pending.message.still_wanted(settings, account, now);
            save |= !wanted && pending.retries > 0;
            wanted
        });
        let before = failed_at.clone();
        self.pending.retain_mut(|pending| {
            if pending.due > now {
                return true;
            }
            let (title, body) = pending.message.render(now, clock);
            if submit(&title, body.as_deref()) {
                *failed_at = None;
                save |= pending.retries > 0;
                return false;
            }
            *failed_at = Some(now.to_rfc3339());
            let Some(delay) = RETRY_DELAYS_SECONDS.get(pending.retries) else {
                save = true;
                return false;
            };
            save |= pending.retries == 0;
            pending.retries += 1;
            pending.due = now + Duration::seconds(*delay);
            true
        });
        Attempt {
            failure_changed: before != *failed_at,
            save,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        model::{AccountSource, Identity, Provider, Snapshot, UsageWindow},
        notifications::{self, AlertMemory},
    };

    const RESET: &str = "2100-01-01T00:00:00Z";

    fn at(seconds: i64) -> DateTime<Utc> {
        "2099-12-31T12:00:00Z".parse::<DateTime<Utc>>().unwrap() + Duration::seconds(seconds)
    }

    fn account(id: &str) -> Account {
        Account {
            id: id.into(),
            provider: Provider::Claude,
            label: "Work".into(),
            color: "#ffffff".into(),
            source: AccountSource::Keychain,
            identity: Identity::default(),
            snapshot: None,
            error: None,
            refreshing: false,
        }
    }

    fn settings() -> Settings {
        Settings {
            notifications: true,
            notification_permission: NotificationPermission::Granted,
            notify_on_reset: true,
            weekly_digest: true,
            ..Settings::default()
        }
    }

    fn reading(used: f64) -> Snapshot {
        Snapshot {
            windows: vec![UsageWindow {
                id: "five_hour".into(),
                label: "Session".into(),
                used_percent: used,
                resets_at: Some(RESET.into()),
                window_seconds: Some(18000),
            }],
            fetched_at: at(0).to_rfc3339(),
            extra_usage: None,
            reset_grants: None,
            balance: None,
            incomplete: false,
        }
    }

    fn threshold(percent: u8) -> Message {
        Message::Alert {
            label: "Work".into(),
            alert: Alert {
                window_id: "five_hour".into(),
                window_label: "Session".into(),
                kind: AlertKind::Threshold(percent),
                resets_at: Some(RESET.into()),
                used_percent: f64::from(percent),
                runs_out: None,
            },
            before: None,
        }
    }

    /// Runs one attempt and returns the titles submitted.
    fn run(
        delivery: &mut Delivery,
        failed_at: &mut Option<String>,
        settings: &Settings,
        now: DateTime<Utc>,
        works: bool,
    ) -> Vec<String> {
        let mut titles = Vec::new();
        delivery.attempt(
            failed_at,
            settings,
            &[account("a")],
            now,
            Clock12h(false),
            |title, _| {
                titles.push(title.to_owned());
                works
            },
        );
        titles
    }

    #[test]
    fn a_waiting_alert_is_dropped_when_its_threshold_is_turned_off() {
        let mut settings = settings();
        let account = account("a");
        assert!(threshold(80).still_wanted(&settings, Some(&account), at(0)));
        settings.thresholds = vec![95];
        assert!(!threshold(80).still_wanted(&settings, Some(&account), at(0)));
    }

    #[test]
    fn a_waiting_alert_is_dropped_once_its_reading_cannot_be_refreshed() {
        let mut account = account("a");
        account.error = Some(UsageError::new("rateLimited", "Slow down"));
        assert!(!threshold(80).still_wanted(&settings(), Some(&account), at(0)));
    }

    #[test]
    fn a_waiting_not_updating_alert_follows_the_current_error() {
        let waiting = Message::NotUpdating {
            label: "Work".into(),
            error: UsageError::new("network", "Offline"),
            last: at(0),
        };
        let mut account = account("a");
        let settings = settings();
        account.error = Some(UsageError::new("network", "Offline"));
        assert!(waiting.still_wanted(&settings, Some(&account), at(0)));
        account.error = Some(UsageError::new("rateLimited", "Slow down"));
        assert!(!waiting.still_wanted(&settings, Some(&account), at(0)));
        account.error = None;
        assert!(!waiting.still_wanted(&settings, Some(&account), at(0)));
    }

    #[test]
    fn a_failed_alert_retries_a_limited_number_of_times_then_stops() {
        let mut delivery = Delivery::default();
        let mut failed_at = None;
        let settings = settings();
        delivery.enqueue(Some("a"), threshold(80), at(0));
        assert_eq!(
            run(&mut delivery, &mut failed_at, &settings, at(0), false).len(),
            1
        );
        assert_eq!(failed_at, Some(at(0).to_rfc3339()));
        assert!(run(&mut delivery, &mut failed_at, &settings, at(59), false).is_empty());
        let mut attempts = 1;
        let mut now = 0;
        for delay in RETRY_DELAYS_SECONDS {
            now += delay;
            attempts += run(&mut delivery, &mut failed_at, &settings, at(now), false).len();
        }
        assert_eq!(attempts, 1 + RETRY_DELAYS_SECONDS.len());
        assert!(delivery.pending.is_empty());
        assert!(run(&mut delivery, &mut failed_at, &settings, at(90_000), false).is_empty());
        assert_eq!(failed_at, Some(at(now).to_rfc3339()));
    }

    #[test]
    fn alert_memory_does_not_block_the_retry_and_success_sends_no_duplicate() {
        let mut memory = AlertMemory::new();
        let mut delivery = Delivery::default();
        let mut failed_at = None;
        let settings = settings();
        let raise = |memory: &mut AlertMemory, delivery: &mut Delivery, now| {
            for alert in notifications::observe(memory, "a", &reading(81.0), &[80], now) {
                delivery.enqueue(
                    Some("a"),
                    Message::Alert {
                        label: "Work".into(),
                        alert,
                        before: None,
                    },
                    now,
                );
            }
        };
        raise(&mut memory, &mut delivery, at(0));
        assert_eq!(
            run(&mut delivery, &mut failed_at, &settings, at(0), false).len(),
            1
        );
        raise(&mut memory, &mut delivery, at(60));
        let retried = run(&mut delivery, &mut failed_at, &settings, at(60), true);
        assert_eq!(retried, vec!["Work · Session 19% left"]);
        assert_eq!(failed_at, None);
        raise(&mut memory, &mut delivery, at(120));
        assert!(run(&mut delivery, &mut failed_at, &settings, at(120), true).is_empty());
        assert!(run(&mut delivery, &mut failed_at, &settings, at(10_000), true).is_empty());
    }

    #[test]
    fn a_success_clears_the_failure_and_only_changes_report_once() {
        let mut delivery = Delivery::default();
        let mut failed_at = Some(at(-5).to_rfc3339());
        delivery.enqueue(Some("a"), threshold(80), at(0));
        let changed = delivery.attempt(
            &mut failed_at,
            &settings(),
            &[account("a")],
            at(0),
            Clock12h(false),
            |_, _| true,
        );
        assert_eq!(
            changed,
            Attempt {
                failure_changed: true,
                save: false
            }
        );
        assert_eq!(failed_at, None);
        assert_eq!(
            delivery.attempt(
                &mut failed_at,
                &settings(),
                &[account("a")],
                at(1),
                Clock12h(false),
                |_, _| true,
            ),
            Attempt::default()
        );
    }

    #[test]
    fn a_higher_threshold_replaces_the_one_still_waiting() {
        let mut delivery = Delivery::default();
        let mut failed_at = None;
        let settings = settings();
        delivery.enqueue(Some("a"), threshold(80), at(0));
        run(&mut delivery, &mut failed_at, &settings, at(0), false);
        delivery.enqueue(Some("a"), threshold(95), at(30));
        delivery.enqueue(Some("b"), threshold(90), at(30));
        assert_eq!(delivery.pending.len(), 2);
        assert_eq!(
            run(&mut delivery, &mut failed_at, &settings, at(30), true),
            vec!["Work · Session 5% left"]
        );
        assert!(delivery.pending.is_empty());
    }

    #[test]
    fn retries_stop_when_the_alert_no_longer_applies() {
        let mut failed_at = None;
        let off = Settings {
            notifications: false,
            ..settings()
        };
        let mut delivery = Delivery::default();
        delivery.enqueue(Some("a"), threshold(80), at(0));
        assert!(run(&mut delivery, &mut failed_at, &off, at(0), true).is_empty());
        assert!(delivery.pending.is_empty());

        let mut delivery = Delivery::default();
        delivery.enqueue(Some("gone"), threshold(80), at(0));
        assert!(run(&mut delivery, &mut failed_at, &settings(), at(0), true).is_empty());

        let mut delivery = Delivery::default();
        delivery.enqueue(Some("a"), threshold(80), at(0));
        delivery.forget("a");
        assert!(delivery.pending.is_empty());

        let mut delivery = Delivery::default();
        delivery.enqueue(Some("a"), threshold(80), at(0));
        let past_reset: DateTime<Utc> = "2100-01-01T00:00:01Z".parse().unwrap();
        assert!(run(&mut delivery, &mut failed_at, &settings(), past_reset, true).is_empty());
    }

    #[test]
    fn a_not_updating_alert_is_dropped_once_the_account_recovers() {
        let error = UsageError::new("network", "Offline");
        let message = Message::NotUpdating {
            label: "Work".into(),
            error: error.clone(),
            last: at(-3600),
        };
        let mut failed = account("a");
        failed.error = Some(error);
        let mut submitted = 0;
        let mut failed_at = None;
        let mut delivery = Delivery::default();
        delivery.enqueue(Some("a"), message, at(0));
        delivery.attempt(
            &mut failed_at,
            &settings(),
            std::slice::from_ref(&failed),
            at(0),
            Clock12h(false),
            |title, body| {
                assert_eq!(title, "Work · Usage not updating");
                assert!(body.is_some_and(|body| body.contains("Can't reach the usage server")));
                submitted += 1;
                false
            },
        );
        assert_eq!(submitted, 1);
        assert_eq!(delivery.pending.len(), 1);
        let recovered = account("a");
        delivery.attempt(
            &mut failed_at,
            &settings(),
            std::slice::from_ref(&recovered),
            at(60),
            Clock12h(false),
            |_, _| panic!("recovered accounts send nothing"),
        );
        assert!(delivery.pending.is_empty());
    }

    const NEXT: &str = "2100-01-01T05:00:00Z";

    fn reading_until(used: f64, reset: &str) -> Snapshot {
        let mut snapshot = reading(used);
        snapshot.windows[0].resets_at = Some(reset.into());
        snapshot
    }

    /// Observes and queues like the poller, with each window's memory from before the reading.
    fn raise(
        memory: &mut AlertMemory,
        delivery: &mut Delivery,
        snapshot: &Snapshot,
        now: DateTime<Utc>,
    ) -> Vec<AlertKind> {
        let before = memory.clone();
        let alerts = notifications::observe(memory, "a", snapshot, &[80, 95], now);
        let kinds = alerts.iter().map(|alert| alert.kind.clone()).collect();
        for alert in alerts {
            let before = before.get(&format!("a/{}", alert.window_id)).cloned();
            delivery.enqueue(
                Some("a"),
                Message::Alert {
                    label: "Work".into(),
                    alert,
                    before,
                },
                now,
            );
        }
        kinds
    }

    /// Alerts the first reading after a restart raises, from the alert memory that was saved.
    fn after_restart(
        memory: &AlertMemory,
        delivery: &Delivery,
        snapshot: &Snapshot,
        thresholds: &[u8],
    ) -> Vec<AlertKind> {
        let mut saved = delivery.saved_alerts(memory);
        notifications::observe(&mut saved, "a", snapshot, thresholds, at(600))
            .into_iter()
            .map(|alert| alert.kind)
            .collect()
    }

    #[test]
    fn a_retry_lost_on_quit_is_raised_again_by_the_next_reading() {
        let mut memory = AlertMemory::new();
        let mut delivery = Delivery::default();
        let mut failed_at = None;
        raise(&mut memory, &mut delivery, &reading(81.0), at(0));
        assert!(after_restart(&memory, &delivery, &reading(81.0), &[80]).is_empty());
        let attempt = delivery.attempt(
            &mut failed_at,
            &settings(),
            &[account("a")],
            at(0),
            Clock12h(false),
            |_, _| false,
        );
        assert!(attempt.save);
        assert_eq!(
            after_restart(&memory, &delivery, &reading(81.0), &[80, 95]),
            vec![AlertKind::Threshold(80)]
        );
        assert_eq!(
            after_restart(&memory, &delivery, &reading(96.0), &[80, 95]),
            vec![AlertKind::Threshold(95)]
        );
    }

    #[test]
    fn a_retry_lost_on_quit_is_not_raised_once_it_no_longer_applies() {
        let mut memory = AlertMemory::new();
        let mut delivery = Delivery::default();
        let mut failed_at = None;
        raise(&mut memory, &mut delivery, &reading(81.0), at(0));
        run(&mut delivery, &mut failed_at, &settings(), at(0), false);
        assert!(after_restart(&memory, &delivery, &reading(81.0), &[95]).is_empty());
        assert!(after_restart(&memory, &delivery, &reading(50.0), &[80]).is_empty());
        let mut saved = delivery.saved_alerts(&memory);
        let next =
            notifications::observe(&mut saved, "a", &reading_until(81.0, NEXT), &[80], at(600));
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].kind, AlertKind::Threshold(80));
        assert_eq!(next[0].resets_at.as_deref(), Some(NEXT));
    }

    #[test]
    fn a_retry_shown_before_quit_is_not_raised_again() {
        let mut memory = AlertMemory::new();
        let mut delivery = Delivery::default();
        let mut failed_at = None;
        raise(&mut memory, &mut delivery, &reading(81.0), at(0));
        run(&mut delivery, &mut failed_at, &settings(), at(0), false);
        let attempt = delivery.attempt(
            &mut failed_at,
            &settings(),
            &[account("a")],
            at(60),
            Clock12h(false),
            |_, _| true,
        );
        assert!(attempt.save);
        assert!(after_restart(&memory, &delivery, &reading(81.0), &[80]).is_empty());
    }

    #[test]
    fn a_retry_dropped_as_stale_stays_recorded() {
        let mut memory = AlertMemory::new();
        let mut delivery = Delivery::default();
        let mut failed_at = None;
        raise(&mut memory, &mut delivery, &reading(81.0), at(0));
        run(&mut delivery, &mut failed_at, &settings(), at(0), false);
        let mut failed = account("a");
        failed.error = Some(UsageError::new("network", "Offline"));
        let attempt = delivery.attempt(
            &mut failed_at,
            &settings(),
            &[failed],
            at(30),
            Clock12h(false),
            |_, _| panic!("stale alerts are not shown"),
        );
        assert!(attempt.save);
        assert!(after_restart(&memory, &delivery, &reading(81.0), &[80]).is_empty());
    }

    #[test]
    fn a_reset_lost_on_quit_is_raised_again_with_its_threshold() {
        let settings = settings();
        let mut failed_at = None;
        for (used, expected) in [
            (85.0, vec![AlertKind::Reset, AlertKind::Threshold(80)]),
            (5.0, vec![AlertKind::Reset]),
        ] {
            let mut memory = AlertMemory::new();
            let mut delivery = Delivery::default();
            raise(&mut memory, &mut delivery, &reading(90.0), at(0));
            run(&mut delivery, &mut failed_at, &settings, at(0), true);
            let next = reading_until(used, NEXT);
            assert_eq!(raise(&mut memory, &mut delivery, &next, at(60)), expected);
            run(&mut delivery, &mut failed_at, &settings, at(60), false);
            assert_eq!(
                after_restart(&memory, &delivery, &next, &[80, 95]),
                expected
            );
        }
    }

    #[test]
    fn a_reset_lost_on_quit_never_repeats_a_threshold_already_shown() {
        let mut memory = AlertMemory::new();
        let mut delivery = Delivery::default();
        let mut failed_at = None;
        raise(&mut memory, &mut delivery, &reading(90.0), at(0));
        run(&mut delivery, &mut failed_at, &settings(), at(0), true);
        let next = reading_until(85.0, NEXT);
        raise(&mut memory, &mut delivery, &next, at(60));
        delivery.attempt(
            &mut failed_at,
            &settings(),
            &[account("a")],
            at(60),
            Clock12h(false),
            |title, _| !title.ends_with("limit reset"),
        );
        assert_eq!(delivery.pending.len(), 1);
        assert!(after_restart(&memory, &delivery, &next, &[80, 95]).is_empty());
    }

    #[test]
    fn a_digest_lost_on_quit_leaves_its_slot_unsent() {
        let state = |at: &str| DigestState {
            last_sent_at: at.into(),
            previous: None,
        };
        let old = state("2099-12-21T09:00:00Z");
        let sent = state("2099-12-28T09:00:00Z");
        let mut delivery = Delivery::default();
        let mut failed_at = None;
        delivery.enqueue(
            None,
            Message::Digest {
                title: "Last week".into(),
                body: "Usage".into(),
                before: Some(old.clone()),
            },
            at(0),
        );
        assert_eq!(
            delivery.saved_digest(Some(sent.clone())),
            Some(sent.clone())
        );
        run(&mut delivery, &mut failed_at, &settings(), at(0), false);
        assert_eq!(delivery.saved_digest(Some(sent.clone())), Some(old));
        run(&mut delivery, &mut failed_at, &settings(), at(60), true);
        assert_eq!(delivery.saved_digest(Some(sent.clone())), Some(sent));
    }
}
