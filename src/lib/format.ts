import type { Account, ResetGrants, Settings, UsageWindow, WindowScope } from './model'
import type { Pace } from './pace'

export function remainingPercent(used: number): number {
  return Math.max(0, Math.min(100, 100 - used))
}

/** Class names for a window with 20% or less remaining, and for one with none. */
export function quotaTone(usedPercent: number): { 'quota-low': boolean; 'quota-empty': boolean } {
  return { 'quota-low': usedPercent >= 80, 'quota-empty': usedPercent >= 100 }
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
  if (modelWindowIds.has(window.id) || window.id.startsWith('weekly_scoped_')) return 'model'
  return window.id.startsWith('extra_') && window.id.endsWith('_secondary_window') ? 'model' : 'other'
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

export function modelWindowSummary(window: UsageWindow, now: number, fetchedAt?: string): string {
  const reset = fetchedAt && unreadSinceReset(window, fetchedAt, now) ? resetPassedText(window, now) : resetCountdown(window.resetsAt, now)
  return `${window.label} weekly · ${reset.charAt(0).toLowerCase()}${reset.slice(1)}`
}

export function resetCountdown(resetsAt: string | null, now: number): string {
  if (!resetsAt) return 'Reset time unavailable'
  const milliseconds = Date.parse(resetsAt) - now
  if (milliseconds <= 0) return 'Reset pending'
  return `Resets in ${shortDuration(milliseconds)}`
}

/** The reset passed with no reading since, so the percentage describes the previous period. */
export function unreadSinceReset(window: UsageWindow, fetchedAt: string, now: number): boolean {
  const reset = window.resetsAt ? Date.parse(window.resetsAt) : Number.NaN
  return reset <= now && !(Date.parse(fetchedAt) >= reset)
}

export function resetPassedText(window: UsageWindow, now: number): string {
  return `Reset ${clockTime(new Date(window.resetsAt ?? now), now)}, not read since`
}

export function timeOfDay(time: Date): string {
  return time.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
}

export function fullDateTime(timestamp: string | null): string {
  const time = timestamp ? Date.parse(timestamp) : Number.NaN
  return Number.isFinite(time) ? new Date(time).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }) : ''
}

export function clockTime(time: Date, now: number): string {
  const today = new Date(now)
  const sameDay = time.getFullYear() === today.getFullYear() && time.getMonth() === today.getMonth() && time.getDate() === today.getDate()
  if (sameDay) return timeOfDay(time)
  const withinWeek = Math.abs(time.getTime() - now) < 6 * 86_400_000
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
  const { windows } = account.snapshot
  const core = windows.filter(window => windowScope(window) === 'core')
  return now - Date.parse(account.snapshot.fetchedAt) > Math.max(settings.idlePollIntervalSeconds * 2, 180) * 1000
    || (core.length ? core : windows).some(window => window.resetsAt !== null && Date.parse(window.resetsAt) <= now)
}

export function retryingMessage(account: Account, settings: Settings, now: number, connectionInterrupted = false): string | null {
  const error = account.error
  if (!error || !transientErrors.has(error.kind) || accountIsStale(account, settings, now, connectionInterrupted)) return null
  if (error.kind !== 'rateLimited') return 'Retrying…'
  return error.retryAt && error.retryAt > now ? `Rate limited · retrying in ${shortDuration(error.retryAt - now)}` : 'Rate limited · retrying…'
}

export function formatMoney(amount: number, currency: string): string {
  const fraction = Number.isInteger(amount) ? 0 : 2
  try {
    return new Intl.NumberFormat(undefined, { style: 'currency', currency, minimumFractionDigits: fraction, maximumFractionDigits: 2 }).format(amount)
  } catch {
    return `${amount.toFixed(fraction)} ${currency}`
  }
}

function formatCount(amount: number): string {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 }).format(amount)
}

export type BalanceLine = { label: string; value: string; warn: boolean }

/** Money for Claude extra usage; credits (a unit, never money) for Codex. */
export function balanceLines(snapshot: Account['snapshot'], now: number): BalanceLine[] {
  if (!snapshot) return []
  const lines: BalanceLine[] = []
  const extra = snapshot.extraUsage
  if (extra) {
    const cap = extra.limit !== null ? ` of ${formatMoney(extra.limit, extra.currency)}` : ''
    lines.push({ label: 'Extra usage', value: `${formatMoney(extra.spent, extra.currency)}${cap} this month`, warn: false })
  }
  const credits = snapshot.balance?.credits
  if (credits) {
    const value = credits.unlimited ? 'unlimited' : credits.amount !== null ? `${formatCount(credits.amount)} left` : 'available'
    lines.push({ label: 'Credits', value, warn: false })
  }
  const limit = snapshot.balance?.spendLimit
  if (limit) {
    const day = resetDay(limit.resetsAt, now)
    const reset = day ? ` · resets ${day}` : ''
    const value = limit.reached ? `reached${reset}` : `${formatCount(limit.used)} of ${formatCount(limit.limit)} used${reset}`
    lines.push({ label: spendLimitLabel(limit.unit), value, warn: limit.reached })
  }
  return lines
}

/** The API names the unit; only a literal `credit` earns the word, anything else is shown as sent. */
export function spendLimitLabel(unit: string | null): string {
  if (unit === 'credit') return 'Monthly credit limit'
  const word = unit?.trim().toLowerCase()
  return word ? `Monthly ${word} limit` : 'Monthly limit'
}

export function planName(plan: string | null): string | null {
  return plan ? plan.replaceAll('_', ' ').replace(/^./, letter => letter.toUpperCase()) : null
}

export function providerName(provider: Account['provider']): string {
  return provider === 'claude' ? 'Claude' : 'Codex'
}

export function accountSource(account: Account): string {
  const source = account.source
  if (source.kind === 'keychain') return 'macOS Keychain'
  return source.keychainSuffix ? `${source.path} · Keychain` : source.path
}

/** The token expired on disk. The CLI refreshes it on its next run, so nothing is wrong with the sign-in. */
export function tokenExpired(account: Account): boolean {
  return account.error?.kind === 'signedOut' && Boolean(account.error.signedOutSince)
}

export function idleStatus(account: Account, now: number): string | null {
  if (!tokenExpired(account)) return null
  return account.snapshot ? `Idle, last read ${relativeAge(account.snapshot.fetchedAt, now)}` : 'Idle, no reading yet'
}

/** The compact row's column is too narrow for the full status. */
export function idleStatusShort(account: Account, now: number): string | null {
  if (!tokenExpired(account)) return null
  return account.snapshot ? `Idle, read ${relativeAge(account.snapshot.fetchedAt, now)}` : 'Idle, no reading yet'
}

export function idleAction(account: Account): string | null {
  if (!tokenExpired(account)) return null
  if (!account.error?.signInCommand) return `Run the ${providerName(account.provider)} CLI to get a new reading.`
  return account.snapshot ? 'To get a new reading, run:' : 'To get a first reading, run:'
}

export function signedOutMessage(account: Account): string | null {
  const error = account.error
  if (error?.kind !== 'signedOut' || tokenExpired(account)) return null
  return error.signInCommand ? 'Signed out. Run:' : `Signed out. Sign in with the ${providerName(account.provider)} CLI, then refresh.`
}

export function orderAccounts(accounts: Account[], settings: Settings, now: number, connectionInterrupted = false): Account[] {
  if (settings.accountOrder === 'added') return accounts
  const pinned = settings.trayMode === 'pinned' ? settings.pinnedAccountId : null
  const rank = (account: Account): [number, number] => {
    if (account.id === pinned) return [0, 0]
    const { session, weekly } = coreSummary(account.snapshot?.windows ?? [])
    const core = [session, weekly].filter(window => window !== null)
    if (!core.length || accountIsStale(account, settings, now, connectionInterrupted)) return [2, 0]
    return [1, Math.min(...core.map(window => remainingPercent(window.usedPercent)))]
  }
  const ranks = new Map(accounts.map(account => [account.id, rank(account)]))
  return [...accounts].sort((a, b) => {
    const [groupA, valueA] = ranks.get(a.id) ?? [2, 0]
    const [groupB, valueB] = ranks.get(b.id) ?? [2, 0]
    return groupA - groupB || valueA - valueB
  })
}

export function resetClock(resetsAt: string | null, now: number): string | null {
  if (!resetsAt) return null
  const time = Date.parse(resetsAt)
  if (!Number.isFinite(time) || time <= now) return null
  return clockTime(new Date(time), now)
}

/** Like resetClock, but a far-off reset shows only its day. */
export function resetDay(resetsAt: string | null, now: number): string | null {
  if (!resetsAt) return null
  const time = Date.parse(resetsAt)
  if (!Number.isFinite(time) || time <= now) return null
  const date = new Date(time)
  const today = new Date(now)
  if (date.getFullYear() === today.getFullYear() && date.getMonth() === today.getMonth() && date.getDate() === today.getDate()) return clockTime(date, now)
  const withinWeek = time - now < 6 * 86_400_000
  return date.toLocaleDateString(undefined, withinWeek ? { weekday: 'short' } : { month: 'short', day: 'numeric' })
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
