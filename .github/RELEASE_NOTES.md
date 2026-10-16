Margin v0.2.1 fixes a macOS popover that could open too small and cut off the interface.

## Fixed

- Keep the popover at its intended size when opening it from the menu bar on Retina and mixed-scale displays.
- Place and size the Mac window together, using the tray icon’s actual screen.
- Keep the panel inside the available screen area, with its arrow aligned to the tray icon.

## Install

Already on v0.2.0? Open **Settings → General → Updates**, check for updates, then install and restart. v0.1.1 and earlier need one manual upgrade.

- **macOS 12+:** universal DMG for Apple Silicon and Intel.
- **Windows x64:** setup EXE or MSI. First installation may need internet access for WebView2.
- **Checksums:** compare your download with `SHA256SUMS.txt`.

Updater signatures are separate from OS publisher certificates. These builds remain ad-hoc signed on macOS, without notarization, and unsigned by a verified Windows publisher. The existing [first-launch instructions](https://github.com/Abulkosim/usagebar#install) still apply.

## Verification

Release verification is in progress. This draft will be updated with completed checks before publication.

The owner reported successful Windows testing of v0.1.1. Interactive Windows self-updating and privileged macOS installation remain unverified. See the [verification record](https://github.com/Abulkosim/usagebar/blob/main/docs/verification.md).

Margin reads existing CLI OAuth credentials. Tokens and provider requests stay in Rust. It does not rotate CLI tokens, change active accounts, or spend reset credits. Update checks contact GitHub only when requested and send no account credentials.
