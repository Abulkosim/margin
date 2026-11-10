import assert from 'node:assert/strict'
import test from 'node:test'
import { coversForecast, forecastPace, paceAt, type Pace, type PaceWindow } from '../../src/lib/pace.ts'
import { paceText } from '../../src/lib/format.ts'
import type { HistoryPoint } from '../../src/lib/history.ts'

// Same cases as src-tauri/src/core/forecast.rs, so the card and the run-out warning agree.
const start = Date.parse('2099-12-31T12:00:00Z')
const resetsAt = '2099-12-31T14:00:00Z'
const minute = 60_000
const at = (minutes: number): number => start + minutes * minute
const point = (minutes: number, usedPercent: number, overrides: Partial<HistoryPoint> = {}): HistoryPoint => ({
  windowId: 'five_hour', at: new Date(at(minutes)).toISOString(), usedPercent,
  resetsAt, windowSeconds: 18_000, gapBefore: false, ...overrides,
})
const ramp = (from: number, to: number, base: number, perMinute: number): HistoryPoint[] =>
  Array.from({ length: Math.floor((to - from) / 5) + 1 }, (_, index) => point(from + index * 5, base + index * 5 * perMinute))
const session = (usedPercent: number): PaceWindow => ({ id: 'five_hour', usedPercent, resetsAt, windowSeconds: 18_000 })
const runsOut = (points: HistoryPoint[], used: number): number | Pace['kind'] => {
  const pace = forecastPace(points, session(used), at(0))
  return pace.kind === 'runs-out' ? pace.at.getTime() : pace.kind
}

test('projects from the last hour of a session', () => {
  assert.equal(runsOut(ramp(-90, 0, 0, 0.5), 45), at(110))
  const pace = forecastPace(ramp(-90, 0, 0, 0.5), session(45), at(0))
  assert.equal(pace.kind === 'runs-out' && pace.resetsAt.toISOString(), new Date(resetsAt).toISOString())
})

test('needs readings spanning half the look-back', () => {
  assert.equal(runsOut(ramp(-25, 0, 50, 1), 75), 'hidden')
  assert.equal(runsOut(ramp(-30, 0, 50, 1), 80), at(20))
  assert.equal(runsOut([point(-40, 10), point(0, 50)], 50), at(50))
})

test('slow usage is on pace; flat, falling, full or unknown usage is hidden', () => {
  assert.equal(runsOut(ramp(-60, 0, 0, 0.1), 6), 'on-pace')
  assert.equal(runsOut(ramp(-60, 0, 40, 0), 40), 'hidden')
  assert.equal(runsOut([point(-60, 41), point(0, 40)], 40), 'hidden')
  assert.equal(runsOut(ramp(-60, 0, 0, 1), 100), 'hidden')
  assert.equal(forecastPace(ramp(-60, 0, 0, 1), { ...session(60), resetsAt: null }, at(0)).kind, 'hidden')
  assert.equal(forecastPace(ramp(-60, 0, 0, 1), { ...session(60), windowSeconds: null }, at(0)).kind, 'hidden')
})

test('a run-out just before the reset counts as on pace', () => {
  assert.equal(runsOut([point(-60, 0), point(0, 40)], 40), at(90))
  assert.equal(runsOut([point(-60, 0), point(0, 34)], 34), 'on-pace')
})

test('interpolates between readings but not across a gap', () => {
  assert.equal(runsOut([point(-180, 10), point(-30, 10), point(0, 50)], 50), at(75))
  assert.equal(runsOut([point(-180, 10), point(0, 50, { gapBefore: true })], 50), 'on-pace')
  assert.equal(runsOut([point(-100, 10), point(0, 60, { gapBefore: true })], 60), at(80))
})

test('earlier periods and other windows never count', () => {
  const old = ramp(-120, -65, 0, 1).map(p => ({ ...p, resetsAt: '2099-12-31T09:00:00Z' }))
  const weekly = ramp(-120, 0, 0, 1).map(p => ({ ...p, windowId: 'seven_day' }))
  const points = [...old, ...weekly, ...ramp(-30, 0, 0, 1)]
  assert.equal(runsOut(points, 30), at(70))
  assert.equal(runsOut(old, 30), 'hidden')
})

test('reset jitter is the same period and order does not matter', () => {
  const points = ramp(-60, 0, 0, 0.8).map((p, index) => ({ ...p, resetsAt: new Date(Date.parse(resetsAt) + (index % 3 - 1) * 30_000).toISOString() }))
  assert.equal(runsOut(points.reverse(), 48), at(65))
})

test('a week reads the last day and a half across gaps', () => {
  const weekReset = '2100-01-03T12:00:00Z'
  const reading = (minutes: number, usedPercent: number, gapBefore = false): HistoryPoint =>
    point(minutes, usedPercent, { windowId: 'seven_day', resetsAt: weekReset, windowSeconds: 604_800, gapBefore })
  const window: PaceWindow = { id: 'seven_day', usedPercent: 70, resetsAt: weekReset, windowSeconds: 604_800 }
  assert.equal(forecastPace([reading(-16 * 60, 50), reading(0, 70)], window, at(0)).kind, 'hidden')
  const pace = forecastPace([reading(-2592, 10), reading(-1440, 40), reading(-240, 50, true), reading(0, 70)], window, at(0))
  assert.equal(pace.kind === 'runs-out' && pace.at.getTime(), at(0) + 80_640_000)
})

test('the card hides a forecast once its reset or run-out time passes', () => {
  const pace = forecastPace(ramp(-60, 0, 0, 1), session(60), at(0))
  assert.equal(pace.kind, 'runs-out')
  assert.equal(paceAt(pace, session(60), at(30)), pace)
  assert.equal(paceAt(pace, session(60), at(40)).kind, 'hidden')
  assert.equal(paceAt({ kind: 'on-pace' }, session(6), at(119)).kind, 'on-pace')
  assert.equal(paceAt({ kind: 'on-pace' }, session(6), at(120)).kind, 'hidden')
  assert.equal(paceAt({ kind: 'on-pace' }, { resetsAt: null }, at(0)).kind, 'hidden')
})

test('a history that covers the look-back forecasts like the full history', () => {
  const previous = '2099-12-31T09:00:00Z'
  const readings: HistoryPoint[] = []
  let used = 0
  for (let minutes = -330; minutes <= -185; minutes += 5) readings.push(point(minutes, 40 + (minutes + 330) / 5, { resetsAt: previous }))
  for (let minutes = -178, step = 0; minutes < 118; step++) {
    readings.push(point(minutes, used, { gapBefore: minutes > -40 && minutes < -30 }))
    const wait = minutes > -100 && minutes < -70 ? 26 : 3 + (step * 5) % 6
    used = Math.min(99, used + wait * [0.2, 0.5, 0.7, 0.3][step % 4]!)
    minutes += wait
  }
  let covered = 0
  let uncovered = 0
  let shown = 0
  for (const reading of readings.filter(entry => entry.resetsAt === resetsAt)) {
    const now = Date.parse(reading.at)
    const full = forecastPace(readings, session(reading.usedPercent), now)
    for (const loadedAt of readings.map(entry => Date.parse(entry.at)).filter(time => time <= now)) {
      const loaded = readings.filter(entry => Date.parse(entry.at) <= loadedAt)
      if (!coversForecast(loaded, session(reading.usedPercent), now)) {
        uncovered++
        continue
      }
      covered++
      if (full.kind === 'runs-out') shown++
      assert.deepEqual(forecastPace(loaded, session(reading.usedPercent), now), full, `at ${reading.at}, loaded ${new Date(loadedAt).toISOString()}`)
    }
  }
  assert.ok(covered > 400 && uncovered > 400 && shown > 200, `${covered} covered (${shown} shown), ${uncovered} not`)
  assert.equal(coversForecast([], session(50), at(0)), false)
  assert.equal(coversForecast([], session(100), at(0)), true)
  assert.equal(coversForecast([], { ...session(50), resetsAt: null }, at(0)), true)
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
