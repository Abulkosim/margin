//! Pins `margin --json` (schema 1) and `margin --line` output at fixed clocks.
//! The expected text lives in `fixtures/golden`; see `core::time_golden`.

use super::*;
use crate::core::time_golden::{
    golden,
    support::{folder, instant},
};
use serde_json::{json, Value};
use std::fmt::Write as _;

fn state(accounts: Value) -> StoredState {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("state.json");
    let value = json!({ "version": 1, "accounts": accounts, "settings": {} });
    std::fs::write(&path, value.to_string()).unwrap();
    read_state(&path).unwrap().unwrap()
}

fn accounts() -> Value {
    let window = |id: &str, used: f64, reset: &str| json!({ "id": id, "label": id, "usedPercent": used, "resetsAt": reset, "windowSeconds": 18000 });
    let account = |provider: &str, id: &str, fetched: &str, windows: Value, error: Value| {
        json!({
            "id": id, "provider": provider, "label": id, "color": "#ffffff",
            "source": { "kind": "configDir", "path": folder(id) },
            "identity": { "email": format!("{id}@example.com"), "plan": "Pro" },
            "snapshot": if fetched.is_empty() { Value::Null } else {
                json!({ "windows": windows, "fetchedAt": fetched, "extraUsage": null })
            },
            "error": error, "refreshing": false
        })
    };
    json!([
        account(
            "claude",
            "fresh",
            "2026-10-26T10:00:00Z",
            json!([
                window("five_hour", 58.0, "2026-10-26T12:18:00Z"),
                window("seven_day", 14.0, "2026-10-28T10:00:00.500+00:00"),
                window("seven_day_opus", 33.3, "2026-10-29T05:00:00-05:00")
            ]),
            Value::Null
        ),
        account(
            "codex",
            "offset",
            "2026-10-26T14:59:30+05:00",
            json!([
                window("primary_window", 5.0, "2026-10-26T10:00:59.999Z"),
                window("secondary_window", 99.5, "2026-11-30T10:00:00Z")
            ]),
            Value::Null
        ),
        account(
            "claude",
            "fraction",
            "2026-10-26T09:59:59.123456789Z",
            json!([
                window("five_hour", 100.0, "2026-10-26T10:00:00.000000001Z"),
                window("seven_day", 0.0, "2026-10-27T09:00:00z")
            ]),
            Value::Null
        ),
        account(
            "codex",
            "old",
            "2026-10-26T09:40:00Z",
            json!([
                window("primary_window", 20.0, "2026-10-26T09:59:59Z"),
                window("secondary_window", 50.0, "2026-10-30T11:00:00Z")
            ]),
            Value::Null
        ),
        account(
            "claude",
            "idle",
            "2026-10-24T08:00:00Z",
            json!([window("five_hour", 10.0, "2026-10-24T12:00:00Z")]),
            json!({ "kind": "signedOut", "message": "Signed out", "retryAfterSeconds": null,
            "signedOutSince": "2026-10-24T09:00:00Z" })
        ),
        account(
            "codex",
            "leap",
            "2026-10-26T09:59:60Z",
            json!([
                window("primary_window", 30.0, "2026-10-26 13:00:00Z"),
                window("secondary_window", 70.0, "2026-10-31T23:59:60+01:00")
            ]),
            Value::Null
        ),
        account("claude", "pending", "", json!([]), Value::Null)
    ])
}

#[test]
fn json_and_lines_at_fixed_clocks() {
    let state = state(accounts());
    let mut out = String::new();
    for now in [
        "2026-10-26T10:00:00Z",
        "2026-10-26T10:00:00.999999999Z",
        "2026-10-26T10:00:59.5Z",
        "2026-10-26T13:07:00Z",
    ] {
        let now = instant(now);
        let (json, empty) = render(
            &Command::Json,
            Some(&state),
            || Some(true),
            now,
            &Env::plain(),
        )
        .unwrap();
        writeln!(out, "json empty={empty}\n{json}").unwrap();
        for all in [false, true] {
            let options = LineOptions {
                all,
                short: false,
                color: ColorChoice::Never,
            };
            let text = line(
                &state.accounts,
                &state.settings,
                options,
                &Env::plain(),
                now,
            );
            writeln!(out, "line all={all}\n{text}").unwrap();
        }
        let short = short_line(&state.accounts, &state.settings, true, Clock12h(false), now);
        writeln!(out, "short all\n{short}").unwrap();
        for reset in [
            "2026-10-26T10:00:30Z",
            "2026-10-26T10:59:59Z",
            "2026-10-26T19:59:00Z",
            "2026-10-27T10:00:00Z",
            "2026-10-27T11:30:00+01:00",
            "2026-11-05T09:00:00.5Z",
            "2026-10-26T09:00:00Z",
        ] {
            writeln!(out, "left {reset} => {:?}", time_left(reset, now)).unwrap();
        }
    }
    golden("cli", &out);
}
