use super::{
    model::UsageError,
    notifications::{reset_clock, Clock12h},
};
use chrono::{DateTime, Duration, Utc};

/// How long an account stays unreadable before Margin says its alerts cannot fire.
const AFTER_MINUTES: i64 = 30;

#[derive(Debug, Default)]
pub struct Outage {
    since: Option<DateTime<Utc>>,
    alerted: bool,
}

/// Errors that hide usage while the account can still be used. A token that expired on disk is
/// refreshed the next time its CLI runs, and a network failure stops the CLI too.
fn hides_usage(error: &UsageError) -> bool {
    match error.kind.as_str() {
        "network" => false,
        "signedOut" => error.signed_out_since.is_none(),
        _ => true,
    }
}

impl Outage {
    /// The last reading's time, once per outage, after the account has failed for 30 minutes
    /// and its last reading is at least that old.
    pub fn observe(
        &mut self,
        error: Option<&UsageError>,
        last_reading: Option<&str>,
        now: DateTime<Utc>,
    ) -> Option<DateTime<Utc>> {
        if !error.is_some_and(hides_usage) {
            *self = Self::default();
            return None;
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

    /// Time spent asleep is not time spent failing.
    pub fn restart(&mut self) {
        self.since = None;
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
        assert_eq!(outage.observe(Some(&error), Some(&last), at(0)), None);
        assert_eq!(outage.observe(Some(&error), Some(&last), at(29)), None);
        assert_eq!(
            outage.observe(Some(&error), Some(&last), at(30)),
            Some(at(-1))
        );
        assert_eq!(outage.observe(Some(&error), Some(&last), at(90)), None);
        let fresh = at(91).to_rfc3339();
        assert_eq!(outage.observe(None, Some(&fresh), at(91)), None);
        assert_eq!(outage.observe(Some(&error), Some(&fresh), at(92)), None);
        assert_eq!(
            outage.observe(Some(&error), Some(&fresh), at(122)),
            Some(at(91))
        );
    }

    #[test]
    fn needs_a_reading_that_is_also_old() {
        let mut outage = Outage::default();
        let error = limited();
        assert_eq!(outage.observe(Some(&error), None, at(0)), None);
        assert_eq!(outage.observe(Some(&error), None, at(60)), None);
        let recent = at(40).to_rfc3339();
        assert_eq!(outage.observe(Some(&error), Some(&recent), at(60)), None);
        assert_eq!(
            outage.observe(Some(&error), Some(&recent), at(70)),
            Some(at(40))
        );
    }

    #[test]
    fn expired_tokens_and_offline_never_alert() {
        let last = at(-600).to_rfc3339();
        let expired = UsageError {
            signed_out_since: Some(at(-300).to_rfc3339()),
            ..UsageError::new("signedOut", "Signed out")
        };
        for error in [UsageError::new("network", "Offline"), expired] {
            let mut outage = Outage::default();
            assert_eq!(outage.observe(Some(&error), Some(&last), at(0)), None);
            assert_eq!(outage.observe(Some(&error), Some(&last), at(600)), None);
        }
        let rejected = UsageError::new("signedOut", "Signed out");
        let mut outage = Outage::default();
        outage.observe(Some(&rejected), Some(&last), at(0));
        assert!(outage
            .observe(Some(&rejected), Some(&last), at(30))
            .is_some());
    }

    #[test]
    fn waking_up_restarts_the_clock() {
        let mut outage = Outage::default();
        let last = at(-600).to_rfc3339();
        let error = UsageError::new("provider", "Unexpected response");
        outage.observe(Some(&error), Some(&last), at(0));
        outage.restart();
        assert_eq!(outage.observe(Some(&error), Some(&last), at(480)), None);
        assert!(outage.observe(Some(&error), Some(&last), at(510)).is_some());
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
            body(&UsageError::new("provider", "x"), last, now, clock),
            format!("Last reading {}", local(last))
        );
    }
}
