# Contributing

Margin shows Claude Code and Codex usage in the menu bar or system tray. Questions go to [Discussions](https://github.com/Abulkosim/margin/discussions); bugs and requests go to [issues](https://github.com/Abulkosim/margin/issues/new/choose); vulnerabilities go to [SECURITY.md](SECURITY.md).

## Scope and rules

- Two providers only: Claude and Codex.
- Usage comes from the providers' OAuth usage APIs only. No browser cookies, no CLI scraping, no running the CLIs.
- Read-only. Never write, move, or refresh CLI credential files or Keychain items. Never redeem or spend reset credits.
- If a request fails, show the last reading with its age. Never estimate a missing value.
- Tokens stay in Rust. The interface receives account details and snapshots only.
- No telemetry, analytics, or crash reporting. Network hosts are listed in [PRIVACY.md](PRIVACY.md#network-requests); adding one needs a strong reason and a PRIVACY.md update.
- Margin displays limits. Do not describe it as a way around them.

## Prerequisites

- Node.js 24 and pnpm 11.25.0 (pinned in `package.json`).
- Stable Rust via [rustup](https://rustup.rs), 1.90 or newer, with `clippy` and `rustfmt`.
- [Tauri's platform prerequisites](https://v2.tauri.app/start/prerequisites/). macOS: Xcode Command Line Tools. Windows: Microsoft C++ Build Tools (Desktop development with C++) and WebView2.
- Playwright browsers for e2e tests: `pnpm exec playwright install chromium webkit`.

## Run

```sh
pnpm install --frozen-lockfile
pnpm tauri dev
```

This reads your real CLI sign-ins. To work on the interface without credentials or network requests, use the browser preview:

```sh
pnpm dev
```

Open the printed URL with `?demo=1`. Sample accounts are labeled as such. The plain URL shows the desktop-app entry screen. Add `&theme=light` or `&theme=dark`, and `&state=` one of:

| State | Shows |
| --- | --- |
| `empty` | No accounts |
| `loading` | First refresh in progress |
| `stale` | Expired session with the last reading and its age |
| `error` | Expired session with no reading |
| `signed-out` | Signed-out account |
| `retrying` | Rate limited and changed-account retries |
| `recovery` | Damaged settings file |
| `recovered` | An account skipped while loading settings |
| `hidden` | Removed accounts kept out of discovery |

States are defined in [`src/lib/demo.ts`](src/lib/demo.ts). Public screenshots follow [docs/screenshots.md](docs/screenshots.md). The [design study](design/directions.html) shows the initial visual directions.

## Test

```sh
pnpm build        # type check and frontend assets; native checks need these
pnpm test
pnpm test:e2e
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

CI runs the same commands ([`ci.yml`](.github/workflows/ci.yml)). Browser tests cover the preview only; native behavior needs the [smoke checks](docs/verification.md#native-smoke-checks). Provider tests use [synthetic and sanitized fixtures](src-tauri/fixtures/README.md) without network. A provider parsing change needs a fixture and a regression test.

Never commit real tokens, credential files, account IDs, unredacted emails, or raw provider responses.

## Layout

```
src-tauri/src/
  commands.rs          Tauri command surface
  updates.rs           explicit update check and install
  core/
    providers/         claude.rs, codex.rs: credentials, fetch, parsing
    accounts.rs        discovery and account list
    poller.rs          refresh schedule, backoff, stale handling
    store.rs           local state file
    history.rs         local usage history
    notifications.rs   threshold notifications
    tray.rs, ring.rs   tray menu and icon rendering
src-tauri/fixtures/    provider response fixtures
src/
  App.vue, components/ popover, settings, history
  design/, styles.css  tokens and controls
  lib/                 state, formatting, demo data
tests/unit/, tests/e2e/
```

## Commits and pull requests

- One-line, imperative commit subjects, e.g. `Add a folder picker`. No trailers.
- Keep pull requests focused. Fill in the template checklist.
- User-facing changes add a short bullet to the next `changelogs/vX.Y.Z.md`.
- UI changes include light and dark screenshots from the preview, with sample data.
- Copy is short and plain: name the action or the information. "Usage remaining", not slogans.

## Releases

The maintainer tags releases. See [docs/releasing.md](docs/releasing.md). Checks actually run for each release are recorded in [docs/verification.md](docs/verification.md).

## Conduct

See [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
