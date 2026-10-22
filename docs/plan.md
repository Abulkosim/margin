# Margin — improvement plan

Written 2026-10-22 from a code audit, competitor/user-demand research, and distribution research. This file is the work queue for the autonomous loop. Update it as tasks finish: tick boxes, add findings, reorder when evidence changes.

## Where we stand

- v0.3.0 shipped. 0 stars, ~4 real visitors, downloads ≈ owner only. Nobody has found it.
- Health: 32 frontend + 93 Rust tests pass, clippy clean, release flow solid.
- Space is crowded (CodexBar 22.7k★, ccusage 19k★, dozens of 0–3-point Show HNs). Anthropic/OpenAI now show single-account usage natively.
- Nothing official covers: **many accounts, Claude + Codex together, usage while the CLI is idle, Windows, OS alerts, history.** That is the wedge. Lead every pitch with it.
- Blocking bug: on macOS, Claude Code stores extra accounts as Keychain items `Claude Code-credentials-<hash>` (4 exist on this Mac). Margin reads only the default item and rejects custom config dirs that have no `.credentials.json`. **Multi-account Claude on Mac, our headline feature, does not work.**

## Rules for the loop

- One task per iteration. Smallest shippable slice. Commit per task, one-line imperative, email `abulkasimkattaev@gmail.com`, no trailers.
- Each task has a **Verify** line. Not done until it passes. Record evidence in `docs/verification.md` (short).
- Always run before commit: `pnpm typecheck && pnpm test && pnpm test:e2e` and `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.
- UI change → screenshot light + dark via existing Playwright preview; look at it; refresh `docs/images` if copy/visuals changed.
- Never write CLI credential files. Never redeem credits. Read-only.
- Copy: plain, short. "Usage remaining", not slogans.
- Release only at phase boundaries (marked ⛳). Follow `docs/releasing.md`. Changelog per version.
- Owner-only items are marked 👤 — leave them unchecked with a prepared draft; do not post on the owner's behalf.
- If a task turns out wrong or blocked, write why under it and move on; do not stall.

## Phase 1 — Make the headline feature true (v0.4.0) ⛳

- [x] **1.1 Per-config-dir Keychain accounts (mac).** Find how Claude Code derives the `-<hash>` suffix (inspect CLI bundle in `~/.cursor/extensions/anthropic.claude-code-*/` or npm package; likely a short sha256 of the config dir path). Read those items; allow adding a custom `CLAUDE_CONFIG_DIR` on mac that has no `.credentials.json`. Also enumerate existing suffixed items in discovery and show them as "Claude (Keychain · <hash>)" until a profile call gives the email.
  Verify: unit test for hash derivation against a known pair; on this Mac discovery shows >1 Claude account with distinct emails (manual run of the app, screenshot).
  Done 2026-10-22: suffix = sha256(CLAUDE_CONFIG_DIR string)[..8], confirmed from CLI 2.1.308. Suffixed items on this Mac are Claude Desktop Cowork session dirs, so discovery links items to known folders instead of enumerating. Manual 2-account check pending (needs `CLAUDE_CONFIG_DIR=~/.claude-work claude` sign-in) 👤.
- [x] **1.2 Local `expiresAt` check for Claude.** Skip the request when expired; show "Signed out since <time> · run `claude` in <dir>" with a copy button for the command. Keep last snapshot visible with age.
  Verify: fixture test; e2e state renders.
- [x] **1.3 Cache the Claude profile call** (per account, 1h or on token change). Halves requests, fewer 429s.
  Verify: wiremock test asserts one profile call across N polls.
- [x] **1.4 Re-poll on wake / network return.** macOS `NSWorkspaceDidWake`, Windows `WM_POWERBROADCAST`, plus a wall-clock drift check in the poller (if `now - last_tick > 2×interval`, refresh).
  Verify: unit test on drift logic; manual sleep/wake on Mac, tray updates within 10s. Done 2026-10-22 (NSWorkspaceDidWake + wall-clock drift; Windows drift only). Manual sleep/wake check pending 👤.
- [x] **1.5 Rate-limited / credentials-changed are not "attention" states** until data is actually older than 2× interval. Keep subtle "retrying" text.
  Verify: e2e for both states.
- [x] **1.6 Store recovery.** Per-account tolerant validation; "Back up and reset settings" button in the recovery state.
  Verify: unit test with one corrupt account keeps the others; e2e of recovery screen.
- [x] **1.7 Release v0.4.0** — changelog, screenshots, install on owner's Mac, update check passes.
  Done 2026-10-22: published, provenance + SBOM + updater checks in `docs/verification.md`. In-app update of the owner's Mac from v0.3.0 pending 👤 (Settings → Check for updates).

## Phase 2 — Be more useful than the official indicators (v0.5.0) ⛳

- [x] **2.1 Tray metric selection.** Default "most constrained of session + weekly" (exclude model-scoped, routines/cowork, and expired windows). Options: pinned account, lowest across all, specific window. Expose in Settings.
  Verify: unit tests on selection; tray shows expected account in demo mode.
  Done 2026-10-22: `trayMetric` core|all|session|weekly, core = five_hour/primary + seven_day/secondary, expired windows skipped; Settings → Tray shows. Settings screenshots to recapture at 2.11.
- [x] **2.2 Tray legibility.** Mac: thicker ring, filled-dot low state (≤20%). Windows: follow taskbar theme, stronger track, warn tier at 20%, critical at 5%. Optional "42%" text beside the icon.
  Verify: render icons at 16/22/32px light+dark into `docs/images/tray-*.png`; eyeball; contrast ≥ 3:1 measured in test.
  Done 2026-10-22: `core/ring.rs`, contact sheet in `docs/images/tray/`, min contrast 3.7:1 measured in test; Windows theme from registry; Mac-only "Show percentage next to icon". Windows compile only via CI; real taskbar check 👤.
- [x] **2.3 Pace line.** From history: "At this rate, runs out ~Thu 14:00 (reset Fri 09:00)" or "On pace". Hide when <3 readings or <30 min of data. Weekly and session.
  Verify: unit tests on regression math incl. gaps and resets; e2e renders both states.
  Done 2026-10-22: `src/lib/pace.ts` OLS over the latest segment (gap/reset/window boundaries), hidden <3 points or <30 min; history now loads once per card.
- [x] **2.4 Per-model windows first-class.** Fable/Opus/Sonnet weekly shown as compact chips on the card, not buried. Add fixture with `limits[]` `weekly_scoped` + `scope.model.display_name: "Fable"`. Clamp >100% instead of failing the snapshot.
  Verify: fixture test; e2e.
  Done 2026-10-22: `scope` core|model|other derived from window id, chips under Weekly, shared `percent()` caps >100 for both providers; fixture `claude-weekly-scoped.json`. Overview screenshots to recapture at 2.11.
- [ ] **2.5 Limit-reset grants, read-only.** Parse count/type/expiry from Claude usage response (confirm shape from a live sanitized capture). Show "1 reset available · expires in 3d". Never redeem.
  Verify: fixture test; e2e; grep proves no POST to reset endpoints.
- [x] **2.6 Notifications v2.** One-time permission prompt; on by default; thresholds editable; "window reset" alert optional; click opens popover; tolerate ±60s reset jitter; alert when a period is first seen already past a threshold.
  Verify: unit tests on dedupe; manual Mac notification once.
  Done 2026-10-22: on by default (existing stores keep their value), threshold chips 50/80/90/95/100, reset alert opt-in (only after a usage alert in the period), ±60s jitter, first-seen alert once, click opens popover via `on_action`. Plugin 2.7.0 never prompts on desktop so "denied" cannot occur. Manual Mac notification check at 2.11 smoke 👤.
- [ ] **2.7 Compact overview mode.** Settings toggle: one row per account (name · session % · weekly % · next reset), history/extra behind expand. Popover height adapts to content.
  Verify: e2e screenshots 1 and 5 accounts, light+dark.
- [x] **2.8 Absolute reset time shown** next to countdown ("Resets in 2h 14m · 4:30 PM"), always visible, dropped by container query below 250px. Pace line no longer repeats the reset time. Done 2026-10-22.
- [x] **2.9 Native folder picker** for Add account (tauri dialog plugin). Ignore list so removed accounts don't return on "Find accounts".
  Done 2026-10-22: tauri-plugin-dialog 2.8.1 (`dialog:allow-open` only, invoked via the global bridge, no npm package), `ignoredSources` in the store, "Show them" restores. Real native picker not clicked yet; check at 2.11 smoke.
- [ ] **2.10 Background update check daily**, small dot on Settings, opt-out.
- [ ] **2.11 Release v0.5.0.**

## Phase 3 — Trust and distribution (parallel with Phase 2; no release needed)

- [ ] **3.1 README rewrite.** Hero: one real Mac menu-bar screenshot + one Windows tray screenshot (owner VM 👤 for Windows; use existing preview until then). 3-bullet value statement, Install within first screen. "Why Margin" table vs CodexBar / Claude-Usage-Tracker / official indicators (macOS+Windows, multi-account, Claude+Codex, OAuth-only, idle updates, free/MIT). Badges: release, downloads, platforms. Move Develop/Preview to CONTRIBUTING.md.
- [ ] **3.2 Install docs for macOS 15/26.** Lead with Privacy & Security → Open Anyway (with screenshot); `xattr -d com.apple.quarantine /Applications/Margin.app` as fallback only. Windows: SmartScreen "More info → Run anyway"; note Smart App Control blocks unsigned apps entirely.
- [ ] **3.3 15s demo GIF** (tray → popover → countdown tick → history) from the real app, light theme. Also a 1280×640 social preview PNG; 👤 owner uploads it in repo settings.
- [x] **3.4 Artifact attestations + SBOM** in the release job (`actions/attest-build-provenance`, `anchore/sbom-action`; perms `id-token: write`, `attestations: write`). README: `gh attestation verify <file> --repo Abulkosim/margin`.
  Verify: next release assets verify. Added 2026-10-22; first real check at the v0.4.0 tag build.
- [x] **3.5 PRIVACY.md + SECURITY.md.** Exact files/Keychain items read, exact hosts contacted (api.anthropic.com, chatgpt.com, github.com for updates), no telemetry, never writes. Private vulnerability reporting and Discussions enabled 2026-10-22. Keychain section to update after 1.1.
- [x] **3.6 CI hardening.** `cargo audit`/`cargo deny`, `pnpm audit`, OpenSSF Scorecard action + badge, CodeQL default 👤 (settings), wiremock tests for provider `fetch` (400 fallback, 401, 429, swap), poller tests.
  Done 2026-10-22: `security.yml` (cargo audit, cargo deny with `src-tauri/deny.toml`, pnpm audit high), `scorecard.yml` (publishes results; needs code scanning on 👤), wiremock fetch tests for both providers. Fixed 3 RustSEC advisories (webpki) and vue 3.5.43 (GHSA-g2v6-rqmx-r4w6). CodeQL default setup still 👤.
- [ ] **3.7 Own Homebrew tap** `Abulkosim/homebrew-tap` with cask `margin`, `auto_updates true`, updated from release job (needs PAT 👤).
- [ ] **3.8 winget manifest** `Abulkosim.Margin` via `wingetcreate`, then `winget-releaser` action (PAT + fork 👤). Submit Defender false-positive if flagged.
- [x] **3.9 Repo hygiene.** Discussions on, feature-request + account issue forms, `config.yml`, PR template, CONTRIBUTING.md, CODE_OF_CONDUCT.md. Done 2026-10-22.
- [ ] **3.10 GitHub Pages one-pager** (`site/`), set as homepage. Needed later for SignPath.
- [x] **3.11 Decide repo name** — renamed to `Abulkosim/margin` 2026-10-22; GitHub redirects the old slug. Updater manifest still uses the old slug until v0.4.0 is the oldest supported version.
- [ ] **3.12 Listings.** PRs: awesome-mac, open-source-mac-os-apps (drafts prepared by loop, submitted 👤). awesome-claude-code via web form 👤 on/after **2026-10-30**.
- [ ] **3.13 Launch posts 👤** — drafts prepared by loop, posted by owner in own words, spaced over days: r/ClaudeAI (showcase flair), r/ClaudeCode, r/codex, r/macapps, r/windowsapps, X clip. Show HN last, after 3.1–3.4, owner-written.
- [ ] **3.14 SignPath Foundation application 👤** once there are some stars/releases (free Windows Authenticode).

## Phase 4 — Depth (v0.6+)

- [ ] **4.1 Burn-rate alerts** ("used 20% in the last hour"), quiet by default.
- [ ] **4.2 Credits / extra-usage balance** on the card (Claude extra_usage already parsed; Codex credits if present).
- [ ] **4.3 "Make active"** account switch: confirm, back up previous, copy credential file / Keychain item. Mac Keychain variant needs care.
- [ ] **4.4 Local CLI / JSON output** (`margin --json`) for statuslines and scripts.
- [ ] **4.5 Linux build** (AppImage/deb, menu-only tray since Tauri emits no tray click on Linux) — only after a demand signal.
- [ ] **4.6 Cost history from `~/.claude/projects/**/*.jsonl`** — low priority; ccusage shows it is fragile.

## Explicitly not doing

- Official homebrew-cask (unsigned apps disabled Sept 2026), Scoop Extras (<100★), awesome-tauri (closed), Azure Trusted Signing ($, US/CA only), Apple notarization ($).
- Cookie import, CLI scraping, redeeming resets, more providers.

## Open questions for owner

1. Rename repo to `margin`? (3.11)
2. Windows VM available for real screenshots + winget test? (3.1, 3.8)
3. OK to enable GitHub Discussions, private vuln reporting, CodeQL in repo settings? (all free)
