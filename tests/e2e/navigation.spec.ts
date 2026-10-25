import { expect, test, type Page } from '@playwright/test'
import { createDemoState } from '../../src/lib/demo.ts'
import type { AppState } from '../../src/lib/model.ts'

declare global {
  interface Window { __NAV_TEST__?: { emit: (event: string, payload: unknown) => void; saved: unknown[] } }
}

async function installBridge(page: Page, state: AppState): Promise<void> {
  await page.addInitScript(initial => {
    let current = initial
    const listeners = new Map<string, (event: { payload: unknown }) => void>()
    const saved: unknown[] = []
    window.__NAV_TEST__ = { emit: (event, payload) => listeners.get(event)?.({ payload }), saved }
    window.__TAURI__ = {
      core: {
        async invoke(command, args) {
          if (command === 'get_state') return current
          if (command === 'get_popover_placement') return { revision: 0, arrowX: null }
          if (command === 'save_settings') {
            saved.push(args?.settings)
            current = { ...current, revision: current.revision + 1, settings: args?.settings as typeof current.settings }
          }
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
  }, state)
}

function bridgeState(scenario: string | null, layout: 'cards' | 'compact' = 'cards'): AppState {
  const state = createDemoState(scenario)
  return { ...state, settings: { ...state.settings, overviewLayout: layout } }
}

const emit = (page: Page, event: string, payload: unknown) => page.evaluate(([event, payload]) => window.__NAV_TEST__?.emit(event as string, payload), [event, payload] as const)
const cardNames = (page: Page) => page.locator('.account-card h2')
const rowNames = (page: Page) => page.locator('.compact-label')
const row = (page: Page, name: string) => page.getByRole('button', { name: new RegExp(`^${name}, `) })

test('cards show the account email beside the plan without extra tab stops', async ({ page }) => {
  await page.goto('/?demo=1&order=added')
  const personal = page.getByRole('article', { name: 'Personal, Claude' })
  await expect(personal.locator('.account-subline')).toHaveText('alex@example.com·Max 5×')
  await expect(personal.locator('h2')).not.toHaveAttribute('tabindex')
  await expect(personal.locator('.quota-reset').first()).not.toHaveAttribute('tabindex')
  await expect(page.getByRole('article', { name: 'Work, Codex' }).locator('.account-email')).toHaveText('alex@studio.example')
  const focusables = await personal.evaluate(element => [...element.querySelectorAll<HTMLElement>('button, [tabindex="0"]')].map(node => node.getAttribute('aria-label') ?? node.textContent?.trim()))
  expect(focusables).toEqual(['Pin Personal to tray', 'Fable weekly remaining', 'Opus weekly remaining', 'Sonnet weekly remaining', 'History'])
})

test('long emails and sign-in commands stay inside the card at 360px', async ({ page }, info) => {
  await page.setViewportSize({ width: 360, height: 660 })
  const state = createDemoState('compact')
  state.settings.overviewLayout = 'cards'
  state.accounts = state.accounts.map(account => account.id === 'demo-claude-client' ? { ...account, identity: { email: 'alexandra.longname@northwind-studio.example', plan: 'Max 20×' }, error: account.error && { ...account.error, signInCommand: 'CLAUDE_CONFIG_DIR=~/Library/Application Support/Northwind/claude-client-account claude' } } : account)
  await installBridge(page, state)
  await page.goto('/')
  const card = page.getByRole('article', { name: 'Client work for Northwind Studio, Claude' })
  await card.scrollIntoViewIfNeeded()
  await expect(card.locator('.account-plan')).toBeVisible()
  await expect(card.locator('.account-email')).toHaveCSS('text-overflow', 'ellipsis')
  const command = card.locator('.command-copy')
  await expect(command).toContainText('claude-client-account claude')
  const [cardBox, commandBox] = await Promise.all([card.boundingBox(), command.boundingBox()])
  expect(commandBox!.x + commandBox!.width).toBeLessThanOrEqual(cardBox!.x + cardBox!.width)
  expect(commandBox!.height).toBeGreaterThan(40)
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(360)
  await card.screenshot({ path: info.outputPath('long-command.png') })
})

test('urgency order puts the pinned account, then the lowest remaining first', async ({ page }) => {
  await page.goto('/?demo=1')
  await expect(cardNames(page)).toHaveText(['Studio', 'Personal', 'Work'])
  await page.goto('/?demo=1&order=added')
  await expect(cardNames(page)).toHaveText(['Personal', 'Work', 'Studio'])
  await page.goto('/?demo=1&state=compact')
  await expect(rowNames(page)).toHaveText(['Personal', 'Studio', 'Work', 'Side project', 'Client work for Northwind Studio'])
})

test('account order setting saves and applies', async ({ page }) => {
  await installBridge(page, bridgeState(null))
  await page.goto('/')
  await page.getByRole('button', { name: 'Open settings', exact: true }).click()
  await expect(page.getByText('Pinned account stays first.')).toBeVisible()
  await page.getByRole('combobox', { name: 'Account order' }).click()
  await page.getByRole('option', { name: 'As added' }).click()
  await expect.poll(() => page.evaluate(() => window.__NAV_TEST__?.saved.at(-1))).toMatchObject({ accountOrder: 'added' })
  await page.getByRole('button', { name: 'Back to overview' }).click()
  await expect(cardNames(page)).toHaveText(['Personal', 'Work', 'Studio'])
})

test('rows keep their place on updates and re-sort after a long hide', async ({ page }) => {
  const state = bridgeState(null)
  await installBridge(page, state)
  await page.goto('/')
  await expect(cardNames(page)).toHaveText(['Studio', 'Personal', 'Work'])
  const next = structuredClone(state)
  next.revision = 5
  next.accounts = next.accounts.map(account => account.id === 'demo-codex-work' && account.snapshot ? { ...account, snapshot: { ...account.snapshot, windows: account.snapshot.windows.map(window => ({ ...window, usedPercent: 99 })) } } : account)
  await emit(page, 'state_changed', next)
  await expect(page.getByRole('article', { name: 'Work, Codex' }).locator('.quota-value').first()).toHaveText('1%')
  await expect(cardNames(page)).toHaveText(['Studio', 'Personal', 'Work'])
  await emit(page, 'popover_shown', { hiddenForMs: 0 })
  await emit(page, 'popover_shown', { hiddenForMs: 59_000 })
  await expect(cardNames(page)).toHaveText(['Studio', 'Personal', 'Work'])
  await emit(page, 'popover_shown', { hiddenForMs: 60_000 })
  await expect(cardNames(page)).toHaveText(['Work', 'Studio', 'Personal'])
})

test('reopening after a minute resets the view; a quick reopen keeps it', async ({ page }) => {
  await installBridge(page, bridgeState('compact', 'compact'))
  await page.goto('/')
  await row(page, 'Work').click()
  await expect(row(page, 'Work')).toHaveAttribute('aria-expanded', 'true')
  const scroller = page.locator('.accounts-scroll')
  await scroller.evaluate(element => { element.scrollTop = 120 })
  await emit(page, 'popover_shown', { hiddenForMs: 20_000 })
  await expect(row(page, 'Work')).toHaveAttribute('aria-expanded', 'true')
  expect(await scroller.evaluate(element => element.scrollTop)).toBeGreaterThan(0)
  await emit(page, 'popover_shown', { hiddenForMs: 'soon' })
  await expect(row(page, 'Work')).toHaveAttribute('aria-expanded', 'true')
  await emit(page, 'popover_shown', { hiddenForMs: 90_000 })
  await expect(row(page, 'Work')).toHaveAttribute('aria-expanded', 'false')
  await expect.poll(() => scroller.evaluate(element => element.scrollTop)).toBe(0)

  await page.getByRole('button', { name: 'Open settings', exact: true }).click()
  await emit(page, 'popover_shown', { hiddenForMs: 5_000 })
  await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible()
  await emit(page, 'popover_shown', { hiddenForMs: null })
  await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toHaveCount(0)
  await expect(page.locator('.compact-row')).toHaveCount(5)

  await emit(page, 'open_settings', null)
  await emit(page, 'popover_shown', { hiddenForMs: 600_000 })
  await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible()
})

test.describe('on macOS', () => {
  test.use({ userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' })
  test('⌘1 and ⌘2 switch the overview layout and save it', async ({ page }) => {
    await installBridge(page, bridgeState(null))
    await page.goto('/')
    await expect(page.locator('.account-card')).toHaveCount(3)
    await page.keyboard.press('Meta+2')
    await expect(page.locator('.compact-row')).toHaveCount(3)
    await page.keyboard.press('Control+1')
    await expect(page.locator('.account-card')).toHaveCount(3)
    await page.keyboard.press('Meta+1')
    const saved = await page.evaluate(() => window.__NAV_TEST__?.saved.map(settings => (settings as { overviewLayout: string }).overviewLayout))
    expect(saved).toEqual(['compact', 'cards'])
    await page.getByRole('button', { name: 'Open settings', exact: true }).click()
    await expect(page.getByText('Compact shows one row per account. ⌘1 / ⌘2 to switch.')).toBeVisible()
  })
})

test.describe('on Windows', () => {
  test.use({ userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' })
  test('the layout hint names Ctrl', async ({ page }) => {
    await page.goto('/?demo=1')
    await page.getByRole('button', { name: 'Open settings', exact: true }).click()
    await expect(page.getByText('Compact shows one row per account. Ctrl+1 / Ctrl+2 to switch.')).toBeVisible()
  })
})

test('the attention footer jumps to the first account that needs it', async ({ page }) => {
  await page.goto('/?demo=1&state=signed-out')
  const footer = page.getByRole('button', { name: '2 accounts need attention', exact: true })
  await footer.click()
  const personal = page.getByRole('article', { name: 'Personal, Claude' })
  await expect(personal).toBeFocused()
  await expect(personal).toBeInViewport()

  await page.goto('/?demo=1&state=compact')
  await page.locator('.account-filters button', { hasText: 'Claude' }).click()
  await expect(row(page, 'Side project')).toHaveCount(0)
  await page.getByRole('button', { name: '2 accounts need attention', exact: true }).click()
  const side = row(page, 'Side project')
  await expect(side).toHaveAttribute('aria-expanded', 'true')
  await expect(side).toBeFocused()
  await expect(page.locator('.account-filters [aria-pressed="true"]')).toHaveText(/^All/)
})

test('arrow keys move between compact rows', async ({ page }) => {
  await page.goto('/?demo=1&state=compact')
  const rows = page.locator('.compact-row')
  await rows.first().focus()
  await page.keyboard.press('ArrowDown')
  await expect(rows.nth(1)).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await expect(rows.nth(2)).toBeFocused()
  await page.keyboard.press('ArrowUp')
  await expect(rows.nth(1)).toBeFocused()
  await page.keyboard.press('End')
  await expect(rows.last()).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await expect(rows.last()).toBeFocused()
  await page.keyboard.press('Home')
  await expect(rows.first()).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(rows.first()).toHaveAttribute('aria-expanded', 'true')
  await page.keyboard.press('ArrowDown')
  await expect(rows.nth(1)).toBeFocused()
})
