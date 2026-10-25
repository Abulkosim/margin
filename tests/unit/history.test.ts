import assert from 'node:assert/strict'
import test from 'node:test'
import { accountHistorySchema, maxHistoryPoints, historyReadings, historySegments, observedHourChange, historyWindows, type HistoryPoint } from '../../src/lib/history.ts'

const time = Date.parse('2026-10-17T10:00:00Z')
const point = (minutes: number, overrides: Partial<HistoryPoint> = {}): HistoryPoint => ({
  windowId: 'primary_window', at: new Date(time + minutes * 60_000).toISOString(), usedPercent: 20 + minutes / 10,
  resetsAt: '2026-10-17T15:00:00Z', windowSeconds: 18_000, gapBefore: false, ...overrides,
})

test('history boundary rejects malformed, oversized, nonfinite and impossible readings', () => {
  const value = { accountId: 'account', points: [point(0)], error: null }
  assert.equal(accountHistorySchema.safeParse(value).success, true)
  for (const invalid of [point(0, { usedPercent: 101 }), point(0, { usedPercent: Infinity }), point(0, { at: 'yesterday' }), point(0, { windowSeconds: -1 })]) {
    assert.equal(accountHistorySchema.safeParse({ ...value, points: [invalid] }).success, false)
  }
  assert.equal(accountHistorySchema.safeParse({ ...value, points: Array.from({ length: maxHistoryPoints + 1 }, () => point(0)) }).success, false)
})

test('history sorts and deduplicates each window without treating another window’s extra reading as a gap', () => {
  const points = [point(10), point(5, { windowId: 'weekly' }), point(0), point(10)]
  const result = historyReadings(points, 'primary_window')
  assert.equal(result.length, 2)
  assert.equal(result[0]?.remaining, 80)
  assert.equal(result[1]?.boundary, null)
  assert.deepEqual(historySegments(result).map(segment => segment.length), [2])
})

test('failed refreshes and offline periods break segments even if usage did not change', () => {
  const result = historyReadings([point(0), point(5), point(25, { gapBefore: true })], 'primary_window')
  assert.deepEqual(historySegments(result).map(segment => segment.length), [2, 1])
  assert.equal(result[2]?.boundary, 'gap')
})

test('only passed known reset boundaries are labeled resets; decreases and duration changes remain readings', () => {
  const reset = historyReadings([point(0, { resetsAt: '2026-10-17T10:03:00Z' }), point(5, { usedPercent: 0 })], 'primary_window')
  assert.equal(reset[1]?.boundary, 'reset')
  for (const next of [point(5, { usedPercent: 0 }), point(5, { windowSeconds: 7200 }), point(5, { resetsAt: '2026-10-17T16:00:00Z' })]) {
    assert.equal(historyReadings([point(0), next], 'primary_window')[1]?.boundary, 'reading')
  }
})

test('observed hour uses percentage points and rejects stale, short, gapped or changed periods', () => {
  const points = Array.from({ length: 13 }, (_, index) => point(index * 5))
  const readings = historyReadings(points, 'primary_window')
  assert.equal(observedHourChange(readings, time + 60 * 60_000), 6)
  assert.equal(observedHourChange(readings, time + 80 * 60_000), null)
  assert.equal(observedHourChange(readings.slice(1), time + 60 * 60_000), null)
  assert.equal(observedHourChange(historyReadings(points.map((p, i) => i === 5 ? { ...p, gapBefore: true } : p), 'primary_window'), time + 60 * 60_000), null)
  assert.equal(observedHourChange(historyReadings(points.map((p, i) => i === 5 ? { ...p, windowSeconds: 3600 } : p), 'primary_window'), time + 60 * 60_000), null)
})

test('window choices use actual labels and never expose historical provider IDs', () => {
  const windows = historyWindows([{ id: 'primary_window', label: 'Weekly', usedPercent: 0, resetsAt: null, windowSeconds: 604800 }], [point(0), point(0, { windowId: 'spark' })])
  assert.deepEqual(windows, [{ id: 'primary_window', label: 'Weekly' }])
  assert.deepEqual(historyWindows([], [point(0)]), [{ id: 'primary_window', label: 'Previous window' }])
})
