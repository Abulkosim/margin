import { expect, test } from '@playwright/test'
import { createDemoState } from '../../src/lib/demo'

for (const theme of ['light', 'dark']) {
  test(`${theme} history has readable values, honest gaps, and keyboard inspection`, async ({ page }, info) => {
    await page.goto(`/?demo=1&theme=${theme}`)
    const account = page.getByRole('article', { name: 'Personal, Claude' })
    await account.getByRole('button', { name: 'History', exact: true }).click()
    const history = account.getByRole('region', { name: 'Usage history' })
    await expect(history).toBeVisible()
    expect((await history.boundingBox())?.height).toBeLessThanOrEqual(350)
    await expect(history.getByRole('tab', { name: 'Session', exact: true })).toHaveAttribute('aria-selected', 'true')
    await expect(history.locator('.history-reading strong')).toContainText('72% remaining')
    await expect(history.locator('.history-line')).toHaveCount(3)
    await expect(history.locator('.history-marker')).toHaveCount(1)
    const chart = history.getByRole('slider')
    await chart.focus()
    await chart.press('Home')
    await expect(chart).toHaveAttribute('aria-valuenow', '1')
    await expect(chart).toHaveAttribute('aria-valuetext', /40% remaining/)
    await chart.press('ArrowRight')
    await expect(chart).toHaveAttribute('aria-valuenow', '2')
    await chart.press('End')
    await expect(chart).toHaveAttribute('aria-valuetext', /72% remaining/)
    await history.getByRole('tab', { name: 'Weekly', exact: true }).click()
    await expect(history.locator('.history-reading strong')).toContainText('42% remaining')
    await expect(history.locator('.history-line')).toHaveCount(2)
    await history.getByRole('tab', { name: 'Session', exact: true }).click()
    await history.scrollIntoViewIfNeeded()
    await info.attach(`history-${theme}`, { body: await page.screenshot(), contentType: 'image/png' })
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
    await account.getByRole('button', { name: 'Hide history', exact: true }).click()
    await expect(history).toHaveCount(0)
  })
}

test('history remains usable on a 360 by 480 display', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 480 })
  await page.goto('/?demo=1')
  const account = page.getByRole('article', { name: 'Work, Codex' })
  await account.getByRole('button', { name: 'History', exact: true }).click()
  const history = account.getByRole('region', { name: 'Usage history' })
  await history.getByRole('slider').scrollIntoViewIfNeeded()
  await history.getByRole('slider').focus()
  await page.keyboard.press('Home')
  await expect(history.getByRole('slider')).toHaveAttribute('aria-valuenow', '1')
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
  const bounds = await history.boundingBox()
  expect(bounds?.width).toBeLessThan(360)
})

test('desktop history handles first reading, errors, and recovery without sample data', async ({ page }) => {
  const state = createDemoState(null)
  const account = state.accounts[0]
  if (!account?.snapshot?.windows[0]) throw new Error('Missing fixture')
  state.accounts = [account]
  const window = account.snapshot.windows[0]
  await page.addInitScript(({ state, account, usage }) => {
    let attempts = 0
    Object.defineProperty(globalThis, '__TAURI__', { value: {
      core: { invoke: async (command: string) => {
        if (command === 'get_state') return state
        if (command === 'get_account_history') {
          attempts++
          if (attempts === 1) throw new Error('Unavailable')
          return { accountId: account.id, points: [{ windowId: usage.id, at: account.snapshot?.fetchedAt, usedPercent: usage.usedPercent, resetsAt: usage.resetsAt, windowSeconds: usage.windowSeconds, gapBefore: false }], error: null }
        }
        return null
      } },
      event: { listen: async () => () => {} },
    } })
  }, { state, account, usage: window })
  await page.goto('/')
  await page.getByRole('button', { name: 'History', exact: true }).click()
  const history = page.getByRole('region', { name: 'Usage history' })
  await expect(history.getByText('Could not load history.')).toBeVisible()
  await history.getByRole('button', { name: 'Try again' }).click()
  await expect(history.getByText('First reading recorded.')).toBeVisible()
  await expect(history.getByRole('slider')).toHaveCount(0)
  await expect(page.getByText('Sample data', { exact: true })).toHaveCount(0)
})

test('history validates account identity and malformed readings', async ({ page }) => {
  const state = createDemoState(null)
  state.accounts = state.accounts.slice(0, 1)
  await page.addInitScript(state => {
    let attempt = 0
    Object.defineProperty(globalThis, '__TAURI__', { value: {
      core: { invoke: async (command: string) => {
        if (command === 'get_state') return state
        if (command === 'get_account_history') {
          attempt++
          if (attempt === 1) return { accountId: 'other-account', points: [], error: null }
          if (attempt === 2) return { accountId: state.accounts[0]?.id, points: [{ usedPercent: 999 }], error: null }
          return { accountId: state.accounts[0]?.id, points: [], error: 'Could not read saved history.' }
        }
        return null
      } }, event: { listen: async () => () => {} },
    } })
  }, state)
  await page.goto('/')
  await page.getByRole('button', { name: 'History', exact: true }).click()
  const history = page.getByRole('region', { name: 'Usage history' })
  await expect(history.getByText('Could not load history.')).toBeVisible()
  await history.getByRole('button', { name: 'Try again' }).click()
  await expect(history.getByText('Could not load history.')).toBeVisible()
  await history.getByRole('button', { name: 'Try again' }).click()
  await expect(history.getByText('No history yet.')).toBeVisible()
  await expect(history.getByText('Could not read saved history.')).toBeVisible()
  await expect(history.getByRole('slider')).toHaveCount(0)
})

test('open history follows snapshots and ignores late replies after refresh or closing', async ({ page }) => {
  const state = createDemoState(null)
  state.accounts = state.accounts.slice(0, 1)
  await page.addInitScript(state => {
    const pending: ((value: unknown) => void)[] = []
    window.addEventListener('history-reply', event => {
      if (event instanceof CustomEvent) pending[event.detail.index]?.(event.detail.payload)
    })
    Object.defineProperty(globalThis, '__TAURI__', { value: {
      core: { invoke: async (command: string) => {
        if (command === 'get_state') return state
        if (command === 'get_account_history') return new Promise(resolve => {
          pending.push(resolve)
          document.documentElement.dataset.historyRequests = String(pending.length)
        })
        return null
      } }, event: { listen: async (name: string, handler: (event: { payload: unknown }) => void) => {
        const listener = (event: Event): void => { if (event instanceof CustomEvent) handler({ payload: event.detail }) }
        window.addEventListener(name, listener)
        return () => window.removeEventListener(name, listener)
      } },
    } })
  }, state)
  await page.goto('/')
  await page.getByRole('button', { name: 'History', exact: true }).click()
  const history = page.getByRole('region', { name: 'Usage history' })
  await expect(history.getByText('Loading history…')).toBeVisible()
  const account = state.accounts[0]
  if (!account?.snapshot) throw new Error('Missing fixture')
  account.snapshot.fetchedAt = new Date(Date.now() + 1000).toISOString()
  state.revision++
  await page.evaluate(state => window.dispatchEvent(new CustomEvent('state_changed', { detail: state })), state)
  await expect(page.locator('html')).toHaveAttribute('data-history-requests', '2')
  await page.evaluate(accountId => window.dispatchEvent(new CustomEvent('history-reply', { detail: { index: 1, payload: { accountId, points: [], error: null } } })), account.id)
  await expect(history.getByText('No history yet.')).toBeVisible()
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('history-reply', { detail: { index: 0, payload: null } })))
  await expect(history.getByText('No history yet.')).toBeVisible()
  await page.getByRole('button', { name: 'Hide history', exact: true }).click()
  await page.getByRole('button', { name: 'History', exact: true }).click()
  await expect(history.getByText('No history yet.')).toBeVisible()
  await expect(page.locator('html')).toHaveAttribute('data-history-requests', '2')
  await page.getByRole('button', { name: 'Hide history', exact: true }).click()
  await expect(history).toHaveCount(0)
})

for (const theme of ['light', 'dark']) {
  test(`${theme} pace line shows on pace and runs out for session and weekly only`, async ({ page }, info) => {
    await page.goto(`/?demo=1&theme=${theme}`)
    const pace = (account: string, label: string) => page.getByRole('article', { name: account }).locator('.quota-window').filter({ has: page.locator('.quota-label', { hasText: new RegExp(`^${label}$`) }) }).locator('.quota-pace')
    await expect(pace('Personal, Claude', 'Session')).toHaveText('On pace')
    await expect(pace('Personal, Claude', 'Weekly')).toHaveText(/^Runs out ~\w{3} .+ · resets \w{3} .+$/)
    await expect(pace('Studio, Claude', 'Session')).toHaveText(/^Runs out ~\d{1,2}:\d{2}(\s?[AP]M)? · resets \d{1,2}:\d{2}(\s?[AP]M)?$/)
    await expect(pace('Studio, Claude', 'Weekly')).toHaveText('On pace')
    await expect(pace('Work, Codex', 'Session')).toHaveText('On pace')
    await expect(pace('Work, Codex', 'Weekly')).toHaveText('On pace')
    const personal = page.getByRole('article', { name: 'Personal, Claude' })
    await personal.getByRole('button', { name: '1 more window' }).click()
    await expect(pace('Personal, Claude', 'Sonnet')).toHaveCount(0)
    await expect(personal.locator('.quota-window')).toHaveCount(3)
    const numerals = await pace('Personal, Claude', 'Session').evaluate(element => getComputedStyle(element).fontVariantNumeric)
    expect(numerals).toContain('tabular-nums')
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
    await page.screenshot({ path: info.outputPath(`pace-${theme}.png`) })
    await page.getByRole('article', { name: 'Studio, Claude' }).screenshot({ path: info.outputPath(`pace-studio-${theme}.png`) })
  })
}

test('pace line stays hidden with under 30 minutes of history', async ({ page }) => {
  const state = createDemoState(null)
  state.accounts = state.accounts.slice(0, 1)
  const account = state.accounts[0]
  if (!account?.snapshot) throw new Error('Missing fixture')
  const now = Date.parse(account.snapshot.fetchedAt)
  const points = account.snapshot.windows.flatMap(window => [0, 10, 20].map(minutes => ({ windowId: window.id, at: new Date(now - (20 - minutes) * 60_000).toISOString(), usedPercent: Math.max(0, window.usedPercent - 20 + minutes), resetsAt: window.resetsAt, windowSeconds: window.windowSeconds, gapBefore: false })))
  await page.addInitScript(({ state, points }) => {
    Object.defineProperty(globalThis, '__TAURI__', { value: {
      core: { invoke: async (command: string) => command === 'get_state' ? state : command === 'get_account_history' ? { accountId: state.accounts[0]?.id, points, error: null } : null },
      event: { listen: async () => () => {} },
    } })
  }, { state, points })
  await page.goto('/')
  await expect(page.locator('.quota-window')).toHaveCount(2)
  await expect(page.locator('.quota-pace')).toHaveCount(0)
})
