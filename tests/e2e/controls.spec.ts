import { expect, test, type Locator, type Page } from '@playwright/test'
import { createDemoState } from '../../src/lib/demo.ts'
import { accountSchema, type AppState } from '../../src/lib/model.ts'

declare global {
  interface Window { __CONTROL_TEST__?: { rejectPending: () => void } }
}

async function installDesktopBridge(page: Page, options: { state: AppState; failure: 'none' | 'save' | 'remove' }): Promise<void> {
  await page.addInitScript(({ state, failure }) => {
    let settings: unknown = state.settings
    let accounts = state.accounts
    let revision = state.revision
    const listeners = new Map<string, (event: { payload: unknown }) => void>()
    let rejectPending: (() => void) | undefined
    window.__CONTROL_TEST__ = { rejectPending: () => { rejectPending?.(); rejectPending = undefined } }
    function fail(message: string): Promise<never> {
      return new Promise((_resolve, reject) => { rejectPending = () => reject(new Error(message)) })
    }
    window.__TAURI__ = {
      core: {
        async invoke(command, args) {
          if (command === 'get_state') return { ...state, revision, settings, accounts }
          if (command === 'get_popover_placement') return { revision: 0, arrowX: null }
          if (command === 'save_settings') {
            if (failure === 'save') return fail('Could not save settings. Try again.')
            settings = args?.settings
            revision++
          }
          if (command === 'remove_account') {
            if (failure === 'remove') return fail('Could not remove account. Try again.')
            accounts = accounts.filter(account => account.id !== args?.id)
            revision++
          }
          if (command === 'hide_window') document.getElementById('app')?.setAttribute('hidden', '')
          return undefined
        },
      },
      event: {
        async listen(event, handler) {
          listeners.set(event, handler)
          return () => { listeners.delete(event) }
        },
      },
    }
  }, options)
}

async function openSettings(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Open settings', exact: true }).click()
  await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible()
}

async function expectWithinViewport(page: Page, locator: Locator): Promise<void> {
  await expect(locator).toBeVisible()
  const bounds = await locator.evaluate(element => {
    const rect = element.getBoundingClientRect()
    return { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom }
  })
  const viewport = await page.evaluate(() => ({ width: innerWidth, height: innerHeight }))
  expect(bounds.left).toBeGreaterThanOrEqual(0)
  expect(bounds.top).toBeGreaterThanOrEqual(0)
  expect(bounds.right).toBeLessThanOrEqual(viewport.width)
  expect(bounds.bottom).toBeLessThanOrEqual(viewport.height)
}

test('selects support Arrow, Home, End, and typeahead selection', async ({ page }) => {
  await page.goto('/?demo=1')
  await openSettings(page)
  const interval = page.getByRole('combobox', { name: 'Refresh while open', exact: true })
  await interval.focus()
  await page.keyboard.press('ArrowDown')
  await expect(page.getByRole('listbox')).toBeVisible()
  await expect(page.getByRole('option', { name: '1 minute', exact: true })).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await expect(page.getByRole('option', { name: '2 minutes', exact: true })).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(interval).toContainText('2 minutes')
  await expect(interval).toBeFocused()

  await page.keyboard.press('Enter')
  await expect(page.getByRole('option', { name: '2 minutes', exact: true })).toBeFocused()
  await page.keyboard.press('End')
  await expect(page.getByRole('option', { name: '5 minutes', exact: true })).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(interval).toContainText('5 minutes')
  await page.keyboard.press('Space')
  await expect(page.getByRole('option', { name: '5 minutes', exact: true })).toBeFocused()
  await page.keyboard.press('Home')
  await expect(page.getByRole('option', { name: '30 seconds', exact: true })).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(interval).toContainText('30 seconds')

  const tray = page.getByRole('combobox', { name: 'Tray account', exact: true })
  await tray.focus()
  await page.keyboard.press('Space')
  await expect(page.getByRole('listbox')).toBeVisible()
  await page.keyboard.type('Stu')
  await expect(page.getByRole('option', { name: 'Studio', exact: true })).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(tray).toContainText('Studio')
  await expect(tray).toBeFocused()
})

test('Escape closes a select before leaving Settings and returns trigger focus', async ({ page }) => {
  await page.goto('/?demo=1')
  await openSettings(page)
  const trigger = page.getByRole('combobox', { name: 'Refresh while open', exact: true })
  await trigger.click()
  await expect(page.getByRole('listbox')).toBeVisible()
  await page.keyboard.press('End')
  await page.keyboard.press('Escape')
  await expect(page.getByRole('listbox')).toHaveCount(0)
  await expect(trigger).toContainText('1 minute')
  await expect(trigger).toBeFocused()
  await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('heading', { name: 'Your headroom', exact: true })).toBeVisible()
})

for (const theme of ['light', 'dark']) {
  test(`${theme} custom menus remain inside a narrow desktop window`, async ({ page }, info) => {
    const state = createDemoState(null)
    state.settings.theme = theme === 'light' ? 'light' : 'dark'
    await installDesktopBridge(page, { state, failure: 'none' })
    await page.setViewportSize({ width: 360, height: 480 })
    await page.goto('/')
    await openSettings(page)
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
    for (const label of ['Tray account', 'Refresh while open', 'Refresh in background']) {
      const trigger = page.getByRole('combobox', { name: label, exact: true })
      await trigger.click()
      const menu = page.getByRole('listbox')
      await expectWithinViewport(page, menu)
      await expect(menu.getByRole('option').first()).toBeVisible()
      await info.attach(`${theme}-${label}`, { body: await page.screenshot(), contentType: 'image/png' })
      await page.keyboard.press('Escape')
      await expect(menu).toHaveCount(0)
      await expect(trigger).toBeFocused()
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  })
}

test('a hundred long account names stay scrollable and keyboard reachable', async ({ page }, info) => {
  const state = createDemoState(null)
  const lastLabel = 'Zeta workspace with a very long account label'
  state.accounts = Array.from({ length: 100 }, (_, index) => accountSchema.parse({
    id: `control-account-${index}`,
    provider: index % 2 === 0 ? 'claude' : 'codex',
    label: index === 99 ? lastLabel : `Workspace ${String(index + 1).padStart(3, '0')} · Research and production teams`,
    color: '#b8d1bf',
    source: { kind: 'configDir', path: `/example/cli/account-${index}` },
    identity: { email: null, plan: null },
    snapshot: null,
    error: null,
    refreshing: false,
  }))
  await installDesktopBridge(page, { state, failure: 'none' })
  await page.setViewportSize({ width: 360, height: 480 })
  await page.goto('/')
  await openSettings(page)
  const trigger = page.getByRole('combobox', { name: 'Tray account', exact: true })
  await trigger.focus()
  await page.keyboard.press('Space')
  const menu = page.getByRole('listbox')
  await expect(menu.getByRole('option')).toHaveCount(101)
  await expectWithinViewport(page, menu)
  await expect(page.getByRole('option', { name: 'Lowest remaining', exact: true })).toBeFocused()
  await page.keyboard.press('End')
  const lastOption = page.getByRole('option', { name: lastLabel, exact: true })
  await expect(lastOption).toBeFocused()
  await expect(lastOption).toBeInViewport({ ratio: 1 })
  await expectWithinViewport(page, lastOption)
  await info.attach('long-account-menu', { body: await page.screenshot(), contentType: 'image/png' })
  await page.keyboard.press('Home')
  await expect(page.getByRole('option', { name: 'Lowest remaining', exact: true })).toBeFocused()
  await page.keyboard.type('Zeta')
  await expect(lastOption).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(trigger).toContainText(lastLabel)
  await expect(trigger).toBeFocused()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
})

test('settings tabs and reminder switch work entirely from the keyboard', async ({ page }) => {
  await page.goto('/?demo=1')
  await openSettings(page)
  const general = page.getByRole('tab', { name: 'General', exact: true })
  const accounts = page.getByRole('tab', { name: /^Accounts/ })
  await general.focus()
  await page.keyboard.press('ArrowRight')
  await expect(accounts).toBeFocused()
  await expect(accounts).toHaveAttribute('aria-selected', 'true')
  await expect(page.getByRole('heading', { name: 'Your accounts', exact: true })).toBeVisible()
  await page.keyboard.press('ArrowLeft')
  await expect(general).toBeFocused()
  await expect(general).toHaveAttribute('aria-selected', 'true')
  const reminders = page.getByRole('switch', { name: 'Usage reminders', exact: true })
  await reminders.focus()
  await page.keyboard.press('Space')
  await expect(reminders).toHaveAttribute('aria-checked', 'true')
  await expect(page.getByRole('button', { name: 'Notify at 80% used', exact: true })).toBeEnabled()
  await expect(reminders).toBeFocused()
  await page.keyboard.press('Space')
  await expect(reminders).toHaveAttribute('aria-checked', 'false')
  await expect(page.getByRole('button', { name: 'Notify at 80% used', exact: true })).toBeDisabled()
})

test('remove confirmation traps focus and Escape restores its trigger', async ({ page }) => {
  await page.goto('/?demo=1')
  await openSettings(page)
  await page.getByRole('tab', { name: /^Accounts/ }).click()
  const trigger = page.getByRole('button', { name: 'Remove Personal', exact: true })
  await trigger.click()
  const dialog = page.getByRole('alertdialog')
  const keep = dialog.getByRole('button', { name: 'Keep account', exact: true })
  const remove = dialog.getByRole('button', { name: 'Remove account', exact: true })
  await expect(keep).toBeFocused()
  await page.keyboard.press('Shift+Tab')
  await expect(remove).toBeFocused()
  await page.keyboard.press('Tab')
  await expect(keep).toBeFocused()
  await page.keyboard.press('Tab')
  await expect(remove).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(dialog).toHaveCount(0)
  await expect(trigger).toBeFocused()
  await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible()
  await trigger.click()
  await keep.click()
  await expect(dialog).toHaveCount(0)
  await expect(trigger).toBeFocused()
})

test('failed removal keeps the confirmation and the account', async ({ page }) => {
  await installDesktopBridge(page, { state: createDemoState(null), failure: 'remove' })
  await page.goto('/')
  await openSettings(page)
  await page.getByRole('tab', { name: /^Accounts/ }).click()
  const trigger = page.getByRole('button', { name: 'Remove Personal', exact: true })
  await trigger.click()
  const dialog = page.getByRole('alertdialog')
  await dialog.getByRole('button', { name: 'Remove account', exact: true }).click()
  await expect(dialog.getByRole('button', { name: 'Remove account', exact: true })).toBeDisabled()
  await expect(dialog.getByRole('button', { name: 'Keep account', exact: true })).toBeDisabled()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeVisible()
  await page.evaluate(() => window.__CONTROL_TEST__?.rejectPending())
  await expect(page.getByText('Could not remove account. Try again.', { exact: true })).toBeVisible()
  await expect(dialog).toBeVisible()
  await expect(dialog.getByRole('alert')).toBeVisible()
  await expect(dialog.getByRole('button', { name: 'Remove account', exact: true })).toBeEnabled()
  await expect(dialog.getByRole('button', { name: 'Remove account', exact: true })).toBeFocused()
  await dialog.getByRole('button', { name: 'Keep account', exact: true }).click()
  await expect(trigger).toBeFocused()
  await expect(page.getByRole('heading', { name: 'Personal', exact: true })).toBeVisible()
})

test('failed saves keep the previously saved select and switch values', async ({ page }) => {
  await installDesktopBridge(page, { state: createDemoState(null), failure: 'save' })
  await page.goto('/')
  await openSettings(page)
  const interval = page.getByRole('combobox', { name: 'Refresh while open', exact: true })
  await interval.click()
  await page.getByRole('option', { name: '2 minutes', exact: true }).click()
  await expect(interval).toBeDisabled()
  await page.evaluate(() => window.__CONTROL_TEST__?.rejectPending())
  await expect(page.getByText('Could not save settings. Try again.', { exact: true })).toBeVisible()
  await expect(page.getByRole('listbox')).toHaveCount(0)
  await expect(interval).toContainText('1 minute')
  await expect(interval).toBeFocused()
  const reminders = page.getByRole('switch', { name: 'Usage reminders', exact: true })
  await reminders.focus()
  await page.keyboard.press('Space')
  await expect(reminders).toBeDisabled()
  await page.evaluate(() => window.__CONTROL_TEST__?.rejectPending())
  await expect(reminders).toBeEnabled()
  await expect(reminders).toHaveAttribute('aria-checked', 'false')
  await expect(reminders).toBeFocused()
})

test('a delayed save does not steal focus moved to another control', async ({ page }) => {
  await installDesktopBridge(page, { state: createDemoState(null), failure: 'save' })
  await page.goto('/')
  await openSettings(page)
  const interval = page.getByRole('combobox', { name: 'Refresh while open', exact: true })
  await interval.click()
  await page.getByRole('option', { name: '2 minutes', exact: true }).click()
  await expect(interval).toBeDisabled()
  const general = page.getByRole('tab', { name: 'General', exact: true })
  await general.focus()
  await page.evaluate(() => window.__CONTROL_TEST__?.rejectPending())
  await expect(interval).toBeEnabled()
  await expect(interval).toContainText('1 minute')
  await expect(general).toBeFocused()
})

test('tooltips support focus and Escape without native title bubbles', async ({ page }) => {
  await installDesktopBridge(page, { state: createDemoState(null), failure: 'none' })
  await page.goto('/')
  await expect(page.locator('[title]')).toHaveCount(0)
  const settings = page.getByRole('button', { name: 'Open settings', exact: true })
  await settings.hover()
  await expect(page.getByRole('tooltip')).toContainText('Settings')
  await page.mouse.move(1, 1)
  await expect(page.getByRole('tooltip')).toHaveCount(0)
  await settings.focus()
  await expect(page.getByRole('tooltip')).toContainText('Settings')
  await page.keyboard.press('Escape')
  await expect(page.getByRole('tooltip')).toHaveCount(0)
  await expect(settings).toBeFocused()
  await expect(page.getByRole('heading', { name: 'Your headroom', exact: true })).toBeVisible()
  await page.keyboard.press('Enter')
  await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible()
  await page.getByRole('tab', { name: /^Accounts/ }).click()
  await expect(page.locator('[title]')).toHaveCount(0)
})
