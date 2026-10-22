import assert from 'node:assert/strict'
import test from 'node:test'
import { defaultSettings, settingsSchema } from '../../src/lib/model.ts'

test('notifications default on with reset alerts off and permission unasked', () => {
  assert.equal(defaultSettings.notifications, true)
  assert.deepEqual(defaultSettings.thresholds, [80, 95, 100])
  assert.equal(defaultSettings.notifyOnReset, false)
  assert.equal(defaultSettings.notificationPermission, 'unasked')
})

test('settings saved before notification v2 parse with defaults', () => {
  const { notifyOnReset: _, notificationPermission: __, ...legacy } = defaultSettings
  const parsed = settingsSchema.parse({ ...legacy, notifications: false })
  assert.equal(parsed.notifications, false)
  assert.equal(parsed.notifyOnReset, false)
  assert.equal(parsed.notificationPermission, 'unasked')
})

test('notification fields reject unknown values', () => {
  assert.equal(settingsSchema.safeParse({ ...defaultSettings, notifyOnReset: 'yes' }).success, false)
  assert.equal(settingsSchema.safeParse({ ...defaultSettings, notificationPermission: 'maybe' }).success, false)
  assert.equal(settingsSchema.parse({ ...defaultSettings, notificationPermission: 'denied' }).notificationPermission, 'denied')
  assert.equal(settingsSchema.safeParse({ ...defaultSettings, thresholds: [50, 80, 90, 95, 100] }).success, true)
})
