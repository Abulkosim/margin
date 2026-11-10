import type { HistoryPoint } from './history.ts'
import type { UsageWindow } from './model.ts'

export type Pace = { kind: 'hidden' } | { kind: 'on-pace' } | { kind: 'runs-out'; at: Date; resetsAt: Date }
export type PaceWindow = Pick<UsageWindow, 'id' | 'usedPercent' | 'resetsAt' | 'windowSeconds'>

const resetJitter = 5 * 60_000
const hidden: Pace = { kind: 'hidden' }

const resetOf = (value: string | null): number | null => value === null ? null : Date.parse(value)
const sameReset = (a: number | null, b: number | null): boolean => a === null || b === null ? a === b : Math.abs(a - b) <= resetJitter
const inPeriod = (point: HistoryPoint, current: PaceWindow, resetsAt: number): boolean =>
  point.windowId === current.id && point.windowSeconds === current.windowSeconds && sameReset(resetOf(point.resetsAt), resetsAt)

/** The window's length, its reset, and the start of the lookback, or null when no forecast is shown. */
function periodOf(current: PaceWindow, at: number): { length: number; resetsAt: number; target: number } | null {
  const length = current.windowSeconds === null || current.windowSeconds <= 0 ? null : current.windowSeconds * 1000
  const resetsAt = resetOf(current.resetsAt)
  if (length === null || resetsAt === null || !Number.isFinite(resetsAt) || resetsAt <= at || current.usedPercent >= 100) return null
  return { length, resetsAt, target: at - Math.trunc(length / 5000) * 1000 }
}

// Mirrors src-tauri/src/core/forecast.rs so the card and the run-out warning agree.
// The rate is the average over the last fifth of the window (1h of a session, ~34h of a week),
// read from readings in the current period spanning at least a tenth of it. Used % only grows
// within a period, so a gap between two readings still gives the exact average between them.
export function forecastPace(points: HistoryPoint[], current: PaceWindow, at: number): Pace {
  const period = periodOf(current, at)
  if (!period) return hidden
  const { length, resetsAt, target } = period
  const readings = points
    .filter(point => inPeriod(point, current, resetsAt))
    .map(point => ({ point, at: Date.parse(point.at) }))
    .filter(entry => Number.isFinite(entry.at) && entry.at <= at)
    .sort((a, b) => a.at - b.at)
  let index = readings.length - 1
  while (index >= 0 && (readings[index]?.at ?? 0) > target) index--
  let from: number
  let baseline: number
  if (index < 0) {
    const first = readings[0]
    if (!first) return hidden
    from = first.at
    baseline = first.point.usedPercent
  } else {
    const before = readings[index] as (typeof readings)[number]
    const after = readings[index + 1]
    if (after && !after.point.gapBefore && after.at > before.at) {
      from = target
      baseline = before.point.usedPercent + (after.point.usedPercent - before.point.usedPercent) * (target - before.at) / (after.at - before.at)
    } else {
      from = before.at
      baseline = before.point.usedPercent
    }
  }
  const span = Math.trunc((at - from) / 1000)
  const increase = current.usedPercent - baseline
  if (span < Math.trunc(length / 10_000) || increase <= 0) return hidden
  const runsOut = at + Math.ceil((100 - current.usedPercent) * span / increase) * 1000
  const margin = Math.trunc(length / 60_000) * 1000
  return runsOut + margin <= resetsAt ? { kind: 'runs-out', at: new Date(runsOut), resetsAt: new Date(resetsAt) } : { kind: 'on-pace' }
}

/**
 * The forecast reads nothing newer than the first reading in its lookback. A history that already
 * holds a reading there holds every reading the forecast at `at` reads, so it needn't be reloaded.
 */
export function coversForecast(points: HistoryPoint[], current: PaceWindow, at: number): boolean {
  const period = periodOf(current, at)
  if (!period) return true
  return points.some(point => {
    if (!inPeriod(point, current, period.resetsAt)) return false
    const time = Date.parse(point.at)
    return time > period.target && time <= at
  })
}

// What the card shows at `now` for a forecast made from an earlier reading.
export function paceAt(pace: Pace, current: Pick<UsageWindow, 'resetsAt'>, now: number): Pace {
  const resetsAt = resetOf(current.resetsAt)
  if (resetsAt === null || resetsAt <= now) return hidden
  return pace.kind === 'runs-out' && pace.at.getTime() <= now ? hidden : pace
}
