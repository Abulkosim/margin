# Verification

Status must describe checks actually run, not the presence of test code. Record the date, command, platform, and outcome when changing this file.

## Current release: v0.5.1

**[v0.5.1 is published](https://github.com/Abulkosim/margin/releases/tag/v0.5.1).** Published 2026-10-23 04:53 UTC; verified on 2026-10-22/23 on macOS 26.6.2. A layout patch: the popover opens straight on the accounts, the title block is gone, and provider filters sit in the header and only appear when both Claude and Codex accounts exist.

- `pnpm typecheck`, 60 frontend unit tests, 164 Chromium/WebKit browser cases, 164 Rust tests, formatting, and strict Clippy passed locally and in [main CI](https://github.com/Abulkosim/margin/actions/runs/39816861155) on `97fe546`. The [release build](https://github.com/Abulkosim/margin/actions/runs/39818075986) passed for the tag, including Windows x64.
- New browser tests assert the first account card starts under 72 CSS pixels from the top (it was about 206) and that the provider filters are absent with a single provider. The header shrinks and scrolls its filters on narrow Linux WebKit fonts, which had overflowed 360 px in CI; the expand re-measure test now polls instead of asserting once.
- All eight listed checksums matched. The exact universal Mac archive passed the real updater test: altered bytes and mismatched signed versions were rejected, and 0.5.1 installed for x86_64 and arm64. `latest.json` reports 0.5.1 and still points at the `Abulkosim/usagebar` slug; the public latest endpoint serves it.
- Attestations were not re-verified from this network (see v0.5.0). Native checks remain pending: in-app update of the owner's Mac, the compact header on a real menu bar, notifications, and Windows.

## Previous release: v0.5.0

**[v0.5.0 is published](https://github.com/Abulkosim/margin/releases/tag/v0.5.0).** Published 2026-10-22 16:19 UTC; verified on 2026-10-22 on macOS 26.6.2. Adds tray metric selection, a more legible tray ring, pace lines, model-limit chips, read-only reset grants, notifications on by default, absolute reset times, a native folder picker with an ignore list, daily update checks, and a compact overview with a popover that fits its content.

- `pnpm typecheck`, 60 frontend unit tests, 160 Chromium/WebKit browser cases, 164 Rust tests, formatting, strict Clippy, and 14 release-script tests passed locally on `d34bf86`. `node scripts/check-release.mjs v0.5.0` passed. The tag `v0.5.0` points at `3cbb8dc`, which adds only a longer frontend job timeout.
- Tray ring contrast is measured in a Rust test at 16/22/32 px on light and dark backgrounds; the minimum is 3.7:1 and the template image varies only in alpha. A contact sheet is in `docs/images/tray/`.
- Pace, reset-grant, window-scope, compact-summary, and reset-clock formatting have unit tests. Provider fetch paths are covered with wiremock for 400/403 retry without `cedar_ember`, 401, 429 with `retry-after`, 5xx, and the Codex reset-credits request, which is only made when the usage response reports a nonzero count. A test scans `src-tauri/src` and fails on any non-GET HTTP call.
- A local optimized build read both accounts within 5 seconds of launch, and the background update check contacted GitHub about two minutes after launch and recorded `lastUpdateCheckAt`. The installed app and its data were restored afterwards. Popover resizing, compact mode, chips, notifications, and the folder picker were checked in browser tests only; the native tray UI cannot be driven from this session.
- [Main CI](https://github.com/Abulkosim/margin/actions/runs/39782117895) passed for `d34bf86` and the [release build](https://github.com/Abulkosim/margin/actions/runs/39801428623) passed for the tag, including Windows x64, which compiles the registry theme read, dialog plugin, notification click handler, and popover placement for the first time. The first two tag builds were cancelled when the Ubuntu runner's apt mirror stalled for 20 minutes during Playwright's dependency install; the frontend job timeout is now 45 minutes.
- All eight listed checksums matched. The exact universal Mac archive passed the real updater test: altered bytes and mismatched signed versions were rejected, and 0.5.0 installed for x86_64 and arm64. `latest.json` still points at the `Abulkosim/usagebar` slug so v0.3.0 clients can update; GitHub redirects it.
- The attestations API reports two attestations (provenance and SBOM) for the DMG, Mac archive, installer, and MSI, and one for `latest.json` and `SHA256SUMS.txt`. Full `gh attestation verify` runs timed out on the verifying network on 2026-10-22; re-run them from another network before relying on them.
- Pending: in-app update of the owner's installed v0.3.0, a manual notification and sleep/wake check on the Mac, the real folder picker, Windows tray tiers and toast clicks, interactive Windows installation, and privileged macOS installation.

## Previous release: v0.4.0

**[v0.4.0 is published](https://github.com/Abulkosim/margin/releases/tag/v0.4.0).** Verified on 2026-10-22 on macOS 26.6.2. Adds Keychain-backed Claude accounts on macOS, a signed-out state, refresh after wake, calmer retry handling, settings recovery, and release provenance. The repository moved from `Abulkosim/usagebar` to `Abulkosim/margin` during this release.

- `pnpm typecheck`, 36 frontend unit tests, 88 Chromium/WebKit browser cases, 115 Rust tests, formatting, and strict Clippy passed locally on `5eda609`.
- Keychain suffix derivation (`sha256(CLAUDE_CONFIG_DIR)[..8]`) was confirmed against Claude Code CLI 2.1.308 and covered by unit tests. Discovery links Keychain items only to known config folders; the four suffixed items on this Mac belong to Claude Desktop session folders and were correctly ignored. A manual two-account sign-in check (`CLAUDE_CONFIG_DIR=~/.claude-work claude`) is still pending.
- Tests cover expired tokens, 401 → signed out, hourly profile caching, wall-clock drift after sleep, transient errors keeping fresh readings live, per-account tolerant settings loading, timestamped backups, and the reset command. The macOS wake notification path is implemented but its manual sleep/wake check is still pending.
- An optimized local Mac build read the Codex file and the Claude Keychain item, fetched both accounts within 8 seconds, and reported no errors.
- [Main CI](https://github.com/Abulkosim/margin/actions/runs/39760737594) and [release CI](https://github.com/Abulkosim/margin/actions/runs/39762408473) passed for `5eda609`, including frontend/browser checks on Ubuntu 24.04, strict Rust checks, macOS universal packaging, and Windows x64 packaging. The frontend job is pinned to `ubuntu-24.04` because Playwright 1.60 does not support Ubuntu 26.04.
- All eight listed checksums matched. `gh attestation verify --repo Abulkosim/margin` passed for the DMG, the Mac updater archive, the Windows installer, the MSI, `latest.json`, and `SHA256SUMS.txt`. The SPDX SBOM attestation passed for the DMG and installer. This is the first release with provenance and SBOM.
- `latest.json` deliberately points download URLs at the old `Abulkosim/usagebar` slug, which GitHub redirects; v0.3.0 clients only trust that prefix. v0.4.0 trusts both slugs and is covered by a unit test. Switch the manifest to `Abulkosim/margin` once v0.4.0 is the oldest supported version.
- The exact universal Mac archive passed the real updater test: altered bytes and mismatched signed versions were rejected, and valid installation passed for x86_64 and arm64. GitHub release notes and updater notes match `changelogs/v0.4.0.md`; the public latest endpoint serves the checked manifest.
- Pending: in-app update of the owner's installed v0.3.0 through **Check for updates** (manual, requires the tray UI), interactive Windows installation, and privileged macOS installation.

## Previous release: v0.3.0

**[v0.3.0 is published](https://github.com/Abulkosim/margin/releases/tag/v0.3.0).** Verified on 2026-10-17 on macOS 26.6.2. Adds local usage history and includes the readability patch below.

- `pnpm build`, 32 frontend unit tests, 80 Chromium/WebKit browser cases, 93 Rust tests, strict Clippy, formatting, and 14 release-script tests passed. The final compact chart was rechecked in all 12 history browser cases; its section fits within 350 CSS pixels at the standard width.
- History persistence tests cover atomic replacement, owner-only permissions, seven-day and 10,000-reading bounds, damaged files, write failures, restarts, missing windows, partial sampling, out-of-order readings, unknown identities, account removal, and workspace changes. Existing v1 settings remain readable; history uses separate files.
- Independent review found and fixed false gaps from independently sampled windows and history loss during temporary profile failures. Stable account/workspace identifiers are hashed for local history matching; tokens are never stored in history. Unknown identity pauses recording without replacing previous history.
- Chart tests cover honest period/gap boundaries, observed percentage-point changes, keyboard inspection, empty/error/loading states, validated IPC, and late responses. Light/dark previews were inspected at 1×/2× and 360 × 480. Seven public sample screenshots were refreshed and visually inspected.
- An optimized, ad-hoc-signed local Mac build launched successfully. Both existing accounts recorded live readings in separate owner-only files. The native interface progressed from its first-reading state to a chart, and keyboard inspection selected an earlier reading. Restart preserved the readings and marked the next point as resumed recording. Light/dark native checks passed; the original light preference was restored.
- [Main CI](https://github.com/Abulkosim/margin/actions/runs/39057547907) and [signed release CI](https://github.com/Abulkosim/margin/actions/runs/39058156374) passed for `9eff289`, including frontend/browser checks, strict Rust checks, macOS universal packaging, and Windows x64 packaging.
- All eight listed checksums and all nine uploaded asset digests matched. Three updater signatures, their signed versions, and all five manifest targets passed. GitHub release notes and updater notes match `changelogs/v0.3.0.md`; the public latest endpoint serves the checked manifest.
- The exact universal Mac archive passed the real updater test: altered bytes and mismatched signed versions were rejected, while valid installation passed version, both architectures, and strict ad-hoc signature checks. The DMG passed integrity verification and contains the identical app. Windows installer formats and the MSI x64 template were checked.
- The downloaded release launched with saved history intact. Native scrolling exposed the complete compact chart and legend. The installed v0.2.2 app then found and installed v0.3.0 through **Install and restart**. Both accounts, the pinned account, theme, polling intervals, disabled notification/autostart preferences, and recorded history were preserved. Settings reports version 0.3.0 and “Up to date.” The installed executable and plist match the verified release; strict signature validation passed.
- Interactive Windows installation/updating and privileged macOS installation remain unverified. The Windows results above cover automated checks and installer packaging, not native UI testing.

## Readability patch included in v0.3.0

Prepared on 2026-10-17 on macOS without a separate release or tag. The initial patch retained version 0.2.2; it is included in v0.3.0.

- Text now starts at 13px, main labels use 14px, and quota values use 21px. Reset times sit below their labels. Secondary text has stronger contrast in both themes; selects keep the default “Lowest remaining” value readable.
- `pnpm build` passed, including type checking. All 68 existing Chromium/WebKit cases passed after the final CSS changes. `git diff --check` passed.
- Light/dark overview, Settings, account lists, and open menus were visually inspected in WebKit at 420 × 660 CSS pixels, at both 1× and 2× scale. Five public sample screenshots were refreshed. Checked normal text/background token pairs meet 4.5:1 contrast; this is not a full accessibility audit.
- `pnpm tauri build --bundles app --ci --config '{"bundle":{"createUpdaterArtifacts":false}}' -- --locked` produced an optimized, ad-hoc-signed local Mac app. Automatic approval review initially blocked launch pending action-time confirmation. The owner subsequently authorized building, testing, and releasing v0.3.0; native light/dark inspection then passed with the history feature included.
- This initial patch changed only styles and screenshot documentation. The v0.3.0 work above adds usage history; native window geometry and updater behavior remain unchanged.

## Previous release: v0.2.2

**[v0.2.2 is published](https://github.com/Abulkosim/margin/releases/tag/v0.2.2).** Verified on 2026-10-16.

This release simplifies interface copy and adds per-version changelogs based on Git tags and published release dates. GitHub release notes and updater notes now use the same Markdown file. The native sizing fix is unchanged.

- Production frontend build, 26 frontend unit tests, 68 Chromium/WebKit browser cases, 75 Rust tests, and 14 release-script tests passed locally.
- All four historical release dates and changelog links were checked against repository history. The new changelog is included in release preflight checks.
- Five public WebKit screenshots were refreshed at 420 × 660 CSS pixels and 2× scale. Light/dark overviews and Settings menus were visually checked; all retain the sample-data label. The browser entry and empty state were also inspected.
- [Main CI](https://github.com/Abulkosim/margin/actions/runs/38865976350) and [release CI](https://github.com/Abulkosim/margin/actions/runs/38866794042) passed, including strict Clippy and both native installer builds.
- All eight listed checksums and all nine remote asset digests matched. All three updater signatures, signed versions, and five manifest targets passed. The GitHub release body and manifest notes match `changelogs/v0.2.2.md`.
- The exact signed universal Mac archive passed the real updater smoke test: altered files and mismatched versions were rejected, and valid installation passed version, both architectures, and strict ad-hoc signature checks. The DMG passed integrity checks and contains the same app.
- The downloaded release launched with “Usage remaining” and the shorter Settings copy, without clipping. The installed v0.2.1 app then found and installed v0.2.2 through **Install and restart**. Accounts, the pinned account, theme, polling intervals, and disabled notification/autostart preferences were preserved. Settings shows version 0.2.2 and a new check reports “Up to date.” The installed executable and plist match the verified release; strict signature validation passed.
- The four older GitHub release descriptions were replaced with their concise historical changelogs. Their published installers, manifests, signatures, and tags were not changed. Interactive Windows updating and privileged macOS installation remain unverified.

## Previous release: v0.2.1

**[v0.2.1 is published](https://github.com/Abulkosim/margin/releases/tag/v0.2.1).** Verified on 2026-10-16.

The owner reported clipping only on the external monitor; reopening restored the correct size. The previous macOS path sized in physical pixels using the target monitor scale, while Tao converted that size using the window’s current scale before an asynchronous move. A 1× destination with a 2× source could therefore produce a 210 × 330 point window. Its monitor lookup also mixed physical tray coordinates with a logical-point API.

The patch obtains the tray status item’s actual screen, calculates in Cocoa points, and applies the complete content frame synchronously on the main thread. Windows retains its existing placement logic.

- 75 Rust unit tests passed, including Cocoa geometry for displays to either side/above the main screen, arrow alignment, and small work areas. Formatting and strict Clippy passed.
- The opt-in native test used one hidden disposable AppKit window on this Mac’s real 2× built-in and 1× external screens. It reproduced 210 × 330 points under the old conversion, then verified 420 × 660 points and the expected backing scale on each screen. No visible window or display preference was changed.
- The native debug build rendered the full overview and Settings at 420 × 660, with the tray arrow and no outer scrollbars. The optimized production build also rendered without clipping at 840 × 1320 physical pixels on Retina. The owner’s reported first-open interaction still needs confirmation after installation.
- The installed v0.2.0 app’s explicit update check successfully reported “You’re up to date” against the published manifest.
- [Main CI](https://github.com/Abulkosim/margin/actions/runs/38863357727) passed: frontend/browser checks, Rust checks, macOS universal packaging, and Windows x64 packaging. [Signed release CI](https://github.com/Abulkosim/margin/actions/runs/38864063245) also passed. All nine uploaded asset digests matched local files; all eight listed checksums, three updater signatures, signed versions, and manifest targets passed. The DMG passed integrity verification and matched the updater-installed app.
- The exact universal signed archive passed the real updater installation smoke test, including tamper/version rejection, both architectures, and strict ad-hoc signature validation. It launched without clipping.
- The owner’s installed v0.2.0 app found v0.2.1 through Settings, installed it with **Install and restart**, and relaunched with both accounts and the pinned account preserved. The installed version is 0.2.1; its executable and plist match the verified universal release, and strict codesign verification passed. The previous installation is backed up locally. This verifies the user-level macOS UI update flow; Windows and privileged macOS updating remain unverified.

Run the optional real-display check in a macOS desktop session:

```sh
MARGIN_TEST_NATIVE_GEOMETRY=1 cargo test --manifest-path src-tauri/Cargo.toml --test native_geometry --locked
```

It uses AppKit on the process main thread, never orders the test window on screen, and does not access Margin accounts or settings. Normal CI skips this desktop-only check.

## Previous release: v0.2.0

**[v0.2.0 is published](https://github.com/Abulkosim/margin/releases/tag/v0.2.0).** Verified on 2026-10-16. This release adds explicit update checks and signed installation through GitHub Releases, and removes the clipped CSS shadow around native windows. The native OS shadow remains.

| Check | Result |
| --- | --- |
| Release preflight and build | Matching package, Cargo, and Tauri versions; production frontend build passed. Public updater key, HTTPS endpoint, and signed-version enforcement configured. |
| Automated checks | 26 frontend unit tests, 68 Chromium/WebKit cases, 73 Rust tests, 14 release-script tests, Rust formatting, and strict Clippy passed locally and in CI. Browser tests include both themes, native-frame transparency, updater progress/errors, retry, stale IPC, duplicate clicks, and navigation during a download. |
| Cross-platform CI | [Main run 38859821037](https://github.com/Abulkosim/margin/actions/runs/38859821037) and [tag run 38860790512](https://github.com/Abulkosim/margin/actions/runs/38860790512) passed for release commit `e27c77e`. The tag build produced signed updater artifacts and a complete manifest. |
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
| macOS universal and Windows x64 CI | Passed | [Main run 38847806220](https://github.com/Abulkosim/margin/actions/runs/38847806220) and [tag run 38848296438](https://github.com/Abulkosim/margin/actions/runs/38848296438) passed for the original release commit `ba4a557`. Both native jobs passed Rust checks and produced installers; the tag run prepared the release draft. |
| Screenshots | Verified locally | Five 840×1320 browser previews were generated and visually inspected: light/dark overview, light settings, and light/dark open settings menus. All show sample data; see [screenshot notes](screenshots.md). |
| Release artifacts and publication | Published | [v0.1.1](https://github.com/Abulkosim/margin/releases/tag/v0.1.1) includes a universal DMG, x64 EXE/MSI, and checksums. All SHA-256 values match. The DMG passed integrity checks, strict ad-hoc signature verification, version validation, and Intel/Apple Silicon architecture checks; it matches the installed tag artifact byte-for-byte. Windows installer formats and the MSI x64 template were verified. |

## Published baseline: v0.1.0

Verification on 2026-10-16. Local frontend/Rust checks, live provider requests, and packaged macOS smoke tests passed. [CI run 38840596545](https://github.com/Abulkosim/margin/actions/runs/38840596545) passed all frontend, macOS universal, and Windows x64 jobs for release commit `a03e880`. The [v0.1.0 tag run](https://github.com/Abulkosim/margin/actions/runs/38841132411) passed all jobs and prepared the release assets.

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

Target: `Abulkosim/margin`. Commit identity: `abulkosim <abulkasimkattaev@gmail.com>`.

Release commit `e13f59d` is pushed to `main` and tagged `v0.2.2`. Main and tag CI passed. [Margin v0.2.2](https://github.com/Abulkosim/margin/releases/tag/v0.2.2) is published as the latest release with installers, updater signatures, the changelog-backed update manifest, and checksums. The installed app was upgraded through its own UI. v0.2.1 remains available for rollback.
