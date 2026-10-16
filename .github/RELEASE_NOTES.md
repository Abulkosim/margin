Margin brings Claude Code and Codex usage to the macOS menu bar and Windows system tray, across multiple accounts.

<p>
  <img src="https://raw.githubusercontent.com/Abulkosim/usagebar/v0.1.0/docs/images/overview-dark.png" alt="Margin dark interface with sample Claude and Codex accounts" width="300" />
  <img src="https://raw.githubusercontent.com/Abulkosim/usagebar/v0.1.0/docs/images/overview-light.png" alt="Margin light interface with sample Claude and Codex accounts" width="300" />
</p>

Browser interface previews with sample data.

## Included

- Existing CLI account discovery and custom configuration folders.
- Remaining quota and reset countdowns for session, weekly, and model-specific windows; Claude extra-usage spend when available.
- A tray indicator for a pinned account or the account with the least remaining quota.
- Last-known readings with clear stale states after failed requests.
- Light, dark, and system themes; configurable polling; optional threshold notifications and launch at login.

## Downloads

- **macOS 12+:** universal `.dmg` for Apple Silicon and Intel.
- **Windows x64:** `-setup.exe` or `.msi`. WebView2 is required; first installation may need internet access.
- **Checksums:** `SHA256SUMS.txt` covers every installer.

These builds have no verified publisher signature. macOS bundles use ad-hoc signing and are not notarized. See the [installation instructions](https://github.com/Abulkosim/usagebar#install) before first launch.

## Verification and limits

Local checks passed: 70 Rust unit tests, strict Clippy, 17 frontend unit tests, and 26 browser tests across Chromium and WebKit. The packaged macOS app displayed live Claude and Codex usage, including scoped windows, and preserved appearance settings across restart. Live Claude usage/profile and Codex usage requests succeeded.

The [release CI run](https://github.com/Abulkosim/usagebar/actions/runs/38841132411) passed every job, including macOS universal and Windows x64 builds. All downloaded installer checksums match. The Mac package passed DMG integrity, strict ad-hoc signature, universal architecture, installation, launch, and restart checks. Windows installer formats were verified.

Interactive Windows testing, including taskbar placement and 125%, 150%, and 200% scaling, is planned in the owner's VM. See [verification status](https://github.com/Abulkosim/usagebar/blob/main/docs/verification.md) for recorded results and remaining checks.

Margin reads existing CLI OAuth credentials. Tokens and provider requests stay in Rust. It does not rotate CLI tokens, change active accounts, or spend reset credits. Provider endpoints are undocumented and may change; failed requests preserve the last successful reading. Independent OAuth login and automatic updates are not included.
