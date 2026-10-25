import { defineConfig, devices } from '@playwright/test'

const port = Number(process.env.MARGIN_E2E_PORT ?? 1420)
const origin = `http://127.0.0.1:${port}`

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL: origin,
    viewport: { width: 420, height: 660 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    contextOptions: { reducedMotion: 'reduce' },
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 420, height: 660 }, deviceScaleFactor: 1.25 } },
    { name: 'webkit', use: { ...devices['Desktop Safari'], viewport: { width: 420, height: 660 }, deviceScaleFactor: 2 } },
  ],
  webServer: {
    command: `pnpm dev --port ${port}`,
    url: origin,
    reuseExistingServer: !process.env.CI,
    timeout: 30000,
  },
})
