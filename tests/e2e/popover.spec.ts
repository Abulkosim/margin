import { expect, test } from '@playwright/test'

test('browser entry never pretends to read real accounts', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'Track your usage.' })).toBeVisible()
  await expect(page.getByRole('article')).toHaveCount(0)
  await page.getByRole('link', { name: /Preview with sample data/ }).click()
  await expect(page.getByText('Sample data', { exact: true })).toBeVisible()
})

for (const theme of ['dark', 'light']) {
  test(`${theme} overview fits a tray-sized window`, async ({ page }, info) => {
    const errors: string[] = []
    page.on('pageerror', error => errors.push(error.message))
    await page.goto(`/?demo=1&theme=${theme}`)
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
    await expect(page.getByRole('article')).toHaveCount(3)
    await expect(page.getByRole('heading', { name: 'Usage remaining' })).toBeVisible()
    await expect(page.getByText('Sample data', { exact: true })).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
    await info.attach(`overview-${theme}`, { body: await page.screenshot(), contentType: 'image/png' })
    expect(errors).toEqual([])
  })
}

test('provider filters and account details are interactive', async ({ page }) => {
  await page.goto('/?demo=1')
  await page.getByRole('button', { name: /^Codex/ }).click()
  await expect(page.getByRole('article')).toHaveCount(1)
  await expect(page.getByRole('heading', { name: 'Work', exact: true })).toBeVisible()
  await page.getByRole('button', { name: /^All/ }).click()
  const personal = page.getByRole('article', { name: 'Personal, Claude' })
  await personal.getByRole('button', { name: '1 more window' }).click()
  await expect(personal.getByText('Sonnet', { exact: true })).toBeVisible()
  await personal.getByRole('button', { name: 'Pin Personal to tray' }).click()
  await expect(personal.getByRole('button', { name: 'Unpin Personal from tray' })).toHaveAttribute('aria-pressed', 'true')
})

test('expired credentials preserve a labeled last-known reading', async ({ page }) => {
  await page.goto('/?demo=1&state=stale')
  const account = page.getByRole('article', { name: 'Personal, Claude' })
  await expect(account.getByText('Last known', { exact: true })).toBeVisible()
  await expect(account.getByText(/Updated 32m ago/)).toBeVisible()
  await expect(account.getByText(/session has expired/)).toBeVisible()
  await expect(account.getByRole('progressbar', { name: 'Session remaining, last known', exact: true })).toHaveAttribute('aria-valuenow', '72')
  await expect(account.getByRole('progressbar', { name: 'Session remaining, last known', exact: true })).toHaveAttribute('aria-valuetext', '72 percent remaining, last known')
})

test('signed-out accounts keep the last reading and offer the sign-in command', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: async (text: string) => { (window as unknown as { copied: string }).copied = text } } })
  })
  await page.goto('/?demo=1&state=signed-out')
  const claude = page.getByRole('article', { name: 'Personal, Claude' })
  await expect(claude.getByText('Last known', { exact: true })).toBeVisible()
  await expect(claude.getByText('Signed out 2h ago · run claude in ~/.claude')).toBeVisible()
  await expect(claude.getByRole('progressbar', { name: 'Session remaining, last known', exact: true })).toHaveAttribute('aria-valuenow', '72')
  await claude.getByRole('button', { name: 'Copy command: claude', exact: true }).click()
  await expect(claude.getByRole('button', { name: 'Command copied' })).toBeVisible()
  expect(await page.evaluate(() => (window as unknown as { copied: string }).copied)).toBe('claude')
  const codex = page.getByRole('article', { name: 'Work, Codex' })
  await expect(codex.getByText('Signed out · run codex login in ~/.codex-work')).toBeVisible()
  await expect(codex.locator('[role="progressbar"]')).toHaveCount(0)
  await expect(codex.getByRole('button', { name: 'Copy command: CODEX_HOME=~/.codex-work codex login', exact: true })).toBeVisible()
  await expect(codex.getByRole('button', { name: /Try again/ })).toBeVisible()
})

test('rate-limited and switched accounts retry without asking for attention', async ({ page }, info) => {
  await page.goto('/?demo=1&state=retrying&theme=dark')
  const limited = page.getByRole('article', { name: 'Personal, Claude' })
  await expect(limited.getByRole('status').filter({ hasText: /^Rate limited · retrying in [34]m$/ })).toBeVisible()
  await expect(limited.getByText('Last known', { exact: true })).toHaveCount(0)
  await expect(limited.getByRole('progressbar', { name: 'Session remaining', exact: true })).toHaveAttribute('aria-valuenow', '72')
  const changed = page.getByRole('article', { name: 'Work, Codex' })
  await expect(changed.getByText('Retrying…', { exact: true })).toBeVisible()
  await expect(changed.getByText('Last known', { exact: true })).toHaveCount(0)
  const old = page.getByRole('article', { name: 'Studio, Claude' })
  await expect(old.getByText('Last known', { exact: true })).toBeVisible()
  await expect(old.getByText(/Updated 32m ago/)).toBeVisible()
  await expect(old.getByText(/retrying in/)).toHaveCount(0)
  await expect(page.getByText('1 account needs attention', { exact: true })).toBeVisible()
  await info.attach('retrying-dark', { body: await page.screenshot(), contentType: 'image/png' })
})

test('account without a reading shows no invented percentage', async ({ page }) => {
  await page.goto('/?demo=1&state=error')
  const account = page.getByRole('article', { name: 'Personal, Claude' })
  await expect(account.getByText('Usage unavailable', { exact: true })).toBeVisible()
  await expect(account.locator('[role="progressbar"]')).toHaveCount(0)
})

test('empty-state discovery is explicitly a preview', async ({ page }) => {
  await page.goto('/?demo=1&state=empty')
  await expect(page.getByRole('heading', { name: 'Add your accounts' })).toBeVisible()
  await page.getByRole('button', { name: 'Find accounts' }).click()
  await expect(page.getByRole('article')).toHaveCount(3)
  await expect(page.getByText('Sample accounts found.')).toBeVisible()
})

test('settings save theme and notification preferences', async ({ page }, info) => {
  await page.goto('/?demo=1&theme=dark')
  await page.getByRole('button', { name: 'Open settings' }).click()
  await page.getByRole('button', { name: 'Light', exact: true }).click()
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light')
  await page.getByRole('switch', { name: 'Usage alerts' }).click()
  await expect(page.getByRole('switch', { name: 'Usage alerts' })).toHaveAttribute('aria-checked', 'true')
  await page.getByRole('combobox', { name: 'Refresh while open', exact: true }).click()
  await page.getByRole('option', { name: '2 minutes', exact: true }).click()
  await expect(page.getByRole('combobox', { name: 'Refresh while open', exact: true })).toContainText('2 minutes')
  await info.attach('settings-light', { body: await page.screenshot(), contentType: 'image/png' })
})

test('removing an account requires the in-app confirmation', async ({ page }) => {
  await page.goto('/?demo=1')
  await page.getByRole('button', { name: 'Open settings' }).click()
  await page.getByRole('tab', { name: /^Accounts/ }).click()
  await page.getByRole('button', { name: 'Remove Personal', exact: true }).click()
  const dialog = page.getByRole('alertdialog')
  await expect(dialog).toBeVisible()
  await expect(page.getByRole('button', { name: 'Keep account' })).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(dialog).toHaveCount(0)
  await page.getByRole('button', { name: 'Remove Personal', exact: true }).click()
  await dialog.getByRole('button', { name: 'Remove account', exact: true }).click()
  await expect(dialog).toHaveCount(0)
  await expect(page.getByRole('heading', { name: 'Personal', exact: true })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Add account', exact: true })).toBeFocused()
})

test('small displays retain navigation without horizontal overflow', async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 480 })
  await page.goto('/?demo=1')
  await expect(page.getByRole('button', { name: 'Open settings' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Add account', exact: true })).toBeVisible()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
})

test('unreadable settings offer a backed-up reset', async ({ page }, info) => {
  await page.goto('/?demo=1&state=recovery')
  const banner = page.getByRole('alert').filter({ hasText: 'Settings file couldn’t be read. Reset keeps a backup next to it.' })
  await expect(banner).toBeVisible()
  await expect(banner.getByText(/state\.json/)).toBeVisible()
  await info.attach('recovery', { body: await page.screenshot(), contentType: 'image/png' })
  await banner.getByRole('button', { name: 'Back up and reset settings' }).click()
  await expect(banner).toHaveCount(0)
  await expect(page.getByRole('article')).toHaveCount(3)
  await expect(page.getByText('Settings reset. The old file was kept as a backup.')).toBeVisible()
})

test('skipped accounts are noted in settings', async ({ page }) => {
  await page.goto('/?demo=1&state=recovered')
  await page.getByRole('button', { name: 'Open settings' }).click()
  await expect(page.getByRole('status').filter({ hasText: 'Skipped account “Old work” because its saved data couldn’t be read. A backup is kept next to the settings file.' })).toBeVisible()
})
