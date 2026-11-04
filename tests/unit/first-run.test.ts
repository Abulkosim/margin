import assert from 'node:assert/strict'
import test from 'node:test'
import { createDemoState } from '../../src/lib/demo.ts'
import { defaultSettings, settingsSchema } from '../../src/lib/model.ts'
import { demoProblemReport, problemReportSchema } from '../../src/lib/report.ts'
import { offersStartAtLogin } from '../../src/lib/startup.ts'

const withAccounts = (settings: Partial<typeof defaultSettings> = {}) => ({ ...createDemoState('single'), settings: { ...defaultSettings, ...settings } })

test('settings saved before the prompt existed parse as not yet answered', () => {
  const { autostartPromptDismissed: _, ...legacy } = defaultSettings
  assert.equal(settingsSchema.parse(legacy).autostartPromptDismissed, false)
  assert.equal(settingsSchema.parse({ ...defaultSettings, autostartPromptDismissed: true }).autostartPromptDismissed, true)
  assert.equal(settingsSchema.safeParse({ ...defaultSettings, autostartPromptDismissed: 'later' }).success, false)
})

test('the prompt shows once an account exists and autostart is off', () => {
  assert.equal(offersStartAtLogin(withAccounts(), false, false), true)
  assert.equal(offersStartAtLogin({ ...withAccounts(), accounts: [] }, false, false), false)
})

test('the prompt never shows after an answer or with autostart on', () => {
  assert.equal(offersStartAtLogin(withAccounts({ autostartPromptDismissed: true }), false, false), false)
  assert.equal(offersStartAtLogin(withAccounts({ autostart: true }), false, false), false)
})

test('the prompt waits while Margin cannot save its answer', () => {
  assert.equal(offersStartAtLogin(withAccounts(), true, false), false)
  assert.equal(offersStartAtLogin(withAccounts(), false, true), false)
  assert.equal(offersStartAtLogin({ ...withAccounts(), recovery: true }, false, false), false)
})

test('sample data hides the prompt so public screenshots stay unchanged', () => {
  assert.equal(createDemoState(null).settings.autostartPromptDismissed, true)
})

test('the preview report holds counts and known error kinds only', () => {
  const { accounts } = createDemoState('compact')
  const report = problemReportSchema.parse(demoProblemReport(accounts, '0.9.0', 'Macintosh'))
  assert.equal(report.version, '0.9.0')
  assert.equal(report.system, 'macOS 15.1')
  assert.equal(report.diagnostics, 'Claude accounts: 3\nClaude errors: signedOut\nCodex accounts: 2\nCodex errors: network')
  const text = JSON.stringify(report)
  for (const account of accounts) {
    assert.ok(!text.includes(account.label), account.label)
    assert.ok(!text.includes(account.identity.email ?? '\0'))
    if (account.source.kind === 'configDir') assert.ok(!text.includes(account.source.path))
    if (account.error) assert.ok(!text.includes(account.error.message))
  }
})

test('the preview report replaces unknown error kinds', () => {
  const [account] = createDemoState('single').accounts
  assert.ok(account)
  const leaky = { ...account, error: { kind: 'alex@example.com', message: 'secret', retryAfterSeconds: null } }
  const report = demoProblemReport([leaky], '0.9.0', 'Windows NT 10.0')
  assert.equal(report.system, 'Windows 11 (10.0.22631)')
  assert.equal(report.diagnostics, 'Claude accounts: 1\nClaude errors: other\nCodex accounts: 0\nCodex errors: none')
})
