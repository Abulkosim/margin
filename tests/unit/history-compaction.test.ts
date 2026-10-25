import assert from 'node:assert/strict'
import test from 'node:test'
import { historyReadings, observedHourChange, type HistoryPoint } from '../../src/lib/history.ts'
import { computePace, fitPace } from '../../src/lib/pace.ts'

const start = Date.parse('2026-10-17T10:00:00Z')
const minute = 60_000
const resetsAt = '2026-10-17T15:00:00Z'
const at = (minutes: number): number => start + minutes * minute

const point = (minutes: number, usedPercent: number, overrides: Partial<HistoryPoint> = {}): HistoryPoint => ({
  windowId: 'five_hour', at: new Date(at(minutes)).toISOString(), usedPercent, resetsAt, windowSeconds: 18_000, gapBefore: false, ...overrides,
})

const same = (a: HistoryPoint, b: HistoryPoint): boolean =>
  a.windowId === b.windowId && a.usedPercent === b.usedPercent && a.resetsAt === b.resetsAt && a.windowSeconds === b.windowSeconds

// Mirrors the Rust store: an unchanged run keeps only its first and last reading.
function compact(points: HistoryPoint[]): HistoryPoint[] {
  return points.filter((current, index) => {
    const previous = points[index - 1]
    const next = points[index + 1]
    const continues = previous !== undefined && !current.gapBefore && same(previous, current)
    const continued = next !== undefined && !next.gapBefore && same(current, next)
    return !(continues && continued)
  })
}

// One reading a minute for four hours: usage climbs in steps, idles for long stretches, and has one gap.
function dense(): HistoryPoint[] {
  const points: HistoryPoint[] = []
  let used = 10
  for (let minutes = 0; minutes <= 240; minutes++) {
    if (minutes >= 150 && minutes < 160) continue
    if ((minutes > 20 && minutes < 50 && minutes % 3 === 0) || (minutes > 170 && minutes % 7 === 0)) used += 1
    points.push(point(minutes, used, { gapBefore: minutes === 160 }))
  }
  return points
}


test('compaction drops most repeated readings', () => {
  assert.ok(compact(dense()).length < dense().length / 3)
})

test('pace is the same for dense and run-length readings', () => {
  for (const end of [40, 60, 120, 149, 200, 240]) {
    const full = dense().filter(p => Date.parse(p.at) <= at(end))
    const short = compact(full)
    const a = fitPace(full)
    const b = fitPace(short)
    assert.equal(a === null, b === null, `fit at ${end}`)
    if (a && b) assert.ok(Math.abs(a.slope - b.slope) <= Math.abs(a.slope) * 1e-9, `slope at ${end}`)
    const current = { usedPercent: full.at(-1)?.usedPercent ?? 0, resetsAt }
    assert.deepEqual(computePace(short, current, at(end)), computePace(full, current, at(end)))
  }
})

test('a long idle stretch weighs by its duration, not its point count', () => {
  const busy = Array.from({ length: 31 }, (_, index) => point(91 + index, 11 + index))
  const stored = [point(0, 10), point(90, 10), ...busy]
  const polled = [...Array.from({ length: 91 }, (_, index) => point(index, 10)), ...busy]
  const a = fitPace(stored)
  const b = fitPace(polled)
  assert.ok(a && b)
  assert.ok(Math.abs(a.slope - b.slope) <= a.slope * 1e-9)
})

test('observed hour change is the same for dense and run-length readings', () => {
  const recorded = (end: number) => dense().filter(p => Date.parse(p.at) <= at(end))
  const change = (points: HistoryPoint[], end: number) => observedHourChange(historyReadings(points, 'five_hour'), at(end))
  for (const end of [55, 60, 61, 90, 120, 149, 155, 220, 240]) {
    assert.equal(change(compact(recorded(end)), end), change(recorded(end), end), `change at ${end}`)
  }
  assert.equal(change(compact(recorded(60)), 60), 10)
  assert.equal(change(compact(recorded(120)), 120), 0)
  assert.equal(change(compact(recorded(220)), 220), 7)
})

test('a run that spans the hour mark counts as observed, a gap before it does not', () => {
  const run = historyReadings([point(0, 10), point(50, 10), point(70, 14)], 'five_hour')
  assert.equal(observedHourChange(run, at(70)), 4)
  const gapped = historyReadings([point(0, 10), point(50, 10, { gapBefore: true }), point(70, 14)], 'five_hour')
  assert.equal(observedHourChange(gapped, at(70)), null)
})
