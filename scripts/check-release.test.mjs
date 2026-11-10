import assert from 'node:assert/strict'
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import test from 'node:test'
import { checkRelease, UPDATER_ENDPOINT } from './check-release.mjs'

const PUBKEY = 'dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk='

async function fixture(t, overrides = {}) {
  const root = await mkdtemp(join(tmpdir(), 'check-release-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  const version = overrides.version ?? '1.2.3'
  const tauri = {
    version: overrides.tauriVersion ?? version,
    bundle: { createUpdaterArtifacts: true },
    plugins: {
      updater: {
        requireSignedVersion: overrides.requireSignedVersion ?? true,
        endpoints: overrides.endpoints ?? [UPDATER_ENDPOINT],
        pubkey: overrides.pubkey ?? PUBKEY,
      },
    },
  }
  await mkdir(join(root, 'src-tauri'), { recursive: true })
  await mkdir(join(root, 'changelogs'), { recursive: true })
  await writeFile(join(root, 'package.json'), JSON.stringify({ version }))
  await writeFile(join(root, 'src-tauri/tauri.conf.json'), JSON.stringify(tauri))
  await writeFile(join(root, 'src-tauri/Cargo.toml'),
    `[package]\nname = "margin"\nversion = "${overrides.cargoVersion ?? version}"\n\n[dependencies]\nserde = { version = "9.9.9" }\n`)
  await writeFile(join(root, 'pnpm-lock.yaml'), '')
  await writeFile(join(root, 'src-tauri/Cargo.lock'), '')
  if (overrides.changelog !== false) await writeFile(join(root, `changelogs/v${version}.md`), '# changelog\n')
  return root
}

test('accepts matching versions, lockfiles, changelog and signed updater config', async (t) => {
  await checkRelease('v1.2.3', await fixture(t))
})

test('rejects tags that are not versions', async (t) => {
  const root = await fixture(t)
  for (const tag of [undefined, '1.2.3', 'v1.2', 'vlatest']) {
    await assert.rejects(checkRelease(tag, root), /Expected a release tag/)
  }
})

test('reads the package version, not a dependency version, from Cargo.toml', async (t) => {
  await assert.rejects(checkRelease('v1.2.3', await fixture(t, { cargoVersion: '1.2.2' })), /Cargo\.toml version must match/)
})

test('rejects each mismatched version file', async (t) => {
  await assert.rejects(checkRelease('v1.2.4', await fixture(t)), /package\.json version must match v1\.2\.4/)
  await assert.rejects(checkRelease('v1.2.3', await fixture(t, { tauriVersion: '1.2.0' })), /tauri\.conf\.json version/)
})

test('requires the changelog for the tag', async (t) => {
  await assert.rejects(checkRelease('v1.2.3', await fixture(t, { changelog: false })), /changelogs\/v1\.2\.3\.md/)
})

test('requires signed, version-bound updates from the public endpoint', async (t) => {
  for (const overrides of [
    { requireSignedVersion: false },
    { endpoints: ['https://example.com/latest.json'] },
    { endpoints: [UPDATER_ENDPOINT, UPDATER_ENDPOINT] },
    { pubkey: 'not-a-minisign-key' },
  ]) {
    await assert.rejects(checkRelease('v1.2.3', await fixture(t, overrides)), /Configure signed/)
  }
})
