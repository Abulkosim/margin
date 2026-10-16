# Verification

Status must describe checks actually run, not the presence of test code. Record the date, command, platform, and outcome when changing this file.

## Current status

Verification on 2026-10-16. Local frontend/Rust checks, live provider requests, and an initial packaged macOS smoke test passed. [CI run 38812812498](https://github.com/Abulkosim/usagebar/actions/runs/38812812498) passed all frontend, macOS universal, and Windows x64 jobs for commit `ab0cd94`. Subsequent changes need a fresh run; tag and release checks are pending.

| Area | Status | Evidence or remaining work |
| --- | --- | --- |
| Dependency installation and locks | Passed locally | Normal dependency resolution and a subsequent frozen offline installation with pnpm 11.25.0 passed. Tauri CLI 2.12.3 is an exact development dependency. Both pnpm and Cargo lockfiles are present. |
| Frontend typecheck and production build | Passed locally and in CI | `pnpm build`, including `vue-tsc --noEmit`, passed locally on macOS and in CI. |
| Frontend unit tests | Passed locally and in CI | `pnpm test`: 17 passed, 0 failed. Includes IPC failures, out-of-order responses, subscription races, stale readings, countdowns, and settings validation. |
| Browser interaction | Passed locally and in CI | All 26 Playwright tests passed locally across Chromium and WebKit, using sample accounts and a mocked desktop bridge. The initial CI run passed the prior 20-test suite. Placement checks cover screen-edge arrows, stale responses, and Windows framing. |
| Browser screenshots | Verified locally | WebKit captures at 840×1320 pixels: dark and light overview plus light settings. All three were visually inspected and contain sample accounts only; see `docs/images/`. |
| Rust core and provider unit tests | Passed locally and in CI | All 70 Rust unit tests passed locally, including 35 provider tests and tray geometry at 100/125/150/200% scaling. The initial CI run passed the prior 65-test suite. Synthetic fixtures cover edge cases; sanitized live-schema fixtures preserve verified response structure with account-specific values replaced. Native checks passed on macOS and Windows runners. |
| Rust formatting and Clippy | Passed locally and in CI | Rust 1.99.1 locally: formatting check and strict `cargo clippy --all-targets --locked -- -D warnings` passed. Both native CI jobs passed the same checks. |
| Claude live usage and profile | Passed locally | Read-only OAuth usage and profile requests returned HTTP 200. The usage query with `cedar_ember=1` was accepted; session, weekly, and scoped windows were verified. |
| Codex live usage | Passed locally | Read-only OAuth usage request returned HTTP 200. A weekly primary window with no secondary window was verified and rendered without inventing a session window. |
| macOS packaged app | Initial smoke passed locally | On macOS 26.6.2, the packaged app launched and displayed live Claude and Codex usage, including scoped windows. Appearance changes were saved to the local store; the bundle passed strict signature verification. The final debug bundle also handled a live Claude rate limit while refreshing Codex. This Mac reported a hidden tray rectangle with zero height; such anchors now fall back to a centered panel without an arrow. Visible tray anchoring and the broader native checks below remain pending. |
| macOS universal build and DMG | Passed in CI | Apple Silicon/Intel app and DMG built. The downloaded DMG passed integrity verification; its app passed strict ad-hoc signature verification and contained both `arm64` and `x86_64` executable slices. |
| Windows compile and installers | Passed in CI | Windows x64 Rust checks and NSIS/MSI packaging passed. Downloaded EXE and MSI file formats were verified; interactive installation remains pending. |
| Windows native UI and DPI | Planned | Owner will test in a VM after the first CI-built release. |
| App icon files | Passed locally | PNG, ICNS, and ICO files decoded with Pillow. Native tray appearance remains unverified. |
| Release helper scripts | Passed locally | Node syntax checks and isolated fixtures: version/lock rejection, required installer formats, duplicate rejection, allowlisted copying, and SHA-256 verification. |
| Workflow YAML and action pins | Verified locally and in CI | Workflow, Dependabot, and issue form parse. All seven action pins were checked against official upstream refs; annotated tags were resolved to their commit SHAs. Native assets build before Rust checks. All three build/test jobs executed successfully. |
| Release preflight | Passed locally | `node scripts/check-release.mjs v0.1.0` confirms matching application versions and both dependency lockfiles. |
| Release automation | Unverified | No pushed tag or successful release run recorded. |

Live probes did not log or save tokens, raw identities, or personal usage values. Public screenshots use explicit sample accounts. The initial CI build and downloaded installers are verified; the tag-triggered draft release still needs a recorded outcome. Interactive Windows and DPI checks are planned separately in the owner's VM.

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

The initial commit is pushed to `main`, the GitHub description and topics are updated, and the first CI run passed. A release tag and published release are pending.
