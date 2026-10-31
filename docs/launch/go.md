# Launch kit (v0.8.1, 2026-10-31)

Current facts for every post. Older drafts in [posts.md](posts.md) predate Make active; use the facts here.

- Free, MIT. macOS 12+ (universal), Windows x64. Unsigned: macOS → Privacy & Security → Open Anyway; Windows → More info → Run anyway. Checksums and build attestations on each release.
- Reads the sign-ins Claude Code and Codex already stored and calls their OAuth usage endpoints. No cookies, no CLI scraping, no telemetry, no backend.
- Read-only, with one exception you trigger yourself: **Make active** swaps the default CLI sign-in with another folder's, after a confirm, with a backup. Swap, not copy, because both CLIs rotate refresh tokens.
- Endpoints are undocumented. On failure it shows the last reading with its age, never guesses.

## 1. Show HN

Post Tuesday to Thursday, around 14:00 UTC. Link the repo. Reply to every comment for the first 3 hours.

**Title** (80 chars, the HN limit):

```
Show HN: Margin – Claude Code and Codex usage for all your accounts, in the tray
```

**URL:** `https://github.com/Abulkosim/margin`

**First comment** (rewrite in your own voice):

> I run several Claude Code and Codex accounts in separate config folders, and the only way to see what was left was to open each CLI and run /usage, or hit a rate limit. Margin shows every account's session, weekly and per-model limits with reset times in the macOS menu bar or the Windows tray.
>
> How it works: it reads the OAuth sign-ins the CLIs already stored (Claude Code's Keychain items per CLAUDE_CONFIG_DIR on macOS, .credentials.json elsewhere, Codex auth.json) and calls each provider's usage endpoint. The endpoints are undocumented, so on failure it keeps the last reading with its age instead of guessing. No cookies, no scraping, no telemetry.
>
> A few things I found useful once it existed: a pace line ("runs out around 16:30"), alerts when a limit burns 20% in an hour, `margin --line` for the Claude Code statusline, and a "make active" switch. The switch swaps sign-ins between folders instead of copying, because both CLIs rotate refresh tokens and two live copies sign each other out.
>
> Stack: Tauri 2, Rust for credentials, polling and the runtime-rendered tray icon, Vue for the popover. Tokens never leave the Rust process. Free and MIT; builds are unsigned for now, with checksums and GitHub attestations.
>
> CodexBar inspired it. Differences: Windows, two providers only, many accounts per provider.

**Likely questions:**
- *Undocumented endpoints will break.* Yes. Every response shape has fixtures and tests; a failure shows stale data with its age.
- *Why Tauri?* One codebase for macOS and Windows; small Rust core. Trade-off: a WebView popover.
- *Is the switch safe?* It holds Claude Code's own lock files, verifies after writing, rolls back on failure, keeps a backup, and refuses while Codex runs. It's the only code that writes CLI files, and a test fails if anything else does.
- *Against ToS?* Your own token, your own account's usage endpoint, read-only.

## 2. awesome-claude-code (form opened 2026-10-30)

Web form only, no PR, no `gh`: https://github.com/hesreallyhim/awesome-claude-code/issues/new?template=recommend-resource.yml

| Field | Value |
| --- | --- |
| Title | `[Resource]: Margin` |
| Display Name | `Margin` |
| Category | `Observability & Monitoring > Usage & Cost` (pick the closest offered) |
| Link | `https://github.com/Abulkosim/margin` |
| Author Name | `Abulkosim` |
| Author Link | `https://github.com/Abulkosim` |

Description (one line, no emoji, 391 chars):

```
Menu bar and system tray app for macOS and Windows that shows remaining Claude Code usage and reset times for several accounts at once, including accounts in separate CLAUDE_CONFIG_DIR folders. Reads the CLI's existing OAuth sign-ins, alerts at thresholds, and prints a statusline with margin --line. Optional account switch swaps sign-ins between folders with a backup. Also supports Codex.
```

Tick the checklist boxes only for things you actually did. Leave the last box ("Do not check the following box") unchecked.

## 3. Real screenshot (5 minutes)

1. Use light mode, with at least two accounts showing numbers.
2. Click the Margin icon so the popover is open.
3. Press **⌘⇧5**, choose **Capture Selected Portion**, and draw a box from just left of the Margin icon down to the bottom of the popover, including a bit of the menu bar.
4. Save it as `~/Desktop/margin-real-light.png`. Repeat in dark mode as `margin-real-dark.png` if you like.
5. Tell me "screenshots ready". I'll crop, blur emails, and put them in the README hero, the site, and the social image.

Emails show on the cards. If you'd rather not show them, rename the accounts first or tell me and I'll blur them.

## 4. Repo topics

Already set: `claude`, `claude-code`, `codex`, `codex-cli`, `macos`, `menu-bar`, `multi-account`, `openai-codex`, `quota-monitor`, `rust`, `system-tray`, `tauri`, `usage-monitor`, `vue`, `windows`. Nothing to do.

Still worth doing in repo Settings while you're there: upload `docs/images/social-preview.png` as the social preview (Settings → General → Social preview).

## Order

1. Screenshots (so the README shows the real thing).
2. Social preview upload.
3. awesome-claude-code form.
4. Show HN, on a Tuesday to Thursday.
