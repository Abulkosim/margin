import assert from 'node:assert/strict'
import test from 'node:test'
import { resetGrantsText } from '../../src/lib/format.ts'
import { accountSchema } from '../../src/lib/model.ts'

const now = Date.parse('2026-10-16T10:00:00Z')

test('reset grants read as one line with singular, plural and optional expiry', () => {
  assert.equal(resetGrantsText({ available: 1, kind: null, expiresAt: '2026-10-19T10:30:00Z' }, now), '1 reset available · expires in 3d')
  assert.equal(resetGrantsText({ available: 2, kind: 'weekly', expiresAt: '2026-10-16T15:20:00Z' }, now), '2 resets available · expires in 5h')
  assert.equal(resetGrantsText({ available: 1, kind: null, expiresAt: '2026-10-16T10:00:20Z' }, now), '1 reset available · expires in 1m')
  assert.equal(resetGrantsText({ available: 3, kind: null, expiresAt: null }, now), '3 resets available')
  assert.equal(resetGrantsText({ available: 1, kind: null, expiresAt: '2026-10-15T10:00:00Z' }, now), '1 reset available')
  assert.equal(resetGrantsText({ available: 0, kind: null, expiresAt: '2026-10-19T10:00:00Z' }, now), null)
  assert.equal(resetGrantsText(null, now), null)
  assert.equal(resetGrantsText(undefined, now), null)
})

test('snapshots stored before reset grants still parse', () => {
  const account = accountSchema.parse({
    id: 'fixture', provider: 'claude', label: 'Personal', color: '#b8d1bf',
    source: { kind: 'configDir', path: '/example/.claude' },
    identity: { email: null, plan: null }, error: null, refreshing: false,
    snapshot: { fetchedAt: '2026-10-16T09:59:30Z', extraUsage: null, windows: [] },
  })
  assert.equal(account.snapshot?.resetGrants, null)
})
