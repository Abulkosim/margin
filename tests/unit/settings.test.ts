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

test('daily update checks default on and older settings parse with them on', () => {
  assert.equal(defaultSettings.autoCheckUpdates, true)
  const { autoCheckUpdates: _, ...legacy } = defaultSettings
  assert.equal(settingsSchema.parse(legacy).autoCheckUpdates, true)
  assert.equal(settingsSchema.parse({ ...defaultSettings, autoCheckUpdates: false }).autoCheckUpdates, false)
  assert.equal(settingsSchema.safeParse({ ...defaultSettings, autoCheckUpdates: 'daily' }).success, false)
})

test('new installs follow the lowest account and order by urgency', () => {
  assert.equal(defaultSettings.trayMode, 'lowest')
  assert.equal(defaultSettings.pinnedAccountId, null)
  assert.equal(defaultSettings.accountOrder, 'urgency')
  assert.equal(settingsSchema.parse({ ...defaultSettings, accountOrder: 'added' }).accountOrder, 'added')
})

test('fast usage alerts default off at 20% and accept only 10, 20, or 30', () => {
  assert.equal(defaultSettings.burnAlerts, false)
  assert.equal(defaultSettings.burnThreshold, 20)
  const { burnAlerts: _, burnThreshold: __, ...legacy } = defaultSettings
  const parsed = settingsSchema.parse(legacy)
  assert.equal(parsed.burnAlerts, false)
  assert.equal(parsed.burnThreshold, 20)
  assert.equal(settingsSchema.parse({ ...defaultSettings, burnAlerts: true, burnThreshold: 30 }).burnThreshold, 30)
  for (const burnThreshold of [0, 15, 25, '20']) {
    assert.equal(settingsSchema.safeParse({ ...defaultSettings, burnThreshold }).success, false)
  }
})
