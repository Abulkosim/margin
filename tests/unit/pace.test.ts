import assert from 'node:assert/strict'
import test from 'node:test'
import { computePace, currentSegment, fitPace } from '../../src/lib/pace.ts'
import { paceText } from '../../src/lib/format.ts'
import type { HistoryPoint } from '../../src/lib/history.ts'

const start = Date.parse('2026-10-17T10:00:00Z')
const resetsAt = '2026-10-17T15:00:00Z'
const minute = 60_000
const point = (minutes: number, usedPercent: number, overrides: Partial<HistoryPoint> = {}): HistoryPoint => ({
  windowId: 'five_hour', at: new Date(start + minutes * minute).toISOString(), usedPercent,
  resetsAt, windowSeconds: 18_000, gapBefore: false, ...overrides,
})
const ramp = (from: number, to: number, perMinute: number, base = 0): HistoryPoint[] =>
  Array.from({ length: Math.floor((to - from) / 5) + 1 }, (_, index) => point(from + index * 5, base + (index * 5) * perMinute))
const at = (minutes: number): number => start + minutes * minute

test('too little data hides the pace', () => {
  assert.deepEqual(computePace([], { usedPercent: 0, resetsAt }, at(0)), { kind: 'hidden' })
  assert.deepEqual(computePace([point(0, 10), point(40, 30)], { usedPercent: 30, resetsAt }, at(40)), { kind: 'hidden' })
  assert.deepEqual(computePace(ramp(0, 25, 1), { usedPercent: 25, resetsAt }, at(25)), { kind: 'hidden' })
  assert.notDeepEqual(computePace(ramp(0, 30, 1), { usedPercent: 30, resetsAt }, at(30)), { kind: 'hidden' })
})

test('missing or passed reset time hides the pace', () => {
  const points = ramp(0, 60, 0.1).map(p => ({ ...p, resetsAt: null }))
  assert.deepEqual(computePace(points, { usedPercent: 6, resetsAt: null }, at(60)), { kind: 'hidden' })
  assert.deepEqual(computePace(ramp(0, 60, 0.1), { usedPercent: 6, resetsAt }, Date.parse(resetsAt) + 1), { kind: 'hidden' })
})

test('flat or falling usage hides the pace', () => {
  assert.deepEqual(computePace(ramp(0, 60, 0, 40), { usedPercent: 40, resetsAt }, at(60)), { kind: 'hidden' })
  const falling = ramp(0, 60, 0, 40).map((p, index) => ({ ...p, usedPercent: 40 - index * 0.1 }))
  assert.deepEqual(computePace(falling, { usedPercent: 38.8, resetsAt }, at(60)), { kind: 'hidden' })
})

test('slow usage is on pace', () => {
  assert.deepEqual(computePace(ramp(0, 60, 0.1), { usedPercent: 6, resetsAt }, at(60)), { kind: 'on-pace' })
})

test('fast usage runs out before reset at the projected time', () => {
  const pace = computePace(ramp(0, 60, 0.5), { usedPercent: 30, resetsAt }, at(60))
  assert.equal(pace.kind, 'runs-out')
  if (pace.kind !== 'runs-out') return
  assert.ok(Math.abs(pace.at.getTime() - at(60 + 140)) < 1000)
  assert.equal(pace.resetsAt.toISOString(), new Date(resetsAt).toISOString())
})

test('a gap starts a new segment so slopes never span it', () => {
  const before = ramp(0, 60, 0, 10)
  const after = ramp(180, 240, 0.1, 50).map((p, index) => index === 0 ? { ...p, gapBefore: true } : p)
  assert.equal(currentSegment([...before, ...after]).length, after.length)
  assert.deepEqual(computePace([...before, ...after], { usedPercent: 56, resetsAt }, at(240)), { kind: 'on-pace' })
  const shortAfter = ramp(180, 200, 1, 50).map((p, index) => index === 0 ? { ...p, gapBefore: true } : p)
  assert.deepEqual(computePace([...ramp(0, 60, 0.5), ...shortAfter], { usedPercent: 70, resetsAt }, at(200)), { kind: 'hidden' })
})

test('a reset mid-history only counts the current period', () => {
  const nextReset = '2026-10-17T20:00:00Z'
  const old = ramp(0, 60, 1, 20)
  const fresh = ramp(65, 125, 0.05).map(p => ({ ...p, resetsAt: nextReset }))
  assert.equal(currentSegment([...old, ...fresh]).length, fresh.length)
  assert.deepEqual(computePace([...old, ...fresh], { usedPercent: 3, resetsAt: nextReset }, at(125)), { kind: 'on-pace' })
  const dropOnly = [...ramp(0, 60, 1, 20), ...ramp(65, 125, 0.05)]
  assert.equal(currentSegment(dropOnly).length, 13)
  assert.deepEqual(computePace(old, { usedPercent: 3, resetsAt: nextReset }, at(125)), { kind: 'hidden' })
})

test('out-of-order and duplicate points give the same result as sorted ones', () => {
  const sorted = ramp(0, 60, 0.5)
  const shuffled = [...sorted].reverse().concat(sorted[3] as HistoryPoint)
  assert.deepEqual(fitPace(shuffled), fitPace(sorted))
})

test('rounding jitter and reset-time jitter do not split the period or flip the result', () => {
  const noisy = ramp(0, 120, 0.1).map((p, index) => ({
    ...p,
    usedPercent: Math.max(0, Math.round(p.usedPercent + (index % 2 ? 0.8 : -0.8))),
    resetsAt: new Date(Date.parse(resetsAt) + (index % 3 - 1) * 30_000).toISOString(),
  }))
  assert.equal(currentSegment(noisy).length, noisy.length)
  assert.deepEqual(computePace(noisy, { usedPercent: 12, resetsAt }, at(120)), { kind: 'on-pace' })
  const fast = ramp(0, 60, 0.5).map((p, index) => ({ ...p, usedPercent: Math.round(p.usedPercent + (index % 2 ? 1 : -1) * 0.9) }))
  assert.equal(computePace(fast, { usedPercent: 30, resetsAt }, at(60)).kind, 'runs-out')
})

test('pace text', () => {
  assert.equal(paceText({ kind: 'hidden' }, at(0)), null)
  assert.equal(paceText({ kind: 'on-pace' }, at(0)), 'On pace')
  const now = new Date(2026, 9, 21, 9, 0).getTime()
  const sameDay = paceText({ kind: 'runs-out', at: new Date(2026, 9, 21, 13, 58), resetsAt: new Date(2026, 9, 21, 15, 0) }, now)
  assert.equal(sameDay, `Runs out ~${new Date(2026, 9, 21, 14, 0).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })}`)
  const later = paceText({ kind: 'runs-out', at: new Date(2026, 9, 22, 14, 0), resetsAt: new Date(2026, 9, 23, 9, 0) }, now)
  assert.equal(later, `Runs out ~${new Date(2026, 9, 22, 14, 0).toLocaleString(undefined, { weekday: 'short', hour: 'numeric', minute: '2-digit' })}`)
})
