# Launch posts (plan 3.13)

**Drafts. Post in your own words. One community per day. Reply to every comment.**

Post only after v0.5.0 is published (bodies describe v0.5.0 features). Say you're the developer in every post. Never "bypass limits"; Margin displays them.

Shared facts (keep them true):
- Free, MIT. macOS 12+ (universal), Windows x64. No Homebrew/winget yet.
- Reads existing Claude Code / Codex OAuth sign-ins, calls the providers' OAuth usage endpoints. No cookies, no CLI scraping, no telemetry. Never writes CLI credential files.
- Unsigned. macOS 15+: System Settings → Privacy & Security → Open Anyway. Windows: More info → Run anyway. Checksums + `gh attestation verify`.
- Endpoints are undocumented and may change. API-key-only accounts are unsupported.

Answers that repeat across posts:
- **Why not CodexBar?** CodexBar is great and Mac-only, with many providers. Margin does two providers, runs on macOS and Windows from one codebase, and is built around several accounts per provider (each config folder, incl. macOS Keychain items per `CLAUDE_CONFIG_DIR`). OAuth-only by design.
- **Does it bypass limits?** No. It shows them.
- **Claude/Codex already show usage.** For the signed-in account, inside the tool. Margin shows all accounts, both providers, while the CLI is idle, with alerts and 7-day history.
- **Is my token safe?** Tokens stay in the Rust process; the UI never gets them. Read-only, no backend. Exact files, Keychain items, hosts: PRIVACY.md.
- **Why unsigned?** Signing certificates cost money; this is a free project. Checksums and build attestations are on each release. Windows signing via SignPath is planned.

---

## r/ClaudeAI

Rules (sidebar via a Reddit mirror, 2026-10-22): the rules text itself was not fetchable. Visible post flairs include **Built with Claude**; no "Showcase" flair seen. Use **Built with Claude** only if true (it was built with Claude Code); that flair historically expected "what you built, how you built it, screenshots". 👤 Read the sub's rules page before posting.

**Title:** I built a free menu bar app that shows Claude Code usage left for all my accounts

**Body:**
> I run Claude Code with a few accounts in separate `CLAUDE_CONFIG_DIR` folders and kept running `/usage` in each one. So I built Margin (I'm the developer). It's a menu bar app (macOS) and tray app (Windows) that shows session, weekly, and per-model usage left plus reset times for every account at once. It also does Codex.
>
> - Reads the sign-ins Claude Code already stored and asks the OAuth usage endpoint. No cookies, no scraping, no telemetry.
> - Read-only: never writes your credential files.
> - Alerts at thresholds, a pace line ("runs out ~Thu 14:00"), 7-day history.
>
> Built with Claude Code (Tauri + Rust + Vue).
>
> Free, MIT. Builds are unsigned: on macOS 15+ use Privacy & Security → Open Anyway.
>
> https://github.com/Abulkosim/margin

**Attach:** `docs/images/overview-dark.png` (or the 3.3 GIF).

**Expect in comments:**
- "Why not CodexBar?" → see shared answer.
- "Doesn't `/usage` do this?" → For one account, in the session. This covers all of them while idle.
- "Is this against ToS?" → It uses your own token against the same account's usage endpoint, read-only. Endpoints are undocumented and could change; if they do, Margin shows last-known values as stale, never guesses.

## r/ClaudeCode

Rules: not fetchable. Visible flairs (mirror): Humor, **Built with Claude**, Help/Question, Discussion, Rant, Bug / Issue, Tips & Workflows. 👤 Check rules first.

**Title:** Usage left for several Claude Code accounts in one menu bar (free, open source)

**Body:**
> If you run more than one Claude Code account via `CLAUDE_CONFIG_DIR`, Margin shows each account's session and weekly limits, model limits (Opus, Sonnet...), and reset times in the menu bar (macOS) or tray (Windows). I'm the developer.
>
> On macOS, Claude Code stores extra accounts in Keychain items named per config folder; Margin finds those too. Signed out? It shows the exact command to run.
>
> What it doesn't do: no cookies, no CLI scraping, no writing to your credentials, no token refresh, no API-key accounts. Tray icon follows the most constrained account or one you pin.
>
> Free, MIT, unsigned builds (Open Anyway on macOS 15+).
>
> https://github.com/Abulkosim/margin

**Attach:** `docs/images/overview-light.png` + `docs/images/history-light.png`.

**Expect in comments:**
- "How does it find my accounts?" → `~/.claude`, `$CLAUDE_CONFIG_DIR`, home dotfolders containing "claude", or folders you add.
- "Can it switch the active account?" → Not yet. Planned, with confirmation and backup.
- "Cost tracking like ccusage?" → No. It shows quota remaining, not token cost. Local cost history is planned.

## r/codex

Verified: r/codex exists (~222k members, "the information and discussion subreddit for OpenAI Codex tools", not official). Flairs (mirror): Question, **Showcase**, Complaint, Praise, Limits, Reset, Humor, Bug, Dots. Sidebar line: "Posting priority favors those who help others here." Rules text not fetchable. r/OpenaiCodex is the older, archived sub.

Flair: **Showcase**.

**Title:** Free tray app: Codex 5h and weekly limits for multiple accounts, Mac and Windows

**Body:**
> I built Margin (developer here) to see Codex usage without opening a session. It reads `auth.json` from each `CODEX_HOME` you use and shows the 5h and weekly windows, extra limits, and reset times in the menu bar (macOS) or tray (Windows). It also shows available limit resets read-only; it never uses them. Claude Code accounts work too.
>
> - Uses the usage endpoint with your existing token; honors `chatgpt_base_url`.
> - No cookies, no telemetry, never writes `auth.json`, never refreshes tokens.
> - Signed out? Run `codex login` with that `CODEX_HOME`.
>
> Free, MIT. Unsigned builds: Windows "More info → Run anyway".
>
> https://github.com/Abulkosim/margin

**Attach:** `docs/images/overview-dark.png`.

**Expect in comments:**
- "Does it use my reset credits?" → Never. It only displays the count.
- "Codex shows limits already." → In the session, one account. This is all accounts, idle, with alerts.
- "Linux?" → Not now. Tauri could, but it isn't built or tested.

## r/macapps

Rules (sidebar via mirror, 2026-10-22), quoted:
- "No main feed promotion unless you qualify." "Use PCP Post template." "Open Source? Prefix title [OS]"
- "Flair Hierarchy: Lifetime>Subscription>Free"
- Rule 3: "not permitted more than once per developer in 30 days." "Always disclose your relationship to your software". "Promoting your own app in comments is disallowed until you earn 10 karma in r/MacApps".
- Rule 8: everyone who does not qualify "through trust or transparency requirements must limit promotion to the monthly Megathread."
- Trust path (from search snippets of the megathread, unverified): Mac App Store, or GitHub project with 1+ year history and 100+ stars, or recognized dev flair; plus 10+ local karma. **Margin does not qualify for Trust.** Transparency path details were not fetchable.

So: 👤 post in the monthly **App Pile megathread** as a comment in PCP format, unless the Transparency path is confirmed. Flair if main feed: **Free**.

**PCP comment (megathread):**
> **[OS] Margin – Claude Code and Codex usage in the menu bar**
>
> **Problem:** I use several Claude Code and Codex accounts and couldn't see how much quota each had left without opening each CLI.
>
> **Comparison:** CodexBar (Mac-only, many providers). Margin does two providers, many accounts each, and also runs on Windows. OAuth-only: no cookies, read-only, no telemetry.
>
> **Pricing:** Free, MIT. https://github.com/Abulkosim/margin
>
> I'm the developer. Unsigned build: System Settings → Privacy & Security → Open Anyway.

**Main-feed title (only if qualified):** `[OS] Margin – Claude Code and Codex usage left, many accounts, in the menu bar`

**Attach:** `docs/images/overview-light.png`.

**Expect in comments:**
- "Why not native Swift?" → Tauri gives one codebase for macOS and Windows. The popover and tray icon were designed for macOS first.
- "Not notarized, why trust it?" → Source is public, PRIVACY.md lists every file and host, releases have checksums and build attestations.
- "Why not CodexBar?" → shared answer.

## r/windowsapps

Verified active (~20k members). Flairs (mirror): **Developer**, App, Support, Question. Rules text not fetchable. 👤 Check rules first. Flair: **Developer**.

**Title:** Margin: free tray app showing Claude Code and Codex usage left, multiple accounts

**Body:**
> I'm the developer. Margin sits in the system tray and shows how much Claude Code and Codex usage is left for each account, with reset times. The tray ring follows the most constrained account and turns amber/red at low levels, matching a light or dark taskbar.
>
> - Reads existing CLI sign-ins (`%USERPROFILE%\.claude`, `.codex`, or folders you add).
> - Read-only, no cookies, no telemetry. Optional alerts and launch at login.
>
> Windows x64, free, MIT. Not code-signed yet: SmartScreen shows "Windows protected your PC" → More info → Run anyway. Smart App Control blocks unsigned apps entirely. Checksums and build attestations are on the release page.
>
> https://github.com/Abulkosim/margin

**Attach:** a real Windows tray screenshot from the VM (👤), else `docs/images/overview-light.png`.

**Expect in comments:**
- "Microsoft Store / winget?" → Not yet; winget is planned.
- "ARM64?" → x64 only for now.
- "Antivirus flagged it." → Unsigned Tauri installers sometimes trigger heuristics. Compare SHA256SUMS and the attestation; report false positives.

## X / Twitter

Under 280 characters (231 including the URL):

> Margin: a free, open-source menu bar and tray app that shows Claude Code and Codex usage left for every account you use, with reset times. macOS and Windows. Read-only, no cookies, no telemetry.
>
> https://github.com/Abulkosim/margin

**Attach:** the 15s demo GIF from plan 3.3 (tray → popover → countdown → history) once it exists. Until then `docs/images/overview-dark.png`.

**Expect:** "vs CodexBar?" → shared answer, short. "Bypass?" → "No, it shows the limits."

---

## Show HN (last)

Post only after README hero (3.1), demo GIF (3.3), and v0.5.0 are done, and after the Reddit posts settle. Owner-written; this is a starting point. HN dislikes marketing: plain, first person, technical. Link to the repo, not the site.

**Title** (80 chars, at the limit; shorter fallback: `Show HN: Margin – Claude Code and Codex usage for many accounts` (63)):

```
Show HN: Margin – Claude Code and Codex usage across accounts, macOS and Windows
```

**URL:** `https://github.com/Abulkosim/margin`

**First comment:**
> I use several Claude Code and Codex accounts (separate config folders per client/project), and the only way to see what was left was to open each CLI and run `/usage`, or wait for a rate-limit message. Margin puts every account's session, weekly, and per-model limits with reset times in the macOS menu bar or Windows tray.
>
> How it works: it reads the OAuth sign-ins the CLIs already stored (`.credentials.json`, the macOS Keychain items Claude Code creates per `CLAUDE_CONFIG_DIR`, Codex `auth.json`) and calls each provider's OAuth usage endpoint. Those endpoints are undocumented, so when a request fails it keeps the last reading with its age instead of guessing. It never writes credential files, never refreshes the CLI's tokens, and doesn't read cookies or scrape the CLI.
>
> Stack: Tauri 2, Rust for credentials, polling and the tray icon (rendered at runtime), Vue for the popover. Tokens never leave the Rust process; the WebView only gets snapshots. History is local, 7 days.
>
> It's free and MIT. Builds aren't signed yet (no paid certificates), so first launch needs Open Anyway / Run anyway; releases have checksums and GitHub build attestations.
>
> CodexBar inspired it. Differences: Windows, two providers only, multiple accounts per provider.
>
> Happy to answer questions, especially about the Keychain lookup or the tray icon rendering.

**Expect in comments:**
- "Undocumented endpoints will break." → Yes. Fixtures and tests per response shape; failure shows stale data with age, not wrong numbers.
- "Why Tauri, not native?" → One codebase for two OSes; Rust core is small and testable. Trade-off: WebView popover.
- "Is reading Keychain items safe?" → Only `Claude Code-credentials` and the per-folder suffixed items for folders Margin knows; macOS prompts for access. Listed in PRIVACY.md.
