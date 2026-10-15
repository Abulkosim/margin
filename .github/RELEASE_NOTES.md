Claude and Codex usage, across accounts, in your menu bar.

- macOS: download the universal `.dmg` for Apple Silicon or Intel.
- Windows: download the x64 `-setup.exe` installer or `.msi`.
- `SHA256SUMS.txt` contains checksums for every installer.

These builds have no verified publisher signature. macOS bundles use ad-hoc signing and are not notarized. See the [installation instructions](https://github.com/Abulkosim/usagebar#install) before first launch.

Margin reads existing CLI OAuth credentials. Usage requests stay in the Rust backend. It does not rotate CLI tokens, change active accounts, or spend reset credits.

Windows installers are built in CI. Interactive Windows testing, including taskbar placement and 125%, 150%, and 200% scaling, is planned in a VM. Check [verification status](https://github.com/Abulkosim/usagebar/blob/main/docs/verification.md) for the exact tests completed.
