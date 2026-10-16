# Verification

Status must describe checks actually run, not the presence of test code. Record the date, command, platform, and outcome when changing this file.

## Patch candidate: v0.2.1

The owner reported clipping only on the external monitor; reopening restored the correct size. The previous macOS path sized in physical pixels using the target monitor scale, while Tao converted that size using the window’s current scale before an asynchronous move. A 1× destination with a 2× source could therefore produce a 210 × 330 point window. Its monitor lookup also mixed physical tray coordinates with a logical-point API.

The patch obtains the tray status item’s actual screen, calculates in Cocoa points, and applies the complete content frame synchronously on the main thread. Windows retains its existing placement logic.

- 75 Rust unit tests passed, including Cocoa geometry for displays to either side/above the main screen, arrow alignment, and small work areas. Formatting and strict Clippy passed.
- The opt-in native test used one hidden disposable AppKit window on this Mac’s real 2× built-in and 1× external screens. It reproduced 210 × 330 points under the old conversion, then verified 420 × 660 points and the expected backing scale on each screen. No visible window or display preference was changed.
- The native debug build rendered the full overview at 420 × 660, with the tray arrow and no outer scrollbars. The owner’s reported first-open interaction still needs confirmation after installation.
- The installed v0.2.0 app’s explicit update check successfully reported “You’re up to date” against the published manifest.
- Release CI and exact signed artifact verification are pending.

Run the optional real-display check in a macOS desktop session:

```sh
MARGIN_TEST_NATIVE_GEOMETRY=1 cargo test --manifest-path src-tauri/Cargo.toml --test native_geometry --locked
```

It uses AppKit on the process main thread, never orders the test window on screen, and does not access Margin accounts or settings. Normal CI skips this desktop-only check.

## Previous release: v0.2.0

**[v0.2.0 is published](https://github.com/Abulkosim/usagebar/releases/tag/v0.2.0).** Verified on 2026-10-16. This release adds explicit update checks and signed installation through GitHub Releases, and removes the clipped CSS shadow around native windows. The native OS shadow remains.

| Check | Result |
| --- | --- |
| Release preflight and build | Matching package, Cargo, and Tauri versions; production frontend build passed. Public updater key, HTTPS endpoint, and signed-version enforcement configured. |
| Automated checks | 26 frontend unit tests, 68 Chromium/WebKit cases, 73 Rust tests, 14 release-script tests, Rust formatting, and strict Clippy passed locally and in CI. Browser tests include both themes, native-frame transparency, updater progress/errors, retry, stale IPC, duplicate clicks, and navigation during a download. |
| Cross-platform CI | [Main run 38859821037](https://github.com/Abulkosim/usagebar/actions/runs/38859821037) and [tag run 38860790512](https://github.com/Abulkosim/usagebar/actions/runs/38860790512) passed for release commit `e27c77e`. The tag build produced signed updater artifacts and a complete manifest. |
| Signature and version checks | All three downloaded updater artifacts independently passed signature and signed-version verification. Release-script tests include a real Tauri CLI signature and rejection of altered bytes, wrong keys, mismatched versions, and unexpected files. |
| Real macOS updater installation | The ignored `updater_install` integration test passed against the exact downloaded universal release archive with `MARGIN_REQUIRE_UNIVERSAL=1`. It used the real plugin and public key through a loopback fixture. Corrupt downloads/installations and inflated manifest versions were rejected while preserving the old test app. Valid installation replaced only a disposable app; the resulting bundle reported 0.2.0, contained `arm64` and `x86_64`, and passed strict ad-hoc signature verification. This exercises the plugin installation path, not the UI's install button or privileged installation. |
| macOS release app | The DMG passed integrity verification. Every app file and symlink matched the updater-installed bundle. The verified universal app was installed in the owner's user Applications folder with the old app backed up. It launched with both existing accounts, the pinned account, and settings preserved; Settings displayed version 0.2.0 and the corrected window edge. The earlier native debug test verified both light/dark edges and recoverable updater errors before the manifest was published. Notifications and autostart remained off. |
| Release assets and endpoint | All eight listed SHA-256 checksums matched; all nine uploaded asset digests, including the checksum file, matched local files. The manifest includes both Mac architectures and separate Windows NSIS/MSI targets. Anonymous installer/checksum downloads returned HTTP 200; the public latest updater manifest returned version 0.2.0. Release/manifest notes were finalized before publication without changing signed binaries. |
| Windows | x64 compile, Rust checks, NSIS/MSI packaging, checksums, updater signatures, and the MSI x64 template passed. The owner reported successful interactive testing of v0.1.1. Interactive v0.2.0 Windows self-updating remains unverified. |
| Signing setup | The owner explicitly approved the dedicated updater key, owner-only local backup, and encrypted repository Actions secret. Only trusted release-tag builds receive it. No paid service or OS publisher certificate was configured. |

v0.1.1 and earlier require one manual upgrade. Subsequent releases use Settings → General → Updates; checks/downloads are explicit, and installation requires the Install and restart action. Publisher signing/notarization, privileged macOS installation, and interactive Windows self-updating remain separate checks.

## Published release: v0.1.1

**v0.1.1 is published.** This update replaces platform-rendered controls with Reka UI selects, tooltips, switches, tabs, and account-removal confirmation. It keeps Margin's existing visual direction and provider behavior. The v0.1.0 results below are historical evidence, not verification of this update.

| v0.1.1 check | Status | Evidence or remaining work |
| --- | --- | --- |
| Version consistency | Passed locally | `node scripts/check-release.mjs v0.1.1` passed. Package, Cargo manifest, Cargo lock entry, and Tauri versions all match `0.1.1`; Reka UI remains pinned to `2.11.0`. |
| Frontend build and unit tests | Passed locally and in CI | `pnpm build` and all 17 Node unit tests passed. |
| Browser interaction and appearance | Passed locally and in CI | All 48 Playwright cases passed: 24 Chromium and 24 WebKit. Covers select keyboard/typeahead navigation, complete visibility of long-list options, tooltip dismissal, switches, tabs, confirmation focus, failed saves/removals, and both themes. Failure scenarios use a mocked desktop bridge. |
| Rust regression checks | Passed locally and in CI | All 70 Rust unit tests, formatting, and strict Clippy with `-D warnings` passed. |
| Windows owner testing | Passed, owner-reported | On 2026-10-16 the owner reported checking the Windows app and finding everything okay. Exact OS build and per-scenario results were not supplied. A subsequent macOS screenshot identified the dark perimeter addressed in v0.2.0. |
| Native macOS controls | Passed in debug and release apps | Light/dark menus were visually inspected. Keyboard selection changed refresh from 1 to 2 minutes, saved, and restored trigger focus; the original interval was restored. Escape closed the select while keeping Settings open. The removal dialog focused Keep account; Escape preserved the account and Settings, then restored trigger focus. Light appearance was restored. The downloaded universal release app was installed and launched with both existing accounts and the pinned account preserved. Its version read 0.1.1; custom menus, keyboard saves, focus return, and Escape behavior passed again. The original one-minute refresh interval was restored. Native notifications and autostart remained off and were not tested. |
| macOS universal and Windows x64 CI | Passed | [Main run 38847806220](https://github.com/Abulkosim/usagebar/actions/runs/38847806220) and [tag run 38848296438](https://github.com/Abulkosim/usagebar/actions/runs/38848296438) passed for the original release commit `ba4a557`. Both native jobs passed Rust checks and produced installers; the tag run prepared the release draft. |
| Screenshots | Verified locally | Five 840×1320 browser previews were generated and visually inspected: light/dark overview, light settings, and light/dark open settings menus. All show sample data; see [screenshot notes](screenshots.md). |
| Release artifacts and publication | Published | [v0.1.1](https://github.com/Abulkosim/usagebar/releases/tag/v0.1.1) includes a universal DMG, x64 EXE/MSI, and checksums. All SHA-256 values match. The DMG passed integrity checks, strict ad-hoc signature verification, version validation, and Intel/Apple Silicon architecture checks; it matches the installed tag artifact byte-for-byte. Windows installer formats and the MSI x64 template were verified. |

## Published baseline: v0.1.0

Verification on 2026-10-16. Local frontend/Rust checks, live provider requests, and packaged macOS smoke tests passed. [CI run 38840596545](https://github.com/Abulkosim/usagebar/actions/runs/38840596545) passed all frontend, macOS universal, and Windows x64 jobs for release commit `a03e880`. The [v0.1.0 tag run](https://github.com/Abulkosim/usagebar/actions/runs/38841132411) passed all jobs and prepared the release assets.

| Area | Status | Evidence or remaining work |
| --- | --- | --- |
| Dependency installation and locks | Passed locally | Normal dependency resolution and a subsequent frozen offline installation with pnpm 11.25.0 passed. Tauri CLI 2.12.3 is an exact development dependency. Both pnpm and Cargo lockfiles are present. |
| Frontend typecheck and production build | Passed locally and in CI | `pnpm build`, including `vue-tsc --noEmit`, passed locally on macOS and in CI. |
| Frontend unit tests | Passed locally and in CI | `pnpm test`: 17 passed, 0 failed. Includes IPC failures, out-of-order responses, subscription races, stale readings, countdowns, and settings validation. |
| Browser interaction | Passed locally and in CI | All 26 Playwright tests passed locally and in CI across Chromium and WebKit, using sample accounts and a mocked desktop bridge. Placement checks cover screen-edge arrows, stale responses, and Windows framing. |
| Browser screenshots | Verified locally | WebKit captures at 840×1320 pixels: dark and light overview plus light settings. All three were visually inspected and contain sample accounts only; see `docs/images/`. |
| Rust core and provider unit tests | Passed locally and in CI | All 70 Rust unit tests passed locally and on both native CI runners, including 35 provider tests and tray geometry at 100/125/150/200% scaling. Synthetic fixtures cover edge cases; sanitized live-schema fixtures preserve verified response structure with account-specific values replaced. Native checks passed on macOS and Windows runners. |
| Rust formatting and Clippy | Passed locally and in CI | Rust 1.99.1 locally: formatting check and strict `cargo clippy --all-targets --locked -- -D warnings` passed. Both native CI jobs passed the same checks. |
| Claude live usage and profile | Passed locally | Read-only OAuth usage and profile requests returned HTTP 200. The usage query with `cedar_ember=1` was accepted; session, weekly, and scoped windows were verified. |
| Codex live usage | Passed locally | Read-only OAuth usage request returned HTTP 200. A weekly primary window with no secondary window was verified and rendered without inventing a session window. |
| macOS packaged app | Initial smoke passed locally | On macOS 26.6.2, the packaged app launched and displayed live Claude and Codex usage, including scoped windows. The final universal release artifact was installed in the user Applications folder, launched, quit, and restarted. Dark appearance persisted across restart, then the original light preference was restored. The bundle passed strict signature verification. The final debug bundle also handled a live Claude rate limit while refreshing Codex. This Mac reported a hidden tray rectangle with zero height; such anchors now fall back to a centered panel without an arrow. Visible tray anchoring and the broader native checks below remain pending. |
| macOS universal build and DMG | Passed in CI | Apple Silicon/Intel app and DMG built. The downloaded DMG passed integrity verification; its app passed strict ad-hoc signature verification and contained both `arm64` and `x86_64` executable slices. |
| Windows compile and installers | Passed in CI | Windows x64 Rust checks and NSIS/MSI packaging passed. Downloaded EXE and MSI file formats were verified; interactive installation remains pending. |
| Windows native UI and DPI | Planned | Owner will test in a VM after the first CI-built release. |
| App icon files | Passed locally | PNG, ICNS, and ICO files decoded with Pillow. Native tray appearance remains unverified. |
| Release helper scripts | Passed locally | Node syntax checks and isolated fixtures: version/lock rejection, required installer formats, duplicate rejection, allowlisted copying, and SHA-256 verification. |
| Workflow YAML and action pins | Verified locally and in CI | Workflow, Dependabot, and issue form parse. All seven action pins were checked against official upstream refs; annotated tags were resolved to their commit SHAs. Native assets build before Rust checks. All three build/test jobs executed successfully. |
| Release preflight | Passed locally | `node scripts/check-release.mjs v0.1.0` confirms matching application versions and both dependency lockfiles. |
| Release automation | Passed | The annotated `v0.1.0` tag triggered all checks and produced a draft with a universal DMG, x64 EXE/MSI, and `SHA256SUMS.txt`. All downloaded checksums match; the Mac release asset matches the installed tag artifact byte-for-byte. |

Live probes did not log or save tokens, raw identities, or personal usage values. Public screenshots use explicit sample accounts. The main and tag CI runs passed. Release downloads passed SHA-256 verification, DMG integrity, strict ad-hoc signature, universal architecture, and Windows installer-format checks. Interactive Windows and DPI checks are planned separately in the owner's VM.

## Native smoke checks

The Windows cells below are the original detailed checklist. The owner subsequently reported successful Windows testing for v0.1.1 on 2026-10-16; individual scenario results and display-scale values were not recorded. Do not treat that report as validation of v0.2.0 installation or updating.

Use downloaded release artifacts and at least one authenticated account from each provider. Use a dedicated custom configuration folder for failure scenarios. Never replace a working CLI credential file just to test Margin.

| Check | macOS | Windows |
| --- | --- | --- |
| Install and launch using the documented unsigned-app flow | Pending | Pending |
| Click tray icon; popover appears on the correct screen and within display bounds | Pending | Pending |
| Hide on blur/Escape; reopen; second launch keeps one instance | Pending | Pending |
| Light, dark, and system themes; reduced-motion preference | Pending | Pending |
| Tray icon contrast on light/dark menu bars or taskbars | Pending | Pending |
| Settings remain usable at small display sizes and with keyboard navigation | Pending | Pending |
| Multiple accounts retain labels, pins, and last-known readings after restart | Observed with both existing accounts | Pending |
| Claude and Codex values match fresh provider responses; reset countdowns agree | Pending | Pending |
| Offline, expired-token, malformed-response, and rate-limit states preserve last good values | Pending | Pending |
| One failing account does not prevent another account refreshing | Observed: Claude rate-limited while Codex refreshed | Pending |
| Notification thresholds deduplicate; permission denial is handled | Pending | Pending |
| Launch at login reflects OS registration and can be disabled | Pending explicit approval for the native toggle test; remains off | Pending |
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

Commit identities were corrected on 2026-10-16. Every existing commit kept its original file tree, message, and date. Release tags now point to `862b8fd` for v0.1.0 (previously `a03e880`) and `7e16644` for v0.1.1 (previously `ba4a557`); published installers and checksums are unchanged. The linked CI runs retain the original commit IDs.

Target: `Abulkosim/usagebar`. Commit identity: `abulkosim <abulkasimkattaev@gmail.com>`.

Release commit `e27c77e` is pushed to `main` and tagged `v0.2.0`. Main and tag CI passed. [Margin v0.2.0](https://github.com/Abulkosim/usagebar/releases/tag/v0.2.0) is published as the latest release with installers, updater signatures, the update manifest, and checksums. v0.1.1 remains available for manual rollback. The README and release notes explain the one-time manual upgrade and the distinction between free updater signatures and OS publisher certificates.
