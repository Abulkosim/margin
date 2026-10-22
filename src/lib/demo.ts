import { appStateSchema, defaultSettings, type AppState } from './model.ts'
import type { UpdateState } from './updater.ts'

export function createDemoState(scenario: string | null): AppState {
  const now = Date.now()
  const future = (hours: number): string => new Date(now + hours * 3_600_000).toISOString()
  const state = appStateSchema.parse({
    revision: 0,
    settings: { ...defaultSettings, notifications: false },
    refreshing: false,
    accounts: [
      {
        id: 'demo-claude-personal', provider: 'claude', label: 'Personal', color: '#b8d1bf',
        source: { kind: 'configDir', path: '~/.claude' },
        identity: { email: 'alex@example.com', plan: 'Max 5×' }, error: null, refreshing: false,
        snapshot: { fetchedAt: new Date(now).toISOString(), extraUsage: null, windows: [
          { id: 'five_hour', label: 'Session', usedPercent: 28, resetsAt: future(2.3), windowSeconds: 18_000 },
          { id: 'seven_day', label: 'Weekly', usedPercent: 58, resetsAt: future(102), windowSeconds: 604_800 },
          { id: 'weekly_scoped_5d1e5ac9f2d0b7a4', label: 'Fable', usedPercent: 66, resetsAt: future(102), windowSeconds: 604_800, scope: 'model' },
          { id: 'weekly_scoped_0c3e7b2a91f4d658', label: 'Opus', usedPercent: 88, resetsAt: future(102), windowSeconds: 604_800, scope: 'model' },
          { id: 'seven_day_sonnet', label: 'Sonnet', usedPercent: 34, resetsAt: future(102), windowSeconds: 604_800, scope: 'model' },
        ] },
      },
      {
        id: 'demo-codex-work', provider: 'codex', label: 'Work', color: '#b8d1bf',
        source: { kind: 'configDir', path: '~/.codex-work' },
        identity: { email: 'alex@studio.example', plan: 'Pro' }, error: null, refreshing: false,
        snapshot: { fetchedAt: new Date(now).toISOString(), extraUsage: null, windows: [
          { id: 'primary_window', label: 'Session', usedPercent: 9, resetsAt: future(3.68), windowSeconds: 18_000 },
          { id: 'secondary_window', label: 'Weekly', usedPercent: 32, resetsAt: future(122), windowSeconds: 604_800 },
          { id: 'extra_8f14e45fceea167a_primary_window', label: 'Spark · session', usedPercent: 12, resetsAt: future(3.68), windowSeconds: 18_000, scope: 'other' },
        ] },
      },
      {
        id: 'demo-claude-studio', provider: 'claude', label: 'Studio', color: '#b8d1bf',
        source: { kind: 'configDir', path: '~/.claude-studio' },
        identity: { email: 'hello@studio.example', plan: 'Pro' }, error: null, refreshing: false,
        snapshot: { fetchedAt: new Date(now).toISOString(), extraUsage: { spent: 4.28, limit: 20, currency: 'USD' }, resetGrants: { available: 1, kind: null, expiresAt: future(74) }, windows: [
          { id: 'five_hour', label: 'Session', usedPercent: 82, resetsAt: future(0.75), windowSeconds: 18_000 },
          { id: 'seven_day', label: 'Weekly', usedPercent: 14, resetsAt: future(56), windowSeconds: 604_800 },
          { id: 'seven_day_sonnet', label: 'Sonnet', usedPercent: 21, resetsAt: future(56), windowSeconds: 604_800, scope: 'model' },
        ] },
      },
    ],
  })
  if (scenario === 'empty') state.accounts = []
  if (scenario === 'loading') {
    state.refreshing = true
    state.accounts = state.accounts.map(account => ({ ...account, snapshot: null, refreshing: true }))
  }
  if (scenario === 'stale' || scenario === 'error') {
    state.accounts = state.accounts.map((account, index) => index === 0 ? {
      ...account,
      snapshot: scenario === 'error' || !account.snapshot ? null : {
        ...account.snapshot,
        fetchedAt: new Date(now - 32 * 60_000).toISOString(),
      },
      error: { kind: 'unauthorized', message: 'Your Claude session has expired. Run claude auth login, then refresh.', retryAfterSeconds: null },
    } : account)
  }
  if (scenario === 'signed-out') {
    state.accounts = state.accounts.map((account, index) => {
      if (index === 0) return {
        ...account,
        snapshot: account.snapshot && { ...account.snapshot, fetchedAt: new Date(now - 2 * 3_600_000).toISOString() },
        error: { kind: 'signedOut', message: 'Claude is signed out. Run claude, then refresh.', retryAfterSeconds: null, signedOutSince: new Date(now - 2 * 3_600_000).toISOString(), signInCommand: 'claude' },
      }
      if (index === 1) return {
        ...account,
        snapshot: null,
        error: { kind: 'signedOut', message: 'Codex is signed out. Run CODEX_HOME=~/.codex-work codex login, then refresh.', retryAfterSeconds: null, signInCommand: 'CODEX_HOME=~/.codex-work codex login' },
      }
      return account
    })
  }
  if (scenario === 'retrying') {
    const fetchedAt = (minutes: number): string => new Date(now - minutes * 60_000).toISOString()
    state.accounts = state.accounts.map((account, index) => {
      if (!account.snapshot) return account
      if (index === 0) return {
        ...account,
        snapshot: { ...account.snapshot, fetchedAt: fetchedAt(2) },
        error: { kind: 'rateLimited', message: 'The provider asked us to wait. Retrying automatically.', retryAfterSeconds: 240, retryAt: now + 4 * 60_000 },
      }
      if (index === 1) return {
        ...account,
        snapshot: { ...account.snapshot, fetchedAt: fetchedAt(1) },
        error: { kind: 'credentialsChanged', message: 'The CLI account changed during refresh. Refresh again to read the current account.', retryAfterSeconds: null },
      }
      return {
        ...account,
        snapshot: { ...account.snapshot, fetchedAt: fetchedAt(32) },
        error: { kind: 'rateLimited', message: 'The provider asked us to wait. Retrying automatically.', retryAfterSeconds: null },
      }
    })
  }
  if (scenario === 'recovery') {
    state.accounts = []
    state.recovery = true
    state.notice = 'Margin’s settings file is damaged or unsupported. Settings file: ~/Library/Application Support/com.abulkosim.margin/state.json.'
  }
  if (scenario === 'hidden') state.ignoredSources = ['claude:~/.claude-old', 'codex:~/.codex-old']
  if (scenario === 'recovered') state.recovered = ['Skipped account “Old work” because its saved data couldn’t be read.']
  return state
}

export function createDemoUpdateState(scenario: string | null): UpdateState {
  return scenario === 'update-available' ? { kind: 'available', version: '0.9.0' } : { kind: 'idle' }
}
