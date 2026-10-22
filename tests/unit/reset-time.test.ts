import assert from 'node:assert/strict'
import test from 'node:test'
import { clockTime, resetClock } from '../../src/lib/format.ts'

const now = new Date(2026, 9, 22, 9, 0).getTime()
const at = (days: number, hours: number, minutes = 0): Date => new Date(2026, 9, 22 + days, hours, minutes)
const part = (date: Date, options: Intl.DateTimeFormatOptions) => date.toLocaleString(undefined, options)

test('same day shows only the time', () => {
  const reset = at(0, 16, 30)
  assert.equal(resetClock(reset.toISOString(), now), reset.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' }))
})

test('tomorrow and within 6 days show the weekday', () => {
  for (const days of [1, 5]) {
    const reset = at(days, 16, 30)
    const value = resetClock(reset.toISOString(), now) ?? ''
    assert.equal(value, clockTime(reset, now))
    assert.ok(value.includes(part(reset, { weekday: 'short' })))
    assert.ok(!value.includes(part(reset, { month: 'short' })))
  }
})

test('further out shows the day and month', () => {
  const reset = at(9, 16, 30)
  const value = resetClock(reset.toISOString(), now) ?? ''
  assert.equal(value, clockTime(reset, now))
  assert.ok(value.includes(part(reset, { month: 'short' })))
  assert.match(value, /\b31\b/)
})

test('missing, invalid, or past resets show nothing', () => {
  assert.equal(resetClock(null, now), null)
  assert.equal(resetClock('not a date', now), null)
  assert.equal(resetClock(new Date(now).toISOString(), now), null)
  assert.equal(resetClock(at(-1, 9).toISOString(), now), null)
})
