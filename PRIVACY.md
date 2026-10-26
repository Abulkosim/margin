# Privacy

Margin reads sign-in data that Claude Code and Codex already store on your computer. It uses that data to request usage from each provider, then shows the result. No Margin account or hosted backend is involved.

This page describes Margin's current release. It is derived from the source code; see the files linked in each section.

## Summary

- No telemetry, analytics, crash reporting, or ads.
- Network requests go only to the provider usage endpoints and to GitHub Releases for update checks.
- CLI credential files are read, never written.
- Tokens stay in the Rust process. The interface never receives them.
- Margin stores settings, last-known usage, and usage history on your computer only.

## What Margin reads

### Files and folders

| Provider | Read | Source |
| --- | --- | --- |
| Claude | `<config folder>/.credentials.json` | [`claude.rs`](src-tauri/src/core/providers/claude.rs) |
| Codex | `<config folder>/auth.json` | [`codex.rs`](src-tauri/src/core/providers/codex.rs) |
| Codex | `<config folder>/config.toml`, only the `chatgpt_base_url` value | [`codex.rs`](src-tauri/src/core/providers/codex.rs) |

Configuration folders come from these places ([`accounts.rs`](src-tauri/src/core/accounts.rs)):

- `~/.claude` and `~/.codex`.
- The folders named by the `CLAUDE_CONFIG_DIR` and `CODEX_HOME` environment variables, when set.
- Hidden folders directly in your home folder whose name contains `claude` or `codex` (case-insensitive). Margin lists your home folder to find them. A folder is added only if it contains the provider's credential file or, for Claude on macOS, has a matching Keychain item.
- Folders you add in Settings.

On Windows, `~` is your user profile folder.

From Claude credentials, Margin uses the `claudeAiOauth` access token, its scopes, and plan hints. From Codex credentials, it uses the access token, the account ID, and the email and plan claims inside the tokens. Refresh tokens are not used.

### macOS Keychain

On macOS only, Claude Code stores tokens in Keychain instead of `.credentials.json`. Margin lists Keychain item names with `/usr/bin/security dump-keychain`. This reads names and attributes only, never passwords. It then reads generic password items with `/usr/bin/security find-generic-password -s <service> -w`:

- `Claude Code-credentials`, for `~/.claude`. Read during discovery when no `~/.claude` account is found, and at refresh before `~/.claude/.credentials.json`, the same order Claude Code uses.
- `Claude Code-credentials-<suffix>`, for another Claude configuration folder Margin knows about. The suffix is the first 8 hex characters of the SHA-256 of the NFC-normalized folder path, as Claude Code computes it. Read at refresh before that folder's `.credentials.json`.

Items that match no known folder are ignored. macOS may ask you to allow access. No other Keychain items are read.

## Network requests

All requests use HTTPS. Provider requests do not follow redirects. Each request sends a `Margin/<version>` user agent.

| Host | Request | When | Sent |
| --- | --- | --- | --- |
| `api.anthropic.com` | `GET /api/oauth/usage` (with `?cedar_ember=1`, retried once without it on 400 or 403) | Each refresh of a Claude account | Access token |
| `api.anthropic.com` | `GET /api/oauth/profile` | After a successful Claude usage request | Access token |
| `chatgpt.com` | `GET /backend-api/wham/usage` | Each refresh of a Codex account | Access token, ChatGPT account ID |
| `chatgpt_base_url` host | Usage path on the host set in that folder's `config.toml` | Instead of `chatgpt.com`, when set | Access token, ChatGPT account ID |
| `github.com` | `GET /Abulkosim/margin/releases/latest/download/latest.json` | Once a day while **Check for updates daily** is on (default), and when you choose **Check for updates** | Nothing about you or your accounts |
| `github.com` | Download of the matching release asset | Only when you choose to install an update | Nothing about you or your accounts |

GitHub may redirect release files to its own download storage. Update requests send no CLI credentials, account identities, or usage readings.

Refreshes follow the polling interval in Settings, or a manual refresh. Usage history adds no requests.

The interface itself has no network access. Its content security policy allows connections only to Margin's local process ([`tauri.conf.json`](src-tauri/tauri.conf.json)).

The Windows installer may download Microsoft's WebView2 setup program if WebView2 is missing. That is part of installation, not the app.

## What Margin writes

Margin writes only to its own data folder:

| OS | Folder |
| --- | --- |
| macOS | `~/Library/Application Support/com.abulkosim.margin/` |
| Windows | `%APPDATA%\com.abulkosim.margin\` |

| File | Contents |
| --- | --- |
| `state.json` | Settings, account list (label, color, provider, folder path or Keychain source), account email, plan, and hashed account identifier, last-known usage and last error per account, notification thresholds already sent |
| `history/<account id>.json` | Usage readings for up to seven days or 10,000 window readings per account, with the account's source, email, plan, and a hashed account identifier |

On macOS, both files are created with owner-only permissions (`0600`). Files are replaced atomically. Removing an account deletes its history file. See [`store.rs`](src-tauri/src/core/store.rs) and [`history.rs`](src-tauri/src/core/history.rs).

Tokens are never written to these files.

`margin --json` and `margin --line` only read `state.json` and print to the terminal. They make no network requests and write nothing. See [`cli.rs`](src-tauri/src/cli.rs).

Other changes Margin makes, only when you turn them on or ask for them:

- **Make active:** after you confirm, swaps the sign-in of the CLI's default folder (`~/.claude` or `~/.codex`) with the folder you chose. For Claude it swaps the credential (the Keychain item on macOS, else `.credentials.json`) and the `oauthAccount` entry of `~/.claude.json` and `<folder>/.claude.json`, leaving every other entry as it was. For Codex it swaps `auth.json`. It holds Claude Code's lock folders while it writes, makes no network request, and checks the result. Before writing it saves the old values: on macOS in Keychain items named `Margin backup`, on Windows in `backups/<id>.json` in its data folder. `switches.json` in the same folder lists recent switches with folder paths and hashes, never tokens. The last 5 switches from the past 7 days are kept. See [`switch.rs`](src-tauri/src/core/switch.rs).

- **Launch at login:** registers Margin with the OS. On macOS this is a LaunchAgent in `~/Library/LaunchAgents`. On Windows this is a per-user startup entry.
- **Notifications:** shown through the OS notification system.
- **Updates:** replaces the installed app with the downloaded release.

## What Margin never does

- Write to, move, or delete CLI credential files or Keychain items, except when you confirm **Make active** or **Restore backup**.
- Refresh or rotate CLI tokens.
- Read browser cookies or run the CLIs.
- Redeem or spend reset credits.
- Send telemetry, analytics, or crash reports.

## Tokens

Tokens are parsed in Rust into structures that cannot be serialized ([`claude.rs`](src-tauri/src/core/providers/claude.rs), [`codex.rs`](src-tauri/src/core/providers/codex.rs)). They are read again for each refresh and not kept between refreshes. The interface receives account details, usage snapshots, and errors. It can listen for state events only ([`capabilities/main.json`](src-tauri/capabilities/main.json)).

## Updates

Margin checks for updates once a day, starting two minutes after launch, unless you turn off **Check for updates daily** in Settings. You can also check at any time. A background check only reads the manifest; it never downloads or installs. Margin installs only after you confirm.

- The manifest URL is fixed to the latest GitHub release of `Abulkosim/margin`.
- Downloads must be HTTPS on `github.com`, from this repository's release for that exact version, with an expected file name ([`updates.rs`](src-tauri/src/updates.rs)).
- Each download must carry a valid minisign signature from the public key in [`tauri.conf.json`](src-tauri/tauri.conf.json). The signed version must match the offered version (`requireSignedVersion`). Otherwise nothing is installed.

## Installers

Installers are not signed by Apple or Microsoft yet. macOS builds are ad-hoc signed. See [Install](README.md#install) for first-launch steps.

Each release includes `SHA256SUMS.txt` with checksums for every installer and updater file. Releases after v0.3.0 also have GitHub build attestations; see [Verify a download](README.md#verify-a-download). Compare your download against the checksums:

```sh
shasum -a 256 Margin_<version>_universal.dmg
```

```powershell
Get-FileHash Margin_<version>_x64-setup.exe -Algorithm SHA256
```

## Removing your data

Quit Margin, delete the app, then delete the data folder listed above. Turn off launch at login first, or remove the LaunchAgent or startup entry by hand.

## Questions

Open an [issue](https://github.com/Abulkosim/margin/issues). For security problems, see [SECURITY.md](SECURITY.md).
