import assert from 'node:assert/strict'
import test from 'node:test'
import { createDemoState } from '../../src/lib/demo.ts'
import { appStateSchema } from '../../src/lib/model.ts'
import { notificationFailureText, notificationHelp, testSentText } from '../../src/lib/notifications.ts'

test('state from older builds has no notification failure', () => {
  const state = appStateSchema.parse({ ...createDemoState(null), notificationFailedAt: undefined })
  assert.equal(state.notificationFailedAt, null)
})

test('the failure is a time, never error text', () => {
  const failedAt = new Date().toISOString()
  assert.equal(appStateSchema.parse({ ...createDemoState(null), notificationFailedAt: failedAt }).notificationFailedAt, failedAt)
  assert.equal(appStateSchema.safeParse({ ...createDemoState(null), notificationFailedAt: 'C:\\Users\\alex\\margin.exe not found' }).success, false)
})

test('the preview can show a failed notification', () => {
  assert.equal(createDemoState(null).notificationFailedAt, null)
  assert.notEqual(createDemoState('notification-failed').notificationFailedAt, null)
})

test('help names each system’s settings and what Margin cannot see', () => {
  const mac = notificationHelp('mac')
  assert.match(mac, /System Settings > Notifications/)
  assert.match(mac, /Focus/)
  const windows = notificationHelp('windows')
  assert.match(windows, /Settings > System > Notifications/)
  assert.match(windows, /Do not disturb/)
  for (const help of [mac, windows]) assert.match(help, /not that it appeared/)
})

test('status lines say sent, not shown', () => {
  const now = Date.now()
  assert.match(testSentText(now, now), /^Sent at .+\. Margin can’t tell if it appeared\.$/)
  assert.match(notificationFailureText(new Date(now).toISOString(), now), /^Couldn’t send the last notification at .+\. Margin retries failed alerts a few times\.$/)
})
