import { z } from 'zod'
import { accountIdSchema, type Account, type UsageWindow } from './model.ts'

export const historyPointSchema = z.object({
  windowId: z.string().min(1),
  at: z.string().datetime({ offset: true }),
  usedPercent: z.number().min(0).max(100),
  resetsAt: z.string().datetime({ offset: true }).nullable(),
  windowSeconds: z.number().int().nonnegative().nullable(),
  gapBefore: z.boolean(),
})
export const accountHistorySchema = z.object({
  accountId: accountIdSchema,
  points: z.array(historyPointSchema).max(10_000),
  error: z.string().nullable(),
})
export type HistoryPoint = z.infer<typeof historyPointSchema>
export type AccountHistory = z.infer<typeof accountHistorySchema>
export type HistoryReading = HistoryPoint & { timestamp: number; remaining: number; boundary: 'gap' | 'reset' | 'reading' | null }

export function historyReadings(points: HistoryPoint[], windowId: string): HistoryReading[] {
  const sorted = points.filter(point => point.windowId === windowId).sort((a, b) => Date.parse(a.at) - Date.parse(b.at))
  const readings: HistoryReading[] = []
  for (const point of sorted) {
    const timestamp = Date.parse(point.at)
    const previous = readings.at(-1)
    if (previous?.timestamp === timestamp) continue
    let boundary: HistoryReading['boundary'] = null
    if (previous) {
      const changedPeriod = point.resetsAt !== previous.resetsAt || point.windowSeconds !== previous.windowSeconds
      const reset = changedPeriod && previous.resetsAt !== null && point.resetsAt !== null
        && Date.parse(previous.resetsAt) <= timestamp && Date.parse(point.resetsAt) > Date.parse(previous.resetsAt)
      if (point.gapBefore) boundary = 'gap'
      else if (reset) boundary = 'reset'
      else if (changedPeriod || point.usedPercent < previous.usedPercent) boundary = 'reading'
    }
    readings.push({ ...point, timestamp, remaining: 100 - point.usedPercent, boundary })
  }
  return readings
}

export function historySegments(readings: HistoryReading[]): HistoryReading[][] {
  const segments: HistoryReading[][] = []
  for (const reading of readings) {
    if (!segments.length || reading.boundary) segments.push([])
    segments.at(-1)?.push(reading)
  }
  return segments
}

export function observedHourChange(readings: HistoryReading[], now: number): number | null {
  const latest = readings.at(-1)
  if (!latest || now - latest.timestamp > 10 * 60_000 || latest.timestamp > now) return null
  const hourAgo = latest.timestamp - 3_600_000
  let startIndex = -1
  readings.forEach((point, index) => { if (point.timestamp <= hourAgo) startIndex = index })
  const start = readings[startIndex]
  if (!start || hourAgo - start.timestamp > 5 * 60_000) return null
  if (readings.slice(startIndex + 1).some(point => point.boundary)) return null
  return Math.round((latest.usedPercent - start.usedPercent) * 10) / 10
}

export function createDemoHistory(account: Account): AccountHistory {
  const points: HistoryPoint[] = []
  const snapshot = account.snapshot
  if (snapshot) for (const window of snapshot.windows) {
    const end = Date.parse(snapshot.fetchedAt)
    const short = window.windowSeconds !== null && window.windowSeconds < 86_400
    const steps = 36
    const spacing = 5 * 60_000
    for (let index = 0; index <= steps; index++) {
      if (index >= 12 && index < 19) continue
      const olderPeriod = short && index < 9
      points.push({
        windowId: window.id,
        at: new Date(end - (steps - index) * spacing).toISOString(),
        usedPercent: olderPeriod ? 60 + index * 3 : short ? window.usedPercent * Math.max(0, index - 9) / (steps - 9) : Math.max(0, window.usedPercent - 1.5 * (steps - index) / steps),
        resetsAt: olderPeriod ? new Date(end - 27 * spacing).toISOString() : window.resetsAt,
        windowSeconds: window.windowSeconds,
        gapBefore: index === 19,
      })
    }
  }
  return { accountId: account.id, points, error: null }
}

export function historyWindows(current: UsageWindow[], points: HistoryPoint[]): Pick<UsageWindow, 'id' | 'label'>[] {
  if (current.length) return current.map(({ id, label }) => ({ id, label }))
  const ids = [...new Set(points.map(point => point.windowId))]
  return ids.map((id, index) => ({ id, label: ids.length === 1 ? 'Previous window' : `Previous window ${index + 1}` }))
}
