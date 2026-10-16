import { expect, test, type Page } from '@playwright/test'
import { createDemoState } from '../../src/lib/demo.ts'

declare global {
  interface Window {
    __UPDATE_TEST__?: {
      calls: { command: string; args?: Record<string, unknown> }[];
      finish: (payload: unknown) => void;
      fail: () => void;
      emit: (payload: unknown) => void;
    }
  }
}

async function bridge(page: Page, theme: 'light' | 'dark' = 'light'): Promise<void> {
  const appState = createDemoState(null)
  appState.settings.theme = theme
  await page.addInitScript(state => {
    const listeners = new Map<string, (event: { payload: unknown }) => void>()
    const calls: { command: string; args?: Record<string, unknown> }[] = []
    let finish: ((payload: unknown) => void) | undefined
    let fail: (() => void) | undefined
    window.__UPDATE_TEST__ = {
      calls,
      finish: payload => { finish?.(payload); finish = undefined; fail = undefined },
      fail: () => { fail?.(); finish = undefined; fail = undefined },
      emit: payload => listeners.get('update_status_changed')?.({ payload }),
    }
    window.__TAURI__ = {
      core: { async invoke(command, args) {
        if (command === 'get_state') return state
        if (command === 'get_popover_placement') return { revision: 0, arrowX: null }
        if (command === 'get_update_status') return { revision: 0, state: { kind: 'idle' } }
        if (command === 'check_for_update' || command === 'install_update') {
          calls.push({ command, args })
          return new Promise((resolve, reject) => {
            finish = resolve
            fail = () => reject(new Error('raw internal error'))
          })
        }
        return undefined
      } },
      event: { async listen(name, handler) {
        listeners.set(name, handler)
        return () => { listeners.delete(name) }
      } },
    }
  }, appState)
}

async function openUpdates(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Open settings', exact: true }).click()
  await page.getByRole('region', { name: 'Updates', exact: true }).scrollIntoViewIfNeeded()
}

test('updates require explicit check and install, preserve focus, and survive Settings reopening', async ({ page }) => {
  await bridge(page)
  await page.goto('/')
  await openUpdates(page)
  expect(await page.evaluate(() => window.__UPDATE_TEST__?.calls)).toEqual([])
  const updates = page.getByRole('region', { name: 'Updates', exact: true })
  const action = updates.getByRole('button')
  await action.focus()
  await page.keyboard.press('Enter')
  await expect(action).toHaveAttribute('aria-disabled', 'true')
  await page.keyboard.press('Enter')
  expect(await page.evaluate(() => window.__UPDATE_TEST__?.calls.length)).toBe(1)
  await page.evaluate(() => window.__UPDATE_TEST__?.finish({ revision: 1, state: { kind: 'available', version: '0.3.0' } }))
  await expect(action).toHaveText('Install and restart')
  await expect(action).toBeFocused()
  expect(await page.evaluate(() => window.__UPDATE_TEST__?.calls.length)).toBe(1)
  await page.keyboard.press('Enter')
  expect(await page.evaluate(() => window.__UPDATE_TEST__?.calls.at(-1))).toEqual({ command: 'install_update', args: { version: '0.3.0' } })
  await page.evaluate(() => window.__UPDATE_TEST__?.emit({ revision: 2, state: { kind: 'downloading', version: '0.3.0', downloaded: 250, total: 1000 } }))
  await expect(updates.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '25')
  await expect(action).toBeFocused()
  await page.getByRole('button', { name: 'Back to overview' }).click()
  await openUpdates(page)
  await expect(updates.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '25')
  await expect(action).toHaveAttribute('aria-disabled', 'true')
  await page.evaluate(() => window.__UPDATE_TEST__?.emit({ revision: 3, state: { kind: 'installing', version: '0.3.0' } }))
  await expect(updates.getByRole('status')).toHaveText('Installing. Margin will restart.')
  await expect(updates.getByRole('progressbar')).toHaveCount(0)
})

test('failed update request keeps keyboard focus and offers a safe retry', async ({ page }) => {
  await bridge(page)
  await page.goto('/')
  await openUpdates(page)
  const updates = page.getByRole('region', { name: 'Updates', exact: true })
  const action = updates.getByRole('button')
  await action.focus()
  await page.keyboard.press('Enter')
  await page.evaluate(() => window.__UPDATE_TEST__?.fail())
  await expect(updates.getByRole('alert')).toHaveText('Couldn’t connect to the updater. Try again.')
  await expect(action).toBeFocused()
  await expect(action).toHaveAttribute('aria-disabled', 'false')
  await page.keyboard.press('Enter')
  await page.evaluate(() => window.__UPDATE_TEST__?.finish({ revision: 2, state: { kind: 'current' } }))
  await expect(updates.getByRole('status')).toHaveText('Up to date.')
  await expect(action).toBeFocused()
})

for (const theme of ['light', 'dark']) {
  test(`${theme} update states fit narrow windows and unknown download lengths`, async ({ page }, info) => {
    await bridge(page, theme === 'light' ? 'light' : 'dark')
    await page.setViewportSize({ width: 360, height: 480 })
    await page.goto('/')
    await openUpdates(page)
    const updates = page.getByRole('region', { name: 'Updates', exact: true })
    await page.evaluate(() => window.__UPDATE_TEST__?.emit({ revision: 1, state: { kind: 'available', version: '0.3.0' } }))
    await expect(updates.getByRole('button', { name: 'Install and restart' })).toBeVisible()
    await info.attach(`updater-${theme}-available`, { body: await page.screenshot(), contentType: 'image/png' })
    await page.evaluate(() => window.__UPDATE_TEST__?.emit({ revision: 2, state: { kind: 'downloading', version: '0.3.0', downloaded: 200, total: null } }))
    await expect(updates.getByRole('progressbar')).not.toHaveAttribute('aria-valuenow')
    await expect(updates.getByRole('button')).toHaveText('Downloading…')
    await page.evaluate(() => window.__UPDATE_TEST__?.emit({ revision: 3, state: { kind: 'error', message: 'Couldn’t install this update. Check again to retry.' } }))
    await expect(updates.getByRole('alert')).toBeVisible()
    await expect(updates.getByRole('button')).toHaveText('Check for updates')
    await info.attach(`updater-${theme}-error`, { body: await page.screenshot(), contentType: 'image/png' })
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  })
}

test('preview explains desktop-only updates without invoking the bridge', async ({ page }) => {
  await bridge(page)
  await page.goto('/?demo=1')
  await openUpdates(page)
  const updates = page.getByRole('region', { name: 'Updates', exact: true })
  await expect(updates.getByRole('status')).toHaveText('Update checks require the desktop app.')
  const action = updates.getByRole('button')
  await expect(action).toHaveAttribute('aria-disabled', 'true')
  await action.focus()
  await page.keyboard.press('Enter')
  expect(await page.evaluate(() => window.__UPDATE_TEST__?.calls)).toEqual([])
})
