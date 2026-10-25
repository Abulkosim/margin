# Screenshots

Public images use explicit sample accounts. Capture from `?demo=1`; never publish a live account, email, configuration path, or credential.

## v0.5.0 captures

Use WebKit at 420×660 CSS pixels and 2× scale. Preserve the sample-data label. Visually inspect each image before linking it in the README or release notes.

| Image | State |
| --- | --- |
| `docs/images/overview-light.png`, `overview-dark.png` | `?demo=1` cards, lowest remaining first: Studio (18% session) with email, plan, pace lines, and the Sonnet chip, then Personal. |
| `docs/images/compact-light.png`, `compact-dark.png` | `?demo=1&state=compact&layout=compact`: five accounts in one row each, pinned first, limiting value at full ink, last-known and signed-out rows last. |
| `docs/images/history-light.png`, `history-dark.png` | History open on Personal, Session tab, scrolled so the panel starts at the top of the list: a new period, a recording gap, and the legend. |
| `docs/images/settings-light.png` | Top of Settings, General tab: theme, Overview, Account order, Tray account, Tray shows, Show percentage, and Refresh while open. Notifications and updates are below the fold and not shown. |
| `docs/images/settings-menu-light.png`, `settings-menu-dark.png` | Settings with the Tray account menu open and its selected option visible. |

All nine PNGs are 840×1320 pixels and retain the sample-data label. `site/overview-light.png` and `site/overview-dark.png` are copies of the overview images; `site/og.png` embeds `overview-light.png`. Earlier screenshots remain available at their release tags.

Caption browser captures as browser previews with sample data. Native screenshots need their own platform label. Release image URLs should use the release tag, not `main`.

## Demo GIF and social preview

Both are browser previews with sample data (`?demo=1`, light theme), not recordings of the native menu bar. The sample-data label stays visible.

| Image | Size | State |
| --- | --- | --- |
| `docs/images/demo.gif` | 420×660, 15 fps, 15 s, about 2.1 MB | Overview → Fable chip tooltip → History on Personal → Weekly tab → hide history → Settings → Overview: Compact → compact list. 1 s hold at start and end. |
| `docs/images/social-preview.png` | 1280×640, about 90 KB | Wordmark, “Claude Code and Codex usage in your menu bar”, “macOS · Windows · Free · MIT”, and `overview-light.png`. Upload in repo Settings → Social preview. |

The GIF is recorded with Playwright Chromium `recordVideo` (420×660 viewport, 2× scale, video at 420×660) and converted with ffmpeg using a two-pass palette (`palettegen`, `paletteuse`). Playwright’s bundled ffmpeg has no palette filters; use a full ffmpeg build. Reset countdowns change once a minute, so the GIF has no countdown beat.

To regenerate, run `pnpm dev --port 1437 --strictPort`, then `node record.mjs <repo>` and `node social.mjs <repo>`. The scripts are in the session scratchpad (`demo/record.mjs`, `demo/social.mjs`, `demo/og.html`) and are not committed. `social.mjs` embeds the current `docs/images/overview-light.png`.
