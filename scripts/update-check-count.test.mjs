import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import { renderSummary, summarizeReleases } from './update-check-count.mjs'

const release = (tag, published, assets, extra = {}) => ({
  tag_name: tag,
  published_at: published,
  draft: false,
  prerelease: false,
  assets: Object.entries(assets).map(([name, download_count]) => ({ name, download_count })),
  ...extra,
})
const v9 = release('v0.9.0', '2026-10-30T10:00:00Z', {
  'latest.json': 412,
  'Margin_0.9.0_universal.dmg': 31,
  'Margin_0.9.0_x64-setup.exe': 12,
  'Margin_0.9.0_x64-setup.exe.sig': 90,
  'Margin_0.9.0_x64_en-US.msi': 3,
  'Margin_0.9.0_x64_en-US.msi.sig': 91,
  'Margin.app.tar.gz': 140,
  'Margin.app.tar.gz.sig': 92,
  'SHA256SUMS.txt': 7,
  'margin-0.9.0.sbom.spdx.json': 2,
})

test('counts each file kind and ignores signatures, checksums and the SBOM', () => {
  assert.deepEqual(summarizeReleases([v9]), [{
    tag: 'v0.9.0', published: '2026-10-30', prerelease: false,
    latestJson: 412, dmg: 31, exe: 12, msi: 3, macUpdate: 140,
  }])
})

test('flattens paginated results, skips drafts, and puts the newest first', () => {
  const older = release('v0.1.0', '2026-01-01T00:00:00Z', { 'Margin_0.1.0_aarch64.dmg': 5, 'Margin_0.1.0_x64.dmg': 6 })
  const draft = release('v1.0.0', null, { 'latest.json': 0 }, { draft: true })
  const rows = summarizeReleases([[older], [v9, draft]])
  assert.deepEqual(rows.map(row => row.tag), ['v0.9.0', 'v0.1.0'])
  assert.equal(rows[1].dmg, 11)
})

test('a file a release never had stays empty rather than zero', () => {
  const [row] = summarizeReleases([release('v0.1.0', '2026-01-01T00:00:00Z', { 'Margin_0.1.0_x64.dmg': 5, 'latest.json': 0 })])
  assert.equal(row.latestJson, 0)
  assert.equal(row.exe, null)
  assert.match(renderSummary([row], '2026-11-04'), /\| v0\.1\.0 \| 2026-01-01 \| 0 \| 5 \| – \| – \| – \|/)
})

test('rejects output it cannot read instead of guessing', () => {
  assert.throws(() => summarizeReleases({ message: 'Not Found' }), /list of releases/)
  assert.throws(() => summarizeReleases([{ tag_name: 'v1' }]), /release shape/)
  assert.throws(() => summarizeReleases([release('v1', null, { 'latest.json': 'many' })]), /download count/)
  assert.throws(() => summarizeReleases([release('v1', null, { 'latest.json': -1 })]), /download count/)
})

test('the summary names the date and the limits', () => {
  const summary = renderSummary(summarizeReleases([v9]), '2026-11-04')
  assert.match(summary, /2026-11-04/)
  assert.match(summary, /not a count of people/)
  assert.match(summary, /\| Release \| Published \| latest\.json \| DMG \| EXE \| MSI \| Mac update \|/)
  assert.match(summary, /\| v0\.9\.0 \| 2026-10-30 \| 412 \| 31 \| 12 \| 3 \| 140 \|/)
})

test('the workflow is read-only and pins its actions', async () => {
  const workflow = await readFile(new URL('../.github/workflows/update-check-count.yml', import.meta.url), 'utf8')
  assert.doesNotMatch(workflow, /:\s*write\b/)
  assert.doesNotMatch(workflow, /git (push|commit)/)
  assert.match(workflow, /permissions:\n {2}contents: read/)
  const actions = [...workflow.matchAll(/uses:\s*(\S+)/g)].map(match => match[1])
  assert.ok(actions.length >= 3)
  for (const action of actions) assert.match(action, /@[0-9a-f]{40}$/)
})
