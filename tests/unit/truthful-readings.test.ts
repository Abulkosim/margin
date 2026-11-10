import assert from 'node:assert/strict'
import test from 'node:test'
import { accountIsStale, clockTime, idleAction, idleStatus, idleStatusShort, modelWindowSummary, orderAccounts, resetPassedText, signedOutMessage, tokenExpired, unreadSinceReset } from '../../src/lib/format.ts'
import { accountSchema, defaultSettings, type Account, type UsageWindow } from '../../src/lib/model.ts'
import { createDemoState, demoPreview } from '../../src/lib/demo.ts'

const now = Date.parse('2026-10-16T14:00:00Z')
const expired = { kind: 'signedOut', message: 'Token expired', retryAfterSeconds: null, signedOutSince: '2026-10-16T12:00:00Z', signInCommand: 'claude' }
const rejected = { kind: 'signedOut', message: 'Signed out', retryAfterSeconds: null, signInCommand: 'claude' }

function window(id: string, usedPercent: number, resetsAt: string | null): UsageWindow {
  return { id, label: id, usedPercent, resetsAt, windowSeconds: 18_000, scope: id === 'five_hour' ? 'core' : 'other' }
}

function account(id: string, windows: UsageWindow[] | null, error: unknown = null, fetchedAt = '2026-10-16T11:00:00Z'): Account {
  return accountSchema.parse({
    id, provider: 'claude', label: id, color: '#b8d1bf', source: { kind: 'configDir', path: `/example/.${id}` },
    identity: { email: null, plan: null }, error, refreshing: false,
    snapshot: windows && { fetchedAt, extraUsage: null, windows },
  })
}

test('an expired token reads as idle, with its last reading or plainly without one', () => {
  const idle = account('idle', [window('five_hour', 40, '2026-10-16T18:00:00Z')], expired)
  assert.equal(tokenExpired(idle), true)
  assert.equal(idleStatus(idle, now), 'Idle, last read 3h ago')
  assert.equal(idleStatusShort(idle, now), 'Idle, read 3h ago')
  assert.equal(idleAction(idle), 'To get a new reading, run:')
  assert.equal(signedOutMessage(idle), null)
  const never = account('never', null, expired)
  assert.equal(idleStatus(never, now), 'Idle, no reading yet')
  assert.equal(idleAction(never), 'To get a first reading, run:')
  const { signInCommand: _, ...noCommand } = expired
  assert.equal(idleAction(account('none', null, noCommand)), 'Run the Claude CLI to get a new reading.')
})

test('a rejected or missing sign-in stays signed out', () => {
  const out = account('out', [window('five_hour', 40, '2026-10-16T18:00:00Z')], rejected)
  assert.equal(tokenExpired(out), false)
  assert.equal(idleStatus(out, now), null)
  assert.equal(idleAction(out), null)
  assert.equal(signedOutMessage(out), 'Signed out. Run:')
  assert.equal(idleStatus(account('fine', []), now), null)
  assert.equal(idleStatus(account('other', [], { kind: 'network', message: 'Offline', retryAfterSeconds: null }), now), null)
})

test('only a reset after the last reading is unread', () => {
  const fetchedAt = '2026-10-16T11:00:00Z'
  assert.equal(unreadSinceReset(window('a', 40, '2026-10-16T12:00:00Z'), fetchedAt, now), true)
  assert.equal(unreadSinceReset(window('a', 40, '2026-10-16T14:00:00Z'), fetchedAt, now), true)
  assert.equal(unreadSinceReset(window('a', 40, '2026-10-16T15:00:00Z'), fetchedAt, now), false)
  assert.equal(unreadSinceReset(window('a', 40, '2026-10-16T10:00:00Z'), fetchedAt, now), false)
  assert.equal(unreadSinceReset(window('a', 40, null), fetchedAt, now), false)
  assert.equal(unreadSinceReset(window('a', 40, '2026-10-16T12:00:00Z'), 'unreadable', now), true)
})

test('a passed reset names its clock time and carries no percentage', () => {
  const reset = '2026-10-16T12:00:00Z'
  const text = resetPassedText(window('a', 40, reset), now)
  assert.equal(text, `Reset ${clockTime(new Date(reset), now)}, not read since`)
  assert.doesNotMatch(text, /%/)
  const model = { ...window('Opus', 40, reset), label: 'Opus' }
  assert.equal(modelWindowSummary(model, now, '2026-10-16T11:00:00Z'), `Opus weekly · reset ${clockTime(new Date(reset), now)}, not read since`)
  assert.equal(modelWindowSummary(model, now), 'Opus weekly · reset pending')
})

test('a reset long past shows its date instead of a weekday', () => {
  const old = new Date(now - 10 * 86_400_000)
  assert.equal(clockTime(old, now), old.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' }))
  const recent = new Date(now - 2 * 86_400_000)
  assert.equal(clockTime(recent, now), recent.toLocaleString(undefined, { weekday: 'short', hour: 'numeric', minute: '2-digit' }))
})

test('urgency order never ranks an account by a window past its reset', () => {
  const passed = account('passed', [window('five_hour', 99, '2026-10-16T12:00:00Z')])
  const current = account('current', [window('five_hour', 10, '2026-10-16T18:00:00Z')], null, '2026-10-16T13:59:50Z')
  const idle = account('idle', [window('five_hour', 95, '2026-10-16T18:00:00Z')], expired)
  assert.equal(accountIsStale(passed, defaultSettings, now), true)
  assert.deepEqual(orderAccounts([passed, idle, current], defaultSettings, now).map(entry => entry.id), ['current', 'passed', 'idle'])
})

test('a stored snapshot without the field reads as complete', () => {
  assert.equal(account('old', [])?.snapshot?.incomplete, false)
})

test('demo scenarios keep each state distinct', () => {
  const idle = createDemoState('idle')
  assert.deepEqual(idle.accounts.map(entry => tokenExpired(entry)), [true, false, true])
  assert.equal(idle.accounts[2]?.snapshot, null)
  const out = createDemoState('signed-out')
  assert.deepEqual(out.accounts.map(entry => tokenExpired(entry)), [false, false, false])
  assert.equal(createDemoState('incomplete').accounts[0]?.snapshot?.incomplete, true)
  const studio = idle.accounts[2]
  assert.ok(studio)
  assert.equal(typeof demoPreview(idle, studio.id, false), 'object')
})
