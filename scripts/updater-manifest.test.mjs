import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import test from 'node:test'
import { collectArtifacts } from './collect-artifacts.mjs'
import { createUpdaterManifest, releaseAssetNames } from './updater-manifest.mjs'
import { verifyUpdaterSignature } from './updater-signature.mjs'

const fixture = fileURLToPath(new URL('./fixtures/updater/', import.meta.url))
const publicKey = await readFile(join(fixture, 'public-key.txt'), 'utf8')
const signature = await readFile(join(fixture, 'artifact.txt.sig'), 'utf8')
const artifact = await readFile(join(fixture, 'artifact.txt'))
const version = '0.2.0'

async function releaseFixture(t) {
  const directory = await mkdtemp(join(tmpdir(), 'margin-updater-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  const assets = releaseAssetNames(version)
  for (const name of Object.values(assets)) await writeFile(join(directory, name), artifact)
  for (const name of [assets.mac, assets.nsis, assets.msi]) await writeFile(join(directory, `${name}.sig`), signature)
  return {
    directory,
    version,
    publicKey,
    repository: 'Abulkosim/usagebar',
    date: new Date('2026-10-16T00:00:00Z'),
  }
}

test('verifies an actual Tauri CLI version-bound signature', async () => {
  await verifyUpdaterSignature(join(fixture, 'artifact.txt'), signature, publicKey, version)
})

test('rejects modified artifact bytes', async (t) => {
  const options = await releaseFixture(t)
  await writeFile(join(options.directory, 'Margin.app.tar.gz'), 'tampered')
  await assert.rejects(createUpdaterManifest(options), /failed signature verification/)
})

test('rejects a manifest version different from the signed version', async () => {
  await assert.rejects(verifyUpdaterSignature(join(fixture, 'artifact.txt'), signature, publicKey, '0.2.1'), /bind the artifact/)
})

test('rejects tampering with the signed version in a signature', async () => {
  const changed = Buffer.from(Buffer.from(signature.trim(), 'base64').toString().replace('version:0.2.0', 'version:0.2.1')).toString('base64')
  await assert.rejects(verifyUpdaterSignature(join(fixture, 'artifact.txt'), changed, publicKey, '0.2.1'), /failed signature verification/)
})

test('rejects a different signing key', async () => {
  const lines = Buffer.from(publicKey.trim(), 'base64').toString().trimEnd().split('\n')
  const key = Buffer.from(lines[1], 'base64')
  key[2] ^= 1
  lines[1] = key.toString('base64')
  const changed = Buffer.from(lines.join('\n')).toString('base64')
  await assert.rejects(verifyUpdaterSignature(join(fixture, 'artifact.txt'), signature, changed, version), /different key/)
})

test('rejects malformed or legacy signature encodings', async () => {
  for (const changed of ['', 'not base64!', Buffer.from('incomplete').toString('base64')]) {
    await assert.rejects(verifyUpdaterSignature(join(fixture, 'artifact.txt'), changed, publicKey, version), /Invalid updater signature/)
  }
  const lines = Buffer.from(signature.trim(), 'base64').toString().trimEnd().split('\n')
  const packet = Buffer.from(lines[1], 'base64')
  packet[1] = 'd'.charCodeAt(0)
  lines[1] = packet.toString('base64')
  await assert.rejects(verifyUpdaterSignature(join(fixture, 'artifact.txt'), Buffer.from(lines.join('\n')).toString('base64'), publicKey, version), /unsupported algorithm/)
})

test('maps both Mac architectures to universal and preserves Windows installer family', async (t) => {
  const manifest = await createUpdaterManifest(await releaseFixture(t))
  assert.equal(manifest.version, version)
  assert.equal(manifest.pub_date, '2026-10-16T00:00:00.000Z')
  assert.deepEqual(Object.keys(manifest.platforms).sort(), [
    'darwin-aarch64', 'darwin-x86_64', 'windows-x86_64', 'windows-x86_64-msi', 'windows-x86_64-nsis',
  ])
  assert.deepEqual(manifest.platforms['darwin-aarch64'], manifest.platforms['darwin-x86_64'])
  assert.match(manifest.platforms['windows-x86_64-nsis'].url, /v0\.2\.0\/Margin_0\.2\.0_x64-setup\.exe$/)
  assert.match(manifest.platforms['windows-x86_64-msi'].url, /\.msi$/)
  assert.deepEqual(manifest.platforms['windows-x86_64'], manifest.platforms['windows-x86_64-nsis'])
  assert.equal(manifest.platforms['darwin-aarch64'].signature, signature.trim())
})

test('refuses incomplete release assets', async (t) => {
  const options = await releaseFixture(t)
  await rm(join(options.directory, 'Margin.app.tar.gz.sig'))
  await assert.rejects(createUpdaterManifest(options), /Missing release asset/)
})

test('refuses extraneous files including accidental private keys', async (t) => {
  const options = await releaseFixture(t)
  await writeFile(join(options.directory, 'private.key'), 'must not be published')
  await assert.rejects(createUpdaterManifest(options), /Unexpected release asset/)
})

test('refuses wrong destination repository and malformed versions', async (t) => {
  const options = await releaseFixture(t)
  await assert.rejects(createUpdaterManifest({ ...options, repository: 'someone/else' }), /Unexpected release repository/)
  await assert.rejects(createUpdaterManifest({ ...options, version: '../0.2.0' }), /Invalid release version/)
})

test('checksums cover installers, Mac archive, signatures, and manifest', async (t) => {
  const options = await releaseFixture(t)
  const manifest = await createUpdaterManifest(options)
  await writeFile(join(options.directory, 'latest.json'), JSON.stringify(manifest))
  execFileSync(process.execPath, [fileURLToPath(new URL('./checksums.mjs', import.meta.url)), options.directory])
  const lines = (await readFile(join(options.directory, 'SHA256SUMS.txt'), 'utf8')).trim().split('\n')
  assert.equal(lines.length, 8)
  assert(lines.every((line) => /^[a-f0-9]{64}  .+$/.test(line)))
  assert(lines.some((line) => line.endsWith('  latest.json')))
  assert.equal(lines.filter((line) => line.endsWith('.sig')).length, 3)
})

async function bundleFixture(t, target, artifacts) {
  const root = await mkdtemp(join(tmpdir(), 'margin-bundles-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  for (const [format, names] of Object.entries(artifacts)) {
    const directory = join(root, 'src-tauri', 'target', target, 'release', 'bundle', format)
    await mkdir(directory, { recursive: true })
    for (const name of names) await writeFile(join(directory, name), name.endsWith('.sig') ? signature : artifact)
  }
  return root
}

test('collects Mac archive and signature only for signed release builds', async (t) => {
  const target = 'universal-apple-darwin'
  const artifacts = { dmg: ['Margin_0.2.0_universal.dmg'], macos: ['Margin.app.tar.gz', 'Margin.app.tar.gz.sig'] }
  const signedRoot = await bundleFixture(t, target, artifacts)
  await collectArtifacts(target, { updater: true, root: signedRoot })
  assert.equal((await readdir(join(signedRoot, 'release-artifacts'))).length, 3)
  const normalRoot = await bundleFixture(t, target, artifacts)
  await collectArtifacts(target, { root: normalRoot })
  assert.deepEqual(await readdir(join(normalRoot, 'release-artifacts')), artifacts.dmg)
})

test('collects both Windows installer signatures', async (t) => {
  const target = 'x86_64-pc-windows-msvc'
  const root = await bundleFixture(t, target, {
    nsis: ['Margin_0.2.0_x64-setup.exe', 'Margin_0.2.0_x64-setup.exe.sig'],
    msi: ['Margin_0.2.0_x64_en-US.msi', 'Margin_0.2.0_x64_en-US.msi.sig'],
  })
  await collectArtifacts(target, { updater: true, root })
  assert.equal((await readdir(join(root, 'release-artifacts'))).length, 4)
})

test('fails collection when signature missing or stale installers coexist', async (t) => {
  const target = 'x86_64-pc-windows-msvc'
  const root = await bundleFixture(t, target, { nsis: ['Margin_0.2.0_x64-setup.exe'] })
  await assert.rejects(collectArtifacts(target, { updater: true, root }), /Missing updater signature/)
  await writeFile(join(root, 'src-tauri', 'target', target, 'release', 'bundle', 'nsis', 'Margin_0.1.1_x64-setup.exe'), artifact)
  await assert.rejects(collectArtifacts(target, { root }), /exactly one/)
})
