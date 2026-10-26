# Releasing Margin

Releases are unsigned by a verified publisher. macOS uses ad-hoc signing for Apple Silicon compatibility; this does not provide notarization or remove Gatekeeper prompts. See [Tauri's signing guidance](https://v2.tauri.app/distribute/sign/macos/#ad-hoc-signing).

## Before tagging

1. Run frontend checks and the macOS native smoke checks in [verification.md](verification.md).
2. Commit both `pnpm-lock.yaml` and `src-tauri/Cargo.lock`. Frontend installs use `--frozen-lockfile`; Rust checks and builds use `--locked`.
3. Set the same version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
4. Write `changelogs/vX.Y.Z.md` and add it to `CHANGELOG.md`. Use the release date and a few short bullets describing user-visible changes. Link to installation instructions instead of repeating them. The release workflow uses this file for GitHub release notes and the update manifest.
5. Run `node --test scripts/*.test.mjs` and `node scripts/check-release.mjs v0.3.0`, substituting the intended version. Release validation requires the public updater key, the GitHub manifest URL, and signed-version enforcement.
6. Push the release commit and confirm all checks on `main` pass. Create and push the matching annotated tag.

Never put real credential files, token values, account IDs, or unredacted provider responses into commits, artifacts, screenshots, or release notes.

## What CI does

- Ubuntu: installs the pinned pnpm version, checks types, builds the interface, runs unit tests, updater release tests, and browser tests.
- macOS: checks Rust formatting, builds frontend assets, runs Clippy and unit tests, builds universal Apple Silicon/Intel app and DMG bundles.
- Windows: checks Rust formatting, builds frontend assets, runs Clippy and unit tests, builds x64 NSIS and MSI installers.
- Release tag pushes in `Abulkosim/margin` only: signs updater artifacts, validates matching versions and lockfiles, waits for all jobs to pass, verifies each updater signature and its signed version, generates `latest.json` and SHA-256 checksums, generates an SPDX SBOM of the Rust and npm lockfiles, records GitHub build provenance for the four installers, `latest.json` and `SHA256SUMS.txt`, attests the SBOM against the four installers, and creates or updates a draft GitHub release.

Actions are pinned to commit SHAs. Dependabot proposes action, frontend, and Rust dependency updates. Builds have read-only repository access; only the release job has write access, plus the OIDC token and attestation permissions needed for provenance. Pull requests, main builds, forks, and manual workflow runs disable updater artifact generation and receive no signing secrets. The release pipeline uses the existing standard GitHub runners and GitHub Releases; no additional hosted service is required.

## Publish

Download and exercise the macOS artifact from the draft release. The owner confirmed Windows v0.1.1 works in their VM. New release changes still need appropriate Windows checks; build success alone does not verify installation, tray placement, rendering, or DPI behavior.

Record the tested OS versions and remaining limitations in `docs/verification.md` and the release notes, then publish through GitHub Releases. The workflow refuses to overwrite assets on a published release; use a new patch version for fixes.

Require green frontend and native CI jobs before publishing. Verify the downloaded files against `SHA256SUMS.txt`. The release contains the DMG, universal `.app.tar.gz`, NSIS EXE, MSI, three updater `.sig` files, `latest.json`, and `margin-X.Y.Z.sbom.spdx.json`; checksums cover the first eight files. Verify provenance and the SBOM attestation for the downloaded installers:

```sh
for f in Margin_X.Y.Z_universal.dmg Margin.app.tar.gz Margin_X.Y.Z_x64-setup.exe Margin_X.Y.Z_x64_en-US.msi latest.json SHA256SUMS.txt; do
  gh attestation verify "$f" --repo Abulkosim/margin
done
gh attestation verify Margin_X.Y.Z_universal.dmg --repo Abulkosim/margin --predicate-type https://spdx.dev/Document/v2.3
``` The pipeline refuses missing, duplicate, unexpected, incorrectly signed, or mismatched-version updater assets.

## Updates

Tauri updater signatures are free. They authenticate downloads against the public key embedded in Margin; they do not provide macOS notarization or Windows Authenticode publisher signing. Initial installation prompts still apply.

Before the first updater release:

1. Generate a dedicated key with `pnpm tauri signer generate --write-keys <secure-path>/margin.key`. Store the private key and its password outside the repository, with a secure backup. Losing the key prevents existing installations from trusting future updates.
2. Put the public `.pub` contents in `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`. Keep `bundle.createUpdaterArtifacts` and `plugins.updater.requireSignedVersion` enabled.
3. Add the private key contents to the repository's `TAURI_SIGNING_PRIVATE_KEY` Actions secret and, if encrypted, its password to `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. An unset password secret resolves to an empty password. Never put the private key or password in source control, build artifacts, logs, or release assets.
4. Test an upgrade from an older test build with the same public key into the actual release artifact. Check both successful installation and rejection of altered artifacts. Record what was tested in `verification.md` before announcing the feature.

On macOS, download the draft's `Margin.app.tar.gz` and `.sig` into an artifact directory, then run:

```sh
MARGIN_UPDATE_ARTIFACT_DIR="$PWD/.cache/release-v0.3.0" MARGIN_REQUIRE_UNIVERSAL=1 cargo test --manifest-path src-tauri/Cargo.toml --test updater_install --locked -- --ignored --nocapture
```

This ignored integration test uses the pinned updater plugin and real public key with a loopback-only fixture server. It rejects altered bytes and mismatched signed versions before replacing a disposable old app, checks the installed version, verifies ad-hoc signing and both Mac architectures, and retains the installed copy under `.cache/upgrade-smoke/` for a launch check. It never opens the installed Margin app or CLI credentials. The fixture allows HTTP only inside the test; production still requires HTTPS. Run it against the exact CI release artifact before publishing. For an early local architecture-only build, omit `MARGIN_REQUIRE_UNIVERSAL`; the test still requires the current Mac architecture. It does not exercise the app's user interface, privileged installation, or Windows installation.

`latest.json` is served directly from the latest published GitHub release. Both macOS architectures use the universal archive. Windows entries preserve the NSIS or MSI installation family, with NSIS as the generic fallback. The updater plugin verifies the artifact signature and the signed version before installation; the release script independently verifies both using Node's Ed25519 and Blake2b primitives. The script tests include a real Tauri CLI signature fixture; no private fixture key is committed.

v0.1.1 and earlier have no updater and need one manual upgrade to the first updater-enabled release. Publish the complete asset set together; a draft does not replace the current public update endpoint. Never edit a published release's signed files or reuse a version. To stop offering a bad release, mark it as a prerelease so the latest-release endpoint returns to the previous stable release, then ship a higher fixed version. Clients already updated do not automatically downgrade.

## Roll back v0.8.0

Quit Margin and reinstall [v0.7.0](https://github.com/Abulkosim/margin/releases/tag/v0.7.0). It ignores `weeklyDigest`, the stored digest summary and `balance` on snapshots, and keeps everything else. A weekly-only Codex window moved to the weekly slot by v0.8.0 shows as Session again in v0.7.0 until its next refresh. To go back further, reinstall [v0.6.0](https://github.com/Abulkosim/margin/releases/tag/v0.6.0). It ignores `burnAlerts`, `burnThreshold` and the fast-alert memory, and keeps everything else. Switches already made stay made: the CLI folders keep their swapped sign-ins, and v0.6.0 shows each account under its new folder. Margin backups in the Keychain (`Margin backup`) or `backups/` stay until you delete them. To go back to v0.5.1 from there: it ignores `accountOrder`; history keeps the same format. To go back further, reinstall [v0.4.0](https://github.com/Abulkosim/margin/releases/tag/v0.4.0): it drops the v0.5 fields (`trayMetric`, `trayShowPercent`, `overviewLayout`, `notifyOnReset`, `autoCheckUpdates`, `ignoredSources`, `lastUpdateCheckAt`) with recovery notes and keeps accounts, intervals, theme, and history. Removed accounts may reappear on Find accounts. CLI credentials remain untouched. Report the failed interaction and platform, then ship the fix as a new patch release.

## Package managers

Files live in `packaging/`. CI jobs `homebrew` and `winget` in `ci.yml` stay skipped until their repository variable is `true`. They run only on a manual dispatch from a **published** release tag, because draft assets are not public and `winget-releaser` refuses drafts:

```sh
gh workflow run ci.yml --ref vX.Y.Z
```

A dispatch also re-runs the frontend and native checks; the release job stays skipped. Job-level `if:` cannot read `secrets`, so each job is gated on a variable and fails if the variable is on but its secret is missing.

### Homebrew (personal tap)

`packaging/homebrew/Casks/margin.rb` is the seed cask; `update-cask.mjs` rewrites `version` and `sha256` from the release's `SHA256SUMS.txt` (`node packaging/homebrew/update-cask.mjs <cask.rb> vX.Y.Z SHA256SUMS.txt`). Cask stanzas follow the [Cask Cookbook](https://docs.brew.sh/Cask-Cookbook) and [livecheck strategies](https://docs.brew.sh/Brew-Livecheck).

👤 Owner steps:

1. Create the public repo `Abulkosim/homebrew-tap` with a `Casks/` folder (the job copies the seed cask in on first run).
2. Create a fine-grained PAT limited to that repo with **Contents: Read and write**.
3. In `Abulkosim/margin` → Settings → Secrets and variables → Actions: add secret `HOMEBREW_TAP_TOKEN`, then variable `HOMEBREW_TAP_ENABLED=true`.
4. After publishing a release, dispatch as above. Check with `brew install --cask Abulkosim/tap/margin` and `brew livecheck --cask Abulkosim/tap/margin`.

`auto_updates true` tells `brew upgrade` to leave updates to Margin's own updater. The cask is not notarized; Homebrew's Gatekeeper rules for `homebrew/cask` do not bind a personal tap, but users still see the macOS block on first launch (the cask's caveats explain Open Anyway).

### winget

`packaging/winget/Abulkosim.Margin/0.4.0/` holds the version, installer and en-US locale manifests for the NSIS setup (`InstallerType: nullsoft`, user scope, `/S`). `ManifestVersion` is 1.12.0, what `wingetcreate` 1.12 currently emits in `microsoft/winget-pkgs`; schema docs list up to 1.28.0 ([schema index](https://github.com/microsoft/winget-pkgs/tree/master/doc/manifest)). Later versions are generated by [`winget-releaser`](https://github.com/vedantmgoyal9/winget-releaser), which needs one version already in the community repo.

👤 Owner steps:

1. Fork `microsoft/winget-pkgs` to `Abulkosim`.
2. First submission, manual: on Windows, `winget validate --manifest packaging\winget\Abulkosim.Margin\0.4.0`, test with `winget install --manifest packaging\winget\Abulkosim.Margin\0.4.0`, then either `wingetcreate submit --token <PAT> packaging\winget\Abulkosim.Margin\0.4.0` or copy the folder to `manifests/a/Abulkosim/Margin/0.4.0/` in the fork and open a PR. If the version is stale by then, run `wingetcreate update Abulkosim.Margin` against the new EXE instead.
3. Create a **classic** PAT with `public_repo` (add `workflow` so the action can sync the fork). Fine-grained PATs are not supported by the action.
4. After the first PR merges: add secret `WINGET_TOKEN` and variable `WINGET_ENABLED=true`. Dispatch after each published release; the action opens the PR from the fork.
5. If Defender or SmartScreen flags the installer during review, submit it at the [Microsoft Security Intelligence portal](https://www.microsoft.com/wdsi/filesubmission).

TODO 👤: the MSI could replace the EXE (`InstallerType: wix`), but its `ProductCode` must be read on Windows first (`wingetcreate new <MSI URL>` fills it in). Until then winget ships the NSIS EXE only.

### README once live

```sh
brew install --cask Abulkosim/tap/margin
winget install Abulkosim.Margin
```
