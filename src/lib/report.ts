import { z } from 'zod'
import type { Account, Provider } from './model.ts'
import { platform, type Platform } from './platform.ts'

export const problemReportSchema = z.object({
  version: z.string().min(1).max(64),
  system: z.string().min(1).max(64),
  diagnostics: z.string().min(1).max(1000),
})
export type ProblemReport = z.infer<typeof problemReportSchema>

const errorKinds = new Set(['auth', 'configuration', 'credentials', 'credentialsChanged', 'invalidResponse', 'network', 'provider', 'rateLimited', 'redirect', 'signedOut', 'stale', 'unavailable'])

function providerLines(name: string, provider: Provider, accounts: Account[]): string[] {
  const owned = accounts.filter(account => account.provider === provider)
  const kinds = new Set(owned.flatMap(account => account.error ? [errorKinds.has(account.error.kind) ? account.error.kind : 'other'] : []))
  return [`${name} accounts: ${owned.length}`, `${name} errors: ${kinds.size ? [...kinds].sort().join(', ') : 'none'}`]
}

/** Browser preview only; the desktop app builds the real report in Rust (`core/report.rs`). */
export function demoProblemReport(accounts: Account[], version: string, system: Platform = platform): ProblemReport {
  return {
    version,
    system: system === 'windows' ? 'Windows 11 (10.0.22631)' : 'macOS 15.1',
    diagnostics: [...providerLines('Claude', 'claude', accounts), ...providerLines('Codex', 'codex', accounts)].join('\n'),
  }
}
