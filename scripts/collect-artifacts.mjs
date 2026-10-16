import { copyFile, mkdir, readdir } from 'node:fs/promises'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

export async function collectArtifacts(target, { updater = false, root = '.' } = {}) {
  const formats = {
    'universal-apple-darwin': [['dmg', '.dmg', false], ...(updater ? [['macos', '.app.tar.gz', true]] : [])],
    'x86_64-pc-windows-msvc': [['nsis', '-setup.exe', updater], ['msi', '.msi', updater]],
  }[target]
  if (!formats) throw new Error('Expected a supported native build target.')

  const destination = join(root, 'release-artifacts')
  await mkdir(destination, { recursive: true })
  const names = new Set((await readdir(destination)).map((name) => name.toLowerCase()))

  for (const [format, suffix, signed] of formats) {
    const directory = join(root, 'src-tauri', 'target', target, 'release', 'bundle', format)
    const entries = await readdir(directory, { withFileTypes: true })
    const matches = entries.filter((entry) => entry.isFile() && entry.name.endsWith(suffix))
    if (matches.length !== 1) throw new Error(`Expected exactly one ${format} release artifact.`)
    const files = [matches[0].name]
    if (signed) {
      const signatureName = `${matches[0].name}.sig`
      if (!entries.some((entry) => entry.isFile() && entry.name === signatureName)) {
        throw new Error(`Missing updater signature: ${signatureName}`)
      }
      files.push(signatureName)
    }

    for (const name of files) {
      if (/[\x00-\x1f\x7f\\]/.test(name) || names.has(name.toLowerCase())) {
        throw new Error('Artifact names must be unique and contain no control characters.')
      }
      names.add(name.toLowerCase())
      await copyFile(join(directory, name), join(destination, name))
      console.log(`Collected ${name}`)
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  await collectArtifacts(process.argv[2], { updater: process.argv.includes('--updater') })
}
