use serde_json::{json, Value};
use std::{path::Path, process::Command};

fn margin_with(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_margin"));
    command.args(args).env("MARGIN_DATA_DIR", dir);
    for name in [
        "CLAUDECODE",
        "NO_COLOR",
        "COLUMNS",
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
    ] {
        command.env_remove(name);
    }
    command.envs(env.iter().copied());
    let output = command.output().unwrap();
    (
        output.status.code().unwrap(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

fn margin(dir: &Path, args: &[&str]) -> (i32, String, String) {
    margin_with(dir, args, &[])
}

fn stamp(hours: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::hours(hours) + chrono::Duration::seconds(30))
        .to_rfc3339()
}

fn codex_account(id: &str, label: &str, dir: &Path, session_used: f64) -> Value {
    json!({
        "id": id, "provider": "codex", "label": label, "color": "#336699",
        "source": { "kind": "configDir", "path": dir },
        "identity": { "email": "work@example.com", "plan": "Pro" },
        "snapshot": {
            "windows": [
                { "id": "primary_window", "label": "Session", "usedPercent": session_used, "resetsAt": stamp(2), "windowSeconds": 18000 },
                { "id": "secondary_window", "label": "Weekly", "usedPercent": 40.0, "resetsAt": stamp(72), "windowSeconds": 604800 }
            ],
            "fetchedAt": stamp(0), "extraUsage": null
        },
        "error": null, "refreshing": false
    })
}

fn write_accounts(dir: &Path, accounts: Vec<Value>) {
    let state = json!({ "version": 1, "accounts": accounts, "settings": {} });
    std::fs::write(dir.join("state.json"), state.to_string()).unwrap();
}

fn write_state(dir: &Path) {
    write_accounts(
        dir,
        vec![codex_account(
            "codex-test",
            "Work",
            &dir.join(".codex"),
            25.0,
        )],
    );
}

#[test]
fn json_reads_the_saved_state() {
    let dir = tempfile::tempdir().unwrap();
    write_state(dir.path());
    let (code, stdout, stderr) = margin(dir.path(), &["--json"]);
    assert_eq!(code, 0, "{stderr}");
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["schema"], 1);
    let account = &value["accounts"][0];
    assert_eq!(account["label"], "Work");
    assert_eq!(account["status"], "ok");
    assert_eq!(account["windows"][1]["remainingPercent"], 60.0);
    assert!(!stdout.contains(".codex"));
}

#[test]
fn line_reads_the_saved_state() {
    let dir = tempfile::tempdir().unwrap();
    write_state(dir.path());
    let (code, stdout, stderr) = margin(dir.path(), &["--line"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "Work 75%·2h00m   60%·3d\n");
    let (code, stdout, _) = margin(dir.path(), &["--line", "--all"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Work 75%·2h00m   60%·3d\n");
    let (code, stdout, _) = margin_with(dir.path(), &["--line"], &[("COLUMNS", "20")]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Work 75%  60%\n");
}

#[test]
fn short_line_keeps_the_one_account_format() {
    let dir = tempfile::tempdir().unwrap();
    write_state(dir.path());
    let (code, stdout, _) = margin(dir.path(), &["--line", "--short"]);
    assert_eq!(code, 0);
    assert!(stdout.starts_with("Work 60% · resets "), "{stdout}");
    let (code, stdout, _) = margin(dir.path(), &["--line", "--short", "--all"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Work 60%\n");
}

#[test]
fn colors_follow_the_environment_and_flags() {
    let dir = tempfile::tempdir().unwrap();
    write_state(dir.path());
    let plain = "Work 75%·2h00m   60%·3d\n";
    let (_, piped, _) = margin(dir.path(), &["--line"]);
    assert_eq!(piped, plain);
    let (_, claude_code, _) = margin_with(dir.path(), &["--line"], &[("CLAUDECODE", "1")]);
    assert!(
        claude_code.contains("\x1b[2m·2h00m\x1b[0m"),
        "{claude_code:?}"
    );
    let (_, no_color, _) = margin_with(
        dir.path(),
        &["--line"],
        &[("CLAUDECODE", "1"), ("NO_COLOR", "1")],
    );
    assert_eq!(no_color, plain);
    let (_, forced, _) = margin_with(dir.path(), &["--line", "--color"], &[("NO_COLOR", "1")]);
    assert!(forced.contains("\x1b["), "{forced:?}");
    let (_, off, _) = margin_with(
        dir.path(),
        &["--line", "--no-color"],
        &[("CLAUDECODE", "1")],
    );
    assert_eq!(off, plain);
}

#[test]
fn line_follows_the_callers_codex_home() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join(".codex");
    let other = dir.path().join(".codex-other");
    std::fs::create_dir(&work).unwrap();
    std::fs::create_dir(&other).unwrap();
    write_accounts(
        dir.path(),
        vec![
            codex_account("codex-work", "Work", &work, 25.0),
            codex_account("codex-other", "Other", &other, 90.0),
        ],
    );
    let (_, tray_pick, _) = margin(dir.path(), &["--line"]);
    assert_eq!(tray_pick, "Other 10%·2h00m   60%·3d\n");
    let home = format!("{}/", work.display());
    let (_, matched, _) = margin_with(dir.path(), &["--line"], &[("CODEX_HOME", &home)]);
    assert_eq!(matched, "Work 75%·2h00m   60%·3d\n");
    let (_, rows, _) = margin(dir.path(), &["--line", "--all"]);
    assert_eq!(rows, "Other 10%·2h00m   60%·3d\nWork  75%·2h00m   60%·3d\n");
}

#[test]
fn missing_and_damaged_state_exit_with_one() {
    let dir = tempfile::tempdir().unwrap();
    let (code, stdout, _) = margin(dir.path(), &["--line"]);
    assert_eq!((code, stdout.as_str()), (1, "No accounts\n"));
    let (code, stdout, _) = margin(dir.path(), &["--json"]);
    assert_eq!(code, 1);
    assert_eq!(
        serde_json::from_str::<Value>(&stdout).unwrap()["accounts"],
        json!([])
    );
    std::fs::write(dir.path().join("state.json"), "{damaged").unwrap();
    let (code, stdout, stderr) = margin(dir.path(), &["--json"]);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(stderr.starts_with("margin: "));
}

#[test]
fn bad_arguments_exit_with_two() {
    let dir = tempfile::tempdir().unwrap();
    let (code, stdout, stderr) = margin(dir.path(), &["--json", "--line"]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("Usage: margin"));
    let (code, _, stderr) = margin(dir.path(), &["--json", "--short"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("--short works with --line"));
    let (code, stdout, _) = margin(dir.path(), &["--version"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, format!("margin {}\n", env!("CARGO_PKG_VERSION")));
}
