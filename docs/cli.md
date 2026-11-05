# Command line

The app binary prints the usage Margin last saved. It makes no network requests and works whether or not Margin is running.

```sh
/Applications/Margin.app/Contents/MacOS/margin --line
# Claude 42%·2h18m   86%·2d     │ Codex 95%·45m     60%·4d
/Applications/Margin.app/Contents/MacOS/margin --line --all
# Personal 42%·2h18m   86%·2d
# Work     71%·4h02m   12%·18h
# Codex    95%·45m     60%·4d
/Applications/Margin.app/Contents/MacOS/margin --line --short    # Personal 42% · resets 16:30
/Applications/Margin.app/Contents/MacOS/margin --json
```

Installed in `~/Applications`? Use that path instead. To type `margin`, link it into a folder on your `PATH`:

```sh
ln -s /Applications/Margin.app/Contents/MacOS/margin ~/.local/bin/margin
```

On Windows, run `%LOCALAPPDATA%\Margin\Margin.exe --line` (MSI installs: `C:\Program Files\Margin\Margin.exe`). It is a windowed app, so the shell may print its prompt before the output; pipes and redirects work as usual.

`--line` prints one Claude and one Codex account: usage remaining and time to reset for the session window, then the weekly window. Columns have fixed widths, so the line does not shift between refreshes. `–` means the provider did not report that window, `~42%` means the reading is old, and `reset` means the window has reset and has not been read since (`–` once the countdowns are gone). `signed out`, `no data` or `…` replace the numbers when there are none. `idle, last read 3h ago` (`idle, no reading yet`) replaces them when the token expired on disk: usage refreshes while the CLI is idle until the token expires, then the last reading stays until the CLI next runs. With several accounts for a provider, it shows the one your terminal uses (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, else `~/.claude` and `~/.codex`), or the tray account when none matches. Set `COLUMNS` and the line shrinks to fit: first the countdowns go, then the labels shorten, then the weekly window goes.

`--all` prints one row per account, Claude first, in your account order. `--short` prints the one-account form from v0.7 (`--short --all`: `Personal 42% · Work 68%`), following **Tray account** and **Tray shows** in Settings.

Colors: on in a terminal or under Claude Code, off when piped or when `NO_COLOR` is set; `--color` and `--no-color` override. Only the 16 basic colors are used, so your terminal theme applies: yellow below 40% remaining, red below 20%, bold red at 0%.

`--json` prints `{"schema": 1, "generatedAt", "appRunning", "accounts": [...]}`. `appRunning` is left out when Margin can't tell. Each account has `id`, `provider`, `label`, `email`, `plan`, `fetchedAt`, `stale`, `status` (`ok`, `stale`, `signedOut`, `error`, `pending`), `idle`, `incomplete`, `error` (`kind`, `message`), and `windows` with `id`, `label`, `scope` (`core`, `model`, `other`), `usedPercent`, `remainingPercent`, `resetsAt`, `resetPassed`. `idle` is true when the token expired on disk; `status` stays `signedOut` and running the CLI gets a new reading. `incomplete` is true when an optional limit could not be read, so `windows` may be missing some. `resetPassed` is true when the window has reset with no reading since: its percentages predate the reset and are not current. New fields may be added; `schema` changes only when existing fields change. The output contains account emails but never tokens.

Exit codes: `0` ok, `1` no accounts or unreadable settings, `2` bad arguments. `MARGIN_DATA_DIR` points it at another settings folder (used by tests).

## Claude Code statusline

In `~/.claude/settings.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "/Applications/Margin.app/Contents/MacOS/margin --line",
    "refreshInterval": 60
  }
}
```

`refreshInterval` keeps the countdowns moving while the session is idle. Claude Code sets `COLUMNS`, so the line fits the terminal. Codex's own status line only shows its built-in items and cannot run a command, so Margin cannot feed it; Codex users see their limits in the Codex TUI and in Margin.

