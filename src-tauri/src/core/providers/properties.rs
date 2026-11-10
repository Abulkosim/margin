use super::{claude, codex, percent, retry_after, timestamp};
use crate::core::model::{Snapshot, UsageError, UsageWindow};
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use chrono::{DateTime, FixedOffset, Utc};
use proptest::prelude::*;
use proptest::sample::Index;
use proptest::test_runner::TestCaseError;
use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
use serde_json::{json, Map, Value};
use std::collections::HashSet;

const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
const VALID_RESET: &str = "2026-10-20T10:00:00Z";

static CLAUDE_USAGE: [&str; 9] = [
    include_str!("../../../fixtures/claude-all-windows.json"),
    include_str!("../../../fixtures/claude-live-schema.json"),
    include_str!("../../../fixtures/claude-weekly-only.json"),
    include_str!("../../../fixtures/claude-weekly-scoped.json"),
    include_str!("../../../fixtures/claude-spend-only.json"),
    include_str!("../../../fixtures/claude-reset-grants.json"),
    include_str!("../../../fixtures/claude-partial-limits.json"),
    include_str!("../../../fixtures/claude-core-malformed.json"),
    include_str!("../../../fixtures/claude-reset-grants-malformed.json"),
];
const CLAUDE_VALID: usize = 6;

static CODEX_USAGE: [&str; 7] = [
    include_str!("../../../fixtures/codex-all-windows.json"),
    include_str!("../../../fixtures/codex-live-schema.json"),
    include_str!("../../../fixtures/codex-live-2026-10-26.json"),
    include_str!("../../../fixtures/codex-weekly-only.json"),
    include_str!("../../../fixtures/codex-credits-unlimited.json"),
    include_str!("../../../fixtures/codex-partial-limits.json"),
    include_str!("../../../fixtures/codex-core-malformed.json"),
];
const CODEX_VALID: usize = 5;

const KEYS: &[&str] = &[
    "five_hour",
    "seven_day",
    "seven_day_sonnet",
    "seven_day_opus",
    "seven_day_routines",
    "seven_day_cowork",
    "utilization",
    "resets_at",
    "limits",
    "kind",
    "percent",
    "scope",
    "model",
    "display_name",
    "id",
    "extra_usage",
    "is_enabled",
    "used_credits",
    "monthly_limit",
    "currency",
    "decimal_places",
    "cedar_ember",
    "eligible",
    "grants",
    "resets_left",
    "resets_total",
    "paused",
    "starts_at",
    "ends_at",
    "rate_limit",
    "primary_window",
    "secondary_window",
    "used_percent",
    "limit_window_seconds",
    "reset_at",
    "additional_rate_limits",
    "limit_name",
    "metered_feature",
    "credits",
    "has_credits",
    "unlimited",
    "balance",
    "spend_control",
    "reached",
    "individual_limit",
    "used",
    "limit",
    "unit",
    "rate_limit_reset_credits",
    "available_count",
    "status",
    "expires_at",
    "reset_type",
    "email",
    "plan_type",
    "account",
    "organization",
    "uuid",
    "subscriptionType",
    "rate_limit_tier",
];

fn key() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => prop::sample::select(KEYS).prop_map(str::to_owned),
        1 => "[a-z_]{1,10}",
    ]
}

fn interesting_number() -> impl Strategy<Value = Value> {
    prop::sample::select(vec![
        json!(0),
        json!(-0.0),
        json!(1),
        json!(-1),
        json!(-0.5),
        json!(50),
        json!(51),
        json!(99.5),
        json!(100),
        json!(100.0001),
        json!(104.2),
        json!(1e300),
        json!(-1e300),
        json!(1e-300),
        json!(i64::MIN),
        json!(i64::MAX),
        json!(u64::MAX),
        json!(18_000),
        json!(604_800),
        json!(MAX_SAFE_INTEGER),
        json!(MAX_SAFE_INTEGER + 1),
        json!(1_792_152_000),
        json!(1_792_152_000.5),
        json!(253_402_300_799i64),
        json!(253_402_300_800i64),
        json!(-62_167_219_200i64),
        json!(-62_167_219_201i64),
    ])
}

fn interesting_string() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "",
        " ",
        "2026-10-16T10:00:00Z",
        "2026-10-16T15:00:00+05:00",
        "9999-12-31T23:59:59Z",
        "9999-12-31T23:59:59-05:00",
        "0000-01-01T00:00:00Z",
        "0000-01-01T00:00:00+01:00",
        "2026-13-01T00:00:00Z",
        "soon",
        "12.5",
        " 7 ",
        "inf",
        "NaN",
        "-1",
        "1e999",
        "weekly_scoped",
        "all models",
        "All Models",
        "available",
        "redeemed",
        "USD",
        "usd",
        "US dollars",
        "Opus",
        "Sonnet",
        "codex-spark",
        "codex-",
        "--",
        "max",
        "default_claude_max_20x",
        "a\nb",
        "\u{0}",
        "é",
    ])
    .prop_map(str::to_owned)
}

fn string() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => interesting_string(),
        2 => "\\PC{0,12}",
        1 => any::<String>(),
        1 => Just("x".repeat(5_000)),
    ]
}

fn leaf() -> BoxedStrategy<Value> {
    prop_oneof![
        1 => Just(Value::Null),
        1 => any::<bool>().prop_map(Value::Bool),
        1 => any::<i64>().prop_map(Value::from),
        1 => any::<u64>().prop_map(Value::from),
        1 => any::<f64>().prop_map(Value::from),
        1 => (-10.0f64..200.0).prop_map(Value::from),
        3 => interesting_number(),
        3 => string().prop_map(Value::String),
    ]
    .boxed()
}

fn json_value() -> BoxedStrategy<Value> {
    leaf()
        .prop_recursive(4, 48, 6, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
                prop::collection::btree_map(key(), inner, 0..6)
                    .prop_map(|map| Value::Object(map.into_iter().collect())),
            ]
        })
        .boxed()
}

fn json_object() -> impl Strategy<Value = Value> {
    prop::collection::btree_map(key(), json_value(), 0..10)
        .prop_map(|map| Value::Object(map.into_iter().collect()))
}

fn object(entries: &[(&str, Value)]) -> Value {
    Value::Object(
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect(),
    )
}

fn readable_timestamp(value: &str) -> Result<(), TestCaseError> {
    prop_assert!(
        DateTime::parse_from_rfc3339(value).is_ok()
            && value.len() == 20
            && value.ends_with('Z')
            && value.as_bytes()[..4].iter().all(u8::is_ascii_digit),
        "timestamp the interface cannot read: {value:?}"
    );
    Ok(())
}

fn readable_amount(value: f64) -> Result<(), TestCaseError> {
    prop_assert!(value.is_finite() && value >= 0.0, "amount {value}");
    Ok(())
}

/// What the interface's schema accepts, plus the provider-independent limits.
fn assert_readable(snapshot: &Snapshot) -> Result<(), TestCaseError> {
    prop_assert!(snapshot.windows.len() <= 64);
    let mut ids = HashSet::new();
    for window in &snapshot.windows {
        prop_assert!(ids.insert(window.id.as_str()), "duplicate id {}", window.id);
        prop_assert!(!window.id.is_empty());
        prop_assert!(!window.label.trim().is_empty(), "blank label in {window:?}");
        prop_assert!(
            (0.0..=100.0).contains(&window.used_percent),
            "percent out of range in {window:?}"
        );
        if let Some(resets_at) = &window.resets_at {
            readable_timestamp(resets_at)?;
        }
        if let Some(seconds) = window.window_seconds {
            prop_assert!(
                (1..=MAX_SAFE_INTEGER).contains(&seconds),
                "duration the interface cannot read in {window:?}"
            );
        }
    }
    readable_timestamp(&snapshot.fetched_at)?;
    if let Some(extra) = &snapshot.extra_usage {
        readable_amount(extra.spent)?;
        if let Some(limit) = extra.limit {
            readable_amount(limit)?;
        }
        prop_assert!(
            extra.currency.len() == 3
                && extra.currency.bytes().all(|byte| byte.is_ascii_uppercase())
        );
    }
    if let Some(grants) = &snapshot.reset_grants {
        prop_assert!((1..=50).contains(&grants.available));
        if let Some(expires_at) = &grants.expires_at {
            readable_timestamp(expires_at)?;
        }
    }
    if let Some(balance) = &snapshot.balance {
        prop_assert!(!balance.is_empty());
        if let Some(amount) = balance.credits.as_ref().and_then(|credits| credits.amount) {
            readable_amount(amount)?;
        }
        if let Some(limit) = &balance.spend_limit {
            readable_amount(limit.used)?;
            readable_amount(limit.limit)?;
            prop_assert!((0.0..=100.0).contains(&limit.used_percent));
            if let Some(resets_at) = &limit.resets_at {
                readable_timestamp(resets_at)?;
            }
        }
    }
    Ok(())
}

fn same_reading(window: &UsageWindow, percent: Option<&Value>, reset: Option<&Value>) -> bool {
    percent
        .and_then(Value::as_f64)
        .is_some_and(|value| value >= 0.0 && value.min(100.0) == window.used_percent)
        && timestamp(reset).ok() == Some(window.resets_at.clone())
}

/// Every Claude window is the reading at its own key, or a scoped limit with its name.
fn claude_provenance(input: &Value, snapshot: &Snapshot) -> Result<(), TestCaseError> {
    for window in &snapshot.windows {
        let found = if window.id.starts_with("weekly_scoped_") {
            input["limits"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|limit| {
                    limit["kind"] == "weekly_scoped"
                        && limit
                            .pointer("/scope/model/display_name")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            == Some(window.label.as_str())
                        && same_reading(window, limit.get("percent"), limit.get("resets_at"))
                })
        } else {
            let source = &input[window.id.as_str()];
            [
                "five_hour",
                "seven_day",
                "seven_day_sonnet",
                "seven_day_opus",
                "seven_day_routines",
                "seven_day_cowork",
            ]
            .contains(&window.id.as_str())
                && same_reading(window, source.get("utilization"), source.get("resets_at"))
        };
        prop_assert!(found, "{window:?} is not in the response");
    }
    Ok(())
}

/// Every Codex window is a reading from the core limit or, for extras, from a named limit.
fn codex_provenance(input: &Value, snapshot: &Snapshot) -> Result<(), TestCaseError> {
    for window in &snapshot.windows {
        let limits: Vec<&Value> = if window.id.starts_with("extra_") {
            input["additional_rate_limits"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|entry| &entry["rate_limit"])
                .collect()
        } else {
            vec![&input["rate_limit"]]
        };
        let found = limits
            .iter()
            .flat_map(|limit| [&limit["primary_window"], &limit["secondary_window"]])
            .any(|source| {
                same_reading(window, source.get("used_percent"), source.get("reset_at"))
                    && source.get("limit_window_seconds").and_then(Value::as_u64)
                        == window.window_seconds
            });
        prop_assert!(found, "{window:?} is not in the response");
    }
    Ok(())
}

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn pointers(value: &Value, at: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = format!("{at}/{}", escape(key));
                out.push(path.clone());
                pointers(child, &path, out);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                let path = format!("{at}/{index}");
                out.push(path.clone());
                pointers(child, &path, out);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Clone)]
enum Mutation {
    Remove,
    Replace(Value),
    Clear,
    Duplicate,
    Insert(String, Value),
}

fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        2 => Just(Mutation::Remove),
        4 => leaf().prop_map(Mutation::Replace),
        1 => json_value().prop_map(Mutation::Replace),
        1 => Just(Mutation::Clear),
        1 => Just(Mutation::Duplicate),
        1 => (key(), leaf()).prop_map(|(key, value)| Mutation::Insert(key, value)),
    ]
}

fn mutations() -> impl Strategy<Value = Vec<(Index, Mutation)>> {
    prop::collection::vec((any::<Index>(), mutation()), 1..=4)
}

fn apply(value: &mut Value, at: &Index, mutation: &Mutation) {
    let mut paths = Vec::new();
    pointers(value, "", &mut paths);
    if paths.is_empty() {
        return;
    }
    let path = at.get(&paths).clone();
    let (parent, last) = path.rsplit_once('/').unwrap_or_default();
    let last = last.replace("~1", "/").replace("~0", "~");
    match mutation {
        Mutation::Remove => match value.pointer_mut(parent) {
            Some(Value::Object(map)) => {
                map.remove(&last);
            }
            Some(Value::Array(items)) => {
                items.remove(last.parse().unwrap());
            }
            _ => {}
        },
        Mutation::Replace(replacement) => {
            if let Some(target) = value.pointer_mut(&path) {
                *target = replacement.clone();
            }
        }
        Mutation::Clear => match value.pointer_mut(&path) {
            Some(Value::Object(map)) => map.clear(),
            Some(Value::Array(items)) => items.clear(),
            Some(other) => *other = Value::Null,
            None => {}
        },
        Mutation::Duplicate => {
            if let Some(Value::Array(items)) = value.pointer_mut(parent) {
                let index: usize = last.parse().unwrap();
                let item = items[index].clone();
                items.insert(index, item);
            } else if let Some(Value::Array(items)) = value.pointer_mut(&path) {
                if let Some(first) = items.first().cloned() {
                    items.push(first);
                }
            }
        }
        Mutation::Insert(key, inserted) => match value.pointer_mut(&path) {
            Some(Value::Object(map)) => {
                map.insert(key.clone(), inserted.clone());
            }
            _ => {
                if let Value::Object(map) = value {
                    map.insert(key.clone(), inserted.clone());
                }
            }
        },
    }
}

fn mutated(fixture: &str, steps: &[(Index, Mutation)]) -> Value {
    let mut value: Value = serde_json::from_str(fixture).unwrap();
    for (at, mutation) in steps {
        apply(&mut value, at, mutation);
    }
    value
}

fn comparable(snapshot: &Snapshot) -> Value {
    let mut value = serde_json::to_value(snapshot).unwrap();
    value.as_object_mut().unwrap().remove("fetchedAt");
    value
}

/// Same meaning, different spelling: unknown keys in non-empty objects, integer readings sent
/// as floats, and timestamps in another offset.
fn respell(value: &mut Value, extra: &Value, offset: &FixedOffset, key: Option<&str>) {
    match value {
        Value::Object(map) => {
            if !map.is_empty() {
                map.insert("zz_unknown".into(), extra.clone());
            }
            for (child_key, child) in map.iter_mut() {
                if child_key != "zz_unknown" {
                    respell(child, extra, offset, Some(child_key.as_str()));
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                respell(item, extra, offset, None);
            }
        }
        Value::Number(number) => {
            let reading = matches!(key, Some("utilization" | "percent" | "used_percent"));
            if let (true, Some(whole)) = (reading, number.as_u64()) {
                *value = json!(whole as f64);
            } else if let (Some("reset_at"), Some(seconds)) = (key, number.as_i64()) {
                if let Some(date) = DateTime::from_timestamp(seconds, 0) {
                    *value = json!(date.with_timezone(offset).to_rfc3339());
                }
            }
        }
        Value::String(text) => {
            if let Ok(date) = DateTime::parse_from_rfc3339(text) {
                *text = date.with_timezone(offset).to_rfc3339();
            }
        }
        _ => {}
    }
}

fn offset() -> impl Strategy<Value = FixedOffset> {
    (-48i32..=56).prop_map(|quarters| FixedOffset::east_opt(quarters * 900).unwrap())
}

#[derive(Debug, Clone)]
enum Window {
    Absent,
    Null,
    Empty,
    Inactive,
    Unreadable,
    NotObject(Value),
    BadPercent(Value),
    BadReset(Value),
    BadDuration(Value),
    Reading(f64, Option<u64>),
}

impl Window {
    fn malformed(&self) -> bool {
        matches!(
            self,
            Window::NotObject(_)
                | Window::BadPercent(_)
                | Window::BadReset(_)
                | Window::BadDuration(_)
        )
    }

    fn reading(&self) -> Option<f64> {
        match self {
            Window::Reading(value, _) => Some(value.min(100.0)),
            _ => None,
        }
    }

    fn render(&self, percent_key: &str, reset: Value) -> Option<Value> {
        let reset_key = if percent_key == "used_percent" {
            "reset_at"
        } else {
            "resets_at"
        };
        Some(match self {
            Window::Absent => return None,
            Window::Null => Value::Null,
            Window::Empty => json!({}),
            Window::Inactive => object(&[(percent_key, Value::Null), (reset_key, Value::Null)]),
            Window::Unreadable => object(&[(reset_key, reset)]),
            Window::NotObject(value) => value.clone(),
            Window::BadPercent(value) => {
                object(&[(percent_key, value.clone()), (reset_key, reset)])
            }
            Window::BadReset(value) => {
                object(&[(percent_key, json!(10)), (reset_key, value.clone())])
            }
            Window::BadDuration(value) => object(&[
                (percent_key, json!(10)),
                (reset_key, reset),
                ("limit_window_seconds", value.clone()),
            ]),
            Window::Reading(value, seconds) => {
                let mut window = object(&[(percent_key, json!(value)), (reset_key, reset)]);
                if let Some(seconds) = seconds {
                    window["limit_window_seconds"] = json!(seconds);
                }
                window
            }
        })
    }
}

fn reading_value() -> impl Strategy<Value = f64> {
    prop_oneof![
        0.0f64..150.0,
        prop::sample::select(vec![0.0, 100.0, 100.5, 1e300]),
    ]
}

fn not_object() -> impl Strategy<Value = Value> {
    prop::sample::select(vec![json!("full"), json!(1), json!([]), json!(true)])
}

fn not_array() -> impl Strategy<Value = Value> {
    prop::sample::select(vec![json!("many"), json!(1), json!({}), json!(true)])
}

fn bad_percent() -> impl Strategy<Value = Value> {
    prop::sample::select(vec![
        json!(-1),
        json!(-0.5),
        json!(-1e300),
        json!("50"),
        json!(true),
        json!([]),
        json!({}),
    ])
}

fn bad_reset(numeric: bool) -> impl Strategy<Value = Value> {
    let mut values = vec![
        json!("soon"),
        json!(""),
        json!("2026-13-01T00:00:00Z"),
        json!("9999-12-31T23:59:59-05:00"),
        json!(true),
        json!([]),
        json!(1.5),
        json!(i64::MAX),
        json!(253_402_300_800i64),
        json!(-62_167_219_201i64),
    ];
    if numeric {
        values.push(json!("1792152000"));
    }
    prop::sample::select(values)
}

fn window(codex: bool) -> impl Strategy<Value = Window> {
    let seconds = if codex {
        prop::sample::select(vec![
            None,
            Some(18_000),
            Some(604_800),
            Some(172_800),
            Some(10_800),
            Some(90),
            Some(2_592_000),
        ])
        .boxed()
    } else {
        Just(None).boxed()
    };
    let duration = if codex {
        prop::sample::select(vec![
            json!(0),
            json!(-1),
            json!(1.5),
            json!("18000"),
            json!(MAX_SAFE_INTEGER + 1),
            json!(u64::MAX),
        ])
        .prop_map(Window::BadDuration)
        .boxed()
    } else {
        Just(Window::Absent).boxed()
    };
    prop_oneof![
        1 => Just(Window::Absent),
        1 => Just(Window::Null),
        1 => Just(Window::Empty),
        1 => Just(Window::Inactive),
        1 => Just(Window::Unreadable),
        1 => not_object().prop_map(Window::NotObject),
        1 => bad_percent().prop_map(Window::BadPercent),
        1 => bad_reset(codex).prop_map(Window::BadReset),
        1 => duration,
        4 => (reading_value(), seconds).prop_map(|(value, seconds)| Window::Reading(value, seconds)),
    ]
}

#[derive(Debug, Clone)]
enum Scoped {
    Reading(&'static str, f64),
    MissingPercent(&'static str),
    NullPercent(&'static str),
    BadPercent(&'static str, Value),
    BadReset(&'static str, Value),
    LongName,
    NotObject(Value),
    OtherKind,
    Nameless,
    AllModels,
}

fn scoped() -> impl Strategy<Value = Scoped> {
    let name = || prop::sample::select(vec!["Fable", "Opus", "Sonnet", "Haiku", "routines"]);
    prop_oneof![
        4 => (name(), reading_value()).prop_map(|(name, value)| Scoped::Reading(name, value)),
        1 => name().prop_map(Scoped::MissingPercent),
        1 => name().prop_map(Scoped::NullPercent),
        1 => (name(), bad_percent()).prop_map(|(name, value)| Scoped::BadPercent(name, value)),
        1 => (name(), bad_reset(false)).prop_map(|(name, value)| Scoped::BadReset(name, value)),
        1 => Just(Scoped::LongName),
        1 => not_object().prop_map(Scoped::NotObject),
        1 => Just(Scoped::OtherKind),
        1 => Just(Scoped::Nameless),
        1 => Just(Scoped::AllModels),
    ]
}

impl Scoped {
    fn render(&self) -> Value {
        let limit = |name: &str, percent: Option<Value>, reset: Value| {
            let mut limit = json!({
                "kind": "weekly_scoped",
                "resets_at": reset,
                "scope": {"model": {"id": null, "display_name": name}},
            });
            if let Some(percent) = percent {
                limit["percent"] = percent;
            }
            limit
        };
        match self {
            Scoped::Reading(name, value) => limit(name, Some(json!(value)), json!(VALID_RESET)),
            Scoped::MissingPercent(name) => limit(name, None, json!(VALID_RESET)),
            Scoped::NullPercent(name) => limit(name, Some(Value::Null), json!(VALID_RESET)),
            Scoped::BadPercent(name, value) => limit(name, Some(value.clone()), json!(VALID_RESET)),
            Scoped::BadReset(name, value) => limit(name, Some(json!(10)), value.clone()),
            Scoped::LongName => limit(&"m".repeat(600), Some(json!(10)), json!(VALID_RESET)),
            Scoped::NotObject(value) => value.clone(),
            Scoped::OtherKind => json!({"kind": "session", "percent": 10, "scope": null}),
            Scoped::Nameless => json!({"kind": "weekly_scoped", "percent": 10, "scope": null}),
            Scoped::AllModels => limit("All models", Some(json!(10)), json!(VALID_RESET)),
        }
    }
}

#[derive(Debug, Clone)]
enum List<T> {
    Absent,
    Null,
    NotArray(Value),
    Entries(Vec<T>),
}

fn list<T: std::fmt::Debug + Clone>(
    entry: impl Strategy<Value = T>,
) -> impl Strategy<Value = List<T>> {
    prop_oneof![
        1 => Just(List::Absent),
        1 => Just(List::Null),
        1 => not_array().prop_map(List::NotArray),
        4 => prop::collection::vec(entry, 0..6).prop_map(List::Entries),
    ]
}

const CLAUDE_FIXED: [(&str, &str); 6] = [
    ("five_hour", "Session"),
    ("seven_day", "Weekly"),
    ("seven_day_sonnet", "Sonnet"),
    ("seven_day_opus", "Opus"),
    ("seven_day_routines", "Routines"),
    ("seven_day_cowork", "Cowork"),
];

/// What `claude::parse_usage` must return for a response built from these parts: the readings
/// in order, and whether something present could not be read; `None` for an error.
fn claude_expected(
    fixed: &[Window; 6],
    limits: &List<Scoped>,
) -> Option<(Vec<(String, f64)>, bool)> {
    let mut windows: Vec<(String, f64)> = Vec::new();
    let mut incomplete = false;
    for (window, (_, label)) in fixed.iter().zip(CLAUDE_FIXED).take(2) {
        if window.malformed() {
            return None;
        }
        incomplete |= matches!(window, Window::Unreadable);
        windows.extend(window.reading().map(|value| (label.to_owned(), value)));
    }
    match limits {
        List::Absent | List::Null => {}
        List::NotArray(_) => incomplete = true,
        List::Entries(entries) => {
            let mut seen = HashSet::new();
            for entry in entries {
                match entry {
                    Scoped::Reading(name, value) => {
                        if seen.insert(*name) {
                            windows.push(((*name).to_owned(), value.min(100.0)));
                        }
                    }
                    Scoped::MissingPercent(_)
                    | Scoped::BadPercent(..)
                    | Scoped::BadReset(..)
                    | Scoped::LongName
                    | Scoped::NotObject(_) => incomplete = true,
                    Scoped::NullPercent(_)
                    | Scoped::OtherKind
                    | Scoped::Nameless
                    | Scoped::AllModels => {}
                }
            }
        }
    }
    for (window, (_, label)) in fixed.iter().zip(CLAUDE_FIXED).skip(2) {
        incomplete |= window.malformed() || matches!(window, Window::Unreadable);
        if let Some(value) = window.reading() {
            if !windows
                .iter()
                .any(|(known, _)| known.eq_ignore_ascii_case(label))
            {
                windows.push((label.to_owned(), value));
            }
        }
    }
    (!windows.is_empty()).then_some((windows, incomplete))
}

#[derive(Debug, Clone)]
enum CoreLimit {
    Absent,
    Null,
    NotObject(Value),
    Windows(Window, Window),
}

#[derive(Debug, Clone)]
enum Named {
    Limit {
        name: Option<&'static str>,
        feature: Option<&'static str>,
        primary: Window,
        secondary: Window,
    },
    Inactive(&'static str),
    NotObject(Value),
}

fn named() -> impl Strategy<Value = Named> {
    let name = prop::sample::select(vec![None, Some("Spark"), Some("Review")]);
    let feature = prop::sample::select(vec![
        None,
        Some("codex-spark"),
        Some("codex-cloud_review"),
        Some("codex-"),
    ]);
    prop_oneof![
        4 => (name, feature, window(true), window(true)).prop_map(|(name, feature, primary, secondary)| {
            Named::Limit { name, feature, primary, secondary }
        }),
        1 => Just(Named::Inactive("Spark")),
        1 => not_object().prop_map(Named::NotObject),
    ]
}

fn codex_rate_limit(primary: &Window, secondary: &Window) -> Value {
    let mut limit = json!({"allowed": true});
    for (key, window) in [("primary_window", primary), ("secondary_window", secondary)] {
        if let Some(window) = window.render("used_percent", json!(1_792_152_000)) {
            limit[key] = window;
        }
    }
    limit
}

impl Named {
    fn render(&self) -> Value {
        match self {
            Named::Limit {
                name,
                feature,
                primary,
                secondary,
            } => json!({
                "limit_name": name,
                "metered_feature": feature,
                "rate_limit": codex_rate_limit(primary, secondary),
            }),
            Named::Inactive(name) => json!({"limit_name": name, "rate_limit": null}),
            Named::NotObject(value) => value.clone(),
        }
    }
}

fn codex_expected(core: &CoreLimit, named: &List<Named>) -> Option<(Vec<f64>, bool)> {
    let mut windows = Vec::new();
    let mut incomplete = false;
    match core {
        CoreLimit::Absent | CoreLimit::Null => {}
        CoreLimit::NotObject(_) => return None,
        CoreLimit::Windows(primary, secondary) => {
            for window in [primary, secondary] {
                if window.malformed() {
                    return None;
                }
                incomplete |= matches!(window, Window::Unreadable);
                windows.extend(window.reading());
            }
        }
    }
    match named {
        List::Absent | List::Null => {}
        List::NotArray(_) => incomplete = true,
        List::Entries(entries) => {
            let mut seen = HashSet::new();
            for entry in entries {
                let Named::Limit {
                    name,
                    feature,
                    primary,
                    secondary,
                } = entry
                else {
                    incomplete |= matches!(entry, Named::NotObject(_));
                    continue;
                };
                let humanized = feature.and_then(|feature| match feature {
                    "codex-spark" => Some("Spark"),
                    "codex-cloud_review" => Some("Cloud Review"),
                    _ => None,
                });
                let Some(label) = name.or(humanized) else {
                    incomplete = true;
                    continue;
                };
                if !seen.insert(feature.unwrap_or(label)) {
                    continue;
                }
                if [primary, secondary]
                    .iter()
                    .any(|window| window.malformed() || matches!(window, Window::Unreadable))
                {
                    incomplete = true;
                    continue;
                }
                windows.extend(primary.reading());
                windows.extend(secondary.reading());
            }
        }
    }
    (!windows.is_empty()).then_some((windows, incomplete))
}

fn credential_bytes(value: &Value, cut: &Index) -> Vec<u8> {
    let bytes = serde_json::to_vec(value).unwrap();
    let length = cut.index(bytes.len() + 1);
    bytes[..length].to_vec()
}

fn jwt(claims: &Value, padded: bool) -> String {
    let bytes = serde_json::to_vec(claims).unwrap();
    let payload = if padded {
        URL_SAFE.encode(bytes)
    } else {
        URL_SAFE_NO_PAD.encode(bytes)
    };
    format!("header.{payload}.signature")
}

fn strings_in(value: &Value, out: &mut HashSet<String>) {
    match value {
        Value::String(text) => {
            out.insert(text.trim().to_owned());
        }
        Value::Array(items) => items.iter().for_each(|item| strings_in(item, out)),
        Value::Object(map) => map.values().for_each(|item| strings_in(item, out)),
        _ => {}
    }
}

fn claims() -> impl Strategy<Value = Value> {
    let text = || prop_oneof![string().prop_map(Value::String), leaf()];
    (
        prop::option::of(text()),
        prop::option::of(text()),
        prop::option::of(text()),
        prop::option::of(text()),
        prop::option::of(text()),
        json_object(),
    )
        .prop_map(|(email, sub, account, plan, user, extra)| {
            let mut claims = extra;
            let mut auth = Map::new();
            for (key, value) in [
                ("chatgpt_account_id", account),
                ("chatgpt_plan_type", plan),
                ("chatgpt_user_id", user),
            ] {
                if let Some(value) = value {
                    auth.insert(key.into(), value);
                }
            }
            claims["https://api.openai.com/auth"] = Value::Object(auth);
            if let Some(email) = email {
                claims["email"] = email;
            }
            if let Some(sub) = sub {
                claims["sub"] = sub;
            }
            claims
        })
}

fn account_id() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(json!("workspace-1")),
        Just(json!("org_2:abc")),
        Just(json!("bad id!")),
        Just(json!("x".repeat(300))),
        "[A-Za-z0-9_:-]{1,12}".prop_map(Value::String),
        leaf(),
    ]
}

fn usage_base() -> impl Strategy<Value = String> {
    prop_oneof![
        string(),
        "(https|http|ftp)://([a-z]{1,8}(:[a-z]{1,4})?@)?[a-z.]{1,12}(:[0-9]{1,5})?(/[a-z-]{0,10}){0,3}/?(\\?[a-z=]{0,5})?(#[a-z]{0,3})?",
        prop::sample::select(vec![
            "https://chatgpt.com",
            "https://chatgpt.com/backend-api/",
            "https://chat.openai.com/",
            "https://example.test/backend-api",
            "https://example.test/proxy",
        ])
        .prop_map(str::to_owned),
    ]
}

#[derive(Debug, Clone)]
struct Grant {
    left: u64,
    paused: bool,
    ends: Option<i64>,
}

fn grants() -> impl Strategy<Value = Vec<Grant>> {
    let left = prop_oneof![0u64..4, Just(50), Just(51), any::<u64>(), Just(u64::MAX)];
    let ends = prop::option::of(prop::sample::select(vec![-86_400i64, 86_400]));
    prop::collection::vec(
        (left, any::<bool>(), ends).prop_map(|(left, paused, ends)| Grant { left, paused, ends }),
        0..6,
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_usage_never_panics_and_yields_readable_snapshots(value in json_object()) {
        if let Ok(snapshot) = claude::parse_usage(&value) {
            assert_readable(&snapshot)?;
            claude_provenance(&value, &snapshot)?;
        }
        if let Ok(snapshot) = codex::parse_usage(&value) {
            assert_readable(&snapshot)?;
            codex_provenance(&value, &snapshot)?;
        }
    }

    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
            let _ = claude::parse_usage(&value);
            let _ = codex::parse_usage(&value);
        }
        let _ = claude::parse_credentials(&bytes);
        let _ = codex::parse_credentials(&bytes);
        let _ = codex::jwt_claims(&String::from_utf8_lossy(&bytes));
    }

    #[test]
    fn mutated_claude_fixtures_fail_or_show_only_their_readings(
        fixture in prop::sample::select(&CLAUDE_USAGE[..]),
        steps in mutations(),
    ) {
        let value = mutated(fixture, &steps);
        if let Ok(snapshot) = claude::parse_usage(&value) {
            assert_readable(&snapshot)?;
            claude_provenance(&value, &snapshot)?;
        }
    }

    #[test]
    fn mutated_codex_fixtures_fail_or_show_only_their_readings(
        fixture in prop::sample::select(&CODEX_USAGE[..]),
        steps in mutations(),
    ) {
        let value = mutated(fixture, &steps);
        if let Ok(snapshot) = codex::parse_usage(&value) {
            assert_readable(&snapshot)?;
            codex_provenance(&value, &snapshot)?;
        }
    }

    #[test]
    fn valid_fixtures_parse_the_same_however_they_are_spelled(
        claude_fixture in prop::sample::select(&CLAUDE_USAGE[..CLAUDE_VALID]),
        codex_fixture in prop::sample::select(&CODEX_USAGE[..CODEX_VALID]),
        extra in json_value(),
        offset in offset(),
    ) {
        for (fixture, parse) in [
            (
                claude_fixture,
                claude::parse_usage as fn(&Value) -> Result<Snapshot, UsageError>,
            ),
            (codex_fixture, codex::parse_usage),
        ] {
            let original: Value = serde_json::from_str(fixture).unwrap();
            let expected = parse(&original).unwrap();
            prop_assert!(!expected.incomplete);
            let mut respelled = original.clone();
            respell(&mut respelled, &extra, &offset, None);
            prop_assert_eq!(comparable(&parse(&respelled).unwrap()), comparable(&expected));
        }
    }

    #[test]
    fn claude_windows_follow_the_reading_contract(
        fixed in prop::array::uniform6(window(false)),
        limits in list(scoped()),
    ) {
        let mut response = Map::new();
        for (window, (id, _)) in fixed.iter().zip(CLAUDE_FIXED) {
            if let Some(window) = window.render("utilization", json!(VALID_RESET)) {
                response.insert(id.into(), window);
            }
        }
        match &limits {
            List::Absent => {}
            List::Null => {
                response.insert("limits".into(), Value::Null);
            }
            List::NotArray(value) => {
                response.insert("limits".into(), value.clone());
            }
            List::Entries(entries) => {
                response.insert("limits".into(), entries.iter().map(Scoped::render).collect());
            }
        }
        let response = Value::Object(response);
        let actual = claude::parse_usage(&response);
        match claude_expected(&fixed, &limits) {
            None => prop_assert!(actual.is_err(), "{response}"),
            Some((windows, incomplete)) => {
                let snapshot = actual.map_err(|error| TestCaseError::fail(format!("{error:?} for {response}")))?;
                let actual: Vec<_> = snapshot
                    .windows
                    .iter()
                    .map(|window| (window.label.clone(), window.used_percent))
                    .collect();
                prop_assert_eq!(actual, windows, "{}", response);
                prop_assert_eq!(snapshot.incomplete, incomplete, "{}", response);
                assert_readable(&snapshot)?;
            }
        }
    }

    #[test]
    fn codex_windows_follow_the_reading_contract(
        core in prop_oneof![
            1 => Just(CoreLimit::Absent),
            1 => Just(CoreLimit::Null),
            1 => not_object().prop_map(CoreLimit::NotObject),
            4 => (window(true), window(true)).prop_map(|(primary, secondary)| CoreLimit::Windows(primary, secondary)),
        ],
        named in list(named()),
    ) {
        let mut response = json!({});
        match &core {
            CoreLimit::Absent => {}
            CoreLimit::Null => response["rate_limit"] = Value::Null,
            CoreLimit::NotObject(value) => response["rate_limit"] = value.clone(),
            CoreLimit::Windows(primary, secondary) => {
                response["rate_limit"] = codex_rate_limit(primary, secondary);
            }
        }
        match &named {
            List::Absent => {}
            List::Null => response["additional_rate_limits"] = Value::Null,
            List::NotArray(value) => response["additional_rate_limits"] = value.clone(),
            List::Entries(entries) => {
                response["additional_rate_limits"] = entries.iter().map(Named::render).collect();
            }
        }
        let actual = codex::parse_usage(&response);
        match codex_expected(&core, &named) {
            None => prop_assert!(actual.is_err(), "{response}"),
            Some((windows, incomplete)) => {
                let snapshot = actual.map_err(|error| TestCaseError::fail(format!("{error:?} for {response}")))?;
                let actual: Vec<_> = snapshot.windows.iter().map(|window| window.used_percent).collect();
                prop_assert_eq!(actual, windows, "{}", response);
                prop_assert_eq!(snapshot.incomplete, incomplete, "{}", response);
                assert_readable(&snapshot)?;
            }
        }
    }

    #[test]
    fn claude_reset_grants_count_only_usable_resets(grants in grants()) {
        let at = DateTime::parse_from_rfc3339(VALID_RESET).unwrap().with_timezone(&Utc);
        let block = json!({
            "eligible": true,
            "grants": grants.iter().map(|grant| json!({
                "resets_left": grant.left,
                "paused": grant.paused,
                "ends_at": grant.ends.map(|offset| (at + chrono::Duration::seconds(offset)).to_rfc3339()),
            })).collect::<Vec<_>>(),
        });
        let mut total: u128 = 0;
        let mut expected = Some(0);
        for grant in &grants {
            if grant.paused || grant.left == 0 || grant.ends.is_some_and(|offset| offset <= 0) {
                continue;
            }
            total += u128::from(grant.left);
            if total > 50 {
                expected = None;
                break;
            }
            expected = Some(total as u32);
        }
        let actual = claude::parse_reset_grants(Some(&block), at).map(|grants| grants.available);
        prop_assert_eq!(actual, expected.filter(|available| *available > 0));
    }

    #[test]
    fn mutated_reset_credits_fail_closed(
        steps in mutations(),
        claude_steps in mutations(),
    ) {
        let at = DateTime::parse_from_rfc3339("2026-10-16T10:00:00Z").unwrap().with_timezone(&Utc);
        let value = mutated(include_str!("../../../fixtures/codex-reset-credits.json"), &steps);
        if let Some(grants) = codex::parse_reset_credits(&value, at) {
            prop_assert_eq!(value["available_count"].as_u64(), Some(u64::from(grants.available)));
            prop_assert!((1..=50).contains(&grants.available));
            prop_assert!(grants.kind.as_ref().is_none_or(|kind| kind.len() <= 64));
            if let Some(expires_at) = &grants.expires_at {
                readable_timestamp(expires_at)?;
            }
        }
        let value = mutated(include_str!("../../../fixtures/claude-reset-grants.json"), &claude_steps);
        if let Some(grants) = claude::parse_reset_grants(value.get("cedar_ember"), at) {
            prop_assert!((1..=50).contains(&grants.available));
            if let Some(expires_at) = &grants.expires_at {
                readable_timestamp(expires_at)?;
            }
        }
    }

    #[test]
    fn claude_credentials_keep_the_token_and_require_profile_scope(
        token in prop_oneof![Just(json!("synthetic-token")), string().prop_map(Value::String), leaf()],
        scopes in prop::option::of(prop_oneof![
            Just(json!(["user:inference", "user:profile"])),
            Just(json!("user:inference user:profile")),
            Just(json!(["user:inference"])),
            Just(json!(["user:profile", 1])),
            json_value(),
        ]),
        plan in prop::option::of(leaf()),
        expires in leaf(),
        extra in json_object(),
        wrapped in any::<bool>(),
        cut in any::<Index>(),
    ) {
        let mut oauth = extra;
        oauth["accessToken"] = token.clone();
        oauth["expiresAt"] = expires.clone();
        if let Some(scopes) = &scopes {
            oauth["scopes"] = scopes.clone();
        }
        if let Some(plan) = plan {
            oauth["subscriptionType"] = plan;
        }
        let file = if wrapped { json!({"claudeAiOauth": oauth}) } else { oauth.clone() };
        let bytes = credential_bytes(&file, &cut);
        if let Ok(credentials) = claude::parse_credentials(&bytes) {
            prop_assert_eq!(bytes.len(), serde_json::to_vec(&file).unwrap().len());
            prop_assert!(wrapped);
            prop_assert_eq!(Some(credentials.access_token.as_str()), token.as_str());
            prop_assert!(!credentials.access_token.is_empty() && credentials.access_token.bytes().all(|byte| byte.is_ascii_graphic()));
            prop_assert_eq!(credentials.expires_at, expires.as_i64());
            let scopes = scopes.filter(|scopes| !scopes.is_null());
            let profile = scopes.is_none_or(|scopes| match scopes {
                Value::Array(items) => items.iter().any(|scope| scope == "user:profile"),
                Value::String(text) => text.split_whitespace().any(|scope| scope == "user:profile"),
                _ => false,
            });
            prop_assert!(profile, "accepted a token without user:profile");
            prop_assert!(credentials.plan.is_none_or(|plan| !plan.is_empty() && !plan.chars().any(char::is_control)));
        }
    }

    #[test]
    fn codex_credentials_keep_the_token_and_a_sendable_account(
        access_claims in claims(),
        id_claims in prop::option::of(claims()),
        padded in any::<bool>(),
        raw_token in prop::option::of(string()),
        account in prop::option::of(account_id()),
        mode in prop::option::of(prop::sample::select(vec!["apikey", "chatgpt"])),
        cut in any::<Index>(),
    ) {
        let access_token = raw_token.clone().unwrap_or_else(|| jwt(&access_claims, padded));
        let mut tokens = json!({"access_token": access_token});
        if let Some(id_claims) = &id_claims {
            tokens["id_token"] = json!(jwt(id_claims, padded));
        }
        if let Some(account) = &account {
            tokens["account_id"] = account.clone();
        }
        let mut file = json!({"tokens": tokens, "last_refresh": "2026-10-16T10:00:00Z"});
        if let Some(mode) = mode {
            file["auth_mode"] = json!(mode);
        }
        let bytes = credential_bytes(&file, &cut);
        if let Ok(credentials) = codex::parse_credentials(&bytes) {
            prop_assert_ne!(mode, Some("apikey"));
            prop_assert_eq!(&credentials.access_token, &access_token);
            if let Some(id) = &credentials.account_id {
                prop_assert!(HeaderValue::from_str(id).is_ok());
                prop_assert!(!id.is_empty() && id.len() <= 256);
                let explicit = account.as_ref().filter(|account| !account.is_null());
                prop_assert!(explicit.is_none_or(|account| account == id));
            }
            let mut known = HashSet::new();
            if raw_token.is_none() {
                strings_in(&access_claims, &mut known);
            }
            if let Some(id_claims) = &id_claims {
                strings_in(id_claims, &mut known);
            }
            for value in [&credentials.identity.email, &credentials.identity.plan].into_iter().flatten() {
                prop_assert!(known.contains(value), "{value:?} is not in the token");
            }
            let history_key = credentials.identity.history_key.as_ref();
            prop_assert!(
                history_key.is_none_or(|key| key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit())),
                "history key {history_key:?}"
            );
        }
    }

    #[test]
    fn token_claims_round_trip_and_bad_tokens_have_none(
        value in json_value(),
        padded in any::<bool>(),
        text in string(),
    ) {
        let token = jwt(&value, padded);
        let fits = token.split('.').nth(1).unwrap().len() <= 32_768;
        let decoded: Value = serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
        prop_assert_eq!(codex::jwt_claims(&token), fits.then_some(decoded));
        let _ = codex::jwt_claims(&text);
        let doubled = format!("{text}.{text}");
        prop_assert!(codex::jwt_claims(&doubled).is_none(), "{doubled:?} has claims");
    }

    #[test]
    fn profiles_yield_only_clean_strings(
        value in json_object(),
        steps in mutations(),
    ) {
        let fixture = mutated(include_str!("../../../fixtures/claude-profile-live-schema.json"), &steps);
        for value in [value, fixture] {
            let identity = claude::parse_profile(&value);
            for text in [&identity.email, &identity.plan].into_iter().flatten() {
                prop_assert!(!text.is_empty() && text.len() <= 4096 && !text.chars().any(char::is_control));
            }
            prop_assert!(identity.history_key.is_none_or(|key| key.len() == 64));
        }
    }

    #[test]
    fn percentages_and_timestamps_come_from_the_value(value in leaf(), seconds in -100_000_000_000i64..300_000_000_000) {
        if let Ok(parsed) = percent(&value) {
            let input = value.as_f64().unwrap();
            prop_assert!(input >= 0.0 && parsed == input.min(100.0));
        }
        if let Ok(Some(parsed)) = timestamp(Some(&value)) {
            readable_timestamp(&parsed)?;
        }
        match timestamp(Some(&json!(seconds))) {
            Ok(Some(parsed)) => {
                readable_timestamp(&parsed)?;
                prop_assert_eq!(DateTime::parse_from_rfc3339(&parsed).unwrap().timestamp(), seconds);
            }
            Ok(None) => prop_assert!(false, "a timestamp became unknown"),
            Err(_) => prop_assert!(!(-62_167_219_200..=253_402_300_799).contains(&seconds)),
        }
    }

    #[test]
    fn retry_after_stays_within_a_day(
        text in prop_oneof![string(), "[0-9]{0,25}", "(Mon|Fri), [0-9]{2} Oct [0-9]{4} [0-9]{2}:[0-9]{2}:[0-9]{2} GMT"],
    ) {
        let Ok(header) = HeaderValue::from_str(&text) else {
            return Ok(());
        };
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, header);
        let now = DateTime::parse_from_rfc3339("2026-10-16T10:00:00Z").unwrap().with_timezone(&Utc);
        prop_assert!(retry_after(&headers, now).is_none_or(|seconds| (1..=86_400).contains(&seconds)));
    }

    #[test]
    fn usage_urls_are_https_without_credentials(base in usage_base()) {
        if let Ok(url) = codex::usage_url(&base) {
            prop_assert_eq!(url.scheme(), "https");
            prop_assert!(url.host_str().is_some_and(|host| !host.is_empty()));
            prop_assert!(url.username().is_empty() && url.password().is_none());
            prop_assert!(url.query().is_none() && url.fragment().is_none());
            let wham = url.path().ends_with("/wham/usage");
            prop_assert!(wham || url.path().ends_with("/api/codex/usage"));
            let resets = codex::reset_credits_url(&url);
            prop_assert_eq!(resets.is_some(), wham);
            if let Some(resets) = resets {
                prop_assert_eq!(resets.origin(), url.origin());
            }
        }
    }
}
