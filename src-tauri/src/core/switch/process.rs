//! Best-effort detection of a running CLI. Unknown counts as running, so the
//! warning is shown whenever detection cannot rule it out.

use crate::core::model::Provider;

pub fn running(provider: Provider) -> bool {
    detect(provider).unwrap_or(true)
}

#[cfg(windows)]
fn detect(provider: Provider) -> Option<bool> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(tasklist_has(
        &String::from_utf8_lossy(&output.stdout),
        provider,
    ))
}

#[cfg_attr(not(windows), allow(dead_code))]
fn tasklist_has(listing: &str, provider: Provider) -> bool {
    let names: &[&str] = match provider {
        Provider::Claude => &["claude.exe"],
        Provider::Codex => &["codex.exe"],
    };
    listing.lines().any(|line| {
        line.split(',')
            .next()
            .map(|image| image.trim().trim_matches('"'))
            .is_some_and(|image| names.iter().any(|name| image.eq_ignore_ascii_case(name)))
    })
}

#[cfg(not(windows))]
fn detect(provider: Provider) -> Option<bool> {
    let queries: &[&[&str]] = match provider {
        Provider::Claude => &[&["-x", "claude"], &["-f", "@anthropic-ai/claude-code"]],
        Provider::Codex => &[&["-x", "codex"], &["-x", "Codex"]],
    };
    let program = if cfg!(target_os = "macos") {
        "/usr/bin/pgrep"
    } else {
        "pgrep"
    };
    let mut found = false;
    for query in queries {
        let status = std::process::Command::new(program)
            .args(*query)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()?;
        match status.code() {
            Some(0) => found = true,
            Some(1) => {}
            _ => return None,
        }
    }
    Some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasklist_matches_cli_images_only() {
        let listing = "\"Claude.exe\",\"10\",\"Console\",\"1\",\"9,000 K\"\r\n\"codex.exe\",\"11\",\"Console\",\"1\",\"9,000 K\"\r\n";
        assert!(tasklist_has(listing, Provider::Codex));
        assert!(tasklist_has(listing, Provider::Claude));
        assert!(!tasklist_has(
            "\"Claude Desktop.exe\",\"10\"\r\n",
            Provider::Claude
        ));
        assert!(!tasklist_has("", Provider::Codex));
    }
}
