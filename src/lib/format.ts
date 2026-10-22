import type { Account, ResetGrants, Settings, UsageWindow, WindowScope } from './model'
import type { Pace } from './pace'

export function remainingPercent(used: number): number {
  return Math.max(0, Math.min(100, 100 - used))
}

export function formatRemaining(used: number): string {
  const remaining = remainingPercent(used)
  return remaining > 0 && remaining < 1 ? '<1' : Math.round(remaining).toString()
}

function shortDuration(milliseconds: number): string {
  if (milliseconds < 60_000) return '<1m'
  const minutes = Math.ceil(milliseconds / 60_000)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ${minutes % 60}m`
  return `${Math.floor(hours / 24)}d ${hours % 24}h`
}

const sessionWindowIds = new Set(['five_hour', 'primary_window'])
const coreWindowIds = new Set([...sessionWindowIds, 'seven_day', 'secondary_window'])
const modelWindowIds = new Set(['seven_day_sonnet', 'seven_day_opus'])

export function windowScope(window: UsageWindow): WindowScope {
  if (window.scope) return window.scope
  if (coreWindowIds.has(window.id)) return 'core'
  return modelWindowIds.has(window.id) || window.id.startsWith('weekly_scoped_') ? 'model' : 'other'
}

export function groupWindows(windows: UsageWindow[]): { rows: UsageWindow[]; models: UsageWindow[]; more: UsageWindow[] } {
  const rows = windows.filter(window => windowScope(window) === 'core')
  if (!rows.length) return { rows: windows.slice(0, 2), models: [], more: windows.slice(2) }
  return { rows, models: windows.filter(window => windowScope(window) === 'model'), more: windows.filter(window => windowScope(window) === 'other') }
}

export function coreSummary(windows: UsageWindow[]): { session: UsageWindow | null; weekly: UsageWindow | null; limiting: UsageWindow | null } {
  const core = windows.filter(window => windowScope(window) === 'core')
  const session = core.find(window => sessionWindowIds.has(window.id)) ?? null
  const weekly = core.find(window => window !== session) ?? null
  const limiting = [session, weekly].reduce<UsageWindow | null>((worst, window) => {
    if (!window || !worst) return worst ?? window
    if (window.usedPercent !== worst.usedPercent) return window.usedPercent > worst.usedPercent ? window : worst
    return Date.parse(window.resetsAt ?? '') < Date.parse(worst.resetsAt ?? '') ? window : worst
  }, null)
  return { session, weekly, limiting }
}

export function resetIn(resetsAt: string | null, now: number): string | null {
  if (!resetsAt) return null
  const milliseconds = Date.parse(resetsAt) - now
  if (!Number.isFinite(milliseconds)) return null
  return milliseconds <= 0 ? 'Pending' : shortDuration(milliseconds)
}

export function modelWindowSummary(window: UsageWindow, now: number): string {
  const reset = resetCountdown(window.resetsAt, now)
  return `${window.label} weekly · ${reset.charAt(0).toLowerCase()}${reset.slice(1)}`
}

export function resetCountdown(resetsAt: string | null, now: number): string {
  if (!resetsAt) return 'Reset time unavailable'
  const milliseconds = Date.parse(resetsAt) - now
  if (milliseconds <= 0) return 'Reset pending'
  return `Resets in ${shortDuration(milliseconds)}`
}

export function clockTime(time: Date, now: number): string {
  const today = new Date(now)
  const sameDay = time.getFullYear() === today.getFullYear() && time.getMonth() === today.getMonth() && time.getDate() === today.getDate()
  if (sameDay) return time.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
  const withinWeek = time.getTime() - now < 6 * 86_400_000
  return time.toLocaleString(undefined, withinWeek ? { weekday: 'short', hour: 'numeric', minute: '2-digit' } : { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
}

export function paceText(pace: Pace, now: number): string | null {
  if (pace.kind === 'hidden') return null
  if (pace.kind === 'on-pace') return 'On pace'
  const step = 5 * 60_000
  return `Runs out ~${clockTime(new Date(Math.round(pace.at.getTime() / step) * step), now)}`
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

const transientErrors = new Set(['rateLimited', 'credentialsChanged'])

export function accountIsStale(account: Account, settings: Settings, now: number, connectionInterrupted = false): boolean {
  if (connectionInterrupted) return true
  if (account.error && !(account.snapshot && transientErrors.has(account.error.kind))) return true
  if (!account.snapshot) return false
  return now - Date.parse(account.snapshot.fetchedAt) > Math.max(settings.idlePollIntervalSeconds * 2, 180) * 1000
    || account.snapshot.windows.some(window => window.resetsAt !== null && Date.parse(window.resetsAt) <= now)
}

export function retryingMessage(account: Account, settings: Settings, now: number, connectionInterrupted = false): string | null {
  const error = account.error
  if (!error || !transientErrors.has(error.kind) || accountIsStale(account, settings, now, connectionInterrupted)) return null
  if (error.kind !== 'rateLimited') return 'Retrying…'
  return error.retryAt && error.retryAt > now ? `Rate limited · retrying in ${shortDuration(error.retryAt - now)}` : 'Rate limited · retrying…'
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
  const source = account.source
  if (source.kind === 'keychain') return 'macOS Keychain'
  return source.keychainSuffix ? `${source.path} · Keychain` : source.path
}

export function signedOutMessage(account: Account, now: number): string | null {
  const error = account.error
  if (error?.kind !== 'signedOut') return null
  const status = error.signedOutSince ? `Signed out ${relativeAge(error.signedOutSince, now)}` : 'Signed out'
  const program = account.provider === 'claude' ? 'claude' : 'codex login'
  const folder = account.source.kind === 'configDir' ? ` in ${account.source.path}` : ''
  return `${status} · run ${program}${folder}`
}

export function resetClock(resetsAt: string | null, now: number): string | null {
  if (!resetsAt) return null
  const time = Date.parse(resetsAt)
  if (!Number.isFinite(time) || time <= now) return null
  return clockTime(new Date(time), now)
}

export function resetGrantsText(grants: ResetGrants | null | undefined, now: number): string | null {
  if (!grants || grants.available <= 0) return null
  const count = `${grants.available} ${grants.available === 1 ? 'reset' : 'resets'} available`
  const milliseconds = grants.expiresAt ? Date.parse(grants.expiresAt) - now : Number.NaN
  if (!(milliseconds > 0)) return count
  const minutes = Math.max(1, Math.floor(milliseconds / 60_000))
  const hours = Math.floor(minutes / 60)
  const expiry = hours >= 24 ? `${Math.floor(hours / 24)}d` : hours ? `${hours}h` : `${minutes}m`
  return `${count} · expires in ${expiry}`
}
