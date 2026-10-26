import assert from 'node:assert/strict'
import test from 'node:test'
import { balanceLines, formatMoney, groupWindows, resetDay, spendLimitLabel, windowScope } from '../../src/lib/format.ts'
import { accountSchema, usageWindowSchema, type Account } from '../../src/lib/model.ts'

const now = Date.parse('2026-10-16T10:00:00Z')
const base = {
  id: 'fixture-codex', provider: 'codex', label: 'Work', color: '#b8d1bf',
  source: { kind: 'configDir', path: '/example/.codex-work' },
  identity: { email: null, plan: 'pro' }, error: null, refreshing: false,
}
const snapshot = (extra: Record<string, unknown>): Account['snapshot'] => accountSchema.parse({
  ...base,
  snapshot: { fetchedAt: '2026-10-16T09:59:30Z', extraUsage: null, windows: [], ...extra },
}).snapshot

test('snapshots from older builds parse without a balance', () => {
  const parsed = snapshot({})
  assert.equal(parsed?.balance, null)
  assert.deepEqual(balanceLines(parsed, now), [])
  assert.deepEqual(balanceLines(null, now), [])
  assert.deepEqual(balanceLines(snapshot({ balance: { credits: null, spendLimit: null } }), now), [])
})

test('claude extra usage reads as money with an optional cap', () => {
  assert.deepEqual(balanceLines(snapshot({ extraUsage: { spent: 4.28, limit: 20, currency: 'USD' } }), now), [
    { label: 'Extra usage', value: '$4.28 of $20 this month', warn: false },
  ])
  assert.deepEqual(balanceLines(snapshot({ extraUsage: { spent: 12.5, limit: null, currency: 'USD' } }), now), [
    { label: 'Extra usage', value: '$12.50 this month', warn: false },
  ])
  assert.equal(formatMoney(20, 'USD'), '$20')
  assert.equal(formatMoney(4.2, 'USD'), '$4.20')
})

test('codex credits are a unit, never money', () => {
  const credits = (credits: Record<string, unknown>) => balanceLines(snapshot({ balance: { credits } }), now)[0]
  assert.deepEqual(credits({ amount: 1250, unlimited: false }), { label: 'Credits', value: '1,250 left', warn: false })
  assert.deepEqual(credits({ amount: 12.5, unlimited: false }), { label: 'Credits', value: '12.5 left', warn: false })
  assert.deepEqual(credits({ amount: null, unlimited: false }), { label: 'Credits', value: 'available', warn: false })
  assert.deepEqual(credits({ amount: null, unlimited: true }), { label: 'Credits', value: 'unlimited', warn: false })
  assert.ok(!balanceLines(snapshot({ balance: { credits: { amount: 1250, unlimited: false } } }), now).some(line => line.value.includes('$')))
})

test('a monthly credit limit shows used of limit and turns to a warning when reached', () => {
  const limit = (spendLimit: Record<string, unknown>) => balanceLines(snapshot({ balance: { spendLimit: { unit: 'credit', ...spendLimit } } }), now)[0]
  const open = limit({ used: 0, limit: 1, usedPercent: 0, resetsAt: '2026-11-01T00:00:01Z', reached: false })
  assert.equal(open?.label, 'Monthly credit limit')
  assert.equal(open?.value, '0 of 1 used · resets Nov 1')
  assert.equal(open?.warn, false)
  const reached = limit({ used: 100, limit: 100, usedPercent: 100, resetsAt: '2026-11-01T00:00:01Z', reached: true })
  assert.equal(reached?.value, 'reached · resets Nov 1')
  assert.equal(reached?.warn, true)
  assert.deepEqual(limit({ used: 320, limit: 500, usedPercent: 64, resetsAt: null, reached: false }), { label: 'Monthly credit limit', value: '320 of 500 used', warn: false })
  assert.deepEqual(limit({ used: 1, limit: 1, usedPercent: 100, resetsAt: '2026-10-16T09:00:00Z', reached: true }), { label: 'Monthly credit limit', value: 'reached', warn: true })
})

test('the limit is only called credits when the API says so', () => {
  const label = (unit: string | null) => balanceLines(snapshot({ balance: { spendLimit: { used: 1, limit: 2, usedPercent: 50, resetsAt: null, reached: false, unit } } }), now)[0]?.label
  assert.equal(label('credit'), 'Monthly credit limit')
  assert.equal(label('token'), 'Monthly token limit')
  assert.equal(label(null), 'Monthly limit')
  assert.equal(spendLimitLabel(' Credit '), 'Monthly credit limit')
  const older = snapshot({ balance: { spendLimit: { used: 1, limit: 2, usedPercent: 50, resetsAt: null, reached: false } } })
  assert.equal(older?.balance?.spendLimit?.unit, null)
})

test('a reset day shows the time today, the weekday this week, and the date beyond', () => {
  assert.equal(resetDay('2026-10-16T12:30:00Z', now), new Date('2026-10-16T12:30:00Z').toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' }))
  assert.equal(resetDay('2026-10-18T12:30:00Z', now), new Date('2026-10-18T12:30:00Z').toLocaleDateString(undefined, { weekday: 'short' }))
  assert.equal(resetDay('2026-11-01T00:00:01Z', now), 'Nov 1')
  assert.equal(resetDay('2026-10-16T09:00:00Z', now), null)
  assert.equal(resetDay(null, now), null)
})

test('credits come before the limit and extra usage stays first', () => {
  const lines = balanceLines(snapshot({
    extraUsage: { spent: 1, limit: null, currency: 'USD' },
    balance: { credits: { amount: 5, unlimited: false }, spendLimit: { used: 1, limit: 2, usedPercent: 50, resetsAt: null, reached: false, unit: 'credit' } },
  }), now)
  assert.deepEqual(lines.map(line => line.label), ['Extra usage', 'Credits', 'Monthly credit limit'])
})

test('named codex weekly limits become chips while their session windows stay folded', () => {
  const window = (id: string, label: string) => usageWindowSchema.parse({ id, label, usedPercent: 19, resetsAt: null, windowSeconds: null })
  const weekly = window('extra_8f14e45fceea167a_secondary_window', 'Spark')
  const session = window('extra_8f14e45fceea167a_primary_window', 'Spark · session')
  assert.equal(windowScope(weekly), 'model')
  assert.equal(windowScope(session), 'other')
  const groups = groupWindows([window('primary_window', 'Session'), session, weekly])
  assert.deepEqual(groups.models.map(entry => entry.label), ['Spark'])
  assert.deepEqual(groups.more.map(entry => entry.label), ['Spark · session'])
})
