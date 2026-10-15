import { copyFile, mkdir, readdir } from 'node:fs/promises'
import { join } from 'node:path'

const target = process.argv[2]
const formats = {
  'universal-apple-darwin': [['dmg', '.dmg']],
  'x86_64-pc-windows-msvc': [['nsis', '-setup.exe'], ['msi', '.msi']],
}[target]
if (!formats) throw new Error('Expected a supported native build target.')

const destination = 'release-artifacts'
await mkdir(destination, { recursive: true })
const names = new Set((await readdir(destination)).map((name) => name.toLowerCase()))

for (const [format, suffix] of formats) {
  const directory = join('src-tauri', 'target', target, 'release', 'bundle', format)
  const entries = (await readdir(directory, { withFileTypes: true }))
    .filter((entry) => entry.isFile() && entry.name.endsWith(suffix))
    .sort((a, b) => a.name.localeCompare(b.name))
  if (entries.length === 0) throw new Error(`No ${format} installer was produced.`)

  for (const entry of entries) {
    if (/[\r\n\\]/.test(entry.name) || names.has(entry.name.toLowerCase())) {
      throw new Error('Installer names must be unique and contain no control characters.')
    }
    names.add(entry.name.toLowerCase())
    await copyFile(join(directory, entry.name), join(destination, entry.name))
    console.log(`Collected ${entry.name}`)
  }
}
