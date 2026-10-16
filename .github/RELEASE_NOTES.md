Margin v0.2.0 brings updates into Settings and cleans up the popover edge. Same quiet view of your Claude and Codex accounts.

## Changed

- Check for updates from Settings → General → Updates.
- Install a newer release when you choose, with download progress and a restart. No background checks or downloads.
- Verify each update and its version against Margin’s signing key before installation.
- Remove the clipped extra shadow around the desktop popover, keeping the native window shadow.

## Install

v0.1.1 and earlier need one manual upgrade from this release. After that, use the Updates section.

- **macOS 12+:** universal DMG for Apple Silicon and Intel.
- **Windows x64:** setup EXE or MSI. First installation may need internet access for WebView2.
- **Checksums:** compare your download with `SHA256SUMS.txt`.

Updater signatures are separate from OS publisher certificates. These builds remain ad-hoc signed on macOS, without notarization, and unsigned by a verified Windows publisher. The existing [first-launch instructions](https://github.com/Abulkosim/usagebar#install) still apply.

## Verification

[Release CI](https://github.com/Abulkosim/usagebar/actions/runs/38860790512) passed: 26 frontend unit tests, 68 Chromium/WebKit browser cases, 73 Rust tests, 14 release-script tests, strict Clippy, and macOS universal/Windows x64 packaging.

The exact signed universal Mac archive passed a real updater installation test. Altered files and mismatched signed versions were rejected without replacing the test app. The installed app passed version, architecture, and strict ad-hoc signature checks. The DMG contains the same app; all release checksums match. The release was installed and launched on macOS with existing accounts and settings preserved.

The owner reported successful Windows testing of v0.1.1. Interactive Windows self-updating and privileged macOS installation remain unverified. See the [verification record](https://github.com/Abulkosim/usagebar/blob/main/docs/verification.md).

Margin reads existing CLI OAuth credentials. Tokens and provider requests stay in Rust. It does not rotate CLI tokens, change active accounts, or spend reset credits. Update checks contact GitHub only when requested and send no account credentials.
