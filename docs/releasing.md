# Releasing Margin

Releases are unsigned by a verified publisher. macOS uses ad-hoc signing for Apple Silicon compatibility; this does not provide notarization or remove Gatekeeper prompts. See [Tauri's signing guidance](https://v2.tauri.app/distribute/sign/macos/#ad-hoc-signing).

## Before tagging

1. Run frontend checks and the macOS native smoke checks in [verification.md](verification.md).
2. Commit both `pnpm-lock.yaml` and `src-tauri/Cargo.lock`. Frontend installs use `--frozen-lockfile`; Rust checks and builds use `--locked`.
3. Set the same version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
4. Update `.github/RELEASE_NOTES.md` with the actual changes, limitations, and unsigned installation guidance.
5. Run `node scripts/check-release.mjs v0.1.1`, substituting the intended version.
6. Push the release commit and confirm all checks on `main` pass. Create and push the matching annotated tag.

Never put real credential files, token values, account IDs, or unredacted provider responses into commits, artifacts, screenshots, or release notes.

## What CI does

- Ubuntu: installs the pinned pnpm version, checks types, builds the interface, runs unit tests and browser tests.
- macOS: checks Rust formatting, builds frontend assets, runs Clippy and unit tests, builds universal Apple Silicon/Intel app and DMG bundles.
- Windows: checks Rust formatting, builds frontend assets, runs Clippy and unit tests, builds x64 NSIS and MSI installers.
- Tag only: validates matching versions and lockfiles, waits for all jobs to pass, gathers installers, generates SHA-256 checksums, and creates or updates a draft GitHub release.

Actions are pinned to commit SHAs. Dependabot proposes action, frontend, and Rust dependency updates. Builds have read-only repository access; only the release job has write access. No signing secrets are needed for these early builds.

## Publish

Download and exercise the macOS artifact from the draft release. Windows compilation and installer packaging are verified by CI for the initial release. Interactive Windows checks, including 125%, 150%, and 200% scaling, are planned in the owner's VM afterward; disclose that limitation in the release notes.

Record the tested OS versions and remaining limitations in `docs/verification.md` and the release notes, then publish through GitHub Releases. The workflow refuses to overwrite assets on a published release; use a new patch version for fixes.

Require green frontend and native CI jobs before publishing. A Windows build proves compilation and packaging; tray placement, rendering, notifications, and DPI behavior remain explicitly unverified until the VM checks run.

## Updates

The updater is deferred. Do not advertise automatic updates or publish an updater manifest until a signing key, verification path, and tested upgrade flow are configured. macOS notarization and Windows Authenticode signing are separate from Tauri updater signatures.

## Roll back v0.1.1

If a control regression blocks normal use, quit Margin and reinstall [v0.1.0](https://github.com/Abulkosim/usagebar/releases/tag/v0.1.0). v0.1.1 does not change the settings schema; keep the existing local settings. CLI credentials remain untouched. Report the failed interaction and platform, then ship the fix as a new patch release.
