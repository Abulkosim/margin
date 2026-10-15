import { access, readFile } from 'node:fs/promises'

const tag = process.argv[2]
if (!tag || !/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag)) {
  throw new Error('Expected a release tag such as v0.1.0.')
}

const [packageText, tauriText, cargoText] = await Promise.all([
  readFile('package.json', 'utf8'),
  readFile('src-tauri/tauri.conf.json', 'utf8'),
  readFile('src-tauri/Cargo.toml', 'utf8'),
])
const packageConfig = JSON.parse(packageText)
const tauriConfig = JSON.parse(tauriText)
const cargoPackage = cargoText.split(/^\[package\]\s*$/m)[1]?.split(/^\[/m)[0]
const cargoVersion = cargoPackage?.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1]
const version = tag.slice(1)

for (const [name, actual] of Object.entries({
  'package.json': packageConfig.version,
  'tauri.conf.json': tauriConfig.version,
  'Cargo.toml': cargoVersion,
})) {
  if (actual !== version) throw new Error(`${name} version must match ${tag}.`)
}

for (const path of ['pnpm-lock.yaml', 'src-tauri/Cargo.lock']) {
  await access(path).catch(() => {
    throw new Error(`Commit ${path} before tagging a release.`)
  })
}

console.log(`Release inputs verified for ${tag}.`)
