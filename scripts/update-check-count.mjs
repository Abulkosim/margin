import { execFileSync } from 'node:child_process'
import { appendFile, writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const assetKinds = [
  ['latestJson', name => name === 'latest.json'],
  ['dmg', name => name.endsWith('.dmg')],
  ['exe', name => name.endsWith('.exe')],
  ['msi', name => name.endsWith('.msi')],
  ['macUpdate', name => name.endsWith('.app.tar.gz')],
]
const columns = [['Release', 'tag'], ['Published', 'published'], ['latest.json', 'latestJson'], ['DMG', 'dmg'], ['EXE', 'exe'], ['MSI', 'msi'], ['Mac update', 'macUpdate']]

/** Accepts the release list from GitHub, flat or paginated. A file a release lacks stays null, not 0. */
export function summarizeReleases(releases) {
  if (!Array.isArray(releases)) throw new Error('Expected a list of releases.')
  return releases.flat()
    .filter(release => !release.draft)
    .map(release => {
      if (typeof release.tag_name !== 'string' || !Array.isArray(release.assets)) throw new Error('Unexpected release shape.')
      const row = { tag: release.tag_name, published: release.published_at?.slice(0, 10) ?? null, prerelease: Boolean(release.prerelease), latestJson: null, dmg: null, exe: null, msi: null, macUpdate: null }
      for (const asset of release.assets) {
        if (!Number.isSafeInteger(asset.download_count) || asset.download_count < 0) throw new Error(`Unexpected download count for ${asset.name} in ${release.tag_name}.`)
        const kind = assetKinds.find(([, matches]) => matches(asset.name))?.[0]
        if (kind) row[kind] = (row[kind] ?? 0) + asset.download_count
      }
      return row
    })
    .sort((a, b) => (b.published ?? '').localeCompare(a.published ?? ''))
}

export function renderSummary(rows, date) {
  const cell = value => value === null ? '–' : String(value)
  const lines = [
    `## Update check counts, ${date}`,
    '',
    'Cumulative per release. A rough signal, not a count of people. See docs/releasing.md.',
    '',
    `| ${columns.map(([label]) => label).join(' | ')} |`,
    `| ${columns.map(() => '---').join(' | ')} |`,
    ...rows.map(row => `| ${columns.map(([, key]) => cell(row[key])).join(' | ')} |`),
  ]
  return `${lines.join('\n')}\n`
}

export function fetchReleases(repository) {
  const output = execFileSync('gh', ['api', '--paginate', '--slurp', `repos/${repository}/releases?per_page=100`], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })
  return JSON.parse(output)
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const flag = process.argv.indexOf('--json')
  const date = new Date().toISOString().slice(0, 10)
  const rows = summarizeReleases(fetchReleases(process.env.GH_REPO ?? 'Abulkosim/margin'))
  const summary = renderSummary(rows, date)
  process.stdout.write(summary)
  if (process.env.GITHUB_STEP_SUMMARY) await appendFile(process.env.GITHUB_STEP_SUMMARY, summary)
  if (flag !== -1) await writeFile(process.argv[flag + 1], `${JSON.stringify({ date, releases: rows }, null, 2)}\n`)
}
