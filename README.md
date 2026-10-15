# Margin

Claude and Codex usage, across accounts, in your menu bar.

Margin is a small macOS and Windows tray app for session limits, weekly limits, model windows, and reset countdowns. Built with Tauri 2, Rust, Vue 3, and TypeScript.

**In development.** Native builds, live provider requests, and Windows behavior still need verification. The browser preview uses clearly labeled sample accounts. See [verification status](docs/verification.md).

<p>
  <img src="docs/images/overview-dark.png" alt="Margin dark browser preview with sample accounts" width="300" />
  <img src="docs/images/overview-light.png" alt="Margin light browser preview with sample accounts" width="300" />
</p>

Browser previews with sample accounts. [Settings preview](docs/images/settings-light.png).

## Preview

Requires Node.js 24 and pnpm 11.25.0.

```sh
pnpm install --frozen-lockfile
pnpm dev
```

Append `?demo=1` to the local URL printed by Vite. This previews clearly labeled sample accounts without reading credentials or making provider requests. The plain URL shows a desktop-app entry screen. The [design study](design/directions.html) explores the initial visual directions.

## Install

Download installers from [GitHub Releases](https://github.com/Abulkosim/usagebar/releases) when a release is available. CI prepares a draft after frontend checks and both native builds pass; release notes state the verification completed.

### macOS

Use the universal `.dmg` on Apple Silicon or Intel with macOS 12 or later. Open it, move Margin to Applications, then launch Margin.

Early releases are ad-hoc signed, without an Apple developer signature or notarization. On older macOS versions, right-click Margin → **Open**. If macOS blocks it, open **System Settings → Privacy & Security → Open Anyway** after the first launch attempt. Follow [Apple's guidance](https://support.apple.com/en-us/102445); managed Macs may prohibit exceptions.

### Windows

Use the x64 `-setup.exe` installer, or `.msi` for Windows Installer. WebView2 is required; the [installer downloads its setup program](https://v2.tauri.app/distribute/windows-installer/#downloaded-bootstrapper) when needed, so first installation may need internet access. Windows builds run in CI; interactive VM testing is planned separately.

Early releases have no verified publisher signature. If SmartScreen shows its reputation warning, choose **More info → Run anyway** after checking the download source. A managed device may disable this option. See [Microsoft's SmartScreen overview](https://learn.microsoft.com/en-us/windows/security/operating-system-security/virus-and-threat-protection/microsoft-defender-smartscreen/).

Each release includes `SHA256SUMS.txt`. Compare the downloaded installer's checksum with that file using `shasum -a 256 <file>` on macOS or `Get-FileHash <file> -Algorithm SHA256` in PowerShell.

## Accounts

Sign in through Claude Code or Codex first. Margin reads those existing OAuth credentials. Add each extra configuration folder in Settings; keep each account in its own folder.

| Provider | Default credentials | Custom configuration |
| --- | --- | --- |
| Claude | `~/.claude/.credentials.json`; macOS `Claude Code-credentials` Keychain item as fallback | `CLAUDE_CONFIG_DIR` or an added folder |
| Codex | `~/.codex/auth.json` | `CODEX_HOME` or an added folder |

Discovery also checks home dotfolders containing `claude` or `codex`. It deduplicates configuration paths. On Windows, `~` means your user profile folder. Apps launched from Finder or the Start menu may not inherit terminal environment variables; add custom folders in Settings if needed.

Tokens and provider requests remain in Rust. The interface receives account details and usage snapshots. Margin does not write to CLI credential files, rotate CLI tokens, read browser cookies, or spend reset credits. App settings and last-known usage are stored locally; they can include account emails and folder paths.

An expired token or failed request leaves the last successful reading visible with its age and stale state. No reading means no percentage. Sign in again through the relevant CLI, then refresh Margin. For a custom Codex account, run `codex login` with that account's `CODEX_HOME` set. Claude usage requires the `user:profile` OAuth scope; inference-only tokens cannot read it.

Provider usage endpoints are undocumented and may change. A quota displayed here is a reading from the provider, not a guarantee that the next request will be accepted.

## Develop

Install [Tauri's platform prerequisites](https://v2.tauri.app/start/prerequisites/) and stable Rust with [rustup](https://rustup.rs). macOS needs Xcode Command Line Tools. Windows needs Microsoft C++ Build Tools with Desktop development with C++ and WebView2.

```sh
pnpm install --frozen-lockfile
pnpm tauri dev
```

Run the checks:

```sh
pnpm build
pnpm test
pnpm exec playwright install chromium webkit
pnpm test:e2e
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

`pnpm build` includes TypeScript checks and produces the frontend assets required by native Rust checks. Browser tests cover the preview interface; native behavior needs the [platform smoke checks](docs/verification.md#native-smoke-checks). Provider parsing tests use synthetic fixtures and no network; they do not establish compatibility with live responses.

Build an installer on the target OS with `pnpm tauri build`. The [release workflow](.github/workflows/ci.yml) builds macOS universal and Windows x64 installers and drafts releases on `v*` tags. Follow the [release runbook](docs/releasing.md) before tagging.

## Scope

Claude and Codex only. Multi-account usage, reset countdowns, stale states, tray selection, and carefully designed dark and light interfaces are the core.

Later: signed updates, explicit account activation with confirmation and backup, burndown history, and local cost history. Independent OAuth login remains deferred until the provider's current public client details are verified.

Inspired by [CodexBar](https://github.com/steipete/CodexBar), with a focused provider set and a shared macOS/Windows codebase. Margin is an independent project, unaffiliated with Anthropic or OpenAI.

[MIT](LICENSE) · [Report a bug](https://github.com/Abulkosim/usagebar/issues/new/choose)
