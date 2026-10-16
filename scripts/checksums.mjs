import { createHash } from 'node:crypto'
import { createReadStream } from 'node:fs'
import { readdir, readFile, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { releaseAssetNames } from './updater-manifest.mjs'

const directory = process.argv[2] ?? 'release-artifacts'
const manifest = JSON.parse(await readFile(join(directory, 'latest.json'), 'utf8'))
const assets = releaseAssetNames(manifest.version)
const required = new Set([
  ...Object.values(assets),
  ...[assets.mac, assets.nsis, assets.msi].map((name) => `${name}.sig`),
  'latest.json',
])
const entries = (await readdir(directory, { withFileTypes: true }))
  .filter((entry) => entry.name !== 'SHA256SUMS.txt')
  .sort((a, b) => a.name.localeCompare(b.name))

if (entries.length !== required.size || entries.some((entry) => !entry.isFile() || !required.has(entry.name))) {
  throw new Error('Release assets must match the complete signed updater asset set.')
}

const lines = []
for (const entry of entries) {
  if (/[\r\n\\]/.test(entry.name)) throw new Error('Invalid installer filename.')
  const hash = createHash('sha256')
  for await (const chunk of createReadStream(join(directory, entry.name))) hash.update(chunk)
  lines.push(`${hash.digest('hex')}  ${entry.name}`)
}
await writeFile(join(directory, 'SHA256SUMS.txt'), `${lines.join('\n')}\n`, 'utf8')
console.log(`Recorded SHA-256 checksums for ${lines.length} release assets.`)
