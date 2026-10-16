Margin v0.1.1 gives settings and account actions a consistent appearance across macOS and Windows, using Reka UI controls styled for Margin.

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

Checks passed locally and in [release CI](https://github.com/Abulkosim/usagebar/actions/runs/38848296438): production build, 17 frontend unit tests, 48 Chromium/WebKit browser cases, 70 Rust unit tests, formatting, and strict Clippy. macOS universal and Windows x64 installers built successfully.

The downloaded installers match their SHA-256 checksums. The Mac DMG passed integrity checks; its app passed strict ad-hoc signature verification and includes Intel and Apple Silicon binaries. The final release app was installed and exercised on macOS: custom menus, keyboard selection, saved settings, focus return, and Escape behavior passed. Light/dark controls and confirmation focus were also checked in the native debug app. Public screenshots contain sample data only. See the [verification record](https://github.com/Abulkosim/usagebar/blob/main/docs/verification.md).

Interactive Windows testing, including taskbar placement and 125%, 150%, and 200% scaling, remains planned in the owner's VM.

Margin reads existing CLI OAuth credentials. Tokens and provider requests stay in Rust. It does not rotate CLI tokens, change active accounts, or spend reset credits. Independent OAuth login and automatic updates are not included.
