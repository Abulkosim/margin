import { appStateSchema, defaultSettings, type AppState } from './model.ts'

export function createDemoState(scenario: string | null): AppState {
  const now = Date.now()
  const future = (hours: number): string => new Date(now + hours * 3_600_000).toISOString()
  const state = appStateSchema.parse({
    revision: 0,
    settings: defaultSettings,
    refreshing: false,
    accounts: [
      {
        id: 'demo-claude-personal', provider: 'claude', label: 'Personal', color: '#b8d1bf',
        source: { kind: 'configDir', path: '~/.claude' },
        identity: { email: 'alex@example.com', plan: 'Max 5×' }, error: null, refreshing: false,
        snapshot: { fetchedAt: new Date(now).toISOString(), extraUsage: null, windows: [
          { id: 'five_hour', label: 'Session', usedPercent: 28, resetsAt: future(2.3), windowSeconds: 18_000 },
          { id: 'seven_day', label: 'Weekly', usedPercent: 58, resetsAt: future(102), windowSeconds: 604_800 },
          { id: 'seven_day_sonnet', label: 'Sonnet', usedPercent: 34, resetsAt: future(102), windowSeconds: 604_800 },
        ] },
      },
      {
        id: 'demo-codex-work', provider: 'codex', label: 'Work', color: '#b8d1bf',
        source: { kind: 'configDir', path: '~/.codex-work' },
        identity: { email: 'alex@studio.example', plan: 'Pro' }, error: null, refreshing: false,
        snapshot: { fetchedAt: new Date(now).toISOString(), extraUsage: null, windows: [
          { id: 'primary_window', label: 'Session', usedPercent: 9, resetsAt: future(3.68), windowSeconds: 18_000 },
          { id: 'secondary_window', label: 'Weekly', usedPercent: 32, resetsAt: future(122), windowSeconds: 604_800 },
        ] },
      },
      {
        id: 'demo-claude-studio', provider: 'claude', label: 'Studio', color: '#b8d1bf',
        source: { kind: 'configDir', path: '~/.claude-studio' },
        identity: { email: 'hello@studio.example', plan: 'Pro' }, error: null, refreshing: false,
        snapshot: { fetchedAt: new Date(now).toISOString(), extraUsage: { spent: 4.28, limit: 20, currency: 'USD' }, windows: [
          { id: 'five_hour', label: 'Session', usedPercent: 82, resetsAt: future(0.75), windowSeconds: 18_000 },
          { id: 'seven_day', label: 'Weekly', usedPercent: 14, resetsAt: future(56), windowSeconds: 604_800 },
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
  return state
}
