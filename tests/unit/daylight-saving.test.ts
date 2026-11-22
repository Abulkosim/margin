import assert from 'node:assert/strict'
import test from 'node:test'
import { clockTime, resetDay } from '../../src/lib/format.ts'

process.env.TZ = 'America/New_York'

const weekdayTime = (date: Date): string => date.toLocaleString(undefined, { weekday: 'short', hour: 'numeric', minute: '2-digit' })

test('next week’s same weekday never reads as today when the clocks change in between', () => {
  const now = new Date(2026, 2, 3, 23, 30).getTime()
  const nextTuesday = new Date(now + 6 * 86_400_000 - 60_000)
  assert.equal(nextTuesday.getDay(), new Date(now).getDay())
  const date = nextTuesday.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
  assert.ok(clockTime(nextTuesday, now).includes(date))
  assert.equal(resetDay(nextTuesday.toISOString(), now), date)
})

test('the weekday is used for every day of the coming week, whatever the hour', () => {
  const now = new Date(2026, 9, 26, 9, 0).getTime()
  const sunday = new Date(2026, 10, 1, 23, 0)
  assert.equal(resetDay(sunday.toISOString(), now), sunday.toLocaleDateString(undefined, { weekday: 'short' }))
  assert.equal(clockTime(sunday, now), weekdayTime(sunday))
  const lastTuesday = new Date(2026, 9, 20, 8, 0)
  assert.equal(clockTime(lastTuesday, now), weekdayTime(lastTuesday))
})
