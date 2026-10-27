import assert from 'node:assert/strict'
import test from 'node:test'
import { createDemoState, demoActivate, demoPreview } from '../../src/lib/demo.ts'
import { accountIdSchema, activatedSchema, appStateSchema, liveRestoreOffer, makeActivePreviewSchema } from '../../src/lib/model.ts'

test('state from older builds parses without switch fields', () => {
  const state = appStateSchema.parse({ ...createDemoState(null), switching: undefined })
  assert.deepEqual(state.switching, { activeAccountIds: [], restore: null })
})

test('previews and results parse and carry no credential fields', () => {
  const preview = makeActivePreviewSchema.parse({ accountId: 'claude-b', provider: 'claude', kind: 'swap', fingerprint: 'f', target: 'work@example.com', displaced: 'me@example.com', folder: '~/.claude-work', running: false })
  assert.ok(!('accessToken' in preview) && !('refreshToken' in preview))
  assert.throws(() => makeActivePreviewSchema.parse({ ...preview, kind: 'copy' }))
  const result = activatedSchema.parse({ state: createDemoState(null), target: 'work@example.com', previousAccountId: null })
  assert.equal(result.previousAccountId, null)
})

test('demo swap moves sources between the two people and keeps ids', () => {
  const state = createDemoState(null)
  assert.deepEqual(state.switching.activeAccountIds, ['demo-claude-personal'])
  const studio = accountIdSchema.parse('demo-claude-studio')
  const preview = demoPreview(state, studio, false)
  assert.ok(typeof preview !== 'string')
  assert.equal(preview.kind, 'swap')
  assert.equal(preview.displaced, 'alex@example.com')
  const { state: next, previousAccountId } = demoActivate(state, studio)
  assert.equal(previousAccountId, 'demo-claude-personal')
  assert.deepEqual(next.switching.activeAccountIds, ['demo-claude-studio'])
  const byId = new Map(next.accounts.map(account => [account.id, account]))
  assert.deepEqual(byId.get(studio)?.source, { kind: 'configDir', path: '~/.claude' })
  assert.deepEqual(byId.get(accountIdSchema.parse('demo-claude-personal'))?.source, { kind: 'configDir', path: '~/.claude-studio' })
  assert.equal(demoPreview(next, studio, false), 'This account is already active.')
})

test('demo refuses signed-out folders and moves into an empty slot', () => {
  const work = accountIdSchema.parse('demo-codex-work')
  const signedOut = demoPreview(createDemoState('signed-out'), work, false)
  assert.equal(signedOut, 'Work is signed out. Run CODEX_HOME=~/.codex-work codex login, then try again.')
  const move = demoPreview(createDemoState(null), work, false)
  assert.ok(typeof move !== 'string')
  assert.equal(move.kind, 'move')
  assert.equal(move.displaced, null)
  assert.equal(demoPreview(createDemoState(null), work, true), 'Codex is still running. Quit the Codex app and any Codex CLI or editor sessions, then try again.')
})

test('demo names sign-ins by label and plan when their emails match', () => {
  const studio = accountIdSchema.parse('demo-claude-studio')
  const preview = demoPreview(createDemoState('same-email'), studio, false)
  assert.ok(typeof preview !== 'string')
  assert.equal(preview.target, 'Claude Team (Team)')
  assert.equal(preview.displaced, 'Personal (Max 5×)')
  const state = createDemoState('same-email')
  state.accounts = state.accounts.map(account => account.id === studio ? { ...account, identity: { ...account.identity, plan: 'Max 5×' } } : account)
  assert.equal(demoPreview(state, studio, false), 'Both folders hold the same account. Sign out of one first.')
})

test('restore offers expire after 30 minutes', () => {
  const now = Date.parse('2026-10-26T12:00:00Z')
  const offer = { id: 'claude-1', provider: 'claude' as const, reason: 'verifyFailed' as const, createdAt: '2026-10-26T11:45:00Z' }
  assert.equal(liveRestoreOffer(offer, now), offer)
  assert.equal(liveRestoreOffer({ ...offer, createdAt: '2026-10-26T11:29:00Z' }, now), null)
  assert.equal(liveRestoreOffer(null, now), null)
})
