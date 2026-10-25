import type { HistoryPoint } from './history.ts'
import type { UsageWindow } from './model.ts'

export type Pace = { kind: 'hidden' } | { kind: 'on-pace' } | { kind: 'runs-out'; at: Date; resetsAt: Date }
export type PaceFit = { slope: number; resetsAt: number }

export const paceWindowIds = new Set(['five_hour', 'seven_day', 'primary_window', 'secondary_window'])
export const minPacePoints = 3
export const minPaceSpan = 30 * 60_000
const usedJitter = 2
const resetJitter = 5 * 60_000

const resetOf = (value: string | null): number | null => value === null ? null : Date.parse(value)
const sameReset = (a: number | null, b: number | null): boolean => a === null || b === null ? a === b : Math.abs(a - b) <= resetJitter

export function currentSegment(points: HistoryPoint[]): HistoryPoint[] {
  const sorted = points
    .map(point => ({ point, at: Date.parse(point.at) }))
    .filter(entry => Number.isFinite(entry.at))
    .sort((a, b) => a.at - b.at)
  let segment: HistoryPoint[] = []
  let previous: HistoryPoint | undefined
  for (const { point } of sorted) {
    if (previous && point.at === previous.at) continue
    const boundary = !previous || point.gapBefore
      || point.usedPercent < previous.usedPercent - usedJitter
      || point.windowSeconds !== previous.windowSeconds
      || !sameReset(resetOf(point.resetsAt), resetOf(previous.resetsAt))
    if (boundary) segment = []
    segment.push(point)
    previous = point
  }
  return segment
}

// Least squares over the line drawn through the readings, so each stretch counts by its duration.
// A flat run stored as two points weighs the same as the same run polled every minute.
export function fitPace(points: HistoryPoint[]): PaceFit | null {
  const segment = currentSegment(points)
  const last = segment.at(-1)
  const resetsAt = last ? resetOf(last.resetsAt) : null
  if (!last || resetsAt === null || segment.length < minPacePoints) return null
  const origin = Date.parse(segment[0]?.at ?? last.at)
  const xs = segment.map(point => (Date.parse(point.at) - origin) / 60_000)
  if ((xs.at(-1) ?? 0) * 60_000 < minPaceSpan) return null
  let span = 0
  let sumX = 0
  let sumY = 0
  let sumXX = 0
  let sumXY = 0
  for (let index = 1; index < segment.length; index++) {
    const x1 = xs[index - 1] ?? 0
    const x2 = xs[index] ?? 0
    const y1 = segment[index - 1]?.usedPercent ?? 0
    const y2 = segment[index]?.usedPercent ?? 0
    const width = x2 - x1
    span += width
    sumX += width * (x1 + x2) / 2
    sumY += width * (y1 + y2) / 2
    sumXX += width * (x1 * x1 + x1 * x2 + x2 * x2) / 3
    sumXY += width * (2 * x1 * y1 + x1 * y2 + x2 * y1 + 2 * x2 * y2) / 6
  }
  const meanX = sumX / span
  const variance = sumXX / span - meanX * meanX
  const covariance = sumXY / span - meanX * sumY / span
  const slope = variance > 0 ? covariance / variance / 60_000 : 0
  return slope > 0 ? { slope, resetsAt } : null
}

export function projectPace(fit: PaceFit | null, current: Pick<UsageWindow, 'usedPercent' | 'resetsAt'>, now: number): Pace {
  const resetsAt = resetOf(current.resetsAt)
  if (!fit || resetsAt === null || resetsAt <= now || current.usedPercent >= 100 || !sameReset(fit.resetsAt, resetsAt)) return { kind: 'hidden' }
  const runsOut = now + (100 - current.usedPercent) / fit.slope
  return runsOut < resetsAt ? { kind: 'runs-out', at: new Date(runsOut), resetsAt: new Date(resetsAt) } : { kind: 'on-pace' }
}

export function computePace(points: HistoryPoint[], current: Pick<UsageWindow, 'usedPercent' | 'resetsAt'>, now: number): Pace {
  return projectPace(fitPace(points), current, now)
}
