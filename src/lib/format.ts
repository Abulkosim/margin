import type { Account, Settings } from './model'

export function remainingPercent(used: number): number {
  return Math.max(0, Math.min(100, 100 - used))
}

export function formatRemaining(used: number): string {
  const remaining = remainingPercent(used)
  return remaining > 0 && remaining < 1 ? '<1' : Math.round(remaining).toString()
}

export function resetCountdown(resetsAt: string | null, now: number): string {
  if (!resetsAt) return 'Reset time unavailable'
  const milliseconds = Date.parse(resetsAt) - now
  if (milliseconds <= 0) return 'Reset pending'
  if (milliseconds < 60_000) return 'Resets in <1m'
  const minutes = Math.ceil(milliseconds / 60_000)
  if (minutes < 60) return `Resets in ${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `Resets in ${hours}h ${minutes % 60}m`
  return `Resets in ${Math.floor(hours / 24)}d ${hours % 24}h`
}

export function relativeAge(timestamp: string, now: number): string {
  const seconds = Math.max(0, Math.floor((now - Date.parse(timestamp)) / 1000))
  if (seconds < 10) return 'just now'
  if (seconds < 60) return `${seconds}s ago`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  return `${Math.floor(hours / 24)}d ago`
}

export function accountIsStale(account: Account, settings: Settings, now: number, connectionInterrupted = false): boolean {
  if (connectionInterrupted || account.error) return true
  if (!account.snapshot) return false
  return now - Date.parse(account.snapshot.fetchedAt) > Math.max(settings.idlePollIntervalSeconds * 2, 180) * 1000
    || account.snapshot.windows.some(window => window.resetsAt !== null && Date.parse(window.resetsAt) <= now)
}

export function formatMoney(amount: number, currency: string): string {
  try {
    return new Intl.NumberFormat(undefined, { style: 'currency', currency, maximumFractionDigits: 2 }).format(amount)
  } catch {
    return `${amount.toFixed(2)} ${currency}`
  }
}

export function providerName(provider: Account['provider']): string {
  return provider === 'claude' ? 'Claude' : 'Codex'
}

export function accountSource(account: Account): string {
  return account.source.kind === 'keychain' ? 'macOS Keychain' : account.source.path
}
