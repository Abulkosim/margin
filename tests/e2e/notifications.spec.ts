import { mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { expect, test, type Page, type TestInfo } from '@playwright/test'
import { createDemoState } from '../../src/lib/demo.ts'
import type { AppState } from '../../src/lib/model.ts'

async function openSettings(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Open settings', exact: true }).click()
  await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible()
}

const send = (page: Page) => page.getByRole('button', { name: 'Send test notification', exact: true })

async function shot(page: Page, info: TestInfo, name: string): Promise<void> {
  const extra = process.env.MARGIN_SHOTS
  await page.screenshot({ path: info.outputPath(`${name}.png`) })
  if (extra) {
    mkdirSync(extra, { recursive: true })
    await page.screenshot({ path: join(extra, `${info.project.name}-${name}.png`) })
  }
}

async function desktopBridge(page: Page, state: AppState, submitted: boolean[]): Promise<void> {
  await page.addInitScript(({ state, submitted }) => {
    let current = state
    window.__TAURI__ = {
      core: {
        async invoke(command) {
          if (command === 'send_test_notification') {
            current = { ...current, revision: current.revision + 1, notificationFailedAt: submitted.shift() ? null : new Date().toISOString() }
            return undefined
          }
          if (command === 'get_state') return current
          if (command === 'get_popover_placement') return { revision: 0, arrowX: null }
          return undefined
        },
      },
      event: { listen: async () => () => {} },
    }
  }, { state, submitted })
}

test('the preview handles a test notification without sending one', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', error => errors.push(error.message))
  await page.goto('/?demo=1')
  await openSettings(page)
  await send(page).click()
  await expect(page.getByRole('status').filter({ hasText: 'Preview only. No notification was sent.' })).toBeVisible()
  await expect(page.getByText(/^Sent at /)).toHaveCount(0)
  expect(errors).toEqual([])
})

test('a failure shows in the preview and clears on the next success', async ({ page }) => {
  await page.goto('/?demo=1&state=notification-failed')
  await openSettings(page)
  const failure = page.getByRole('alert').filter({ hasText: 'Couldn’t send the last notification' })
  await expect(failure).toBeVisible()
  await expect(failure).toContainText('Margin retries failed alerts a few times.')
  await send(page).click()
  await expect(failure).toHaveCount(0)
})

test('the app says a test was sent, not that it appeared', async ({ page }) => {
  await desktopBridge(page, createDemoState(null), [true])
  await page.goto('/')
  await openSettings(page)
  await expect(page.getByText('Margin can tell it sent a notification, not that it appeared.')).toBeVisible()
  await send(page).click()
  await expect(page.getByText(/^Sent at .+\. Margin can’t tell if it appeared\.$/)).toBeVisible()
})

test('a failed test shows the failure and a later success clears it', async ({ page }) => {
  await desktopBridge(page, createDemoState(null), [false, true])
  await page.goto('/')
  await openSettings(page)
  await send(page).click()
  const failure = page.getByRole('alert').filter({ hasText: 'Couldn’t send the last notification' })
  await expect(failure).toBeVisible()
  await expect(page.getByText(/^Sent at /)).toHaveCount(0)
  await send(page).click()
  await expect(failure).toHaveCount(0)
  await expect(page.getByText(/^Sent at /)).toBeVisible()
})

test('a test still works with usage alerts off', async ({ page }) => {
  await page.goto('/?demo=1')
  await openSettings(page)
  await expect(page.getByRole('switch', { name: 'Usage alerts', exact: true })).toHaveAttribute('aria-checked', 'false')
  await expect(send(page)).toBeEnabled()
})

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} notification test states`, async ({ page }, info) => {
    await page.goto(`/?demo=1&theme=${theme}&state=notification-failed`)
    await openSettings(page)
    await send(page).scrollIntoViewIfNeeded()
    await expect(send(page)).toBeInViewport({ ratio: 1 })
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
    await shot(page, info, `notifications-failed-${theme}`)
  })

  test(`${theme} test sent state`, async ({ page }, info) => {
    const state = createDemoState(null)
    state.settings.theme = theme
    await desktopBridge(page, state, [true])
    await page.goto('/')
    await openSettings(page)
    await send(page).click()
    await expect(page.getByText(/^Sent at /)).toBeInViewport()
    await shot(page, info, `notifications-sent-${theme}`)
  })
}
