import { expect, test } from '@playwright/test'
import { defaultSettings } from '../../src/lib/model.ts'

for (const platform of ['mac', 'windows']) {
  test.describe(`${platform} window frame`, () => {
    test.use({ userAgent: platform === 'mac' ? 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' : 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' })

    for (const theme of ['light', 'dark']) {
      test(`${theme} settings leave the native window perimeter transparent`, async ({ page }, info) => {
        await page.addInitScript(settings => {
          window.__TAURI__ = {
            core: {
              async invoke(command) {
                if (command === 'get_state') return { revision: 0, accounts: [], settings, refreshing: false, notice: null }
                if (command === 'get_popover_placement') return { revision: 0, arrowX: 210 }
                return undefined
              },
            },
            event: { async listen() { return () => {} } },
          }
        }, { ...defaultSettings, theme })
        await page.goto('/')
        await page.getByRole('button', { name: 'Open settings', exact: true }).click()
        await expect(page.getByRole('heading', { name: 'Settings', exact: true })).toBeVisible()
        await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
        const screenshot = await page.screenshot({ omitBackground: true, scale: 'css' })
        await info.attach('transparent-native-frame', { body: screenshot, contentType: 'image/png' })
        const perimeterAlpha = await page.evaluate(async png => {
          const image = new Image()
          image.src = `data:image/png;base64,${png}`
          await image.decode()
          const canvas = document.createElement('canvas')
          canvas.width = image.naturalWidth
          canvas.height = image.naturalHeight
          const context = canvas.getContext('2d')
          if (!context) throw new Error('Canvas context unavailable')
          context.drawImage(image, 0, 0)
          const { data, width, height } = context.getImageData(0, 0, canvas.width, canvas.height)
          let maximumAlpha = 0
          for (let y = 0; y < height; y++) {
            for (let x = 0; x < width; x++) {
              if (x < 5 || x >= width - 5 || y < 5 || y >= height - 5) {
                maximumAlpha = Math.max(maximumAlpha, data[(y * width + x) * 4 + 3] ?? 0)
              }
            }
          }
          return maximumAlpha
        }, screenshot.toString('base64'))
        expect(perimeterAlpha).toBe(0)
      })
    }
  })
}

test('browser preview keeps its presentation shadow', async ({ page }) => {
  await page.goto('/?demo=1')
  await expect(page.locator('.popover')).not.toHaveCSS('box-shadow', 'none')
})
