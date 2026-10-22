# Listings (plan 3.12)

Drafts. Submit 👤 by hand, one PR or form per list. Rules fetched 2026-10-22; re-read before submitting.

Repo link everywhere: `https://github.com/Abulkosim/margin`. Swap in `https://abulkosim.github.io/margin/` as the homepage link only once Pages is live (plan 3.10).

## 1. awesome-mac

Source: https://github.com/jaywcjlove/awesome-mac — rules: https://github.com/jaywcjlove/awesome-mac/blob/master/docs/CONTRIBUTING.md, legend in README.md.

Rules followed (quoted):
- "Link additions should be added in alphabetical order in the relevant category."
- "Create individual pull requests/issues for each suggestion."
- "keep descriptions concise and limited to one sentence"
- "sync entries across `README.md`, `README-zh.md`, `README-ja.md`, and `README-ko.md`"
- Legend: "![Open-Source Software][OSS Icon] means **open source**. click the icon to see the item's repository", "![Freeware][Freeware Icon] means **free** to use", "![Native App][Native Icon] means **native app**".

Section: **AI Tools** (`## AI Tools`). Comparable entries live there: CodexBar, Usage4Claude, Claude Usage Monitor, TokenMeter. Not Menu Bar Tools.
Placement: directly after `* [Maestro](https://runmaestro.ai) ...` and before `* [MiniClaw]...` in all four files (the AI Tools section is only loosely alphabetical; "Mae" < "Mar" < "Min" holds in all four).
No Native icon: Margin is Tauri (WebView), not native AppKit/SwiftUI.

`README.md`:

```markdown
* [Margin](https://github.com/Abulkosim/margin) - Menu bar app that shows Claude Code and Codex usage remaining and reset times across multiple accounts. [![Open-Source Software][OSS Icon]](https://github.com/Abulkosim/margin) ![Freeware][Freeware Icon]
```

`README-zh.md` (section `## AI 工具`):

```markdown
* [Margin](https://github.com/Abulkosim/margin) - 在菜单栏显示多个账户的 Claude Code 和 Codex 剩余用量与重置时间。 [![Open-Source Software][OSS Icon]](https://github.com/Abulkosim/margin) ![Freeware][Freeware Icon]
```

`README-ja.md` (section `## AIツール`):

```markdown
* [Margin](https://github.com/Abulkosim/margin) - 複数アカウントのClaude CodeとCodexの残り使用量とリセット時刻をメニューバーに表示。 [![Open-Source Software][OSS Icon]](https://github.com/Abulkosim/margin) ![Freeware][Freeware Icon]
```

`README-ko.md` (section `## AI 도구`):

```markdown
* [Margin](https://github.com/Abulkosim/margin) - 여러 계정의 Claude Code와 Codex 남은 사용량과 초기화 시간을 메뉴 막대에 표시. [![Open-Source Software][OSS Icon]](https://github.com/Abulkosim/margin) ![Freeware][Freeware Icon]
```

Translations are mine; existing entries use the same style (zh ends with "。", ko/ja with "." / "。"). Fine to ask a native speaker or let the maintainer adjust.

PR title (rule: "The pull request and commit should have a useful title"):

```
Add Margin to AI Tools
```

PR body:

```markdown
Adds [Margin](https://github.com/Abulkosim/margin) to AI Tools, after Maestro, in README.md, README-zh.md, README-ja.md and README-ko.md.

Margin is a free, MIT-licensed menu bar app (also runs in the Windows tray) that shows remaining Claude Code and Codex usage and reset times for several accounts at once. It reads existing CLI sign-ins and is read-only.

- Open source: yes (MIT)
- Free: yes
- Native: no (Tauri), so no Native icon
- I'm the developer.
```

## 2. open-source-mac-os-apps

Source: https://github.com/serhii-londar/open-source-mac-os-apps — rules: https://github.com/serhii-londar/open-source-mac-os-apps/blob/master/CONTRIBUTING.md, categories: https://github.com/serhii-londar/open-source-mac-os-apps/blob/master/categories.json.

Rules followed (quoted):
- "Edit applications.json instead of README.md." (README is generated.)
- "Make an individual pull request for each suggestion."
- "Keep descriptions short and simple, but descriptive."
- "End all descriptions with a full stop/period."
- "List of all categories can be found in categories.json."
- Ineligible if "Lack recent commit" or "README is not clear or not written in English" (both fine).

File: `applications.json`. Placement: append as the last object of the `"applications"` array (the file is not alphabetical; recent additions, e.g. `usage`, `Reprise`, are appended). Remember the comma after the previous last object.
Categories: `menubar`, `development` (same pair as the comparable `usage` entry). Languages use lowercase ids (`rust`, `typescript` are the common ones in the file; no `vue` id is used anywhere).
Icon and screenshot URLs checked to resolve on `main` (HTTP 200).

```json
        {
            "title": "Margin",
            "short_description": "Menu bar app that shows Claude Code and Codex usage remaining and reset times across multiple accounts.",
            "categories": [
                "menubar",
                "development"
            ],
            "repo_url": "https://github.com/Abulkosim/margin",
            "icon_url": "https://raw.githubusercontent.com/Abulkosim/margin/main/src-tauri/icons/128x128@2x.png",
            "screenshots": [
                "https://raw.githubusercontent.com/Abulkosim/margin/main/docs/images/overview-light.png",
                "https://raw.githubusercontent.com/Abulkosim/margin/main/docs/images/history-light.png"
            ],
            "official_site": "https://github.com/Abulkosim/margin",
            "languages": [
                "rust",
                "typescript"
            ]
        }
```

PR title (no rule found; matches recent PRs' plain style):

```
Add Margin
```

PR body:

```markdown
Adds Margin to applications.json (README not edited, per CONTRIBUTING).

- Repo: https://github.com/Abulkosim/margin (MIT)
- What: menu bar app showing Claude Code and Codex usage remaining and reset times for several accounts. Also runs on Windows.
- Categories: menubar, development
- Languages: Rust, TypeScript (Tauri 2)

I'm the developer.
```

## 3. awesome-claude-code

**Submit on or after 2026-10-30** (owner's date; also the first day the repo passes their 14-day rule: first commit 2026-10-16). Submit 👤 by hand: "resource recommendations must be created by human beings", and "It is **not** possible to submit a resource recommendation using the `gh` CLI."

Source: https://github.com/hesreallyhim/awesome-claude-code/blob/main/CONTRIBUTING.md and the form definition https://github.com/hesreallyhim/awesome-claude-code/blob/main/.github/ISSUE_TEMPLATE/recommend-resource.yml.

Rules followed (quoted):
- "ALL RECOMMENDATIONS MUST BE MADE USING THE WEB UI ISSUE FORM TEMPLATE" — form: https://github.com/hesreallyhim/awesome-claude-code/issues/new?template=recommend-resource.yml
- "Do not open a PR. Just fill out the form."
- Eligibility: "(a) at least 14 days of active development since the first commit to the default branch; OR (b) at least 100 stars."
- "You may only submit one resource at a time."
- Description: "1-3 sentences. Descriptive, not promotional. Don't address the reader. (10-500 characters.)"; "Keep it formatted to one line. Don't use any emojis."
- Guideline (not hard): resources that "focus on the unique features and functionality of Claude Code". Margin also covers Codex; the description leads with Claude Code and says so plainly.

Form values:

| Field | Value |
| --- | --- |
| Title | `[Resource]: Margin` |
| Display Name | `Margin` |
| Category | `Observability & Monitoring > Usage & Cost` |
| Link | `https://github.com/Abulkosim/margin` |
| Author Name | `Abulkosim` |
| Author Link | `https://github.com/Abulkosim` |

Description (one line, 310 chars):

```
Menu bar and system tray app for macOS and Windows that shows remaining Claude Code usage and reset times for several accounts at once, including accounts in separate CLAUDE_CONFIG_DIR folders. Reads the CLI's existing OAuth sign-ins, never writes credentials, and can alert at thresholds. Also supports Codex.
```

Checklist: tick the first five only after actually doing each. Leave the last box ("Do not check the following box") **unchecked**.

## Optional lists

### awesome-codex-cli (RoggeOhta) — optional, better after some stars

Source: https://github.com/RoggeOhta/awesome-codex-cli/blob/main/CONTRIBUTING.md

Rules followed (quoted):
- Format: `- [owner/repo](https://github.com/owner/repo) - One-sentence description explaining its value. ![GitHub stars](https://img.shields.io/github/stars/owner/repo?style=flat-square)`
- "Be directly related to Codex CLI"; "Add entries to the most specific category."
- Won't accept: "Self-promotion without substance — your project needs real users or a clear unique value."

Section: `## Monitoring & Analytics` (has CodexBar, ccusage, PulseMeter). Not alphabetical; append at the end of the section.

```markdown
- [Abulkosim/margin](https://github.com/Abulkosim/margin) - macOS menu bar and Windows tray app showing Codex and Claude Code usage limits and reset times for several accounts, read from existing CLI sign-ins. ![GitHub stars](https://img.shields.io/github/stars/Abulkosim/margin?style=flat-square)
```

PR title: `Add Abulkosim/margin to Monitoring & Analytics`. Body: one line on what it does + "I'm the developer."

### awesome-codex-cli (milisp) — optional

Source: https://github.com/milisp/awesome-codex-cli (README; no CONTRIBUTING.md found at `main`). Format inferred from existing entries in `### Usage & Monitoring`: `- [Name](url) - Description.` Not alphabetical; append at the end of the section. The README also has a "Recently Added" page (`RECENTLY_ADDED.md`); check whether PRs are expected to update it.

```markdown
- [Margin](https://github.com/Abulkosim/margin) - macOS menu bar and Windows tray app showing Codex and Claude Code usage limits and reset times for several accounts.
```

PR title: `Add Margin to Usage & Monitoring`.

### Checked, not fitting

- awesome-tauri (https://github.com/tauri-apps/awesome-tauri, `dev` branch README): sections are only Guides & Tutorials, Articles, Templates, Plugins, Integrations. No apps section now, so nowhere to add Margin.
- Awesome-Windows: raw README not found at the expected paths; not checked further.
