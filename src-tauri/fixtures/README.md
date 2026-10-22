All fixtures contain synthetic account data and usage values. No captured tokens, account identifiers, email addresses, balances, or personal usage are retained.

The `*-live-schema.json` fixtures preserve response structures verified with authorized read-only requests on 2026-10-16. Codex usage, Claude usage, and Claude profile returned HTTP 200. Codex credentials came from the CLI's `auth.json`; Claude credentials came from its OAuth Keychain item. Claude accepted `cedar_ember=1` without fallback. Only key/type shapes were logged. Identity, usage, reset times, and other account-specific values were replaced; window durations and recognized structural labels were preserved.

Observed cases covered by regression tests:

- Codex's primary window can be weekly (`604800` seconds), with a null secondary window. No session window is invented. The response supplies authoritative email and plan, with JWT claims as fallback.
- Claude returned session, weekly, and scoped weekly windows. The scoped row had no model ID, so its display name supplies the stable identity. Null extra-usage measurements remain absent.
- Claude profile returned email under `account.email` and the plan hint under `organization.rate_limit_tier`. An explicit subscription type retains precedence.

`claude-credentials-expired.json` is a constructed CLI credential file whose `expiresAt` (ms epoch) is in the past; it must never reach the network.

The other fixtures are constructed examples covering additional documented shapes. All parser tests run without network or real credentials. Live verification did not refresh credentials, overwrite CLI files, or redeem reset credits.

Schema references:

- [Codex backend client](https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client.rs): usage windows, additional limits, account header, endpoint path rules.
- [CodexBar Claude OAuth decoder](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Claude/ClaudeOAuth/ClaudeOAuthUsageFetcher.swift): OAuth payloads and profile fields.
- [Claude scoped-limit change](https://github.com/steipete/CodexBar/pull/1851): flat `limits` entries, including enforceable rows marked inactive.
- [Claude usage mapper](https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Claude/ClaudeUsageFetcher.swift): OAuth spend values are cents.

Parser tests also cover missing/null measurements, malformed values, bounds, absent reset times, weekly-only responses, spend-only responses, scoped limits, token shapes, partial credential writes, HTTP status handling, and custom endpoint validation.
