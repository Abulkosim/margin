use serde_json::{json, Value};
use std::{path::Path, process::Command};

fn margin(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_margin"))
        .args(args)
        .env("MARGIN_DATA_DIR", dir)
        .output()
        .unwrap();
    (
        output.status.code().unwrap(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

fn write_state(dir: &Path) {
    let now = chrono::Utc::now();
    let stamp = |hours: i64| (now + chrono::Duration::hours(hours)).to_rfc3339();
    let state = json!({
        "version": 1,
        "accounts": [{
            "id": "codex-test", "provider": "codex", "label": "Work", "color": "#336699",
            "source": { "kind": "configDir", "path": dir.join(".codex") },
            "identity": { "email": "work@example.com", "plan": "Pro" },
            "snapshot": {
                "windows": [
                    { "id": "primary_window", "label": "Session", "usedPercent": 25.0, "resetsAt": stamp(2), "windowSeconds": 18000 },
                    { "id": "secondary_window", "label": "Weekly", "usedPercent": 40.0, "resetsAt": stamp(72), "windowSeconds": 604800 }
                ],
                "fetchedAt": stamp(0), "extraUsage": null
            },
            "error": null, "refreshing": false
        }],
        "settings": {}
    });
    std::fs::write(dir.join("state.json"), state.to_string()).unwrap();
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
    let (code, stdout, _) = margin(dir.path(), &["--line"]);
    assert_eq!(code, 0);
    assert!(stdout.starts_with("Work 60% · resets "), "{stdout}");
    let (code, stdout, _) = margin(dir.path(), &["--line", "--all"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Work 60%\n");
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
    let (code, stdout, _) = margin(dir.path(), &["--version"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, format!("margin {}\n", env!("CARGO_PKG_VERSION")));
}
