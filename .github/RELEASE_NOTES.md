Margin v0.1.1 gives settings and account actions a consistent appearance across macOS and Windows, using Reka UI controls styled for Margin.

**Draft — release verification is in progress.**

<p>
  <img src="https://raw.githubusercontent.com/Abulkosim/usagebar/v0.1.1/docs/images/settings-menu-light.png" alt="Margin light settings preview with a styled account menu and sample accounts" width="260" />
  <img src="https://raw.githubusercontent.com/Abulkosim/usagebar/v0.1.1/docs/images/settings-menu-dark.png" alt="Margin dark settings preview with a styled account menu and sample accounts" width="260" />
</p>

Browser interface previews with sample data.

## Changed

- Custom select menus with matching surfaces, selected states, and keyboard navigation.
- Styled switches and tabs that follow the app's light, dark, and system themes.
- Tooltips for compact actions, with consistent spacing and typography.
- Account-removal confirmation with explicit cancel and remove actions.
- Consistent fields and focus states within the existing layout and color palette.

## Downloads

- **macOS 12+:** universal `.dmg` for Apple Silicon and Intel.
- **Windows x64:** `-setup.exe` or `.msi`. WebView2 is required; first installation may need internet access.
- **Checksums:** `SHA256SUMS.txt` covers every installer.

These builds have no verified publisher signature. macOS bundles use ad-hoc signing and are not notarized. See the [installation instructions](https://github.com/Abulkosim/usagebar#install) before first launch.

## Verification and limits

Local checks passed: production build, 17 frontend unit tests, 70 Rust unit tests, formatting, strict Clippy, and all 48 browser tests across Chromium and WebKit. Native macOS debug checks passed for light/dark menus, keyboard selection and saved refresh intervals, Escape handling, and account-removal confirmation focus. Five sample screenshots were refreshed and visually inspected.

macOS/Windows CI and release artifacts still need final verification. See the [verification record](https://github.com/Abulkosim/usagebar/blob/main/docs/verification.md) for recorded results. Save/removal failure cases were exercised through a mocked desktop bridge; native OS notifications and launch at login were not tested in this update.

Interactive Windows testing, including taskbar placement and 125%, 150%, and 200% scaling, remains planned in the owner's VM.

Margin reads existing CLI OAuth credentials. Tokens and provider requests stay in Rust. It does not rotate CLI tokens, change active accounts, or spend reset credits. Independent OAuth login and automatic updates are not included.
