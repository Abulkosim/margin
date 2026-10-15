import { createHash } from 'node:crypto'
import { createReadStream } from 'node:fs'
import { readdir, writeFile } from 'node:fs/promises'
import { join } from 'node:path'

const directory = process.argv[2] ?? 'release-artifacts'
const entries = (await readdir(directory, { withFileTypes: true }))
  .filter((entry) => entry.isFile() && /\.(dmg|exe|msi)$/.test(entry.name))
  .sort((a, b) => a.name.localeCompare(b.name))

for (const extension of ['.dmg', '-setup.exe', '.msi']) {
  if (!entries.some((entry) => entry.name.endsWith(extension))) {
    throw new Error(`Missing ${extension} installer; release is incomplete.`)
  }
}

const lines = []
for (const entry of entries) {
  if (/[\r\n\\]/.test(entry.name)) throw new Error('Invalid installer filename.')
  const hash = createHash('sha256')
  for await (const chunk of createReadStream(join(directory, entry.name))) hash.update(chunk)
  lines.push(`${hash.digest('hex')}  ${entry.name}`)
}
await writeFile(join(directory, 'SHA256SUMS.txt'), `${lines.join('\n')}\n`, 'utf8')
console.log(`Recorded SHA-256 checksums for ${lines.length} installers.`)
