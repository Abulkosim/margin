import { mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { expect, test, type Page, type TestInfo } from '@playwright/test'
import { createDemoState } from '../../src/lib/demo.ts'

declare global {
  interface Window { __SWITCH_TEST__?: { commands: { command: string; args?: Record<string, unknown> }[] } }
}

async function openAccounts(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Open settings' }).click()
  await page.getByRole('tab', { name: /^Accounts/ }).click()
}

async function shot(page: Page, info: TestInfo, name: string, target = page.locator('.app-shell')): Promise<void> {
  const path = info.outputPath(`${name}.png`)
  await target.screenshot({ path })
  const extra = process.env.MARGIN_SHOTS
  if (extra) {
    mkdirSync(extra, { recursive: true })
    await target.screenshot({ path: join(extra, `${info.project.name}-${name}.png`) })
  }
}

const row = (page: Page, label: string) => page.locator('.managed-account').filter({ has: page.getByRole('heading', { name: label, exact: true }) })

test('the default-slot account carries an Active tag', async ({ page }) => {
  await page.goto('/?demo=1')
  const personal = page.locator('.account-card[data-account-id="demo-claude-personal"]')
  await expect(personal.locator('.active-tag')).toHaveText('Active')
  await expect(page.locator('.account-card[data-account-id="demo-claude-studio"] .active-tag')).toHaveCount(0)
  await expect(page.locator('.active-tag')).toHaveCount(1)
})

test('make active swaps, confirms, and undoes', async ({ page }) => {
  await page.goto('/?demo=1')
  await openAccounts(page)
  await expect(row(page, 'Personal').locator('.active-tag')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Make Personal active', exact: true })).toHaveCount(0)
  const trigger = page.getByRole('button', { name: 'Make Studio active', exact: true })
  await trigger.click()
  const dialog = page.getByRole('alertdialog')
  await expect(dialog.getByRole('heading', { name: 'Make hello@studio.example active?' })).toBeVisible()
  await expect(dialog).toContainText('claude will use hello@studio.example. alex@example.com moves to ~/.claude-studio.')
  await expect(dialog.getByRole('button', { name: 'Cancel', exact: true })).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(dialog).toHaveCount(0)
  await expect(trigger).toBeFocused()
  await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible()

  await trigger.click()
  await dialog.getByRole('button', { name: 'Make active', exact: true }).click()
  await expect(dialog).toHaveCount(0)
  const toast = page.getByRole('status').filter({ hasText: 'hello@studio.example is active.' })
  await expect(toast).toBeVisible()
  await expect(row(page, 'Studio').locator('.active-tag')).toBeVisible()
  await expect(row(page, 'Studio')).toContainText('~/.claude')
  await expect(row(page, 'Personal')).toContainText('~/.claude-studio')
  await expect(row(page, 'Personal').locator('.active-tag')).toHaveCount(0)

  await toast.getByRole('button', { name: 'Undo', exact: true }).click()
  await expect(page.getByText('alex@example.com is active.', { exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Undo', exact: true })).toHaveCount(0)
  await expect(row(page, 'Personal').locator('.active-tag')).toBeVisible()
  await expect(row(page, 'Studio').locator('.active-tag')).toHaveCount(0)
})

test('moving into an empty default slot warns that the folder signs out and has no undo', async ({ page }) => {
  await page.goto('/?demo=1')
  await openAccounts(page)
  await page.getByRole('button', { name: 'Make Work active', exact: true }).click()
  const dialog = page.getByRole('alertdialog')
  await expect(dialog).toContainText('codex will use alex@studio.example. ~/.codex-work will be signed out.')
  await dialog.getByRole('button', { name: 'Make active', exact: true }).click()
  await expect(page.getByText('alex@studio.example is active. Reopen the Codex app to use it.', { exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Undo', exact: true })).toHaveCount(0)
  await expect(row(page, 'Work').locator('.active-tag')).toBeVisible()
})

test('a running Codex is refused before any dialog', async ({ page }) => {
  await page.goto('/?demo=1&running=1')
  await openAccounts(page)
  await page.getByRole('button', { name: 'Make Work active', exact: true }).click()
  await expect(page.getByRole('alertdialog')).toHaveCount(0)
  await expect(page.getByRole('alert').filter({ hasText: 'Codex is still running. Quit the Codex app and any Codex CLI or editor sessions, then try again.' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Switch anyway' })).toHaveCount(0)
  await expect(row(page, 'Work').locator('.active-tag')).toHaveCount(0)
})

test('a running Claude Code gets a plain warning', async ({ page }) => {
  await page.goto('/?demo=1&running=1')
  await openAccounts(page)
  await page.getByRole('button', { name: 'Make Studio active', exact: true }).click()
  const dialog = page.getByRole('alertdialog')
  await expect(dialog).toContainText('Claude Code is running. Open sessions switch within a minute. Restart them to be sure.')
  await expect(dialog.getByRole('button', { name: 'Make active', exact: true })).toBeVisible()
})

test('a signed-out folder is refused with its sign-in command', async ({ page }) => {
  await page.goto('/?demo=1&state=signed-out')
  await openAccounts(page)
  await page.getByRole('button', { name: 'Make Work active', exact: true }).click()
  await expect(page.getByRole('alertdialog')).toHaveCount(0)
  await expect(page.getByRole('alert').filter({ hasText: 'Work is signed out. Run CODEX_HOME=~/.codex-work codex login, then try again.' })).toBeVisible()
})

test('a failed switch offers its backup in Settings', async ({ page }) => {
  await page.goto('/?demo=1&state=switch-restore')
  await openAccounts(page)
  const notice = page.locator('.switch-restore')
  await expect(notice).toContainText('Claude Code changed the sign-in during the switch.')
  await expect(notice).toContainText('Backups older than a few hours may need sign-in again.')
  await notice.getByRole('button', { name: 'Restore backup', exact: true }).click()
  await expect(page.getByText('Backup restored.', { exact: true })).toBeVisible()
  await expect(notice).toHaveCount(0)
})

test('the desktop flow sends the previewed fingerprint and shows errors plainly', async ({ page }) => {
  const state = createDemoState(null)
  await page.addInitScript(({ state }) => {
    const commands: { command: string; args?: Record<string, unknown> }[] = []
    window.__SWITCH_TEST__ = { commands }
    let attempts = 0
    window.__TAURI__ = {
      core: {
        async invoke(command, args) {
          if (!['get_state', 'get_popover_placement', 'fit_popover'].includes(command)) commands.push({ command, args })
          if (command === 'get_state') return state
          if (command === 'get_popover_placement') return { revision: 0, arrowX: null }
          if (command === 'preview_make_active') return { accountId: args?.id, provider: 'claude', kind: 'swap', fingerprint: 'f'.repeat(64), target: 'hello@studio.example', displaced: 'alex@example.com', folder: '~/.claude-studio', running: false }
          if (command === 'make_active') {
            attempts++
            if (attempts === 1) throw 'Claude Code is busy. Try again in a moment.'
            return { state: { ...state, revision: 5, switching: { activeAccountIds: ['demo-claude-studio'], restore: null } }, target: 'hello@studio.example', previousAccountId: 'demo-claude-personal' }
          }
          return undefined
        },
      },
      event: { async listen() { return () => {} } },
    }
  }, { state })
  await page.goto('/')
  await openAccounts(page)
  const trigger = page.getByRole('button', { name: 'Make Studio active', exact: true })
  await trigger.click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Make active', exact: true }).click()
  await expect(page.getByRole('alert').filter({ hasText: 'Claude Code is busy. Try again in a moment.' })).toBeVisible()
  await trigger.click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Make active', exact: true }).click()
  await expect(page.getByText('hello@studio.example is active.', { exact: true })).toBeVisible()
  await expect(row(page, 'Studio').locator('.active-tag')).toBeVisible()
  const commands = await page.evaluate(() => window.__SWITCH_TEST__?.commands ?? [])
  expect(commands.filter(entry => entry.command === 'make_active').map(entry => entry.args)).toEqual([
    { id: 'demo-claude-studio', fingerprint: 'f'.repeat(64) },
    { id: 'demo-claude-studio', fingerprint: 'f'.repeat(64) },
  ])
})

for (const theme of ['light', 'dark'] as const) {
  test(`${theme} make active screenshots`, async ({ page }, info) => {
    await page.goto(`/?demo=1&theme=${theme}`)
    await shot(page, info, `make-active-overview-${theme}`)
    await openAccounts(page)
    await shot(page, info, `make-active-accounts-${theme}`)
    await page.getByRole('button', { name: 'Make Studio active', exact: true }).click()
    await expect(page.getByRole('alertdialog')).toBeVisible()
    await shot(page, info, `make-active-dialog-${theme}`)
    await page.keyboard.press('Escape')
    await page.goto(`/?demo=1&theme=${theme}&running=1`)
    await openAccounts(page)
    await page.getByRole('button', { name: 'Make Studio active', exact: true }).click()
    await expect(page.getByRole('alertdialog')).toBeVisible()
    await shot(page, info, `make-active-claude-running-${theme}`)
    await page.goto(`/?demo=1&theme=${theme}&state=switch-restore`)
    await openAccounts(page)
    await shot(page, info, `make-active-restore-${theme}`)
  })
}
