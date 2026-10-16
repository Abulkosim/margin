import { readdir, readFile, writeFile } from 'node:fs/promises'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { verifyUpdaterSignature } from './updater-signature.mjs'

export function releaseAssetNames(version) {
  return {
    dmg: `Margin_${version}_universal.dmg`,
    mac: 'Margin.app.tar.gz',
    nsis: `Margin_${version}_x64-setup.exe`,
    msi: `Margin_${version}_x64_en-US.msi`,
  }
}

export async function createUpdaterManifest({ directory, version, publicKey, repository, notes = '', date = new Date() }) {
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) throw new Error('Invalid release version.')
  if (repository !== 'Abulkosim/usagebar') throw new Error('Unexpected release repository.')
  const assets = releaseAssetNames(version)
  const required = [...Object.values(assets), ...[assets.mac, assets.nsis, assets.msi].map((name) => `${name}.sig`)]
  const allowed = new Set([...required, 'latest.json', 'SHA256SUMS.txt'])
  const entries = await readdir(directory, { withFileTypes: true })
  if (entries.some((entry) => !entry.isFile() || !allowed.has(entry.name))) {
    throw new Error('Unexpected release asset; refusing to publish unreviewed files.')
  }
  const names = new Set(entries.map((entry) => entry.name))
  for (const name of required) {
    if (!names.has(name)) throw new Error(`Missing release asset: ${name}`)
  }
  const platforms = {}
  for (const [name, targets] of [
    [assets.mac, ['darwin-aarch64', 'darwin-x86_64']],
    [assets.nsis, ['windows-x86_64-nsis', 'windows-x86_64']],
    [assets.msi, ['windows-x86_64-msi']],
  ]) {
    const signature = (await readFile(join(directory, `${name}.sig`), 'utf8')).trim()
    await verifyUpdaterSignature(join(directory, name), signature, publicKey, version)
    const platform = {
      signature,
      url: `https://github.com/${repository}/releases/download/v${version}/${encodeURIComponent(name)}`,
    }
    for (const target of targets) platforms[target] = platform
  }
  return { version, notes, pub_date: date.toISOString(), platforms }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [directory = 'release-artifacts', tag, repository = process.env.GH_REPO] = process.argv.slice(2)
  const config = JSON.parse(await readFile('src-tauri/tauri.conf.json', 'utf8'))
  if (tag !== `v${config.version}`) throw new Error('Release tag must match the app version.')
  const manifest = await createUpdaterManifest({
    directory,
    version: config.version,
    publicKey: config.plugins?.updater?.pubkey,
    repository,
    notes: await readFile('.github/RELEASE_NOTES.md', 'utf8'),
  })
  await writeFile(join(directory, 'latest.json'), `${JSON.stringify(manifest, null, 2)}\n`)
  console.log(`Verified three signed artifacts and generated updater manifest for ${tag}.`)
}
