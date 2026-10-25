import assert from 'node:assert/strict'
import test from 'node:test'
import { orderAccounts } from '../../src/lib/format.ts'
import { accountSchema, defaultSettings, settingsSchema, type Account } from '../../src/lib/model.ts'

const now = Date.parse('2026-10-16T10:00:00Z')
const fetchedAt = '2026-10-16T09:59:30Z'
const resetsAt = '2026-10-16T14:00:00Z'

function account(id: string, session: number | null, weekly: number | null, extra: Partial<Account> = {}): Account {
  const windows = [
    ...session === null ? [] : [{ id: 'five_hour', label: 'Session', usedPercent: session, resetsAt, windowSeconds: 18000 }],
    ...weekly === null ? [] : [{ id: 'seven_day', label: 'Weekly', usedPercent: weekly, resetsAt, windowSeconds: 604800 }],
  ]
  return accountSchema.parse({
    id, provider: 'claude', label: id, color: '#b8d1bf', source: { kind: 'configDir', path: `/example/.${id}` },
    identity: { email: null, plan: null }, error: null, refreshing: false,
    snapshot: windows.length ? { fetchedAt, extraUsage: null, windows } : null,
    ...extra,
  })
}

const ids = (accounts: Account[]): string[] => accounts.map(entry => entry.id)
const signedOut = { kind: 'signedOut', message: 'Signed out', retryAfterSeconds: null, signInCommand: 'claude' }

test('urgency order puts the lowest remaining core window first', () => {
  const accounts = [account('roomy', 10, 20), account('weekly-low', 5, 90), account('session-low', 70, 30), account('waiting', null, null)]
  assert.deepEqual(ids(orderAccounts(accounts, defaultSettings, now)), ['weekly-low', 'session-low', 'roomy', 'waiting'])
})

test('pinned account stays first and unavailable accounts go last', () => {
  const settings = settingsSchema.parse({ ...defaultSettings, trayMode: 'pinned', pinnedAccountId: 'roomy' })
  const accounts = [
    account('out', null, null, { error: signedOut }),
    account('stale', 99, 99, { error: { kind: 'network', message: 'Offline', retryAfterSeconds: null } }),
    account('tight', 60, 95),
    account('roomy', 0, 0),
  ]
  assert.deepEqual(ids(orderAccounts(accounts, settings, now)), ['roomy', 'tight', 'out', 'stale'])
  assert.deepEqual(ids(orderAccounts(accounts, { ...settings, trayMode: 'lowest' }, now)), ['tight', 'roomy', 'out', 'stale'])
})

test('ties keep the order accounts were added', () => {
  const accounts = [account('b', 50, 40), account('a', 40, 50), account('c', 50, 50)]
  assert.deepEqual(ids(orderAccounts(accounts, defaultSettings, now)), ['b', 'a', 'c'])
})

test('rate-limited accounts with a fresh reading still sort by usage', () => {
  const accounts = [account('roomy', 0, 0), account('limited', 80, 20, { error: { kind: 'rateLimited', message: 'Wait', retryAfterSeconds: 60 } })]
  assert.deepEqual(ids(orderAccounts(accounts, defaultSettings, now)), ['limited', 'roomy'])
  assert.deepEqual(ids(orderAccounts(accounts, defaultSettings, now, true)), ['roomy', 'limited'])
})

test('added order leaves the list untouched, even with a pin', () => {
  const settings = settingsSchema.parse({ ...defaultSettings, accountOrder: 'added', trayMode: 'pinned', pinnedAccountId: 'c' })
  const accounts = [account('a', 0, 0), account('b', 99, 99), account('c', 50, 50)]
  assert.deepEqual(ids(orderAccounts(accounts, settings, now)), ['a', 'b', 'c'])
})

test('account order defaults to urgency for older settings', () => {
  const { accountOrder: _, ...legacy } = defaultSettings
  assert.equal(settingsSchema.parse(legacy).accountOrder, 'urgency')
  assert.equal(settingsSchema.safeParse({ ...defaultSettings, accountOrder: 'alphabetical' }).success, false)
})
