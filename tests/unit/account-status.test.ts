import assert from 'node:assert/strict'
import test from 'node:test'
import { effectScope, reactive } from 'vue'
import { useAccountStatus, type AccountStatusSource } from '../../src/lib/accountStatus.ts'
import { planName } from '../../src/lib/format.ts'
import { accountSchema, defaultSettings } from '../../src/lib/model.ts'

const now = Date.parse('2026-10-16T10:00:00Z')
const account = accountSchema.parse({
  id: 'fixture-claude', provider: 'claude', label: 'Work', color: '#b8d1bf',
  source: { kind: 'configDir', path: '/example/.claude-work' },
  identity: { email: null, plan: 'max_20x' }, error: null, refreshing: true,
  snapshot: {
    fetchedAt: '2026-10-16T09:59:30Z', extraUsage: null,
    windows: [{ id: 'five_hour', label: 'Session', usedPercent: 40, resetsAt: '2026-10-16T12:00:00Z', windowSeconds: 18000, scope: 'core' }],
  },
})

function status(source: AccountStatusSource) {
  const scope = effectScope()
  const result = scope.run(() => useAccountStatus(source))
  assert.ok(result)
  return { ...result, stop: () => scope.stop() }
}

test('plan names read as words', () => {
  assert.equal(planName('max_20x'), 'Max 20x')
  assert.equal(planName('pro'), 'Pro')
  assert.equal(planName(null), null)
  assert.equal(planName(''), null)
})

test('account status follows the account and the desktop connection', () => {
  const source = reactive<AccountStatusSource>({ account, settings: defaultSettings, now, connectionError: null })
  const { stale, retrying, refreshing, idle, plan, stop } = status(source)
  assert.equal(stale.value, false)
  assert.equal(refreshing.value, true)
  assert.equal(retrying.value, null)
  assert.equal(idle.value, null)
  assert.equal(plan.value, 'Max 20x')

  source.account = { ...account, refreshing: false, error: { kind: 'rateLimited', message: 'Wait', retryAfterSeconds: 60, retryAt: now + 60_000 } }
  assert.equal(retrying.value, 'Rate limited · retrying in 1m')
  assert.equal(stale.value, false)

  source.account = { ...account, error: { kind: 'signedOut', message: 'Expired', retryAfterSeconds: null, signedOutSince: '2026-10-16T09:00:00Z' } }
  assert.match(idle.value ?? '', /^Idle, last read /)

  source.connectionError = 'Couldn’t read your accounts. Refresh to reconnect.'
  assert.equal(stale.value, true)
  assert.equal(refreshing.value, false)
  assert.equal(retrying.value, null)
  assert.equal(idle.value, null)
  stop()
})
