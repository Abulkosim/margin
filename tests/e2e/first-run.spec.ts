import { expect, test, type Page } from '@playwright/test'
import { createDemoState } from '../../src/lib/demo.ts'

const prompt = (page: Page) => page.getByRole('complementary', { name: 'Start at login' })
const warning = 'Move Margin to Applications so Start at login keeps working.'

declare global {
  interface Window { __FIRST_RUN__?: { saved: { autostart: boolean; autostartPromptDismissed: boolean }[]; opened: number; failSave: boolean } }
}

/** A native stand-in whose settings survive a reload, like the real settings file. */
async function bridge(page: Page, { warn = false, settings = {} }: { warn?: boolean; settings?: Record<string, unknown> } = {}): Promise<void> {
  const state = createDemoState(null)
  state.settings = { ...state.settings, autostartPromptDismissed: false, ...settings }
  await page.addInitScript(({ state, warn }) => {
    const stored = localStorage.getItem('settings')
    let revision = Number(localStorage.getItem('revision') ?? 0)
    const current = { ...state, revision, settings: stored ? JSON.parse(stored) : state.settings }
    const test: NonNullable<Window['__FIRST_RUN__']> = window.__FIRST_RUN__ = { saved: [], opened: 0, failSave: false }
    window.__TAURI__ = {
      core: { async invoke(command, args) {
        if (command === 'get_state') return current
        if (command === 'get_popover_placement') return { revision: 0, arrowX: null }
        if (command === 'get_update_status') return { revision: 0, state: { kind: 'idle' } }
        if (command === 'get_install_warning') return warn
        if (command === 'get_problem_report') return { version: '9.9.9', system: 'macOS 15.1', diagnostics: 'Claude accounts: 2\nClaude errors: network\nCodex accounts: 1\nCodex errors: none' }
        if (command === 'open_problem_report') { test.opened++; return null }
        if (command === 'save_settings') {
          if (test.failSave) throw 'Could not update Start at login.'
          const settings = (args as { settings: typeof current.settings }).settings
          test.saved.push({ autostart: settings.autostart, autostartPromptDismissed: settings.autostartPromptDismissed })
          current.settings = settings
          current.revision = ++revision
          localStorage.setItem('settings', JSON.stringify(settings))
          localStorage.setItem('revision', String(revision))
        }
        return undefined
      } },
      event: { async listen() { return () => {} } },
    }
  }, { state, warn })
}

const saved = (page: Page) => page.evaluate(() => window.__FIRST_RUN__?.saved)

test('sample data does not show the prompt unless asked', async ({ page }) => {
  await page.goto('/?demo=1')
  await expect(page.getByRole('heading', { name: 'Personal' })).toBeVisible()
  await expect(prompt(page)).toHaveCount(0)
})

test('the prompt appears only once an account exists', async ({ page }) => {
  await page.goto('/?demo=1&state=empty&prompt=1')
  await expect(page.getByRole('heading', { name: 'Add your accounts' })).toBeVisible()
  await expect(prompt(page)).toHaveCount(0)
  await page.getByRole('button', { name: 'Find accounts' }).click()
  await expect(prompt(page)).toBeVisible()
})

test('Turn on saves autostart and the prompt stays gone after a reload', async ({ page }) => {
  await bridge(page)
  await page.goto('/')
  await expect(prompt(page)).toBeVisible()
  await prompt(page).getByRole('button', { name: 'Turn on' }).click()
  await expect(prompt(page)).toHaveCount(0)
  expect(await saved(page)).toEqual([{ autostart: true, autostartPromptDismissed: true }])
  await page.reload()
  await expect(page.getByRole('heading', { name: 'Personal' })).toBeVisible()
  await expect(prompt(page)).toHaveCount(0)
})

test('Not now leaves autostart off and the prompt stays gone after a reload', async ({ page }) => {
  await bridge(page)
  await page.goto('/')
  await prompt(page).getByRole('button', { name: 'Not now' }).click()
  await expect(prompt(page)).toHaveCount(0)
  expect(await saved(page)).toEqual([{ autostart: false, autostartPromptDismissed: true }])
  await page.reload()
  await expect(page.getByRole('heading', { name: 'Personal' })).toBeVisible()
  await expect(prompt(page)).toHaveCount(0)
})

test('the prompt stays when turning on fails', async ({ page }) => {
  await bridge(page)
  await page.goto('/')
  await page.evaluate(() => { if (window.__FIRST_RUN__) window.__FIRST_RUN__.failSave = true })
  await prompt(page).getByRole('button', { name: 'Turn on' }).click()
  await expect(page.getByRole('alert').filter({ hasText: 'Could not update Start at login.' })).toBeVisible()
  await expect(prompt(page)).toBeVisible()
})

test('the prompt does not return once autostart was set in Settings', async ({ page }) => {
  await bridge(page)
  await page.goto('/')
  await page.getByRole('button', { name: 'Open settings' }).click()
  const toggle = page.getByRole('switch', { name: 'Start at login' })
  await toggle.click()
  await expect(toggle).toBeChecked()
  await toggle.click()
  await expect(toggle).not.toBeChecked()
  await page.getByRole('button', { name: 'Back to overview' }).click()
  await expect(page.getByRole('heading', { name: 'Personal' })).toBeVisible()
  await expect(prompt(page)).toHaveCount(0)
})

test('people upgrading with autostart on never see the prompt', async ({ page }) => {
  await bridge(page, { settings: { autostart: true } })
  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'Personal' })).toBeVisible()
  await expect(prompt(page)).toHaveCount(0)
})

test('a copy outside Applications warns at the prompt and in Settings', async ({ page }) => {
  await bridge(page, { warn: true })
  await page.goto('/')
  await expect(prompt(page).getByText(warning)).toBeVisible()
  await prompt(page).getByRole('button', { name: 'Not now' }).click()
  await expect(page.getByText(warning)).toHaveCount(0)
  await page.getByRole('button', { name: 'Open settings' }).click()
  await expect(page.getByText(warning)).toBeVisible()
})

test('an installed copy shows no warning', async ({ page }) => {
  await bridge(page)
  await page.goto('/')
  await expect(prompt(page)).toBeVisible()
  await expect(page.getByText(warning)).toHaveCount(0)
  await page.getByRole('button', { name: 'Open settings' }).click()
  await expect(page.getByRole('switch', { name: 'Start at login' })).toBeVisible()
  await expect(page.getByText(warning)).toHaveCount(0)
})

test('the window fits again when the prompt goes away', async ({ page }) => {
  await page.goto('/?demo=1&state=single&prompt=1&location=outside')
  const shell = page.locator('.app-shell')
  await expect(prompt(page)).toBeVisible()
  await expect.poll(async () => Number(await shell.getAttribute('data-fit-height'))).toBeGreaterThan(0)
  const before = Number(await shell.getAttribute('data-fit-height'))
  await prompt(page).getByRole('button', { name: 'Not now' }).click()
  await expect.poll(async () => Number(await shell.getAttribute('data-fit-height'))).toBeLessThan(before)
})

for (const scenario of ['single', 'compact']) {
  test(`the prompt leaves accounts reachable with ${scenario} accounts`, async ({ page }) => {
    await page.goto(`/?demo=1&state=${scenario}&layout=cards&prompt=1&location=outside`)
    await expect(prompt(page).getByText(warning)).toBeVisible()
    const box = await prompt(page).boundingBox()
    const footer = await page.locator('.app-footer').boundingBox()
    const list = await page.locator('.accounts-scroll').boundingBox()
    expect(box && footer && box.y + box.height <= footer.y + 1).toBe(true)
    expect(box && list && list.height >= 150 && list.y + list.height <= box.y + 1).toBe(true)
    await expect(page.locator('html')).toHaveJSProperty('scrollWidth', await page.evaluate(() => document.documentElement.clientWidth))
  })
}

test('Report a problem previews the details and opens only on click', async ({ page }) => {
  await bridge(page)
  await page.goto('/')
  await page.getByRole('button', { name: 'Open settings' }).click()
  await page.getByRole('button', { name: 'Report a problem' }).click()
  const preview = page.getByRole('group', { name: 'Report preview' })
  await expect(preview.getByText('9.9.9')).toBeVisible()
  await expect(preview.getByText('macOS 15.1')).toBeVisible()
  await expect(preview.getByTestId('report-diagnostics')).toHaveText('Claude accounts: 2\nClaude errors: network\nCodex accounts: 1\nCodex errors: none', { useInnerText: true })
  expect(await page.evaluate(() => window.__FIRST_RUN__?.opened)).toBe(0)
  await preview.getByRole('button', { name: 'Cancel' }).click()
  await expect(preview).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Report a problem' })).toBeFocused()
  expect(await page.evaluate(() => window.__FIRST_RUN__?.opened)).toBe(0)
  await page.getByRole('button', { name: 'Report a problem' }).click()
  await preview.getByRole('button', { name: 'Open GitHub issue' }).click()
  await expect(preview).toHaveCount(0)
  expect(await page.evaluate(() => window.__FIRST_RUN__?.opened)).toBe(1)
})

test('the report preview works without the desktop app and shows no account details', async ({ page }) => {
  const opened: string[] = []
  page.on('popup', popup => opened.push(popup.url()))
  await page.goto('/?demo=1&state=compact')
  await page.getByRole('button', { name: 'Open settings' }).click()
  await page.getByRole('button', { name: 'Report a problem' }).click()
  const preview = page.getByRole('group', { name: 'Report preview' })
  await expect(preview.getByTestId('report-diagnostics')).toHaveText('Claude accounts: 3\nClaude errors: signedOut\nCodex accounts: 2\nCodex errors: network', { useInnerText: true })
  const text = await preview.innerText()
  for (const secret of ['alex@', '~/.claude', '~/.codex', 'Personal', 'Studio', 'Northwind', 'Run claude']) expect(text).not.toContain(secret)
  await preview.getByRole('button', { name: 'Open GitHub issue' }).click()
  await expect(page.getByRole('status').filter({ hasText: 'Preview only.' })).toBeVisible()
  expect(opened).toEqual([])
})
