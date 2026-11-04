import { expect, test, type Locator, type Page } from '@playwright/test'

const card = (page: Page, name: string): Locator => page.getByRole('article', { name })
const quota = (account: Locator, label: string): Locator => account.locator('.quota-window').filter({ has: account.page().locator('.quota-label', { hasText: new RegExp(`^${label}$`) }) })
const row = (page: Page, label: string): Locator => page.locator('.compact-row').filter({ has: page.locator('.compact-label', { hasText: label }) })

test('an expired token reads as idle and offers the command for a new reading', async ({ page }) => {
  await page.goto('/?demo=1&state=idle')
  const personal = card(page, 'Personal, Claude')
  await expect(personal.locator('.stale-badge')).toHaveText('Idle')
  await expect(personal.locator('.idle-note')).toHaveText('Idle, last read 3h ago. To get a new reading, run:')
  await expect(personal.getByRole('button', { name: 'Copy command: claude', exact: true })).toBeVisible()
  await expect(personal.getByText(/signed out/i)).toHaveCount(0)
})

test('an expired token with no reading says so plainly', async ({ page }) => {
  await page.goto('/?demo=1&state=idle')
  const studio = card(page, 'Studio, Claude')
  await expect(studio.getByText('Idle, no reading yet', { exact: true })).toBeVisible()
  await expect(studio.getByText('To get a first reading, run:', { exact: true })).toBeVisible()
  await expect(studio.getByRole('button', { name: 'Copy command: CLAUDE_CONFIG_DIR=~/.claude-studio claude', exact: true })).toBeVisible()
  await expect(studio.getByRole('button', { name: /Check again/ })).toBeVisible()
  await expect(studio.locator('[role="progressbar"]')).toHaveCount(0)
  await expect(studio.getByText(/signed out|unavailable/i)).toHaveCount(0)
})

test('a window past its reset shows no percentage and never a guess', async ({ page }) => {
  await page.goto('/?demo=1&state=idle')
  const personal = card(page, 'Personal, Claude')
  const session = quota(personal, 'Session')
  await expect(session.locator('.quota-reset')).toHaveText(/^Reset .*\d{1,2}[:.]\d{2}.*, not read since$/)
  await expect(session.locator('.quota-value')).toHaveCount(0)
  await expect(session.locator('[role="progressbar"]')).toHaveCount(0)
  await expect(session.locator('.quota-fill')).toHaveCount(0)
  expect(await session.textContent()).not.toMatch(/%|100/)
  await expect(personal.getByRole('progressbar', { name: 'Weekly remaining, last known', exact: true })).toHaveAttribute('aria-valuenow', '42')
  const sonnet = personal.locator('.model-chip').filter({ hasText: 'Sonnet' })
  await expect(sonnet.locator('.model-chip-value')).toHaveText('–')
  await expect(sonnet).toHaveAttribute('aria-label', /^Sonnet weekly: Reset .*, not read since$/)
  await expect(sonnet).not.toHaveAttribute('role', 'progressbar')
})

test('a window past its reset is unread on an account that is not idle too', async ({ page }) => {
  await page.goto('/?demo=1&state=idle')
  const work = card(page, 'Work, Codex')
  await expect(work.locator('.stale-badge')).toHaveText('Last known')
  await expect(quota(work, 'Session').locator('.quota-reset')).toHaveText(/, not read since$/)
  await expect(quota(work, 'Session').locator('.quota-value')).toHaveCount(0)
  await expect(work.getByText(/Updated 4h ago/)).toBeVisible()
})

test('idle accounts do not count as needing attention', async ({ page }) => {
  await page.goto('/?demo=1&state=idle')
  await expect(page.getByRole('button', { name: '1 account needs attention', exact: true })).toBeVisible()
})

test('compact rows carry the same states', async ({ page }) => {
  await page.goto('/?demo=1&state=idle&layout=compact')
  const personal = row(page, 'Personal')
  await expect(personal.locator('.compact-status')).toHaveText('Idle, read 3h ago')
  await expect(personal.locator('.compact-status')).toHaveAttribute('title', 'Idle, last read 3h ago')
  await expect(personal.locator('.compact-value')).toHaveText(['–', '42%'])
  await expect(personal.locator('.compact-value').first()).toHaveAttribute('title', /^Reset .*, not read since$/)
  await expect(personal.locator('.compact-value').nth(1)).toHaveClass(/limiting/)
  await expect(personal).toHaveAttribute('aria-label', /session reset .*, not read since, weekly 42 percent left/)
  await expect(personal).toHaveAttribute('aria-label', /idle, last read 3h ago/)
  expect(await personal.locator('.compact-status').evaluate(node => node.scrollWidth <= node.clientWidth)).toBe(true)
  const studio = row(page, 'Studio')
  await expect(studio.locator('.compact-status')).toHaveText('Idle, no reading yet')
  await expect(studio.locator('.compact-value')).toHaveCount(0)
  await expect(row(page, 'Work').locator('.compact-status')).toHaveText('Last known')
})

test('a rejected sign-in stays signed out next to idle wording', async ({ page }) => {
  await page.goto('/?demo=1&state=signed-out')
  const claude = card(page, 'Personal, Claude')
  await expect(claude.locator('.stale-badge')).toHaveText('Last known')
  await expect(claude.locator('.stale-note')).toContainText('Signed out. Run:')
  await expect(claude.locator('.idle-note')).toHaveCount(0)
})

test('an incomplete reading is marked on the card and the compact row', async ({ page }) => {
  await page.goto('/?demo=1&state=incomplete')
  const personal = card(page, 'Personal, Claude')
  await expect(personal.getByText('Incomplete: some limits couldn’t be read.', { exact: true })).toBeVisible()
  await expect(personal.getByRole('progressbar', { name: 'Session remaining', exact: true })).toBeVisible()
  await expect(personal.locator('.stale-badge')).toHaveCount(0)
  await expect(card(page, 'Work, Codex').getByText(/Incomplete/)).toHaveCount(0)
  await page.goto('/?demo=1&state=incomplete&layout=compact')
  await expect(row(page, 'Personal').locator('.compact-status')).toHaveText('Incomplete')
  await expect(row(page, 'Work').locator('.compact-status')).toHaveCount(0)
})

for (const theme of ['light', 'dark']) {
  for (const [scenario, layout] of [['idle', 'cards'], ['idle', 'compact'], ['incomplete', 'cards']] as const) {
    test(`${scenario} ${layout} ${theme} fits without overflow`, async ({ page }, info) => {
      await page.goto(`/?demo=1&state=${scenario}&layout=${layout}&theme=${theme}`)
      await page.waitForTimeout(300)
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
      await page.screenshot({ path: info.outputPath(`${scenario}-${layout}-${theme}.png`) })
      for (const [index, account] of (await page.locator('.account-card').all()).entries()) {
        await account.screenshot({ path: info.outputPath(`${scenario}-${theme}-card-${index}.png`) })
      }
    })
  }
}
