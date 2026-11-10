import { clockTime } from './format.ts'
import type { Platform } from './platform.ts'

export function notificationHelp(platform: Platform): string {
  const where = platform === 'mac' ?'System Settings > Notifications, and check Focus' : 'Settings > System > Notifications, and check Do not disturb'
  return `If nothing appears, allow Margin in ${where}. Margin can tell it sent a notification, not that it appeared.`
}

export function testSentText(sentAt: number, now: number): string {
  return `Sent at ${clockTime(new Date(sentAt), now)}. Margin can’t tell if it appeared.`
}

export function notificationFailureText(failedAt: string, now: number): string {
  return `Couldn’t send the last notification at ${clockTime(new Date(failedAt), now)}. Margin retries failed alerts a few times.`
}
