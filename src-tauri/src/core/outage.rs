use super::{
    model::{Account, UsageError},
    notifications::{reset_clock, Clock12h},
};
use chrono::{DateTime, Duration, Utc};

/// How long an account stays unreadable before Margin says its alerts cannot fire.
const AFTER_MINUTES: i64 = 30;

#[derive(Debug, Default)]
pub struct Outage {
    since: Option<DateTime<Utc>>,
    failed: Option<DateTime<Utc>>,
    alerted: bool,
}

/// Errors that hide usage while the account can still be used. A token that expired on disk is
/// refreshed the next time its CLI runs.
pub fn hides_usage(error: &UsageError) -> bool {
    error.kind != "signedOut" || error.signed_out_since.is_none()
}

/// The newest reading from an account on another usage host that is reading fine right now.
/// Providers stand in for hosts: two Codex accounts with different `chatgpt_base_url` values count
/// as one host, which can only keep Margin quiet.
pub fn connection_proof(accounts: &[Account], account: &Account) -> Option<DateTime<Utc>> {
    accounts
        .iter()
        .filter(|other| other.provider != account.provider && other.error.is_none())
        .filter_map(|other| other.snapshot.as_ref())
        .filter_map(|snapshot| DateTime::parse_from_rfc3339(&snapshot.fetched_at).ok())
        .map(|at| at.with_timezone(&Utc))
        .max()
}

impl Outage {
    /// A request for this account just failed.
    pub fn failed(&mut self, now: DateTime<Utc>) {
        self.failed = Some(now);
    }

    /// The last reading's time, once per outage, after the account has failed for 30 minutes
    /// and its last reading is at least that old.
    ///
    /// A `network` error looks the same whether the usage host is down or the computer is offline.
    /// It counts only while `proof` shows the connection works: its clock starts at a failure seen
    /// while another host reads fine, and it alerts only after that host has succeeded during the
    /// failure. Without proof the clock stops, as it does during sleep. So with accounts on one
    /// host only, or when every host fails at once, a `network` failure stays silent.
    pub fn observe(
        &mut self,
        error: Option<&UsageError>,
        last_reading: Option<&str>,
        proof: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Option<DateTime<Utc>> {
        let Some(error) = error.filter(|error| hides_usage(error)) else {
            *self = Self::default();
            return None;
        };
        if error.kind == "network" {
            let Some(proof) = proof else {
                self.restart();
                return None;
            };
            let since = match self.since {
                Some(since) => since,
                None => *self.since.insert(self.failed?),
            };
            if proof <= since {
                return None;
            }
        }
        let since = *self.since.get_or_insert(now);
        let last = last_reading
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())?
            .with_timezone(&Utc);
        let after = Duration::minutes(AFTER_MINUTES);
        if self.alerted || now - since < after || now - last < after {
            return None;
        }
        self.alerted = true;
        Some(last)
    }

    /// Time spent asleep or offline is not time spent failing, and a failure from before it
    /// says nothing about the connection now.
    pub fn restart(&mut self) {
        self.since = None;
        self.failed = None;
    }
}

pub fn title(account_label: &str) -> String {
    format!("{account_label} · Usage not updating")
}

pub fn body(
    error: &UsageError,
    last: DateTime<Utc>,
    now: DateTime<Utc>,
    clock: Clock12h,
) -> String {
    let reading = format!("Last reading {}", reset_clock(last, now, clock));
    let until = error
        .retry_at
        .and_then(DateTime::from_timestamp_millis)
        .filter(|at| *at > now);
    match (error.kind.as_str(), until) {
        ("rateLimited", Some(until)) => format!(
            "Rate limited until {} · {reading}",
            reset_clock(until, now, clock)
        ),
        ("rateLimited", None) => format!("Rate limited · {reading}"),
        ("signedOut", _) => format!("Signed out · {reading}"),
        ("network", _) => format!("Can't reach the usage server · {reading}"),
        _ => reading,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: i64) -> DateTime<Utc> {
        "2099-12-31T12:00:00Z".parse::<DateTime<Utc>>().unwrap() + Duration::minutes(minutes)
    }

    fn limited() -> UsageError {
        UsageError::new("rateLimited", "Slow down")
    }

    #[test]
    fn alerts_once_after_half_an_hour_of_failures() {
        let mut outage = Outage::default();
        let last = at(-1).to_rfc3339();
        let error = limited();
        assert_eq!(outage.observe(Some(&error), Some(&last), None, at(0)), None);
        assert_eq!(
            outage.observe(Some(&error), Some(&last), None, at(29)),
            None
        );
        assert_eq!(
            outage.observe(Some(&error), Some(&last), None, at(30)),
            Some(at(-1))
        );
        assert_eq!(
            outage.observe(Some(&error), Some(&last), None, at(90)),
            None
        );
        let fresh = at(91).to_rfc3339();
        assert_eq!(outage.observe(None, Some(&fresh), None, at(91)), None);
        assert_eq!(
            outage.observe(Some(&error), Some(&fresh), None, at(92)),
            None
        );
        assert_eq!(
            outage.observe(Some(&error), Some(&fresh), None, at(122)),
            Some(at(91))
        );
    }

    #[test]
    fn needs_a_reading_that_is_also_old() {
        let mut outage = Outage::default();
        let error = limited();
        assert_eq!(outage.observe(Some(&error), None, None, at(0)), None);
        assert_eq!(outage.observe(Some(&error), None, None, at(60)), None);
        let recent = at(40).to_rfc3339();
        assert_eq!(
            outage.observe(Some(&error), Some(&recent), None, at(60)),
            None
        );
        assert_eq!(
            outage.observe(Some(&error), Some(&recent), None, at(70)),
            Some(at(40))
        );
    }

    #[test]
    fn expired_tokens_never_alert() {
        let last = at(-600).to_rfc3339();
        let expired = UsageError {
            signed_out_since: Some(at(-300).to_rfc3339()),
            ..UsageError::new("signedOut", "Signed out")
        };
        let mut outage = Outage::default();
        outage.failed(at(0));
        assert_eq!(
            outage.observe(Some(&expired), Some(&last), Some(at(1)), at(0)),
            None
        );
        assert_eq!(
            outage.observe(Some(&expired), Some(&last), Some(at(599)), at(600)),
            None
        );
        let rejected = UsageError::new("signedOut", "Signed out");
        let mut outage = Outage::default();
        outage.observe(Some(&rejected), Some(&last), None, at(0));
        assert!(outage
            .observe(Some(&rejected), Some(&last), None, at(30))
            .is_some());
    }

    fn network() -> UsageError {
        UsageError::new("network", "Could not reach the provider")
    }

    /// A check at `minute`, with `proof` as the other host's newest reading.
    fn network_tick(outage: &mut Outage, minute: i64, proof: Option<i64>) -> Option<DateTime<Utc>> {
        let last = at(-600).to_rfc3339();
        outage.observe(Some(&network()), Some(&last), proof.map(at), at(minute))
    }

    /// The same check right after a failed request.
    fn network_poll(outage: &mut Outage, minute: i64, proof: Option<i64>) -> Option<DateTime<Utc>> {
        outage.failed(at(minute));
        network_tick(outage, minute, proof)
    }

    #[test]
    fn a_failure_of_only_one_host_alerts_once_after_half_an_hour() {
        let mut outage = Outage::default();
        assert_eq!(network_poll(&mut outage, 0, Some(-1)), None);
        for minute in 1..30 {
            assert_eq!(network_tick(&mut outage, minute, Some(minute)), None);
        }
        assert_eq!(network_tick(&mut outage, 30, Some(30)), Some(at(-600)));
        assert_eq!(network_tick(&mut outage, 31, Some(31)), None);
        assert_eq!(network_poll(&mut outage, 90, Some(90)), None);
    }

    #[test]
    fn recovery_clears_the_outage_and_a_later_one_alerts_again() {
        let mut outage = Outage::default();
        network_poll(&mut outage, 0, Some(-1));
        assert!(network_tick(&mut outage, 30, Some(30)).is_some());
        let fresh = at(40).to_rfc3339();
        assert_eq!(
            outage.observe(None, Some(&fresh), Some(at(40)), at(40)),
            None
        );
        assert_eq!(network_tick(&mut outage, 41, Some(41)), None);
        let mut failing = |minute: i64, polled: bool| {
            if polled {
                outage.failed(at(minute));
            }
            outage.observe(Some(&network()), Some(&fresh), Some(at(minute)), at(minute))
        };
        assert_eq!(failing(50, true), None);
        assert_eq!(failing(79, false), None);
        assert_eq!(failing(80, false), Some(at(40)));
    }

    #[test]
    fn one_host_only_stays_silent() {
        let mut outage = Outage::default();
        for minute in (0..600).step_by(10) {
            assert_eq!(network_poll(&mut outage, minute, None), None);
        }
    }

    #[test]
    fn the_other_host_must_succeed_during_the_failure() {
        let mut outage = Outage::default();
        assert_eq!(network_poll(&mut outage, 0, Some(-5)), None);
        assert_eq!(network_tick(&mut outage, 45, Some(-5)), None);
        assert_eq!(network_tick(&mut outage, 46, Some(46)), Some(at(-600)));
    }

    #[test]
    fn offline_time_stays_silent_and_coming_back_does_not_alert() {
        let mut outage = Outage::default();
        assert_eq!(network_poll(&mut outage, 0, Some(-1)), None);
        assert_eq!(network_tick(&mut outage, 1, Some(1)), None);
        for minute in 2..300 {
            assert_eq!(network_poll(&mut outage, minute, None), None);
        }
        for minute in 300..360 {
            assert_eq!(network_tick(&mut outage, minute, Some(minute)), None);
        }
        assert_eq!(network_poll(&mut outage, 360, Some(359)), None);
        assert_eq!(network_tick(&mut outage, 389, Some(389)), None);
        assert_eq!(network_tick(&mut outage, 390, Some(390)), Some(at(-600)));
    }

    #[test]
    fn sleep_discards_the_failure_from_before_it() {
        let mut outage = Outage::default();
        network_poll(&mut outage, 0, Some(-1));
        network_tick(&mut outage, 5, Some(5));
        outage.restart();
        assert_eq!(network_tick(&mut outage, 480, Some(5)), None);
        assert_eq!(network_tick(&mut outage, 481, Some(481)), None);
        assert_eq!(network_tick(&mut outage, 600, Some(600)), None);
        assert_eq!(network_poll(&mut outage, 601, Some(600)), None);
        assert_eq!(network_tick(&mut outage, 631, Some(631)), Some(at(-600)));
    }

    #[test]
    fn an_alert_already_sent_is_not_repeated_after_going_offline() {
        let mut outage = Outage::default();
        network_poll(&mut outage, 0, Some(-1));
        assert!(network_tick(&mut outage, 30, Some(30)).is_some());
        assert_eq!(network_poll(&mut outage, 40, None), None);
        assert_eq!(network_poll(&mut outage, 50, Some(49)), None);
        assert_eq!(network_tick(&mut outage, 90, Some(90)), None);
    }

    #[test]
    fn proof_comes_from_another_provider_that_reads_fine() {
        use crate::core::model::{AccountSource, Identity, Provider, Snapshot};
        let account = |id: &str, provider, minute: Option<i64>, failing: bool| Account {
            id: id.into(),
            provider,
            label: id.into(),
            color: "#ffffff".into(),
            source: AccountSource::Keychain,
            identity: Identity::default(),
            snapshot: minute.map(|minute| Snapshot {
                windows: Vec::new(),
                fetched_at: at(minute).to_rfc3339(),
                extra_usage: None,
                reset_grants: None,
                balance: None,
                incomplete: false,
            }),
            error: failing.then(network),
            refreshing: false,
        };
        let failing = account("a", Provider::Claude, Some(0), true);
        let same_host = account("b", Provider::Claude, Some(9), false);
        let also_failing = account("c", Provider::Codex, Some(9), true);
        let never_read = account("d", Provider::Codex, None, false);
        assert_eq!(
            connection_proof(
                &[failing.clone(), same_host, also_failing, never_read],
                &failing
            ),
            None
        );
        let older = account("e", Provider::Codex, Some(3), false);
        let newer = account("f", Provider::Codex, Some(7), false);
        assert_eq!(
            connection_proof(&[failing.clone(), older, newer], &failing),
            Some(at(7))
        );
    }

    #[test]
    fn waking_up_restarts_the_clock() {
        let mut outage = Outage::default();
        let last = at(-600).to_rfc3339();
        let error = UsageError::new("provider", "Unexpected response");
        outage.observe(Some(&error), Some(&last), None, at(0));
        outage.restart();
        assert_eq!(
            outage.observe(Some(&error), Some(&last), None, at(480)),
            None
        );
        assert!(outage
            .observe(Some(&error), Some(&last), None, at(510))
            .is_some());
    }

    #[test]
    fn copy_names_the_reason_and_the_last_reading() {
        let clock = Clock12h(false);
        let now = at(0);
        let last = at(-45);
        assert_eq!(title("Work"), "Work · Usage not updating");
        let local = |at: DateTime<Utc>| reset_clock(at, now, clock);
        let until = UsageError {
            retry_at: Some(at(50).timestamp_millis()),
            ..limited()
        };
        assert_eq!(
            body(&until, last, now, clock),
            format!(
                "Rate limited until {} · Last reading {}",
                local(at(50)),
                local(last)
            )
        );
        assert_eq!(
            body(&limited(), last, now, clock),
            format!("Rate limited · Last reading {}", local(last))
        );
        assert_eq!(
            body(&UsageError::new("signedOut", "x"), last, now, clock),
            format!("Signed out · Last reading {}", local(last))
        );
        assert_eq!(
            body(&network(), last, now, clock),
            format!(
                "Can't reach the usage server · Last reading {}",
                local(last)
            )
        );
        assert_eq!(
            body(&UsageError::new("provider", "x"), last, now, clock),
            format!("Last reading {}", local(last))
        );
    }
}
