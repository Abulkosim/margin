import assert from 'node:assert/strict'
import test from 'node:test'
import { accountIsStale, accountSource, formatRemaining, remainingPercent, relativeAge, resetCountdown, retryingMessage, signedOutMessage } from '../../src/lib/format.ts'
import { accountSchema, appStateSchema, defaultSettings, settingsSchema, usageWindowSchema } from '../../src/lib/model.ts'

const now = Date.parse('2026-10-16T10:00:00Z')
const account = accountSchema.parse({
  id: 'fixture-codex', provider: 'codex', label: 'Work', color: '#b8d1bf',
  source: { kind: 'configDir', path: '/example/.codex-work' },
  identity: { email: null, plan: 'pro' }, error: null, refreshing: false,
  snapshot: {
    fetchedAt: '2026-10-16T09:59:30Z', extraUsage: null,
    windows: [{ id: 'session', label: 'Session', usedPercent: 93.2, resetsAt: '2026-10-16T12:00:00Z', windowSeconds: 18000 }],
  },
})

test('remaining quota never implies available quota below 1%', () => {
  assert.equal(remainingPercent(100), 0)
  assert.equal(formatRemaining(99.9), '<1')
  assert.equal(formatRemaining(100), '0')
  assert.equal(formatRemaining(0), '100')
  assert.equal(formatRemaining(93.2), '7')
})

test('countdown preserves unknown and pending reset states', () => {
  assert.equal(resetCountdown(null, now), 'Reset time unavailable')
  assert.equal(resetCountdown('2026-10-16T09:59:00Z', now), 'Reset pending')
  assert.equal(resetCountdown('2026-10-16T10:00:20Z', now), 'Resets in <1m')
  assert.equal(resetCountdown('2026-10-16T12:13:00Z', now), 'Resets in 2h 13m')
  assert.equal(resetCountdown('2026-10-18T13:00:00Z', now), 'Resets in 2d 3h')
})

test('reset timestamps with offsets refer to the same instant', () => {
  assert.equal(resetCountdown('2026-10-16T17:13:00+05:00', now), 'Resets in 2h 13m')
})

test('errors and elapsed resets mark last-known values stale', () => {
  assert.equal(accountIsStale(account, defaultSettings, now), false)
  assert.equal(accountIsStale({ ...account, error: { kind: 'auth', message: 'Sign in again', retryAfterSeconds: null } }, defaultSettings, now), true)
  assert.equal(accountIsStale(account, defaultSettings, Date.parse('2026-10-16T10:11:00Z')), true)
  const expired = accountSchema.parse({ ...account, snapshot: {
    fetchedAt: '2026-10-16T09:59:30Z', extraUsage: null,
    windows: [{ id: 'session', label: 'Session', usedPercent: 93.2, resetsAt: '2026-10-16T09:59:45Z', windowSeconds: 18000 }],
  } })
  assert.equal(accountIsStale(expired, defaultSettings, now), true)
  assert.equal(expired.snapshot?.windows[0]?.usedPercent, 93.2)
})

test('rate limits and account switches retry quietly until the reading is old', () => {
  const later = Date.parse('2026-10-16T10:11:00Z')
  const rateLimited = { ...account, error: { kind: 'rateLimited', message: 'Wait', retryAfterSeconds: 240, retryAt: now + 240_000 } }
  assert.equal(accountIsStale(rateLimited, defaultSettings, now), false)
  assert.equal(retryingMessage(rateLimited, defaultSettings, now), 'Rate limited · retrying in 4m')
  assert.equal(retryingMessage({ ...rateLimited, error: { ...rateLimited.error, retryAt: now - 1 } }, defaultSettings, now), 'Rate limited · retrying…')
  assert.equal(accountIsStale(rateLimited, defaultSettings, later), true)
  assert.equal(retryingMessage(rateLimited, defaultSettings, later), null)

  const changed = { ...account, error: { kind: 'credentialsChanged', message: 'Changed', retryAfterSeconds: null } }
  assert.equal(accountIsStale(changed, defaultSettings, now), false)
  assert.equal(retryingMessage(changed, defaultSettings, now), 'Retrying…')
  assert.equal(accountIsStale(changed, defaultSettings, later), true)
  assert.equal(retryingMessage(changed, defaultSettings, later), null)

  assert.equal(accountIsStale({ ...changed, snapshot: null }, defaultSettings, now), true)
  assert.equal(retryingMessage(changed, defaultSettings, now, true), null)
  assert.equal(retryingMessage({ ...account, error: { kind: 'network', message: 'Offline', retryAfterSeconds: null } }, defaultSettings, now), null)
})

test('no snapshot remains unknown instead of becoming empty quota', () => {
  const unread = accountSchema.parse({ ...account, snapshot: null })
  assert.equal(unread.snapshot, null)
  assert.equal(accountIsStale(unread, defaultSettings, now), false)
})

test('cached readings disclose their age', () => {
  assert.equal(relativeAge('2026-10-16T09:59:55Z', now), 'just now')
  assert.equal(relativeAge('2026-10-16T09:28:00Z', now), '32m ago')
  assert.equal(relativeAge('2026-10-15T05:00:00Z', now), '1d ago')
})

test('IPC validation rejects impossible or malformed quota data', () => {
  const valid = { id: 'a', label: 'Session', usedPercent: 50, resetsAt: null, windowSeconds: 18000 }
  for (const usedPercent of [-1, 101, NaN, Infinity, '50']) {
    assert.equal(usageWindowSchema.safeParse({ ...valid, usedPercent }).success, false)
  }
  assert.equal(usageWindowSchema.safeParse({ ...valid, resetsAt: 'tomorrow' }).success, false)
  assert.equal(usageWindowSchema.safeParse({ ...valid, windowSeconds: 1.5 }).success, false)
  assert.equal(accountSchema.safeParse({ ...account, provider: 'other-provider' }).success, false)
})

test('settings use the same limits as Rust', () => {
  assert.equal(settingsSchema.safeParse(defaultSettings).success, true)
  const { trayMetric: _, ...legacy } = defaultSettings
  assert.equal(settingsSchema.parse(legacy).trayMetric, 'core')
  assert.equal(settingsSchema.safeParse({ ...defaultSettings, trayMetric: 'opus' }).success, false)
  const { trayShowPercent: __, ...withoutPercent } = defaultSettings
  assert.equal(settingsSchema.parse(withoutPercent).trayShowPercent, false)
  assert.equal(settingsSchema.safeParse({ ...defaultSettings, trayShowPercent: 'yes' }).success, false)
  assert.equal(settingsSchema.safeParse({ ...defaultSettings, pollIntervalSeconds: 15 }).success, false)
  assert.equal(settingsSchema.safeParse({ ...defaultSettings, pollIntervalSeconds: 600, idlePollIntervalSeconds: 300 }).success, false)
  for (const thresholds of [[], [80, 80], [95, 80], [80.5], [101]]) {
    assert.equal(settingsSchema.safeParse({ ...defaultSettings, thresholds }).success, false)
  }
})

test('spend-only accounts remain valid without inventing quota windows', () => {
  const spendOnly = accountSchema.parse({ ...account, snapshot: {
    fetchedAt: '2026-10-16T09:59:30Z', windows: [], extraUsage: { spent: 4.28, limit: null, currency: 'USD' },
  } })
  const state = appStateSchema.parse({ revision: 0, accounts: [spendOnly], settings: defaultSettings, refreshing: false })
  assert.equal(state.accounts[0]?.snapshot?.windows.length, 0)
  assert.equal(state.notice, null)
})

test('keychain-backed Claude sources parse and describe where the token lives', () => {
  const claude = (source: unknown) => accountSchema.parse({ ...account, provider: 'claude', source })
  assert.equal(accountSource(claude({ kind: 'keychain' })), 'macOS Keychain')
  assert.throws(() => claude({ kind: 'keychainItem', suffix: '52eda417' }))
  assert.equal(accountSource(claude({ kind: 'configDir', path: '/example/.claude-work', keychainSuffix: 'dd1118a7' })), '/example/.claude-work · Keychain')
  assert.equal(accountSource(account), '/example/.codex-work')
})

test('signed-out copy names the tool and the account folder', () => {
  const claude = accountSchema.parse({
    ...account, provider: 'claude', source: { kind: 'configDir', path: '/example/.claude-work' },
    error: { kind: 'signedOut', message: 'Claude is signed out.', retryAfterSeconds: null, signedOutSince: '2026-10-16T08:00:00Z', signInCommand: 'CLAUDE_CONFIG_DIR=/example/.claude-work claude' },
  })
  assert.equal(signedOutMessage(claude, now), 'Signed out 2h ago · run claude in /example/.claude-work')
  const keychain = accountSchema.parse({ ...claude, source: { kind: 'keychain' } })
  assert.equal(signedOutMessage(keychain, now), 'Signed out 2h ago · run claude')
  const codex = accountSchema.parse({ ...account, error: { kind: 'signedOut', message: 'Codex is signed out.', retryAfterSeconds: null, signInCommand: 'CODEX_HOME=/example/.codex-work codex login' } })
  assert.equal(signedOutMessage(codex, now), 'Signed out · run codex login in /example/.codex-work')
  assert.equal(signedOutMessage(account, now), null)
  assert.equal(signedOutMessage(accountSchema.parse({ ...account, error: { kind: 'auth', message: 'Denied', retryAfterSeconds: null } }), now), null)
})
