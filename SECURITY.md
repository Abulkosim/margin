# Security

## Supported versions

Only the [latest release](https://github.com/Abulkosim/margin/releases/latest) receives security fixes. Update before reporting, if you can.

## Report a vulnerability

Report privately through [GitHub private vulnerability reporting](https://github.com/Abulkosim/margin/security/advisories/new).

If that is disabled, open an [issue](https://github.com/Abulkosim/margin/issues/new) titled `Security` with no details. The maintainer will reach out.

Include:

- Margin version and OS.
- What an attacker can do, and what they need first.
- Steps to reproduce.

Never include real tokens, credential files, or unredacted account emails. Use synthetic values.

## What to expect

Margin is an open-source project maintained by one person. Responses are best effort. There is no bug bounty.

Confirmed issues are fixed in a new release. The advisory is published after the fix ships, with credit if you want it.

## Scope

In scope:

- Exposure of CLI tokens or credential contents outside Margin's Rust process, including to the interface, logs, or Margin's own files.
- Any write to CLI credential files or Keychain items.
- Requests to hosts other than those listed in [PRIVACY.md](PRIVACY.md#network-requests), or tokens sent to the wrong host.
- Update checks or installs that accept an unsigned, altered, or mismatched-version release, or a download from an unexpected address.
- Reading files or Keychain items beyond those listed in [PRIVACY.md](PRIVACY.md#what-margin-reads).
- Code execution through crafted provider responses or malformed credential, config, or state files.

Out of scope:

- The missing Apple notarization and Windows publisher signature. These are known. See [Install](README.md#install).
- Attacks that need an already compromised user account or administrator access.
- Changes in the undocumented provider endpoints themselves.
- Vulnerabilities in Claude Code, Codex, Anthropic, or OpenAI services. Report those to their vendors.
- Missing hardening without a concrete attack.
