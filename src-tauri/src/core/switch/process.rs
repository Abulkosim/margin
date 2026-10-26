//! Best-effort detection of a running CLI. Unknown counts as running, so the
//! warning is shown whenever detection cannot rule it out.

use crate::core::model::Provider;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Running {
    No,
    Cli,
    /// Only the Codex desktop app's `codex app-server` helper, which outlives
    /// the app window and still refreshes `auth.json`.
    Helper,
}

impl Running {
    pub fn any(self) -> bool {
        self != Self::No
    }
}

pub fn running(provider: Provider) -> Running {
    detect(provider).unwrap_or(Running::Cli)
}

#[cfg(windows)]
fn detect(provider: Provider) -> Option<Running> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        if tasklist_has(&String::from_utf8_lossy(&output.stdout), provider) {
            Running::Cli
        } else {
            Running::No
        },
    )
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
fn detect(provider: Provider) -> Option<Running> {
    match provider {
        Provider::Claude => {
            let mut found = pgrep(&["-x", "claude"])?;
            found.extend(pgrep(&["-f", "@anthropic-ai/claude-code"])?);
            Some(if found.is_empty() {
                Running::No
            } else {
                Running::Cli
            })
        }
        Provider::Codex => {
            let mut named = pgrep(&["-x", "codex"])?;
            named.extend(pgrep(&["-x", "Codex"])?);
            let helpers = pgrep(&["-f", "codex app-server"])?;
            Some(classify_codex(&named, &helpers))
        }
    }
}

/// `named` are processes called `codex`; `helpers` are those whose command
/// line is `codex app-server`, the desktop app's daemon. The helper is also
/// named `codex`, so only when every named process is a helper is nothing
/// else running.
#[cfg_attr(windows, allow(dead_code))]
fn classify_codex(named: &[u32], helpers: &[u32]) -> Running {
    if named.is_empty() && helpers.is_empty() {
        Running::No
    } else if named.iter().all(|pid| helpers.contains(pid)) {
        Running::Helper
    } else {
        Running::Cli
    }
}

#[cfg(not(windows))]
fn pgrep(query: &[&str]) -> Option<Vec<u32>> {
    let program = if cfg!(target_os = "macos") {
        "/usr/bin/pgrep"
    } else {
        "pgrep"
    };
    let output = std::process::Command::new(program)
        .args(query)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    match output.status.code() {
        Some(0) => Some(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.trim().parse().ok())
                .collect(),
        ),
        Some(1) => Some(Vec::new()),
        _ => None,
    }
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

    #[test]
    fn the_desktop_helper_alone_is_told_apart_from_a_cli_session() {
        assert_eq!(classify_codex(&[], &[]), Running::No);
        assert_eq!(classify_codex(&[41], &[]), Running::Cli);
        assert_eq!(classify_codex(&[41], &[41]), Running::Helper);
        assert_eq!(classify_codex(&[41, 42], &[41]), Running::Cli);
        assert_eq!(classify_codex(&[], &[43]), Running::Helper);
        assert_eq!(classify_codex(&[41, 43], &[41, 43]), Running::Helper);
        assert!(Running::Helper.any() && Running::Cli.any() && !Running::No.any());
    }
}
