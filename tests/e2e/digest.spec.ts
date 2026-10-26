import { expect, test, type Page } from '@playwright/test'
import { createDemoState } from '../../src/lib/demo.ts'
import type { AppState } from '../../src/lib/model.ts'

declare global {
  interface Window { __DIGEST_TEST__?: { saved: unknown[] } }
}

async function installDesktopBridge(page: Page, state: AppState): Promise<void> {
  await page.addInitScript((state: AppState) => {
    let settings: unknown = state.settings
    let revision = state.revision
    const saved: unknown[] = []
    window.__DIGEST_TEST__ = { saved }
    window.__TAURI__ = {
      core: {
        async invoke(command, args) {
          if (command === 'get_state') return { ...state, revision, settings }
          if (command === 'get_popover_placement') return { revision: 0, arrowX: null }
          if (command === 'save_settings') {
            settings = args?.settings
            saved.push(settings)
            revision++
          }
          return undefined
        },
      },
      event: {
        async listen() {
          return () => {}
        },
      },
    }
  }, state)
}

async function openSettings(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Open settings', exact: true }).click()
  await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible()
}

function notificationState(): AppState {
  const state = createDemoState(null)
  return { ...state, settings: { ...state.settings, notifications: true } }
}

async function savedSettings(page: Page): Promise<AppState['settings'][]> {
  return page.evaluate(() => (window.__DIGEST_TEST__?.saved ?? []) as AppState['settings'][])
}

test('weekly digest starts off, saves, and follows the usage alerts switch', async ({ page }) => {
  await installDesktopBridge(page, notificationState())
  await page.goto('/')
  await openSettings(page)
  const digest = page.getByRole('switch', { name: 'Weekly digest', exact: true })
  await expect(digest).toHaveAttribute('aria-checked', 'false')
  await expect(page.getByText("Mondays at 09:00: last week's peaks and limit hits.", { exact: true })).toBeVisible()
  await digest.click()
  await expect(digest).toHaveAttribute('aria-checked', 'true')
  await page.getByRole('button', { name: 'Back to overview', exact: true }).click()
  await openSettings(page)
  await expect(digest).toHaveAttribute('aria-checked', 'true')
  await page.getByRole('switch', { name: 'Usage alerts', exact: true }).click()
  await expect(digest).toBeDisabled()
  const saved = await savedSettings(page)
  expect(saved).toHaveLength(2)
  expect(saved[0]).toMatchObject({ weeklyDigest: true, notifications: true })
  expect(saved[1]).toMatchObject({ weeklyDigest: true, notifications: false })
})

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} notification settings with the weekly digest on`, async ({ page }, info) => {
    await page.goto(`/?demo=1&theme=${theme}`)
    await openSettings(page)
    await page.getByRole('switch', { name: 'Usage alerts', exact: true }).click()
    await page.getByRole('switch', { name: 'Weekly digest', exact: true }).click()
    const section = page.locator('.settings-section').filter({ has: page.getByRole('heading', { name: 'Notifications', exact: true }) })
    const row = section.locator('.digest-row')
    await row.scrollIntoViewIfNeeded()
    await expect(row).toBeInViewport({ ratio: 1 })
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
    await page.screenshot({ path: info.outputPath(`settings-digest-${theme}.png`) })
  })
}
