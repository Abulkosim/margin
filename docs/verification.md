# Verification

Status must describe checks actually run, not the presence of test code. Record the date, command, platform, and outcome when changing this file.

## Current status

Verification on 2026-10-16. Frontend and Rust checks pass locally; native app and release checks are underway.

| Area | Status | Evidence or remaining work |
| --- | --- | --- |
| Dependency installation and locks | Passed locally | Online installation with pnpm 11.25.0 succeeded. The pnpm lock was regenerated with the normal resolver; Tauri CLI 2.12.3 is an exact development dependency. Cargo resolved the native dependencies and generated its lockfile. |
| Frontend typecheck and production build | Passed locally | `pnpm build`, including `vue-tsc --noEmit`, passed on macOS. |
| Frontend unit tests | Passed locally | `pnpm test`: 17 passed, 0 failed. Includes IPC failures, out-of-order responses, subscription races, stale readings, countdowns, and settings validation. |
| Browser interaction | Passed locally | All 20 Playwright tests passed across Chromium and WebKit, using explicitly labeled demo accounts. |
| Browser screenshots | Verified locally | WebKit captures at 840×1320 pixels: dark and light overview plus light settings. All three were visually inspected and contain sample accounts only; see `docs/images/`. |
| Rust core and provider unit tests | Passed locally | All 61 Rust unit tests passed. Five provider fixtures are synthetic; fixture tests do not establish live endpoint compatibility. |
| Rust formatting and Clippy | Passed locally | Rust 1.99.1: formatting check and strict `cargo clippy --all-targets -- -D warnings` passed. |
| Claude live usage | Unverified | No successful live endpoint request recorded. |
| Codex live usage | Unverified | No successful live endpoint request recorded. |
| macOS tray and native popover | In progress | Native launch and tray interaction being checked; no completed result recorded yet. |
| Windows compile and installers | Unverified | Workflow prepared; no successful run recorded. |
| Windows native UI and DPI | Planned | Owner will test in a VM after the first CI-built release. |
| App icon files | Passed locally | PNG, ICNS, and ICO files decoded with Pillow. Native tray appearance remains unverified. |
| Release helper scripts | Passed locally | Node syntax checks and isolated fixtures: version/lock rejection, required installer formats, duplicate rejection, allowlisted copying, and SHA-256 verification. |
| Workflow YAML and action pins | Verified locally | Workflow, Dependabot, and issue form parse. All seven action pins were checked against official upstream refs; annotated tags were resolved to their commit SHAs. Native assets build before Rust checks. GitHub execution remains unverified. |
| Release preflight | Passed locally | `node scripts/check-release.mjs v0.1.0` confirms matching application versions and both dependency lockfiles. |
| Release automation | Unverified | No pushed tag or successful release run recorded. |

Earlier environment restrictions are resolved. Live provider requests, native macOS behavior, and GitHub CI results still need recorded outcomes. Windows compilation and packaging run in CI; interactive Windows and DPI checks are planned separately in the owner's VM.

## Native smoke checks

Use downloaded release artifacts and at least one authenticated account from each provider. Use a dedicated custom configuration folder for failure scenarios. Never replace a working CLI credential file just to test Margin.

| Check | macOS | Windows |
| --- | --- | --- |
| Install and launch using the documented unsigned-app flow | Pending | Pending |
| Click tray icon; popover appears on the correct screen and within display bounds | Pending | Pending |
| Hide on blur/Escape; reopen; second launch keeps one instance | Pending | Pending |
| Light, dark, and system themes; reduced-motion preference | Pending | Pending |
| Tray icon contrast on light/dark menu bars or taskbars | Pending | Pending |
| Settings remain usable at small display sizes and with keyboard navigation | Pending | Pending |
| Multiple accounts retain labels, pins, and last-known readings after restart | Pending | Pending |
| Claude and Codex values match fresh provider responses; reset countdowns agree | Pending | Pending |
| Offline, expired-token, malformed-response, and rate-limit states preserve last good values | Pending | Pending |
| One failing account does not prevent another account refreshing | Pending | Pending |
| Notification thresholds deduplicate; permission denial is handled | Pending | Pending |
| Launch at login reflects OS registration and can be disabled | Pending | Pending |
| Suspend/resume and display changes do not strand the popover | Pending | Pending |
| Retina layout and popover placement follow the menu-bar icon | Pending | N/A |
| 100%, 125%, 150%, and 200% scaling; taskbar and secondary monitor placement | N/A | Pending |
| Uninstall exits cleanly and leaves CLI credentials untouched | Pending | Pending |

Confirm CLI credential files are unchanged after normal use. Treat account emails, configuration paths, screenshots, and snapshots as private even though they contain no tokens. Share only sanitized fixtures.

## Boundary checks

- Browser preview always identifies sample data and never presents it as current usage.
- IPC output contains snapshots and account metadata, never access or refresh tokens.
- Missing or invalid provider windows do not become 0% used or 100% remaining.
- A failed request retains the prior successful timestamp and value.
- Out-of-order IPC responses cannot replace newer state.
- Damaged settings stay untouched; the app shows a recovery message and blocks writes.
- HTTP 429 follows provider backoff; manual refresh cannot create a tight retry loop.
- Claude retries without `cedar_ember` only after HTTP 400 or 403.
- Codex retries partial credential writes without writing to its source file.
- Provider requests do not follow a credential-bearing redirect to another host.
- No workflow or application path redeems credits or automatically activates an account.

## Repository

Target: `Abulkosim/usagebar`. Commit identity: `abulkosim <abulkosimkattaev@gmail.com>`.

Git is initialized on `main`, the remote is configured, and the GitHub description and topics are updated. First push, CI, and release are pending.
