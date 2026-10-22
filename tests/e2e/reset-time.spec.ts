import { expect, test } from '@playwright/test'

const pattern = /^Resets in \d+[dhm]( \d+[hm])? · .*\d{1,2}[:.]\d{2}/

for (const theme of ['light', 'dark']) {
  test(`${theme} session and weekly show the absolute reset time`, async ({ page }, info) => {
    await page.goto(`/?demo=1&theme=${theme}`)
    const account = page.getByRole('article', { name: 'Personal, Claude' })
    for (const label of ['Session', 'Weekly']) {
      const row = account.locator('.quota-window').filter({ has: page.locator('.quota-label', { hasText: new RegExp(`^${label}$`) }) })
      const reset = row.locator('.quota-reset')
      await expect(reset).toHaveText(pattern)
      await expect(row.locator('.quota-reset-at')).toBeVisible()
      expect(await reset.evaluate(node => node.scrollWidth <= node.clientWidth)).toBe(true)
    }
    await info.attach(`reset-time-${theme}`, { body: await account.screenshot(), contentType: 'image/png' })
  })
}

for (const width of [360, 420]) {
  test(`reset time fits without overflow at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 660 })
    await page.goto('/?demo=1')
    await expect(page.getByRole('article', { name: 'Personal, Claude' }).locator('.quota-reset').first()).toHaveText(/^Resets in/)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true)
    const fits = await page.locator('.quota-reset').evaluateAll(nodes => nodes.every(node => node.scrollWidth <= node.clientWidth && node.getClientRects().length === 1))
    expect(fits).toBe(true)
  })
}

test('narrow rows drop the absolute time instead of wrapping', async ({ page }) => {
  await page.goto('/?demo=1')
  const row = page.getByRole('article', { name: 'Personal, Claude' }).locator('.quota-window').first()
  await expect(row.locator('.quota-reset-at')).toBeVisible()
  await row.evaluate(node => { (node as HTMLElement).style.width = '220px' })
  await expect(row.locator('.quota-reset-at')).toBeHidden()
  await expect(row.locator('.quota-reset')).toHaveText(/^Resets in \d/)
})
