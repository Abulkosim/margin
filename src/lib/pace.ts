import type { HistoryPoint } from './history.ts'
import type { UsageWindow } from './model.ts'

export type Pace = { kind: 'hidden' } | { kind: 'on-pace' } | { kind: 'runs-out'; at: Date; resetsAt: Date }
export type PaceWindow = Pick<UsageWindow, 'id' | 'usedPercent' | 'resetsAt' | 'windowSeconds'>

export const paceWindowIds = new Set(['five_hour', 'seven_day', 'primary_window', 'secondary_window'])
const resetJitter = 5 * 60_000
const hidden: Pace = { kind: 'hidden' }

const resetOf = (value: string | null): number | null => value === null ? null : Date.parse(value)
const sameReset = (a: number | null, b: number | null): boolean => a === null || b === null ? a === b : Math.abs(a - b) <= resetJitter

// Mirrors src-tauri/src/core/forecast.rs so the card and the run-out warning agree.
// The rate is the average over the last fifth of the window (1h of a session, ~34h of a week),
// read from readings in the current period spanning at least a tenth of it. Used % only grows
// within a period, so a gap between two readings still gives the exact average between them.
export function forecastPace(points: HistoryPoint[], current: PaceWindow, at: number): Pace {
  const length = current.windowSeconds === null || current.windowSeconds <= 0 ? null : current.windowSeconds * 1000
  const resetsAt = resetOf(current.resetsAt)
  if (length === null || resetsAt === null || !Number.isFinite(resetsAt) || resetsAt <= at || current.usedPercent >= 100) return hidden
  const readings = points
    .filter(point => point.windowId === current.id && point.windowSeconds === current.windowSeconds && sameReset(resetOf(point.resetsAt), resetsAt))
    .map(point => ({ point, at: Date.parse(point.at) }))
    .filter(entry => Number.isFinite(entry.at) && entry.at <= at)
    .sort((a, b) => a.at - b.at)
  const target = at - Math.trunc(length / 5000) * 1000
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

// What the card shows at `now` for a forecast made from an earlier reading.
export function paceAt(pace: Pace, current: Pick<UsageWindow, 'resetsAt'>, now: number): Pace {
  const resetsAt = resetOf(current.resetsAt)
  if (resetsAt === null || resetsAt <= now) return hidden
  return pace.kind === 'runs-out' && pace.at.getTime() <= now ? hidden : pace
}
