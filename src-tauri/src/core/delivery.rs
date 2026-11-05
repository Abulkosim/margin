use super::{
    model::{Account, UsageError},
    notifications::{Alert, AlertKind, Clock12h},
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
    },
    NotUpdating {
        label: String,
        error: UsageError,
        last: DateTime<Utc>,
    },
    Digest {
        title: String,
        body: String,
    },
}

impl Message {
    fn render(&self, now: DateTime<Utc>, clock: Clock12h) -> (String, Option<String>) {
        match self {
            Self::Alert { label, alert } => {
                (alert.title(label, now, clock), alert.body(now, clock))
            }
            Self::NotUpdating { label, error, last } => (
                outage::title(label),
                Some(outage::body(error, *last, now, clock)),
            ),
            Self::Digest { title, body } => (title.clone(), Some(body.clone())),
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

/// Notifications waiting to be submitted, in memory. Alert memory records that an alert was
/// raised, so it is never raised twice; this queue is what gets it to the OS. A retry still
/// waiting when Margin quits is lost, and alert memory will not raise it again.
#[derive(Debug, Default)]
pub struct Delivery {
    pending: Vec<Pending>,
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

    /// Submits what is due. `failed_at` holds the latest failure until the next success. Returns
    /// whether it changed.
    pub fn attempt(
        &mut self,
        failed_at: &mut Option<String>,
        settings: &Settings,
        accounts: &[Account],
        now: DateTime<Utc>,
        clock: Clock12h,
        mut submit: impl FnMut(&str, Option<&str>) -> bool,
    ) -> bool {
        let allowed = settings.notifications
            && settings.notification_permission == NotificationPermission::Granted;
        self.pending.retain(|pending| {
            let account = pending
                .account_id
                .as_deref()
                .and_then(|id| accounts.iter().find(|account| account.id == id));
            allowed && pending.message.still_wanted(settings, account, now)
        });
        let before = failed_at.clone();
        self.pending.retain_mut(|pending| {
            if pending.due > now {
                return true;
            }
            let (title, body) = pending.message.render(now, clock);
            if submit(&title, body.as_deref()) {
                *failed_at = None;
                return false;
            }
            *failed_at = Some(now.to_rfc3339());
            let Some(delay) = RETRY_DELAYS_SECONDS.get(pending.retries) else {
                return false;
            };
            pending.retries += 1;
            pending.due = now + Duration::seconds(*delay);
            true
        });
        before != *failed_at
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
        assert!(changed);
        assert_eq!(failed_at, None);
        assert!(!delivery.attempt(
            &mut failed_at,
            &settings(),
            &[account("a")],
            at(1),
            Clock12h(false),
            |_, _| true,
        ));
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
}
