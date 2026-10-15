# Brief: AI usage tray app (Claude + Codex, macOS + Windows)

You are building this from zero. This file is the whole context. Read it once, then own the project.

## What and why

Menu-bar / system-tray app that shows how much of your Claude Code and Codex quota is left, per window (5h session, 7d weekly, model-specific), with reset countdowns, for **many accounts at once**.

Inspiration: https://github.com/steipete/CodexBar (Swift, macOS, ~100 providers, 22k stars). We are deliberately **not** cloning it. We want:
- 2 providers only: Claude, Codex. Nothing else, ever, unless asked.
- macOS **and** Windows from one codebase.
- First-class multi-account (user may run 5 Claude accounts).
- Our own look. CodexBar is utilitarian. Ours should feel designed. You have taste; use it.

Owner: Abulkosim, frontend lead (Vue/Nuxt/TS). On a Mac. Prefers extremely concise replies. No Windows machine in the loop at first; Windows is built via CI and tested by the owner in a VM later. Rust is **not installed** on this machine yet; first step is `rustup`.

## Decisions already made (don't relitigate)

- **Stack:** Tauri 2 + Rust core + Vue 3 + TypeScript + Vite. pnpm. Tauri plugins: tray (built-in), autostart, notification, updater, store (or own JSON), single-instance, positioner.
- **Data path: OAuth usage APIs only.** No browser-cookie import, no Keychain cookie decryption, no PTY scraping of the CLI. If OAuth fails, show last-known values with their age and a clear "stale" state. Fail closed, never guess.
- **Never write to the CLI's credential files**, except the explicit "make this account active" action (see Multi-account). Never redeem/consume anything (e.g. Claude reset credits). Read-only by default.
- **Distribution:** GitHub Releases, unsigned at first. Mac: document right-click → Open. Win: SmartScreen "run anyway". Signing later.
- **Commits:** one-line, imperative, no attribution trailers, no Co-Authored-By. No comments added to code unless non-obvious.
- **Tone:** never market this as "bypass limits". It displays limits.

## Open to you (be creative, propose before building)

- **Name.** Propose 3, pick one unless owner objects. Short, memorable, not "XBar".
- **Visual direction.** Propose 2–3 directions as quick HTML/Vue mockups before committing: e.g. minimal monochrome with a single accent, warm "dashboard instrument" style, playful. Dark + light. Must look native-ish on both OSes without pretending to be native.
- **Tray icon language.** How do we encode "remaining" in a 16–22px glyph? Ring, bar, number, dot color, animated on refresh? Multi-account in one icon: worst account? pinned account? Decide and justify in one line.
- **Popover layout.** Accounts list, per-account windows, reset countdowns, spend if available. Density vs clarity is yours.
- **Notification voice.** Thresholds (e.g. 80/95/100%) and copy. Keep it terse, not cute.
- **Micro-interactions.** Refresh animation, countdown ticking, hover reveals. Earn them; don't add noise.

## Design bar (non-negotiable)

This must look like a 2026 indie Mac app that a designer shipped, not a dev tool. Reference quality: Raycast, Linear, Arc, CleanShot, Ice. If a screenshot of the popover would not get upvotes on its own, it is not done.

- Typography first. One good UI font (Inter / Geist / system), tight hierarchy, tabular numerals for percentages and countdowns.
- Restraint. One accent color, generous spacing, no borders where shadow or spacing does the job. No default Tauri/Vite styles leaking through.
- Motion with purpose. Bars ease to their value, countdowns tick, refresh is a subtle pulse not a spinner. 150–250ms, never bouncy.
- Dark and light both first-class, follow OS, switchable.
- macOS: vibrancy/translucent popover with arrow, rounded 12px, respects menu bar position, looks right on Retina and in a dark menu bar.
- Windows: Mica/acrylic if available (Win11), else solid. Anchored to tray, 8px radius, no title bar, proper DPI scaling at 125/150/200%. Tray icon must be legible at 16px on both light and dark taskbars.
- Tray icon is monochrome template on mac (adapts to menu bar), colored-if-needed on Windows. Same metaphor on both.
- Empty, loading, error, stale states each designed, not an afterthought.
- Settings page gets the same care as the popover.

When unsure about taste: build the smallest version, screenshot it, iterate. Don't ask the owner to pick between things you can decide.

## Provider facts (verified against CodexBar docs on 2026-10-16; endpoints are undocumented and may drift)

### Claude
- Usage: `GET https://api.anthropic.com/api/oauth/usage?cedar_ember=1`
  Headers: `Authorization: Bearer <access_token>`, `anthropic-beta: oauth-2025-04-20`.
  On 400/403 retry once without `cedar_ember`. Don't retry 401/429 that way.
- Identity: `GET https://api.anthropic.com/api/oauth/profile` (email, plan hints: `subscriptionType`, then `rate_limit_tier`).
- Response windows: `five_hour`, `seven_day`, `seven_day_sonnet`, `seven_day_opus`, `seven_day_routines`, `seven_day_cowork`, `limits[].weekly_scoped`, `extra_usage` (monthly spend/limit). Each has utilization + reset timestamp. If `five_hour` missing, fall back to `seven_day`.
- Token scope must include `user:profile`. `user:inference`-only tokens can't read usage.
- Token sources, in order: our own store → `$CLAUDE_CONFIG_DIR/.credentials.json` → `~/.claude/.credentials.json` → macOS Keychain item `Claude Code-credentials` (JSON with `claudeAiOauth{accessToken, refreshToken, expiresAt}`; may contain only `mcpOAuth` on some versions, treat as "no token"). On Windows, Claude Code uses `.credentials.json` in the config dir.
- Refresh tokens rotate. If the CLI also owns that account, let the CLI refresh; we just re-read the file. Only refresh ourselves for accounts we logged in via our own OAuth flow.
- Own OAuth login (for accounts not in any CLI): PKCE with Claude Code's public client id. Verify current client id, authorize and token URLs from the Claude Code CLI binary / docs before implementing; don't trust memory.
- Local cost history (optional, later): scan `~/.claude/projects/**/*.jsonl`, lines with `type:"assistant"` and `message.usage`, dedupe by `message.id + requestId`.

### Codex
- Usage: `GET https://chatgpt.com/backend-api/wham/usage`, `Authorization: Bearer <token>`. Base URL overridable by `chatgpt_base_url` in `$CODEX_HOME/config.toml`.
- Response: `rate_limit.primary_window` = 5h, `secondary_window` = weekly, `additional_rate_limits[]` = named extras (e.g. spark). Each has used % + reset timestamp. Plan from response, fallback JWT claims.
- Token: `~/.codex/auth.json` or `$CODEX_HOME/auth.json`. Contains OAuth tokens, `account_id`, `last_refresh`; email/plan in JWT claims. Inspect a real file on this Mac to pin the exact shape before coding.
- No refresh from our side. Stale → tell user to run `codex login` with that `CODEX_HOME`. Re-read file twice, 50ms apart, on partial writes.
- Optional: `GET .../backend-api/wham/rate-limit-reset-credits` (display only).

## Multi-account model

```
Account {
  id, provider: claude|codex, label, color,
  source: ConfigDir{path} | OwnOAuth | Manual,
  identity: {email?, plan?},
  lastSnapshot: {windows[], fetchedAt, error?}
}
```
- Discovery: scan default dirs + any `*claude*` / `*codex*` dotdirs in `$HOME`; user can add paths.
- Poll each account independently, staggered, backoff on 429. Default interval ~60s active window, slower when idle.
- "Make active": copies that account's credential file into the default config dir (what claude-swap does). Confirm before overwrite, back up previous.
- Tray shows one account (your call: worst remaining or pinned). Popover shows all.

## Architecture sketch (adjust freely)

```
src-tauri/
  core/providers/{claude,codex}.rs   trait Provider { discover(), fetch(account) -> Snapshot }
  core/accounts.rs                   store, discovery, make_active
  core/poller.rs                     scheduling, backoff, stale handling
  core/tray.rs                       icon rendering (generate PNG at runtime), menu
  commands.rs                        tauri::command surface
src/
  popover/                           Vue, main window
  settings/                          accounts, intervals, thresholds, autostart
  design/                            tokens, components
```
- Rust owns all secrets and all network. Frontend only sees snapshots.
- Fixtures for every provider response shape; unit test parsing without network.
- Windows: no native popover; frameless window anchored to tray via positioner, hide on blur.

## Workstreams (independent; run in parallel if you can)

1. Toolchain + Tauri scaffold + tray + frameless popover on mac.
2. Claude provider: discovery, parse, fetch, fixtures, tests.
3. Codex provider: same.
4. Design: name, 2–3 mockup directions, tokens, components.
5. CI: GitHub Actions matrix (macos-latest, windows-latest), artifacts, release on tag.

## Milestones

- **M1 Alive:** tray icon, popover opens, one Claude account read from `~/.claude`, bars + reset countdown. Screenshot it.
- **M2 Real:** both providers, N accounts, discovery, stale state, settings, autostart, notifications at thresholds.
- **M3 Ship:** chosen design applied, Windows build green in CI, README with install notes for unsigned builds, v0.1.0 tag.
- **M4 Nice:** updater, "make active", sparkline burndown, cost history.

## How to work

- Install rustup first, then `pnpm create tauri-app`. Verify `pnpm tauri dev` boots before anything else.
- Inspect real files on this machine (`~/.claude`, `~/.codex`) before writing parsers. Redact when logging.
- Hit the real endpoints early with the owner's own token to confirm shapes; save sanitized responses as fixtures.
- Keep the owner posted in short lines. Show screenshots. Decide small things yourself.
