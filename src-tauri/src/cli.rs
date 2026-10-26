use crate::core::{
    model::{Account, Provider, UsageWindow, WindowScope},
    notifications::reset_clock,
    settings::{AccountOrder, Settings, TrayMode},
    store::{self, StoredState},
    tray,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use std::{
    ffi::OsString,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const SCHEMA: u32 = 1;
pub const DATA_DIR_ENV: &str = "MARGIN_DATA_DIR";
const IDENTIFIER: &str = "com.abulkosim.margin";
const FLAGS: [&str; 7] = [
    "--json",
    "--line",
    "--all",
    "--help",
    "-h",
    "--version",
    "-V",
];
const USAGE: &str = "Usage: margin [--json | --line [--all]]

  --json     Print every account as JSON
  --line     Print one short line for statuslines
  --all      With --line, list every account
  --version  Print the version
  --help     Print this help

Reads the usage Margin last saved. Margin does not need to be running.";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Json,
    Line { all: bool },
    Help,
    Version,
}

pub fn run(args: impl IntoIterator<Item = OsString>) -> Option<i32> {
    let args: Vec<String> = args
        .into_iter()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let command = parse(&args)?;
    attach_console();
    let mut out = io::stdout().lock();
    let mut err = io::stderr().lock();
    let code = execute(command, &mut out, &mut err);
    let _ = out.flush();
    let _ = err.flush();
    Some(code)
}

fn parse(args: &[String]) -> Option<Result<Command, String>> {
    if !args.iter().any(|arg| FLAGS.contains(&arg.as_str())) {
        return None;
    }
    let (mut json, mut line, mut all, mut help, mut version) = (false, false, false, false, false);
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            "--line" => line = true,
            "--all" => all = true,
            "--help" | "-h" => help = true,
            "--version" | "-V" => version = true,
            other => return Some(Err(format!("unknown option {other}"))),
        }
    }
    Some(if help {
        Ok(Command::Help)
    } else if version {
        Ok(Command::Version)
    } else {
        match (json, line) {
            (true, true) => Err("choose --json or --line, not both".into()),
            (true, false) if all => Err("--all works with --line".into()),
            (true, false) => Ok(Command::Json),
            (false, true) => Ok(Command::Line { all }),
            (false, false) => Err("--all works with --line".into()),
        }
    })
}

fn execute(command: Result<Command, String>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let command = match command {
        Ok(command) => command,
        Err(message) => {
            let _ = writeln!(err, "margin: {message}\n\n{USAGE}");
            return 2;
        }
    };
    match command {
        Command::Help => {
            let _ = writeln!(out, "{USAGE}");
            0
        }
        Command::Version => {
            let _ = writeln!(out, "margin {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Command::Json | Command::Line { .. } => {
            let state = match state_path()
                .ok_or_else(|| "could not find the app data folder".to_owned())
                .and_then(|path| read_state(&path))
            {
                Ok(state) => state,
                Err(message) => {
                    let _ = writeln!(err, "margin: {message}");
                    return 1;
                }
            };
            match render(&command, state.as_ref(), app_running, Utc::now()) {
                Ok((text, empty)) => {
                    let _ = writeln!(out, "{text}");
                    i32::from(empty)
                }
                Err(message) => {
                    let _ = writeln!(err, "margin: {message}");
                    1
                }
            }
        }
    }
}

fn render(
    command: &Command,
    state: Option<&StoredState>,
    app_running: fn() -> Option<bool>,
    now: DateTime<Utc>,
) -> Result<(String, bool), String> {
    let empty = state.is_none_or(|state| state.accounts.is_empty());
    let text = match command {
        Command::Line { all } => {
            let default = Settings::default();
            let (accounts, settings) = state.map_or((&[][..], &default), |state| {
                (&state.accounts[..], &state.settings)
            });
            line(accounts, settings, *all, now)
        }
        _ => serde_json::to_string(&report(state, app_running(), now))
            .map_err(|_| "could not encode the report".to_owned())?,
    };
    Ok((text, empty))
}

fn state_path() -> Option<PathBuf> {
    std::env::var_os(DATA_DIR_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| dir.join(IDENTIFIER)))
        .map(|dir| dir.join("state.json"))
}

fn read_state(path: &Path) -> Result<Option<StoredState>, String> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        _ => store::read(path).map(Some),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Report<'a> {
    schema: u32,
    generated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_running: Option<bool>,
    accounts: Vec<AccountReport<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountReport<'a> {
    id: &'a str,
    provider: Provider,
    label: &'a str,
    email: Option<&'a str>,
    plan: Option<&'a str>,
    fetched_at: Option<&'a str>,
    stale: bool,
    status: Status,
    windows: Vec<WindowReport<'a>>,
    error: Option<ErrorReport<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum Status {
    Ok,
    Stale,
    SignedOut,
    Error,
    Pending,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowReport<'a> {
    id: &'a str,
    label: &'a str,
    scope: WindowScope,
    used_percent: f64,
    remaining_percent: f64,
    resets_at: Option<&'a str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorReport<'a> {
    kind: &'a str,
    message: &'a str,
}

fn report(
    state: Option<&StoredState>,
    app_running: Option<bool>,
    now: DateTime<Utc>,
) -> Report<'_> {
    let accounts = state.map_or_else(Vec::new, |state| {
        ordered(&state.accounts, &state.settings, now)
            .into_iter()
            .map(|account| account_report(account, &state.settings, now))
            .collect()
    });
    Report {
        schema: SCHEMA,
        generated_at: now.to_rfc3339_opts(SecondsFormat::Secs, true),
        app_running,
        accounts,
    }
}

fn account_report<'a>(
    account: &'a Account,
    settings: &Settings,
    now: DateTime<Utc>,
) -> AccountReport<'a> {
    let stale = tray::stale(account, settings, now);
    AccountReport {
        id: &account.id,
        provider: account.provider,
        label: &account.label,
        email: account.identity.email.as_deref(),
        plan: account.identity.plan.as_deref(),
        fetched_at: account
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.fetched_at.as_str()),
        stale,
        status: status(account, stale),
        windows: account
            .snapshot
            .iter()
            .flat_map(|snapshot| &snapshot.windows)
            .map(|window| WindowReport {
                id: &window.id,
                label: &window.label,
                scope: window.scope(),
                used_percent: rounded(used(window)),
                remaining_percent: rounded(100.0 - used(window)),
                resets_at: window.resets_at.as_deref(),
            })
            .collect(),
        error: account.error.as_ref().map(|error| ErrorReport {
            kind: &error.kind,
            message: &error.message,
        }),
    }
}

fn status(account: &Account, stale: bool) -> Status {
    match &account.error {
        Some(error) if error.kind == "signedOut" => Status::SignedOut,
        Some(_) if account.snapshot.is_none() => Status::Error,
        _ if stale => Status::Stale,
        _ if account.snapshot.is_none() => Status::Pending,
        _ => Status::Ok,
    }
}

fn used(window: &UsageWindow) -> f64 {
    if window.used_percent.is_finite() {
        window.used_percent.clamp(0.0, 100.0)
    } else {
        100.0
    }
}

fn rounded(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn ordered<'a>(
    accounts: &'a [Account],
    settings: &Settings,
    now: DateTime<Utc>,
) -> Vec<&'a Account> {
    let mut accounts: Vec<&Account> = accounts.iter().collect();
    if settings.account_order == AccountOrder::Added {
        return accounts;
    }
    let pinned = (settings.tray_mode == TrayMode::Pinned)
        .then_some(settings.pinned_account_id.as_deref())
        .flatten();
    let rank = |account: &Account| -> (u8, f64) {
        if Some(account.id.as_str()) == pinned {
            return (0, 0.0);
        }
        let windows = account
            .snapshot
            .as_ref()
            .map_or(&[][..], |snapshot| &snapshot.windows);
        let core: Vec<&UsageWindow> = windows
            .iter()
            .filter(|window| window.scope() == WindowScope::Core)
            .collect();
        let session = core
            .iter()
            .position(|window| matches!(window.id.as_str(), "five_hour" | "primary_window"));
        let weekly = (0..core.len()).find(|index| Some(*index) != session);
        let lowest = [session, weekly]
            .into_iter()
            .flatten()
            .map(|index| 100.0 - used(core[index]))
            .reduce(f64::min);
        match lowest {
            Some(value) if !tray::stale(account, settings, now) => (1, value),
            _ => (2, 0.0),
        }
    };
    accounts.sort_by(|left, right| {
        let (left, right) = (rank(left), rank(right));
        left.0.cmp(&right.0).then(left.1.total_cmp(&right.1))
    });
    accounts
}

fn line(accounts: &[Account], settings: &Settings, all: bool, now: DateTime<Utc>) -> String {
    if accounts.is_empty() {
        return "No accounts".into();
    }
    if all {
        return ordered(accounts, settings, now)
            .into_iter()
            .map(|account| segment(account, settings, false, now))
            .collect::<Vec<_>>()
            .join(" · ");
    }
    tray::selected(accounts, settings, now)
        .map(|account| segment(account, settings, true, now))
        .unwrap_or_else(|| "No accounts".into())
}

fn segment(account: &Account, settings: &Settings, with_reset: bool, now: DateTime<Utc>) -> String {
    let Some(window) = tray::limiting_window(account, settings.tray_metric, now) else {
        let state = match &account.error {
            Some(error) if error.kind == "signedOut" => "signed out",
            _ => "no data",
        };
        return format!("{} {state}", account.label);
    };
    let mut text = format!(
        "{} {}%",
        account.label,
        tray::percent_label(100.0 - used(window))
    );
    if with_reset {
        if let Some(reset) = window
            .resets_at
            .as_deref()
            .and_then(|reset| DateTime::parse_from_rfc3339(reset).ok())
            .map(|reset| reset.with_timezone(&Utc))
            .filter(|reset| *reset > now)
        {
            text.push_str(&format!(" · resets {}", reset_clock(reset, now)));
        }
    }
    if tray::stale(account, settings, now) {
        text.push_str(" (stale)");
    }
    text
}

#[cfg(target_os = "macos")]
fn app_running() -> Option<bool> {
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;

    let current = NSRunningApplication::currentApplication();
    let running = NSRunningApplication::runningApplicationsWithBundleIdentifier(
        &NSString::from_str(IDENTIFIER),
    );
    Some(
        running
            .iter()
            .any(|app| !app.isTerminated() && *app != *current),
    )
}

#[cfg(windows)]
fn app_running() -> Option<bool> {
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;

    let wide = |text: String| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let class = wide(format!("{IDENTIFIER}-sic"));
    let title = wide(format!("{IDENTIFIER}-siw"));
    Some(!unsafe { FindWindowW(class.as_ptr(), title.as_ptr()) }.is_null())
}

#[cfg(not(any(target_os = "macos", windows)))]
fn app_running() -> Option<bool> {
    None
}

#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE},
        System::Console::{
            AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
            STD_OUTPUT_HANDLE,
        },
    };

    let usable = |handle: HANDLE| !handle.is_null() && handle != INVALID_HANDLE_VALUE;
    let handles = [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE].map(|id| (id, unsafe { GetStdHandle(id) }));
    if handles.iter().all(|(_, handle)| usable(*handle)) {
        return;
    }
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }
    for (id, handle) in handles {
        if usable(handle) {
            unsafe { SetStdHandle(id, handle) };
        }
    }
}

#[cfg(not(windows))]
fn attach_console() {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn at(hours: i64) -> String {
        (Utc::now() + chrono::Duration::hours(hours)).to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    fn account(id: &str, label: &str, windows: Value) -> Value {
        json!({
            "id": id, "provider": "claude", "label": label, "color": "#ffffff",
            "source": { "kind": "configDir", "path": std::env::temp_dir().join("claude-secret-path") },
            "identity": { "email": format!("{id}@example.com"), "plan": "Max 5×" },
            "snapshot": { "windows": windows, "fetchedAt": at(0), "extraUsage": null },
            "error": null, "refreshing": false
        })
    }

    fn window(id: &str, used: f64, reset_hours: i64) -> Value {
        json!({ "id": id, "label": id, "usedPercent": used, "resetsAt": at(reset_hours), "windowSeconds": 18000 })
    }

    fn state(accounts: Vec<Value>, settings: Value) -> StoredState {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        let value = json!({ "version": 1, "accounts": accounts, "settings": settings });
        std::fs::write(&path, value.to_string()).unwrap();
        read_state(&path).unwrap().unwrap()
    }

    fn json_of(state: Option<&StoredState>, running: fn() -> Option<bool>) -> (Value, bool) {
        let (text, empty) = render(&Command::Json, state, running, Utc::now()).unwrap();
        (serde_json::from_str(&text).unwrap(), empty)
    }

    fn line_of(state: &StoredState, all: bool) -> String {
        render(&Command::Line { all }, Some(state), || None, Utc::now())
            .unwrap()
            .0
    }

    #[test]
    fn only_cli_flags_take_the_cli_path() {
        assert_eq!(parse(&args(&[])), None);
        assert_eq!(parse(&args(&["--background"])), None);
        assert_eq!(parse(&args(&["--json"])), Some(Ok(Command::Json)));
        assert_eq!(
            parse(&args(&["--line"])),
            Some(Ok(Command::Line { all: false }))
        );
        assert_eq!(
            parse(&args(&["--all", "--line"])),
            Some(Ok(Command::Line { all: true }))
        );
        assert_eq!(parse(&args(&["--json", "-h"])), Some(Ok(Command::Help)));
        assert_eq!(parse(&args(&["-V"])), Some(Ok(Command::Version)));
        for bad in [
            &["--json", "--line"][..],
            &["--all"],
            &["--json", "--all"],
            &["--json", "--background"],
            &["--line", "extra"],
        ] {
            assert!(matches!(parse(&args(bad)), Some(Err(_))), "{bad:?}");
        }
    }

    #[test]
    fn help_version_and_bad_arguments_set_exit_codes() {
        let run = |values: &[&str]| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = execute(parse(&args(values)).unwrap(), &mut out, &mut err);
            (
                code,
                String::from_utf8(out).unwrap(),
                String::from_utf8(err).unwrap(),
            )
        };
        let (code, out, _) = run(&["--help"]);
        assert_eq!(code, 0);
        assert!(out.starts_with("Usage: margin"));
        let (code, out, _) = run(&["--version"]);
        assert_eq!(code, 0);
        assert_eq!(out, format!("margin {}\n", env!("CARGO_PKG_VERSION")));
        let (code, out, err) = run(&["--json", "--nope"]);
        assert_eq!(code, 2);
        assert!(out.is_empty());
        assert!(err.contains("unknown option --nope"));
    }

    #[test]
    fn identifier_matches_the_app_config() {
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], IDENTIFIER);
    }

    #[test]
    fn json_reports_fresh_accounts_without_credentials() {
        let state = state(
            vec![account(
                "claude-a",
                "Personal",
                json!([
                    window("five_hour", 58.0, 2),
                    window("seven_day", 20.0, 48),
                    window("seven_day_opus", 90.5, 48),
                    window("seven_day_cowork", 10.0, 48)
                ]),
            )],
            json!({}),
        );
        let (value, empty) = json_of(Some(&state), || Some(true));
        assert!(!empty);
        assert_eq!(value["schema"], 1);
        assert_eq!(value["appRunning"], true);
        assert!(value["generatedAt"].as_str().unwrap().ends_with('Z'));
        let account = &value["accounts"][0];
        assert_eq!(account["id"], "claude-a");
        assert_eq!(account["provider"], "claude");
        assert_eq!(account["label"], "Personal");
        assert_eq!(account["email"], "claude-a@example.com");
        assert_eq!(account["plan"], "Max 5×");
        assert_eq!(account["status"], "ok");
        assert_eq!(account["stale"], false);
        assert!(account["error"].is_null());
        let windows = account["windows"].as_array().unwrap();
        assert_eq!(windows.len(), 4);
        assert_eq!(windows[0]["id"], "five_hour");
        assert_eq!(windows[0]["scope"], "core");
        assert_eq!(windows[0]["usedPercent"], 58.0);
        assert_eq!(windows[0]["remainingPercent"], 42.0);
        assert_eq!(windows[2]["scope"], "model");
        assert_eq!(windows[2]["remainingPercent"], 9.5);
        assert_eq!(windows[3]["scope"], "other");
        assert!(windows[0]["resetsAt"].as_str().is_some());
        let text = value.to_string();
        for secret in ["secret-path", "source", "historyKey", "color", "Token"] {
            assert!(!text.contains(secret), "{secret}");
        }
    }

    #[test]
    fn json_marks_stale_signed_out_and_failed_accounts() {
        let mut old = account("claude-old", "Old", json!([window("five_hour", 10.0, 2)]));
        old["snapshot"]["fetchedAt"] = json!(at(-2));
        let expired = account(
            "claude-exp",
            "Expired",
            json!([window("five_hour", 10.0, -1)]),
        );
        let mut retrying = account(
            "claude-retry",
            "Retry",
            json!([window("five_hour", 10.0, 2)]),
        );
        retrying["error"] =
            json!({ "kind": "rateLimited", "message": "Rate limited", "retryAfterSeconds": 60 });
        let mut signed_out = account("claude-out", "Out", json!([]));
        signed_out["snapshot"] = Value::Null;
        signed_out["error"] =
            json!({ "kind": "signedOut", "message": "Signed out", "retryAfterSeconds": null });
        let mut failed = account("claude-fail", "Fail", json!([]));
        failed["snapshot"] = Value::Null;
        failed["error"] =
            json!({ "kind": "network", "message": "Offline", "retryAfterSeconds": null });
        let mut pending = account("claude-new", "New", json!([]));
        pending["snapshot"] = Value::Null;
        let state = state(
            vec![old, expired, retrying, signed_out, failed, pending],
            json!({ "accountOrder": "added" }),
        );
        let (value, _) = json_of(Some(&state), || None);
        assert!(value.get("appRunning").is_none());
        let summary: Vec<(&str, &str, bool)> = value["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|account| {
                (
                    account["label"].as_str().unwrap(),
                    account["status"].as_str().unwrap(),
                    account["stale"].as_bool().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("Old", "stale", true),
                ("Expired", "stale", true),
                ("Retry", "ok", false),
                ("Out", "signedOut", true),
                ("Fail", "error", true),
                ("New", "pending", false),
            ]
        );
        assert_eq!(value["accounts"][3]["error"]["kind"], "signedOut");
        assert_eq!(value["accounts"][3]["fetchedAt"], Value::Null);
        assert_eq!(value["accounts"][3]["windows"], json!([]));
    }

    #[test]
    fn json_without_saved_state_is_empty() {
        let (value, empty) = json_of(None, || Some(false));
        assert!(empty);
        assert_eq!(value["accounts"], json!([]));
        assert_eq!(value["appRunning"], false);
        let state = state(Vec::new(), json!({}));
        assert!(json_of(Some(&state), || None).1);
    }

    #[test]
    fn unreadable_state_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        assert!(read_state(&path).unwrap().is_none());
        std::fs::write(&path, "{damaged").unwrap();
        assert!(read_state(&path).is_err());
        std::fs::write(&path, r#"{"version": 9}"#).unwrap();
        assert!(read_state(&path).is_err());
    }

    #[test]
    fn urgency_order_puts_pinned_then_lowest_then_stale() {
        let mut stale = account("c", "Stale", json!([window("five_hour", 99.0, 2)]));
        stale["snapshot"]["fetchedAt"] = json!(at(-3));
        let accounts = vec![
            account("a", "Roomy", json!([window("five_hour", 10.0, 2)])),
            stale,
            account(
                "b",
                "Tight",
                json!([window("five_hour", 10.0, 2), window("seven_day", 80.0, 48)]),
            ),
            account("d", "Pinned", json!([window("five_hour", 5.0, 2)])),
        ];
        let labels = |settings: Value| {
            let state = state(accounts.clone(), settings);
            ordered(&state.accounts, &state.settings, Utc::now())
                .iter()
                .map(|account| account.label.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(json!({})), ["Tight", "Roomy", "Pinned", "Stale"]);
        assert_eq!(
            labels(json!({ "trayMode": "pinned", "pinnedAccountId": "d" })),
            ["Pinned", "Tight", "Roomy", "Stale"]
        );
        assert_eq!(
            labels(json!({ "accountOrder": "added" })),
            ["Roomy", "Stale", "Tight", "Pinned"]
        );
    }

    #[test]
    fn line_follows_the_tray_account() {
        let accounts = vec![
            account(
                "a",
                "Personal",
                json!([
                    window("five_hour", 58.0, 2),
                    window("seven_day_opus", 99.0, 48)
                ]),
            ),
            account("b", "Work", json!([window("five_hour", 32.0, 2)])),
        ];
        let reset = |hours| {
            let reset = DateTime::parse_from_rfc3339(&at(hours))
                .unwrap()
                .with_timezone(&Utc);
            reset_clock(reset, Utc::now())
        };
        let lowest = state(accounts.clone(), json!({}));
        assert_eq!(
            line_of(&lowest, false),
            format!("Personal 42% · resets {}", reset(2))
        );
        let pinned = state(
            accounts.clone(),
            json!({ "trayMode": "pinned", "pinnedAccountId": "b" }),
        );
        assert_eq!(
            line_of(&pinned, false),
            format!("Work 68% · resets {}", reset(2))
        );
        let every = state(accounts.clone(), json!({ "trayMetric": "all" }));
        assert_eq!(
            line_of(&every, false),
            format!("Personal 1% · resets {}", reset(48))
        );
        assert_eq!(line_of(&lowest, true), "Personal 42% · Work 68%");
        let added = state(
            accounts,
            json!({ "accountOrder": "added", "trayMode": "pinned", "pinnedAccountId": "b" }),
        );
        assert_eq!(line_of(&added, true), "Personal 42% · Work 68%");
    }

    #[test]
    fn line_marks_stale_and_missing_readings() {
        let mut old = account("a", "Personal", json!([window("five_hour", 58.0, 2)]));
        old["snapshot"]["fetchedAt"] = json!(at(-2));
        let mut out = account("b", "Work", json!([]));
        out["snapshot"] = Value::Null;
        out["error"] =
            json!({ "kind": "signedOut", "message": "Signed out", "retryAfterSeconds": null });
        let mut new = account("c", "Spare", json!([]));
        new["snapshot"] = Value::Null;
        let only_old = state(vec![old.clone()], json!({}));
        assert!(line_of(&only_old, false).ends_with(" (stale)"));
        let mixed = state(vec![old, out, new], json!({ "accountOrder": "added" }));
        assert_eq!(
            line_of(&mixed, true),
            "Personal 42% (stale) · Work signed out · Spare no data"
        );
        let empty = state(Vec::new(), json!({}));
        assert_eq!(
            render(
                &Command::Line { all: false },
                Some(&empty),
                || None,
                Utc::now()
            )
            .unwrap(),
            ("No accounts".to_owned(), true)
        );
        assert_eq!(
            render(&Command::Line { all: true }, None, || None, Utc::now()).unwrap(),
            ("No accounts".to_owned(), true)
        );
    }
}
