//! Golden tests that pin every date and time format Margin reads or writes.
//!
//! Each test renders a table of inputs and results and compares it with a file
//! in `fixtures/golden`. The files were written by the chrono-based code, so a
//! different date-time library must reproduce them byte for byte. Rewrite them
//! with `MARGIN_BLESS_GOLDEN=1` only when a format change is intended.
//! Only `support` may name the date-time library.

pub(crate) mod support;

use super::{
    digest::{self, AccountInput, DigestState},
    history::{HistoryPoint, HistoryStore},
    model::{Account, Provider, Snapshot, UsageWindow},
    notifications::{self, Alert, AlertKind, Clock12h},
    providers, store,
    switch::{self, SwitchRecord},
    tray,
};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::path::PathBuf;
use support::{instant, plus_seconds, show, zone};

pub(crate) fn golden(name: &str, actual: &str) {
    let actual = support::unix_folders(actual);
    let actual = actual.as_str();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("MARGIN_BLESS_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing golden file {}", path.display()));
    if expected != actual {
        let first = expected
            .lines()
            .zip(actual.lines())
            .position(|(left, right)| left != right)
            .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()));
        panic!(
            "{name} differs from its golden file at line {}:\nexpected: {:?}\nactual:   {:?}",
            first + 1,
            expected.lines().nth(first),
            actual.lines().nth(first)
        );
    }
}

/// Text timestamps as providers, stored files and old versions wrote them,
/// plus the edges of the RFC 3339 grammar.
pub(crate) const TEXT_TIMESTAMPS: &[&str] = &[
    "2026-10-16T12:00:00Z",
    "2026-10-16T12:00:00+00:00",
    "2026-10-16T12:00:00-00:00",
    "2026-10-16T12:00:00z",
    "2026-10-16t12:00:00Z",
    "2026-10-16 12:00:00Z",
    "2026-10-16T17:30:00+05:30",
    "2026-10-16T07:00:00-05:00",
    "2026-10-16T12:00:00+23:59",
    "2026-10-16T12:00:00-23:59",
    "2026-10-16T12:00:00+24:00",
    "2026-10-16T12:00:00+05:60",
    "2026-10-16T12:00:00+0500",
    "2026-10-16T12:00:00+05",
    "2026-10-16T12:00:00\u{2212}05:00",
    "2026-10-16T12:00:00.5Z",
    "2026-10-16T12:00:00.500Z",
    "2026-10-16T12:00:00.123Z",
    "2026-10-16T12:00:00.123456Z",
    "2026-10-16T12:00:00.123456789Z",
    "2026-10-16T12:00:00.1234567891Z",
    "2026-10-16T12:00:00.123456789123456789+01:00",
    "2026-10-16T12:00:00.000Z",
    "2026-10-16T12:00:00.000001Z",
    "2026-10-16T12:00:00.000000001Z",
    "2026-10-16T12:00:00.999999999Z",
    "2026-10-16T12:00:00.5-00:00",
    "2026-10-16T12:00:00.Z",
    "2026-10-16T12:00:00,5Z",
    "2026-10-16T12:00:00",
    "2026-10-16T12:00Z",
    "2026-10-16",
    "2026-10-16T24:00:00Z",
    "2026-10-16T12:60:00Z",
    "2026-10-16T23:59:60Z",
    "2026-10-16T23:59:60.5Z",
    "2026-10-16T12:30:60+01:00",
    "2026-10-16T12:00:61Z",
    "2026-02-29T00:00:00Z",
    "2024-02-29T00:00:00Z",
    "2026-13-01T00:00:00Z",
    "2026-00-10T00:00:00Z",
    "2026-10-00T00:00:00Z",
    "2026-1-16T12:00:00Z",
    "26-10-16T12:00:00Z",
    "+2026-10-16T12:00:00Z",
    "+002026-10-16T12:00:00Z",
    " 2026-10-16T12:00:00Z",
    "2026-10-16T12:00:00Z ",
    "2026-10-16T12:00:00ZZ",
    "2026-10-16T12:00:00Z[UTC]",
    "2026-10-16T12:00:00+05:30[Asia/Kolkata]",
    "2026-10-16T12:00:00UTC",
    "2026-10-16T12:00:00 Z",
    "2026-10-16T12:00:00+05:30:00",
    "",
    "now",
    "1792180800",
    "0000-01-01T00:00:00Z",
    "0000-01-01T00:00:00+00:01",
    "0000-01-01T00:00:00-00:01",
    "0001-01-01T00:00:00Z",
    "1969-12-31T23:59:59.999999999Z",
    "1970-01-01T00:00:00Z",
    "9999-12-30T21:59:59Z",
    "9999-12-30T22:00:00Z",
    "9999-12-31T23:59:59Z",
    "9999-12-31T23:59:59.999999999Z",
    "9999-12-31T23:59:59+00:01",
    "9999-12-31T23:59:59-00:01",
    "2026-03-08T02:30:00-05:00",
    "2026-11-01T01:30:00-04:00",
    "2026-11-01T01:30:00-05:00",
    "2026-10-16T12:00:00.123456789123456789123456789Z",
];

fn result<T: std::fmt::Debug, E>(value: Result<T, E>) -> String {
    match value {
        Ok(value) => format!("{value:?}"),
        Err(_) => "error".into(),
    }
}

fn account(value: Value) -> Account {
    serde_json::from_value(value).unwrap()
}

fn snapshot_account(fetched_at: &str, windows: Value) -> Account {
    account(json!({
        "id": "codex-golden", "provider": "codex", "label": "Work", "color": "#336699",
        "source": { "kind": "configDir", "path": support::folder("codex") },
        "identity": { "email": "work@example.com", "plan": "Pro" },
        "snapshot": { "windows": windows, "fetchedAt": fetched_at, "extraUsage": null },
        "error": null, "refreshing": false
    }))
}

#[test]
fn text_timestamps_parse_like_chrono() {
    let now = instant("2026-10-16T12:00:00Z");
    let mut out = String::new();
    for text in TEXT_TIMESTAMPS {
        let provider = result(providers::timestamp(Some(&json!(text))));
        let state = serde_json::from_value::<DigestState>(json!({ "lastSentAt": text }))
            .ok()
            .map(|state| {
                format!(
                    "{} {:?}",
                    result(state.validate()),
                    state.last_sent_at().map(show)
                )
            });
        let window = UsageWindow {
            id: "five_hour".into(),
            label: "Session".into(),
            used_percent: 10.0,
            resets_at: Some((*text).to_owned()),
            window_seconds: Some(18_000),
        };
        let snapshot = Snapshot {
            windows: vec![window.clone()],
            fetched_at: "2026-10-16T11:00:00Z".into(),
            extra_usage: None,
            reset_grants: None,
            balance: None,
            incomplete: false,
        };
        let fetched = Snapshot {
            windows: Vec::new(),
            fetched_at: (*text).to_owned(),
            ..snapshot.clone()
        };
        let stale_account = snapshot_account(text, json!([]));
        let settings = super::settings::Settings::default();
        writeln!(
            out,
            "{text:?}\n  provider={provider}\n  store={}\n  digest={state:?}\n  unread={} unread_fetched={}\n  age={:?} stale={}\n  update_due={}",
            store::valid_timestamp(text),
            snapshot.unread_since_reset(&window, now),
            fetched.unread_since_reset(&UsageWindow {
                resets_at: Some("2026-10-16T11:30:00Z".into()),
                ..window.clone()
            }, now),
            tray::relative_age(text, now),
            tray::stale(&stale_account, &settings, now),
            crate::updates::due(Some(text), now),
        )
        .unwrap();
    }
    golden("text-timestamps", &out);
}

#[test]
fn numeric_and_odd_provider_timestamps() {
    let mut out = String::new();
    for value in [
        json!(0),
        json!(-1),
        json!(1_792_180_800),
        json!(1_792_180_800_000_i64),
        json!(253_402_214_399_i64),
        json!(253_402_214_400_i64),
        json!(253_402_300_799_i64),
        json!(253_402_300_800_i64),
        json!(-62_167_219_200_i64),
        json!(-62_167_219_201_i64),
        json!(-377_705_116_800_i64),
        json!(i64::MAX),
        json!(i64::MIN),
        json!(u64::MAX),
        json!(1.5),
        json!(1_792_180_800.0),
        json!("1792180800"),
        json!(true),
        json!([]),
        json!({}),
        Value::Null,
    ] {
        writeln!(
            out,
            "{value} => {}",
            result(providers::timestamp(Some(&value)))
        )
        .unwrap();
    }
    writeln!(out, "missing => {}", result(providers::timestamp(None))).unwrap();
    golden("provider-numeric-timestamps", &out);
}

#[test]
fn retry_after_reads_seconds_and_http_dates() {
    let now = instant("2026-10-21T07:00:00Z");
    let mut out = String::new();
    for value in [
        "120",
        "0",
        "1",
        "86400",
        "999999",
        " 30 ",
        "-5",
        "1.5",
        "",
        "garbage",
        "Wed, 21 Oct 2026 07:28:00 GMT",
        "Wed, 21 Oct 2026 07:28:00 UT",
        "Wed, 21 Oct 2026 07:28:00 UTC",
        "Wed, 21 Oct 2026 07:28:00 +0000",
        "Wed, 21 Oct 2026 09:28:00 +0200",
        "Wed, 21 Oct 2026 03:28:00 EDT",
        "Wed, 21 Oct 2026 02:28:00 EST",
        "Wed, 21 Oct 2026 00:28:00 PDT",
        "Wed, 21 Oct 2026 07:28:00 Z",
        "Wed, 21 Oct 2026 07:28:00 A",
        "Wed, 21 Oct 2026 07:28:00 -0000",
        "Wed, 21 Oct 2026 07:28:00",
        "21 Oct 2026 07:28:00 GMT",
        "Wed, 21 Oct 2026 07:28 GMT",
        "Wed, 21 Oct 26 07:28:00 GMT",
        "Wed, 21 Oct 126 07:28:00 GMT",
        "Thu, 21 Oct 2026 07:28:00 GMT",
        "wed, 21 oct 2026 07:28:00 gmt",
        "WED, 21 OCT 2026 07:28:00 GMT",
        "Wed,  21 Oct 2026 07:28:00 GMT",
        "Wed, 1 Oct 2026 07:28:00 GMT",
        "Wed, 21 Oct 2026 07:28:60 GMT",
        "Wednesday, 21-Oct-26 07:28:00 GMT",
        "Wed Oct 21 07:28:00 2026",
        "Wed, 21 Oct 2026 06:00:00 GMT",
        "Wed, 21 Oct 2026 07:00:00 GMT",
        "Wed, 21 Oct 2026 07:00:01 GMT",
        "Wed, 21 Oct 2026 07:00:00.5 GMT",
        "Sat, 31 Oct 2026 07:00:00 GMT",
        "Fri, 31 Dec 9999 23:59:59 GMT",
        "2026-10-21T07:28:00Z",
    ] {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            reqwest::header::HeaderValue::from_str(value).unwrap(),
        );
        writeln!(
            out,
            "{value:?} => {:?}",
            providers::retry_after(&headers, now)
        )
        .unwrap();
    }
    golden("retry-after", &out);
}

/// Instants with every fractional precision the formatters distinguish.
const INSTANTS: &[&str] = &[
    "2026-10-16T12:00:00Z",
    "2026-10-16T12:00:00.5Z",
    "2026-10-16T12:00:00.25Z",
    "2026-10-16T12:00:00.123Z",
    "2026-10-16T12:00:00.1234Z",
    "2026-10-16T12:00:00.123456Z",
    "2026-10-16T12:00:00.1234567Z",
    "2026-10-16T12:00:00.123456789Z",
    "2026-10-16T12:00:00.000001Z",
    "2026-10-16T12:00:00.000000001Z",
    "2026-10-16T12:00:00.999999999Z",
    "2026-10-16T23:59:59.999Z",
    "2024-02-29T00:00:00Z",
    "1970-01-01T00:00:00Z",
    "1969-12-31T23:59:59.5Z",
    "0001-01-01T00:00:00Z",
    "9999-12-30T21:59:59.123Z",
];

#[test]
fn written_timestamps_keep_their_format() {
    let mut out = String::new();
    for text in INSTANTS {
        let now = instant(text);
        let state = DigestState::enabled_at(now);
        let report_stamp = SwitchRecord::restorable(
            &serde_json::from_value(json!({
                "id": "x", "provider": "claude", "createdAt": text, "restore": false,
                "state": "started", "items": [], "locks": [], "live": []
            }))
            .unwrap(),
            now,
        );
        writeln!(
            out,
            "{text}\n  rfc3339={}\n  json={}\n  backup={}\n  record={}\n  aside={}\n  restorable={report_stamp}",
            state.last_sent_at,
            serde_json::to_string(&state).unwrap(),
            store::backup_stamp(now),
            switch::record_stamp(now),
            switch::aside_stamp(now),
        )
        .unwrap();
    }
    golden("written-timestamps", &out);
}

#[test]
fn backup_names_sort_like_before() {
    let mut out = String::new();
    for suffix in [
        "20261016T120000Z",
        "20261016T120000Z-1",
        "20261016T120000Z-99",
        "20261016T120000Z-0",
        "20261016T120000Z-x",
        "20261016T120000Z-",
        "20261016T235960Z",
        "20261016T240000Z",
        "20260229T120000Z",
        "20240229T120000Z",
        "20261316T120000Z",
        "20261016T120000",
        "20261016T120000z",
        "20261016t120000Z",
        "2026101T6120000Z",
        "+0261016T120000Z",
        "-0261016T120000Z",
        " 0261016T120000Z",
        "20261016T12000 Z",
        "00001016T120000Z",
        "99991231T235959Z",
        "20261016T120000.000Z",
        "",
    ] {
        writeln!(out, "{suffix:?} => {:?}", store::backup_order(suffix)).unwrap();
    }
    golden("backup-names", &out);
}

#[test]
fn switch_records_restore_within_thirty_minutes() {
    let now = instant("2026-10-16T12:00:00Z");
    let mut out = String::new();
    for created in [
        "2026-10-16T12:00:00Z",
        "2026-10-16T11:59:59.5Z",
        "2026-10-16T11:30:00.000000001Z",
        "2026-10-16T11:30:00Z",
        "2026-10-16T11:29:59Z",
        "2026-10-16T12:00:01Z",
        "2026-10-16T11:59:00.000000001Z",
        "2026-10-16T17:15:00+05:15",
        "not a time",
    ] {
        let record: SwitchRecord = serde_json::from_value(json!({
            "id": "x", "provider": "codex", "createdAt": created, "restore": false,
            "state": "verifyFailed", "items": [], "locks": [], "live": []
        }))
        .unwrap();
        writeln!(out, "{created} => {}", record.restorable(now)).unwrap();
    }
    golden("switch-restorable", &out);
}

#[test]
fn reset_clocks_in_zones() {
    let pairs = [
        ("2026-10-16T12:00:00Z", "2026-10-16T13:45:00Z"),
        ("2026-10-16T12:00:00Z", "2026-10-16T23:30:00Z"),
        ("2026-10-16T12:00:00Z", "2026-10-17T03:30:00Z"),
        ("2026-10-16T12:00:00Z", "2026-10-21T12:00:00Z"),
        ("2026-10-16T12:00:00Z", "2026-10-22T11:59:59Z"),
        ("2026-10-16T12:00:00Z", "2026-10-22T12:00:00Z"),
        ("2026-10-16T12:00:00Z", "2026-10-10T12:00:00Z"),
        ("2026-10-16T12:00:00Z", "2026-10-16T00:05:00Z"),
        ("2026-03-07T20:00:00Z", "2026-03-08T06:59:59Z"),
        ("2026-03-07T20:00:00Z", "2026-03-08T07:00:00Z"),
        ("2026-03-07T20:00:00Z", "2026-03-08T07:30:00Z"),
        ("2026-03-07T20:00:00Z", "2026-03-08T17:00:00Z"),
        ("2026-10-31T20:00:00Z", "2026-11-01T05:30:00Z"),
        ("2026-10-31T20:00:00Z", "2026-11-01T06:30:00Z"),
        ("2026-10-31T20:00:00Z", "2026-11-01T07:30:00Z"),
        ("2026-10-31T20:00:00Z", "2026-11-07T07:30:00Z"),
        ("2026-12-31T22:00:00Z", "2027-01-01T00:30:00Z"),
        ("2026-12-31T22:00:00Z", "2027-01-01T05:30:00Z"),
        ("2026-12-31T22:00:00Z", "2027-02-01T12:00:00Z"),
        ("2026-10-16T12:00:00Z", "2026-10-16T12:00:00.999Z"),
    ];
    let mut out = String::new();
    for name in ["America/New_York", "Asia/Tashkent", "UTC"] {
        let zone = zone(name);
        writeln!(out, "{name}").unwrap();
        for (now, at) in pairs {
            let (now, at) = (instant(now), instant(at));
            writeln!(
                out,
                "  now={} at={} => {:?} / {:?}",
                show(now),
                show(at),
                notifications::clock_in(&zone, Clock12h(false), at, now),
                notifications::clock_in(&zone, Clock12h(true), at, now),
            )
            .unwrap();
        }
    }
    golden("reset-clocks", &out);
}

#[test]
fn alert_copy_in_zones() {
    let now = instant("2026-10-31T22:40:00Z");
    let alerts = [
        Alert {
            window_id: "five_hour".into(),
            window_label: "Session".into(),
            kind: AlertKind::Threshold(80),
            resets_at: Some("2026-11-01T05:30:00.250Z".into()),
            used_percent: 81.5,
            runs_out: None,
        },
        Alert {
            window_id: "seven_day".into(),
            window_label: "Weekly".into(),
            kind: AlertKind::RunsOut,
            resets_at: Some("2026-11-03T14:00:00+00:00".into()),
            used_percent: 64.0,
            runs_out: Some(instant("2026-11-01T06:10:00Z")),
        },
        Alert {
            window_id: "seven_day".into(),
            window_label: "Weekly".into(),
            kind: AlertKind::Burn { increase: 12 },
            resets_at: Some("2026-11-09T14:00:00-05:00".into()),
            used_percent: 40.0,
            runs_out: Some(instant("2026-11-02T01:00:00Z")),
        },
        Alert {
            window_id: "five_hour".into(),
            window_label: "Session".into(),
            kind: AlertKind::Reset,
            resets_at: Some("2026-10-31T22:40:30Z".into()),
            used_percent: 0.0,
            runs_out: None,
        },
        Alert {
            window_id: "five_hour".into(),
            window_label: "Session".into(),
            kind: AlertKind::Threshold(100),
            resets_at: Some("bad".into()),
            used_percent: 100.0,
            runs_out: None,
        },
    ];
    let mut out = String::new();
    for name in ["America/New_York", "Asia/Tashkent", "UTC"] {
        let zone = zone(name);
        writeln!(out, "{name}").unwrap();
        for alert in &alerts {
            for clock in [Clock12h(false), Clock12h(true)] {
                writeln!(
                    out,
                    "  {:?}\n    {:?}",
                    alert.title_in(&zone, clock, "Work", now),
                    alert.body_in(&zone, clock, now)
                )
                .unwrap();
            }
        }
    }
    golden("alert-copy", &out);
}

#[test]
fn digest_slots_in_zones() {
    let nows = [
        "2026-03-09T12:59:59Z",
        "2026-03-09T13:00:00Z",
        "2026-03-09T14:00:00Z",
        "2026-03-08T12:00:00Z",
        "2026-03-02T14:00:00Z",
        "2026-11-02T13:59:59Z",
        "2026-11-02T14:00:00Z",
        "2026-11-01T06:30:00Z",
        "2026-10-26T13:00:00Z",
        "2026-10-26T04:00:00Z",
        "2026-10-26T03:59:59Z",
        "2026-10-25T23:00:00Z",
        "2026-12-28T09:00:00Z",
        "2027-01-04T08:59:59.999Z",
        "2027-01-04T09:00:00Z",
    ];
    let lasts = [
        None,
        Some("2026-03-02T14:00:00Z"),
        Some("2026-10-26T13:00:00Z"),
        Some("2026-10-27T13:00:00Z"),
        Some("2026-10-20T13:00:00Z"),
    ];
    let mut out = String::new();
    for name in ["America/New_York", "Asia/Tashkent", "UTC"] {
        let zone = zone(name);
        writeln!(out, "{name}").unwrap();
        for now in nows {
            let now = instant(now);
            write!(
                out,
                "  now={} due={}",
                show(now),
                show(digest::due_at(now, &zone))
            )
            .unwrap();
            for last in lasts {
                let sent = digest::should_send(last.map(instant), now, &zone);
                write!(out, " | {}", sent.map_or("-".into(), show)).unwrap();
            }
            writeln!(out).unwrap();
        }
    }
    golden("digest-slots", &out);
}

/// A week of readings that crosses the end of US daylight time and several
/// local midnights that are not UTC midnights.
fn digest_points() -> Vec<HistoryPoint> {
    let start = instant("2026-10-26T14:00:00Z");
    let mut points = Vec::new();
    for hour in 0..168_i64 {
        let at = plus_seconds(start, hour * 3600);
        let period = hour / 5;
        let into = hour % 5;
        let used = if period % 4 == 0 {
            (into as f64 * 30.0).min(100.0)
        } else {
            into as f64 * 12.5
        };
        let session_reset = plus_seconds(start, (period + 1) * 5 * 3600);
        points.push(HistoryPoint {
            window_id: "five_hour".into(),
            at: show(at),
            used_percent: used,
            resets_at: Some(show(session_reset)),
            window_seconds: Some(18_000),
            gap_before: hour == 0 || hour == 90,
        });
        points.push(HistoryPoint {
            window_id: "seven_day".into(),
            at: show(at),
            used_percent: (10.0 + hour as f64 * 0.6).min(100.0),
            resets_at: Some("2026-11-02T14:00:00Z".into()),
            window_seconds: Some(604_800),
            gap_before: hour == 0,
        });
    }
    points
}

#[test]
fn digest_weeks_in_zones() {
    let points = digest_points();
    let short: Vec<HistoryPoint> = points[..60].to_vec();
    let inputs = [
        AccountInput {
            id: "claude-1",
            label: "Personal",
            provider: Provider::Claude,
            points: &points,
        },
        AccountInput {
            id: "codex-1",
            label: "Work",
            provider: Provider::Codex,
            points: &short,
        },
    ];
    let previous: digest::WeeklySummary = serde_json::from_value(json!({
        "from": "2026-10-19T13:00:00+00:00",
        "to": "2026-10-26T13:00:00.250+00:00",
        "accounts": [{ "id": "claude-1", "weeklyPeak": 37.5, "sessionHits": 1, "sessionPeriods": 9 }]
    }))
    .unwrap();
    let mut out = String::new();
    for name in ["America/New_York", "Asia/Tashkent", "UTC"] {
        let zone = zone(name);
        for now in ["2026-11-02T14:00:00.250Z", "2026-11-02T14:00:00Z"] {
            let digest = digest::digest(&inputs, Some(&previous), instant(now), &zone).unwrap();
            writeln!(
                out,
                "{name} {now}\n  title={:?}\n  body={:?}\n  summary={}\n  coverage={:?}",
                digest.title(),
                digest.body(),
                serde_json::to_string(&digest.summary()).unwrap(),
                digest
                    .accounts
                    .iter()
                    .map(|account| account.coverage_hours)
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
    }
    golden("digest-weeks", &out);
}

#[test]
fn history_files_and_payloads() {
    let directory = tempfile::tempdir().unwrap();
    let old = json!({
        "version": 1,
        "provider": "codex",
        "source": { "kind": "configDir", "path": support::folder("codex") },
        "identity": { "email": "work@example.com", "plan": "Pro" },
        "points": [
            { "windowId": "five_hour", "at": "2026-10-01T10:00:00+00:00", "usedPercent": 5.0, "resetsAt": "2026-10-01T15:00:00+00:00", "windowSeconds": 18000, "gapBefore": true },
            { "windowId": "five_hour", "at": "2026-10-08T09:00:00.123456789+00:00", "usedPercent": 5.0, "resetsAt": "2026-10-08T15:00:00Z", "windowSeconds": 18000, "gapBefore": true },
            { "windowId": "five_hour", "at": "2026-10-09T12:00:00.5+02:00", "usedPercent": 5.0, "resetsAt": "2026-10-09T15:00:00Z", "windowSeconds": 18000, "gapBefore": false },
            { "windowId": "seven_day", "at": "2026-10-15T11:00:00Z", "usedPercent": 40.0, "resetsAt": "2026-10-20T00:00:00-04:00", "windowSeconds": 604800, "gapBefore": true },
            { "windowId": "seven_day", "at": "2026-10-16T11:58:00Z", "usedPercent": 41.0, "resetsAt": "2026-10-20T04:00:30Z", "windowSeconds": 604800, "gapBefore": false }
        ]
    });
    std::fs::write(
        directory.path().join("codex-golden.json"),
        serde_json::to_string(&old).unwrap(),
    )
    .unwrap();
    let mut store = HistoryStore::new(directory.path().into());
    let now = instant("2026-10-16T12:00:00.250Z");
    let current = snapshot_account(
        "2026-10-16T12:00:00Z",
        json!([
            { "id": "five_hour", "label": "Session", "usedPercent": 12.0, "resetsAt": "2026-10-16T15:00:00Z", "windowSeconds": 18000 },
            { "id": "seven_day", "label": "Weekly", "usedPercent": 42.0, "resetsAt": "2026-10-20T04:00:00Z", "windowSeconds": 604800 }
        ]),
    );
    let mut out = String::new();
    writeln!(
        out,
        "get={}",
        serde_json::to_string(&store.get(&current, now)).unwrap()
    )
    .unwrap();
    store.record(&current, 60, now);
    writeln!(
        out,
        "recent={}",
        serde_json::to_string(&store.recent(
            &current,
            "seven_day",
            instant("2026-10-16T11:59:00Z")
        ))
        .unwrap()
    )
    .unwrap();
    writeln!(
        out,
        "file={}",
        std::fs::read_to_string(directory.path().join("codex-golden.json")).unwrap()
    )
    .unwrap();
    golden("history", &out);
}

#[test]
fn windows_folders_read_as_unix_in_golden_text() {
    let stored = serde_json::to_string(r"C:\tmp\golden-codex").unwrap();
    assert_eq!(support::unix_folders(&stored), "\"/tmp/golden-codex\"");
}
