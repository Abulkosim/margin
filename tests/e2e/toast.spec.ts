import { expect, test } from '@playwright/test'

test('toast live regions stay mounted and hovering or focusing a toast pauses its dismissal', async ({ page }) => {
  await page.clock.install()
  await page.goto('/?demo=1')
  const regions = page.locator('.toast-region')
  await expect(regions.getByRole('status')).toHaveCount(1)
  await expect(regions.getByRole('alert')).toHaveCount(1)
  await expect(regions.getByRole('status')).toBeEmpty()

  await page.getByRole('button', { name: 'Refresh usage' }).click()
  const toast = regions.getByRole('status').filter({ hasText: 'Preview refreshed. Values are sample data.' })
  await expect(toast).toBeVisible()
  await toast.hover()
  await page.clock.runFor(20_000)
  await expect(toast).toBeVisible()
  await page.mouse.move(5, 5)
  await page.clock.runFor(4_000)
  await expect(toast).toBeHidden()

  await page.getByRole('button', { name: 'Refresh usage' }).click()
  await expect(toast).toBeVisible()
  await toast.getByRole('button', { name: 'Dismiss message' }).focus()
  await page.clock.runFor(20_000)
  await expect(toast).toBeVisible()
  await page.getByRole('button', { name: 'Refresh usage' }).focus()
  await page.clock.runFor(4_000)
  await expect(toast).toBeHidden()
})
