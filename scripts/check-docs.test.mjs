import assert from 'node:assert/strict'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { anchors, brokenReferences, checkDocs, packagingDrift, slug, versionDrift } from './check-docs.mjs'

async function tree(t, files) {
  const root = await mkdtemp(join(tmpdir(), 'check-docs-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  for (const [path, text] of Object.entries(files)) {
    await mkdir(dirname(join(root, path)), { recursive: true })
    await writeFile(join(root, path), text)
  }
  return root
}

const release = version => ({
  'package.json': JSON.stringify({ version }),
  'CHANGELOG.md': `| Version | Date |\n| --- | --- |\n| [v${version}](changelogs/v${version}.md) | 2026-11-10 |\n`,
  'README.md': `1. Download **v${version}**: [DMG](https://github.com/Abulkosim/margin/releases/download/v${version}/Margin_${version}_universal.dmg)\n`,
  '.github/ISSUE_TEMPLATE/bug.yml': `      placeholder: ${version}\n`,
})

test('slugs match GitHub heading anchors', () => {
  assert.equal(slug('Verify a download'), 'verify-a-download')
  assert.equal(slug('Phase 7 — Make the claims true'), 'phase-7--make-the-claims-true')
  assert.equal(slug('`margin --json` output'), 'margin---json-output')
  assert.equal(slug('Nested <<b>i>tags'), 'nested-tags')
  assert.deepEqual([...anchors('# A\n## A\n```\n# not a heading\n```\n<a id="custom"></a>\n')], ['a', 'a-1', 'custom'])
})

test('reports missing files, missing anchors and missing quoted paths', async (t) => {
  const root = await tree(t, {
    'README.md': '## Install\n[ok](#install) [gone](#uninstall) [file](docs/install.md#windows) [img](docs/images/demo.gif)\n<img src="docs/images/hero.png">\nSee `scripts/missing.mjs` and `docs/install.md`.\n',
    'docs/install.md': '# Install\n## macOS\n',
  })
  assert.deepEqual(brokenReferences(root, ['README.md']), [
    'README.md: #uninstall has no matching heading',
    'README.md: docs/install.md#windows has no matching heading',
    'README.md: docs/images/demo.gif does not exist',
    'README.md: docs/images/hero.png does not exist',
    'README.md: `scripts/missing.mjs` does not exist',
  ])
})

test('ignores code, external links, placeholders, and files a deploy copies in', async (t) => {
  const root = await tree(t, {
    'site/index.html': '<img src="overview-light.png"><a href="https://example.com/x">x</a>',
    'docs/a.md': '```sh\n[x](missing.md)\n```\nRun `[x](gone.md)`, see `changelogs/vX.Y.Z.md`, `history/<id>.json`, `docs/images/tray-*.png`.\n[mail](mailto:a@b.c)\n',
  })
  assert.deepEqual(brokenReferences(root, ['site/index.html', 'docs/a.md'], { site: ['overview-light.png'] }), [])
})

test('flags public text that names another release than package.json', async (t) => {
  assert.deepEqual(versionDrift(await tree(t, release('1.2.3'))), [])
  const stale = await tree(t, {
    ...release('1.2.3'),
    'README.md': '1. Download **v1.2.2**: [DMG](https://github.com/Abulkosim/margin/releases/download/v1.2.2/Margin_1.2.2_universal.dmg)\n',
    '.github/ISSUE_TEMPLATE/bug.yml': '      placeholder: 1.2.2\n',
  })
  assert.deepEqual(versionDrift(stale), [
    'README.md: names v1.2.2, package.json is 1.2.3',
    '.github/ISSUE_TEMPLATE/bug.yml: placeholder 1.2.2, package.json is 1.2.3',
  ])
})

test('flags winget manifests that disagree with their folder', async (t) => {
  const root = await tree(t, {
    'packaging/winget/Abulkosim.Margin/1.2.3/Abulkosim.Margin.installer.yaml':
      'PackageVersion: 1.2.3\n    InstallerUrl: https://github.com/Abulkosim/margin/releases/download/v1.2.2/Margin_1.2.2_x64-setup.exe\n',
  })
  assert.deepEqual(packagingDrift(root), ['packaging/winget/Abulkosim.Margin/1.2.3/Abulkosim.Margin.installer.yaml: names 1.2.2'])
})

test('the repository docs have no broken links, stale release names or missing paths', () => {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
  assert.deepEqual(checkDocs(root), [])
})
