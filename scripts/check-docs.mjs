import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { dirname, join, posix, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

/** GitHub's heading anchor: lowercase, punctuation dropped, spaces to hyphens. */
export function slug(heading) {
  return heading
    .replace(/<[^>]+>/g, '')
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/[`*_~]/g, '')
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s-]/gu, '')
    .replace(/\s/g, '-')
}

/** Markdown without fenced code blocks and inline code, which hold examples, not links. */
function prose(text) {
  return text
    .replace(/^([ \t]*)(```|~~~)[^\n]*\n[\s\S]*?^\1\2[^\n]*$/gm, '')
    .replace(/`[^`\n]*`/g, '')
}

export function anchors(text) {
  const found = new Set()
  const counts = new Map()
  for (const line of prose(text).split('\n')) {
    const heading = line.match(/^#{1,6}\s+(.+?)\s*#*\s*$/)
    if (heading) {
      const base = slug(heading[1])
      const seen = counts.get(base) ?? 0
      counts.set(base, seen + 1)
      found.add(seen ? `${base}-${seen}` : base)
    }
    for (const match of line.matchAll(/<a\s[^>]*(?:id|name)="([^"]+)"/g)) found.add(match[1])
  }
  return found
}

export function links(text) {
  const body = prose(text)
  const targets = []
  for (const match of body.matchAll(/!?\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)/g)) targets.push(match[1])
  for (const match of body.matchAll(/^\s*\[[^\]]+\]:\s*<?(\S+?)>?(?:\s+"[^"]*")?\s*$/gm)) targets.push(match[1])
  for (const match of body.matchAll(/\b(?:href|src)="([^"]+)"/g)) targets.push(match[1])
  for (const match of body.matchAll(/\bsrcset="([^"]+)"/g)) {
    for (const candidate of match[1].split(',')) targets.push(candidate.trim().split(/\s+/)[0])
  }
  return targets
}

/** Repository paths quoted in backticks, such as `scripts/checksums.mjs`. Placeholders and globs are skipped. */
export function quotedPaths(text) {
  const tops = ['.github/', 'changelogs/', 'design/', 'docs/', 'packaging/', 'scripts/', 'site/', 'src/', 'src-tauri/', 'tests/']
  const paths = []
  const withoutFences = text.replace(/^([ \t]*)(```|~~~)[^\n]*\n[\s\S]*?^\1\2[^\n]*$/gm, '')
  for (const match of withoutFences.matchAll(/`([^`\s]+)`/g)) {
    const path = match[1].replace(/:\d+$/, '')
    if (!tops.some(top => path.startsWith(top))) continue
    if (/[<>*{}$]|X\.Y\.Z|vX|\.\.\./.test(path)) continue
    paths.push(path)
  }
  return paths
}

const isExternal = target => /^[a-z][a-z0-9+.-]*:/i.test(target) || target.startsWith('//')

/**
 * Broken relative links, anchors and quoted repository paths in `files` (paths relative to `root`).
 * `served` maps a published folder to extra files a deploy copies into it.
 */
export function brokenReferences(root, files, served = {}) {
  const problems = []
  const anchorCache = new Map()
  const anchorsOf = path => {
    if (!anchorCache.has(path)) anchorCache.set(path, anchors(readFileSync(join(root, path), 'utf8')))
    return anchorCache.get(path)
  }
  const exists = path => existsSync(join(root, path)) || (served[posix.dirname(path)] ?? []).includes(posix.basename(path))
  for (const file of files) {
    const text = readFileSync(join(root, file), 'utf8')
    const isMarkdown = file.endsWith('.md')
    for (const target of links(text)) {
      if (isExternal(target) || target.startsWith('mailto:')) continue
      const [rawPath, fragment] = target.split('#')
      const path = rawPath ? posix.normalize(posix.join(posix.dirname(file), decodeURIComponent(rawPath))) : file
      if (rawPath && !exists(path)) {
        problems.push(`${file}: ${target} does not exist`)
        continue
      }
      if (fragment && path.endsWith('.md') && existsSync(join(root, path)) && !anchorsOf(path).has(fragment)) {
        problems.push(`${file}: ${target} has no matching heading`)
      }
    }
    if (!isMarkdown) continue
    for (const path of quotedPaths(text)) {
      if (!existsSync(join(root, path))) problems.push(`${file}: \`${path}\` does not exist`)
    }
  }
  return problems
}

/** Public places that name the current release and must move with `package.json`. */
export function versionDrift(root) {
  const version = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version
  const problems = []
  const readme = readFileSync(join(root, 'README.md'), 'utf8')
  const named = [...readme.matchAll(/Download \*\*v(\d+\.\d+\.\d+)\*\*/g)].map(match => match[1])
  const downloads = [...readme.matchAll(/releases\/download\/v([^/]+)\/Margin_([^_]+)_/g)].flatMap(match => [match[1], match[2]])
  if (!named.length) problems.push('README.md: no "Download **vX.Y.Z**" line')
  for (const found of new Set([...named, ...downloads])) {
    if (found !== version) problems.push(`README.md: names v${found}, package.json is ${version}`)
  }
  const changelog = readFileSync(join(root, 'CHANGELOG.md'), 'utf8')
  const latest = changelog.match(/^\| \[v([^\]]+)\]/m)?.[1]
  if (latest !== version) problems.push(`CHANGELOG.md: first row is v${latest}, package.json is ${version}`)
  const templates = join(root, '.github/ISSUE_TEMPLATE')
  if (existsSync(templates)) {
    for (const name of readdirSync(templates).filter(name => name.endsWith('.yml'))) {
      const text = readFileSync(join(templates, name), 'utf8')
      for (const match of text.matchAll(/placeholder:\s*"?(\d+\.\d+\.\d+)"?\s*$/gm)) {
        if (match[1] !== version) problems.push(`.github/ISSUE_TEMPLATE/${name}: placeholder ${match[1]}, package.json is ${version}`)
      }
    }
  }
  return problems
}

/** Each winget folder's manifests and installer URL name the folder's version; the cask's URL uses its version. */
export function packagingDrift(root) {
  const problems = []
  const winget = join(root, 'packaging/winget/Abulkosim.Margin')
  if (existsSync(winget)) {
    for (const folder of readdirSync(winget).filter(name => statSync(join(winget, name)).isDirectory())) {
      for (const name of readdirSync(join(winget, folder))) {
        const text = readFileSync(join(winget, folder, name), 'utf8')
        const versions = [
          ...[...text.matchAll(/^PackageVersion:\s*(\S+)/gm)].map(match => match[1]),
          ...[...text.matchAll(/releases\/(?:download|tag)\/v([^/\s]+)/g)].map(match => match[1]),
          ...[...text.matchAll(/Margin_([^_]+)_x64/g)].map(match => match[1]),
        ]
        for (const found of new Set(versions)) {
          if (found !== folder) problems.push(`packaging/winget/Abulkosim.Margin/${folder}/${name}: names ${found}`)
        }
      }
    }
  }
  return problems
}

export function trackedFiles(root) {
  return execFileSync('git', ['ls-files', '-z', '--', '*.md', 'site/*.html'], { cwd: root, encoding: 'utf8' })
    .split('\0')
    .filter(Boolean)
}

/** Files the Pages workflow copies into the site before deploying. */
export function pagesCopies(root) {
  const workflow = join(root, '.github/workflows/pages.yml')
  if (!existsSync(workflow)) return {}
  const copied = []
  for (const match of readFileSync(workflow, 'utf8').matchAll(/^\s*run:\s*cp\s+(.+?)\s+site\/\s*$/gm)) {
    copied.push(...match[1].split(/\s+/).map(path => posix.basename(path)))
  }
  return { site: copied }
}

export function checkDocs(root = '.') {
  return [
    ...brokenReferences(root, trackedFiles(root), pagesCopies(root)),
    ...versionDrift(root),
    ...packagingDrift(root),
  ]
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const root = resolve(dirname(process.argv[1]), '..')
  const problems = checkDocs(root)
  for (const problem of problems) console.error(problem)
  if (problems.length) process.exit(1)
  console.log('Docs links, paths and versions are consistent.')
}
