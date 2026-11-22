import { mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { expect, test, type Page, type TestInfo } from '@playwright/test'
import { createDemoState } from '../../src/lib/demo.ts'
import type { AppState, MakeActivePreview } from '../../src/lib/model.ts'

// Walks every visible state in both themes and fails on defects a screenshot would show: horizontal
// page scroll, text clipped without an ellipsis, ellipsized text with no way to read it in full,
// overlapping text, text or icons below WCAG contrast, and clipped focus rings.
// Set MARGIN_WALK_DIR to keep the PNGs for review.

type Defect = { kind: 'scroll' | 'clipped' | 'overlap' | 'contrast' | 'icon' | 'truncated'; text: string; detail: string }

async function findDefects(page: Page): Promise<Defect[]> {
  return page.evaluate(() => {
    const defects: { kind: 'scroll' | 'clipped' | 'overlap' | 'contrast' | 'icon' | 'truncated'; text: string; detail: string }[] = []
    if (document.documentElement.scrollWidth > window.innerWidth + 1) defects.push({ kind: 'scroll', text: '', detail: `page ${document.documentElement.scrollWidth}px wide in ${window.innerWidth}px` })

    type Rgba = [number, number, number, number]
    const parse = (value: string): Rgba => {
      const srgb = value.match(/color\(srgb ([^)]+)\)/)?.[1]
      const numbers = srgb
        ? srgb.split(/[\s/]+/).filter(Boolean).map((part, index) => index < 3 ? Number(part) * 255 : Number(part))
        : (value.match(/rgba?\(([^)]+)\)/)?.[1] ?? '0 0 0 0').split(/[\s,/]+/).filter(Boolean).map(Number)
      const [r = 0, g = 0, b = 0, a = 1] = numbers
      return [r, g, b, a]
    }
    const blend = ([r, g, b, a]: Rgba, [r2, g2, b2, a2]: Rgba): Rgba => {
      const alpha = a + a2 * (1 - a)
      if (!alpha) return [0, 0, 0, 0]
      const mix = (top: number, bottom: number): number => (top * a + bottom * a2 * (1 - a)) / alpha
      return [mix(r, r2), mix(g, g2), mix(b, b2), alpha]
    }
    const luminance = ([r, g, b]: Rgba): number => {
      const linear = (value: number): number => {
        const channel = value / 255
        return channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
      }
      return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
    }
    const contrast = (one: Rgba, two: Rgba): number => {
      const [x, y] = [luminance(one), luminance(two)]
      return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05)
    }
    const backgroundOf = (element: Element): Rgba | null => {
      const layers: Rgba[] = []
      for (let node: Element | null = element; node; node = node.parentElement) {
        if (node.classList.contains('preview-stage')) break
        const style = getComputedStyle(node)
        if (style.backgroundImage !== 'none') return null
        const color = parse(style.backgroundColor)
        if (color[3] > 0) layers.push(color)
        if (color[3] >= 1) break
      }
      let result: Rgba = document.documentElement.dataset.theme === 'dark' ? [27, 32, 29, 1] : [245, 246, 241, 1]
      for (const layer of layers.reverse()) result = blend(layer, result)
      return result
    }
    const opacityOf = (element: Element): number => {
      let value = 1
      for (let node: Element | null = element; node; node = node.parentElement) value *= Number(getComputedStyle(node).opacity)
      return value
    }
    const disabled = (element: Element): boolean => Boolean(element.closest('button:disabled, [aria-disabled="true"], .is-disabled, input:disabled'))

    const visuallyHidden = (element: Element): boolean => {
      for (let node: Element | null = element; node; node = node.parentElement) {
        const style = getComputedStyle(node)
        if (style.clip !== 'auto' && style.clip !== '' && style.position === 'absolute') return true
        if (style.clipPath.startsWith('inset(50%')) return true
      }
      return false
    }
    /** Menus, dialogs, tooltips and toasts float over the page; text under them is covered, not colliding. */
    const layerOf = (element: Element): Element | null => element.closest('[role="listbox"], [role="dialog"], [role="alertdialog"], [role="tooltip"], .toast-message, [data-reka-popper-content-wrapper]')
    /** The part of a text box not cut off by scrolling or clipping ancestors. */
    const visiblePart = (element: Element, rect: DOMRect): DOMRect | null => {
      let [left, top, right, bottom] = [rect.left, rect.top, rect.right, rect.bottom]
      for (let node: Element | null = element; node && node !== document.documentElement; node = node.parentElement) {
        const style = getComputedStyle(node)
        if (style.overflowX === 'visible' && style.overflowY === 'visible') continue
        const box = node.getBoundingClientRect()
        left = Math.max(left, box.left); top = Math.max(top, box.top); right = Math.min(right, box.right); bottom = Math.min(bottom, box.bottom)
      }
      return right - left > 0.5 && bottom - top > 0.5 ? new DOMRect(left, top, right - left, bottom - top) : null
    }
    type Item = { node: Text; parent: Element; rects: DOMRect[]; text: string }
    const items: Item[] = []
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT)
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = node.textContent?.trim()
      const parent = node.parentElement
      if (!text || !parent || parent.closest('.sr-only, script, style') || visuallyHidden(parent)) continue
      const style = getComputedStyle(parent)
      if (style.visibility === 'hidden' || opacityOf(parent) < 0.05) continue
      const range = document.createRange()
      range.selectNodeContents(node)
      const rects = [...range.getClientRects()].filter(rect => rect.width > 0.5 && rect.height > 0.5)
      if (!rects.length) continue
      items.push({ node: node as Text, parent, rects, text })
    }
    const visibleOnTop = (item: Item, rect: DOMRect): boolean => {
      const x = rect.left + rect.width / 2
      const y = rect.top + rect.height / 2
      if (x < 0 || y < 0 || x > window.innerWidth || y > window.innerHeight) return false
      const hit = document.elementFromPoint(x, y)
      return Boolean(hit && (item.parent.contains(hit) || hit.contains(item.parent)))
    }

    for (const item of items) {
      for (const rect of item.rects) {
        for (let node: Element | null = item.parent; node && node !== document.body; node = node.parentElement) {
          const style = getComputedStyle(node)
          if (style.overflowX === 'visible' && style.overflowY === 'visible') continue
          const box = node.getBoundingClientRect()
          const horizontal = rect.left < box.left - 1 || rect.right > box.right + 1
          const vertical = (style.overflowY === 'hidden' || style.overflowY === 'clip') && (rect.top < box.top - 2 || rect.bottom > box.bottom + 2)
          if ((horizontal || vertical) && style.textOverflow !== 'ellipsis' && getComputedStyle(item.parent).textOverflow !== 'ellipsis') {
            if (box.width > 2 && box.height > 2) defects.push({ kind: 'clipped', text: item.text.slice(0, 60), detail: `${horizontal ? 'x' : 'y'} in .${[...node.classList].join('.') || node.tagName}` })
          }
          break
        }
      }
    }

    const shown = items.flatMap(item => item.rects.flatMap(rect => {
      const part = visiblePart(item.parent, rect)
      return part && visibleOnTop(item, part) ? [{ item, rect: part }] : []
    }))
    shown.forEach((a, index) => {
      for (const b of shown.slice(index + 1)) {
        if (a.item === b.item || layerOf(a.item.parent) !== layerOf(b.item.parent)) continue
        const width = Math.min(a.rect.right, b.rect.right) - Math.max(a.rect.left, b.rect.left)
        const height = Math.min(a.rect.bottom, b.rect.bottom) - Math.max(a.rect.top, b.rect.top)
        if (width > 2 && height > 4) defects.push({ kind: 'overlap', text: `${a.item.text.slice(0, 40)} / ${b.item.text.slice(0, 40)}`, detail: `${Math.round(width)}x${Math.round(height)}` })
      }
    })

    const checked = new Set<Element>()
    for (const { item } of shown) {
      if (checked.has(item.parent) || disabled(item.parent)) continue
      checked.add(item.parent)
      const style = getComputedStyle(item.parent)
      const background = backgroundOf(item.parent)
      if (!background) continue
      const foreground = parse(style.color)
      foreground[3] *= opacityOf(item.parent)
      const ratio = contrast(blend(foreground, background), background)
      const size = parseFloat(style.fontSize)
      const large = size >= 24 || (size >= 18.66 && Number(style.fontWeight) >= 700)
      if (ratio < (large ? 3 : 4.5)) defects.push({ kind: 'contrast', text: item.text.slice(0, 60), detail: `${ratio.toFixed(2)}:1 at ${size}px` })
    }
    // Text cut with an ellipsis must stay readable in full on hover: a title or a tooltip trigger around it.
    for (const element of document.querySelectorAll('body *')) {
      const style = getComputedStyle(element)
      if (style.textOverflow !== 'ellipsis' || element.scrollWidth <= element.clientWidth + 1 || visuallyHidden(element)) continue
      const text = element.textContent?.trim() ?? ''
      const box = element.getBoundingClientRect()
      if (!text || box.width < 2 || box.bottom < 0 || box.top > window.innerHeight) continue
      const titled = element.closest('[title]')?.getAttribute('title')?.includes(text)
      const tooltip = element.closest('[data-state][aria-describedby], [data-grace-area-trigger], [data-state="closed"][tabindex]')
      if (!titled && !tooltip) defects.push({ kind: 'truncated', text: text.slice(0, 60), detail: `.${[...element.classList].join('.')}` })
    }
    for (const icon of document.querySelectorAll('svg')) {
      const box = icon.getBoundingClientRect()
      if (box.width < 4 || disabled(icon) || visuallyHidden(icon) || opacityOf(icon) < 0.05) continue
      const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2)
      if (!hit || !(icon === hit || icon.contains(hit) || hit.contains(icon))) continue
      const background = backgroundOf(icon.parentElement ?? icon)
      if (!background) continue
      const stroke = parse(getComputedStyle(icon).color)
      stroke[3] *= opacityOf(icon)
      const ratio = contrast(blend(stroke, background), background)
      if (ratio < 3) defects.push({ kind: 'icon', text: icon.closest('[aria-label]')?.getAttribute('aria-label') ?? icon.parentElement?.className.toString() ?? '', detail: `${ratio.toFixed(2)}:1` })
    }
    return defects
  })
}

async function snap(page: Page, info: TestInfo, theme: string, name: string): Promise<void> {
  await page.evaluate(() => document.fonts.ready)
  await page.waitForTimeout(50)
  const dir = process.env.MARGIN_WALK_DIR
  const file = `${info.project.name}-${theme}-${name}.png`
  const path = dir ? (mkdirSync(dir, { recursive: true }), join(dir, file)) : info.outputPath(file)
  await page.screenshot({ path, scale: 'css', animations: 'disabled' })
  const defects = await findDefects(page)
  expect.soft(defects, `${name} in ${theme}`).toEqual([])
}

/** Also captures the end of a scrolling overview or settings list. */
async function snapAll(page: Page, info: TestInfo, theme: string, name: string): Promise<void> {
  await snap(page, info, theme, name)
  const scroller = page.locator('.accounts-scroll, .settings-content[data-state="active"]').first()
  if (!await scroller.count()) return
  const scrolls = await scroller.evaluate(element => element.scrollHeight > element.clientHeight + 4)
  if (!scrolls) return
  await scroller.evaluate(element => { element.scrollTop = element.scrollHeight })
  await snap(page, info, theme, `${name}-end`)
}

const longLabel = 'Client work for Northwind Engineering Studio 24'
const longEmail = 'alexandra.konstantinopoulou@northwind-engineering.example'

function longState(): AppState {
  const state = createDemoState(null)
  state.accounts = state.accounts.map((account, index) => index === 0 ? { ...account, label: longLabel, identity: { ...account.identity, email: longEmail, plan: 'Max 20×' } } : account)
  return state
}

async function bridge(page: Page, state: AppState | null, preview?: MakeActivePreview): Promise<void> {
  await page.addInitScript(({ state, preview }) => {
    window.__TAURI__ = {
      core: {
        async invoke(command) {
          if (command === 'get_state') {
            if (!state) throw new Error('offline')
            return state
          }
          if (command === 'get_popover_placement') return { revision: 0, arrowX: null }
          if (command === 'get_update_status') return { kind: 'idle' }
          if (command === 'get_account_history') return { points: [] }
          if (command === 'preview_make_active') return preview
          return null
        },
      },
      event: { async listen() { return () => {} } },
    }
  }, { state, preview: preview ?? null })
}

async function settings(page: Page, tab: 'General' | 'Accounts' = 'General'): Promise<void> {
  await page.getByRole('button', { name: /settings/i }).first().click()
  if (tab === 'Accounts') await page.getByRole('tab', { name: /^Accounts/ }).click()
}

async function section(page: Page, heading: string): Promise<void> {
  await page.locator('.settings-content[data-state="active"]').evaluate((element, heading) => {
    const target = [...element.querySelectorAll('h2')].find(node => node.textContent?.trim() === heading)
    if (target) element.scrollTop += target.getBoundingClientRect().top - element.getBoundingClientRect().top - 8
  }, heading)
}

type Shot = { name: string; url?: string; size?: { width: number; height: number }; all?: boolean; setup?: (page: Page) => Promise<void>; run?: (page: Page) => Promise<void> }

const overviewStates: [string, string][] = [
  ['cards-3', ''], ['cards-1', '&state=single'], ['cards-5', '&state=compact&layout=cards'],
  ['compact-1', '&state=single&layout=compact'], ['compact-3', '&layout=compact'], ['compact-5', '&state=compact'],
  ['loading', '&state=loading'], ['loading-compact', '&state=loading&layout=compact'],
  ['stale', '&state=stale'], ['stale-compact', '&state=stale&layout=compact'],
  ['error', '&state=error'], ['error-compact', '&state=error&layout=compact'],
  ['signed-out', '&state=signed-out'], ['signed-out-compact', '&state=signed-out&layout=compact'],
  ['idle', '&state=idle'], ['idle-compact', '&state=idle&layout=compact'],
  ['incomplete', '&state=incomplete'], ['incomplete-compact', '&state=incomplete&layout=compact'],
  ['retrying', '&state=retrying'], ['retrying-compact', '&state=retrying&layout=compact'],
  ['recovery', '&state=recovery'], ['empty', '&state=empty'], ['first-run', '&state=empty&prompt=1'],
  ['prompt-outside', '&state=single&prompt=1&location=outside'], ['update-available', '&state=update-available'],
]

const shots: Shot[] = [
  ...overviewStates.map(([name, query]) => ({ name, url: `/?demo=1${query}`, all: true })),
  { name: 'compact-5-expanded', url: '/?demo=1&state=compact', all: true, run: async page => { await page.locator('.compact-row').first().click() } },
  { name: 'idle-compact-expanded', url: '/?demo=1&state=idle&layout=compact', run: async page => { await page.locator('.compact-row').first().click() } },
  { name: 'retrying-compact-expanded', url: '/?demo=1&state=retrying&layout=compact', run: async page => { await page.locator('.compact-row').nth(2).click() } },
  { name: 'details-open', url: '/?demo=1&order=added', all: true, run: async page => {
    await page.getByRole('button', { name: '1 more window' }).click()
    await page.getByRole('button', { name: 'Extra usage' }).click()
  } },
  { name: 'tallest', url: '/?demo=1&state=compact&layout=cards', all: true, run: async page => {
    const toggles = page.getByRole('button', { name: /more window|Extra usage|Balance/ })
    while (await toggles.count()) await toggles.first().click()
  } },
  { name: 'history', url: '/?demo=1', all: true, run: async page => {
    const account = page.getByRole('article', { name: 'Personal, Claude' })
    await account.getByRole('button', { name: 'History', exact: true }).click()
    await account.getByRole('region', { name: 'Usage history' }).evaluate(element => element.scrollIntoView({ block: 'start' }))
  } },
  { name: 'history-weekly', url: '/?demo=1', run: async page => {
    const account = page.getByRole('article', { name: 'Work, Codex' })
    await account.getByRole('button', { name: 'History', exact: true }).click()
    const history = account.getByRole('region', { name: 'Usage history' })
    await history.getByRole('tab', { name: 'Weekly', exact: true }).click()
    await history.evaluate(element => element.scrollIntoView({ block: 'start' }))
  } },
  { name: 'history-compact', url: '/?demo=1&layout=compact', run: async page => {
    await page.locator('.compact-row').first().click()
    await page.getByRole('button', { name: 'History', exact: true }).click()
    await page.getByRole('region', { name: 'Usage history' }).evaluate(element => element.scrollIntoView({ block: 'end' }))
  } },
  { name: 'toast-success', url: '/?demo=1', run: async page => {
    await page.getByRole('button', { name: 'Refresh usage' }).click()
    await page.mouse.move(2, 2)
  } },
  { name: 'toast-error', url: '/?demo=1&running=1', run: async page => {
    await settings(page, 'Accounts')
    await page.getByRole('button', { name: 'Make Work active', exact: true }).click()
  } },
  { name: 'toast-undo', url: '/?demo=1', run: async page => {
    await settings(page, 'Accounts')
    await page.getByRole('button', { name: 'Make Studio active', exact: true }).click()
    await page.getByRole('alertdialog').getByRole('button', { name: 'Make active', exact: true }).click()
  } },
  { name: 'make-active', url: '/?demo=1', run: async page => {
    await settings(page, 'Accounts')
    await page.getByRole('button', { name: 'Make Studio active', exact: true }).click()
  } },
  { name: 'make-active-running', url: '/?demo=1&running=1', run: async page => {
    await settings(page, 'Accounts')
    await page.getByRole('button', { name: 'Make Studio active', exact: true }).click()
  } },
  { name: 'remove', url: '/?demo=1', run: async page => {
    await settings(page, 'Accounts')
    await page.getByRole('button', { name: 'Remove Studio', exact: true }).click()
  } },
  { name: 'settings-general', url: '/?demo=1', all: true, run: page => settings(page) },
  ...['Tray & refresh', 'Notifications', 'Startup', 'Updates', 'Support'].map(heading => ({
    name: `settings-${heading.toLowerCase().replace(/[^a-z]+/g, '-')}`, url: '/?demo=1', run: async (page: Page) => { await settings(page); await section(page, heading) },
  })),
  { name: 'settings-fast-usage', url: '/?demo=1', run: async page => {
    await settings(page)
    await page.getByRole('switch', { name: 'Usage alerts', exact: true }).click()
    await page.getByRole('switch', { name: 'Fast usage alerts' }).click()
    await section(page, 'Notifications')
  } },
  ...['overview-layout', 'account-order', 'tray-account', 'tray-metric', 'refresh-interval', 'idle-interval'].map(id => ({
    name: `menu-${id}`, url: '/?demo=1', run: async (page: Page) => {
      await settings(page)
      const trigger = page.locator(`#${id}`)
      await trigger.scrollIntoViewIfNeeded()
      await trigger.click()
      await expect(page.getByRole('listbox')).toBeVisible()
    },
  })),
  { name: 'settings-focus', url: '/?demo=1', run: async page => {
    await settings(page)
    await page.locator('#overview-layout').focus()
    await page.keyboard.press('Shift+Tab')
    await page.keyboard.press('Tab')
  } },
  ...([
    ['filter', '/?demo=1', 'button[aria-pressed]:has-text("Claude")'],
    ['chip', '/?demo=1', '.model-chip'],
    ['compact-row', '/?demo=1&layout=compact', '.compact-row'],
    ['copy', '/?demo=1&state=signed-out&layout=compact', '.compact-row >> nth=1'],
    ['details', '/?demo=1', '.details-toggle'],
  ] as const).map(([name, url, selector]) => ({ name: `focus-${name}`, url, run: async (page: Page) => {
    if (name === 'copy') { await page.locator(selector).click(); await page.locator('.command-copy').first().focus() }
    else await page.locator(selector).first().focus()
    await page.keyboard.press('Shift')
  } })),
  { name: 'focus-history-tab', url: '/?demo=1', run: async page => {
    const account = page.getByRole('article', { name: 'Personal, Claude' })
    await account.getByRole('button', { name: 'History', exact: true }).click()
    await account.getByRole('tab', { name: 'Weekly', exact: true }).focus()
    await page.keyboard.press('Shift')
  } },
  { name: 'report-preview', url: '/?demo=1', run: async page => {
    await settings(page)
    await page.getByRole('button', { name: 'Report a problem' }).click()
    await expect(page.getByRole('group', { name: 'Report preview' })).toBeVisible()
  } },
  { name: 'settings-update', url: '/?demo=1&state=update-available', run: async page => { await settings(page); await section(page, 'Updates') } },
  { name: 'settings-recovered', url: '/?demo=1&state=recovered', run: page => settings(page) },
  { name: 'settings-notification-failed', url: '/?demo=1&state=notification-failed', run: async page => {
    await settings(page)
    await page.locator('.notification-blocked').evaluate(element => element.scrollIntoView({ block: 'end' }))
  } },
  { name: 'settings-accounts', url: '/?demo=1', all: true, run: page => settings(page, 'Accounts') },
  { name: 'settings-accounts-5', url: '/?demo=1&state=compact', all: true, run: page => settings(page, 'Accounts') },
  { name: 'settings-accounts-empty', url: '/?demo=1&state=empty', run: page => settings(page, 'Accounts') },
  { name: 'settings-hidden', url: '/?demo=1&state=hidden', run: page => settings(page, 'Accounts') },
  { name: 'settings-switch-restore', url: '/?demo=1&state=switch-restore', run: page => settings(page, 'Accounts') },
  { name: 'settings-add', url: '/?demo=1', run: async page => {
    await settings(page, 'Accounts')
    await page.getByRole('button', { name: 'Add account', exact: true }).click()
    await page.getByRole('button', { name: 'Connect account' }).click()
    await page.locator('.add-account-form').evaluate(element => element.scrollIntoView({ block: 'end' }))
  } },
  { name: 'settings-rename', url: '/?demo=1', run: async page => {
    await settings(page, 'Accounts')
    await page.getByRole('button', { name: 'Rename Work', exact: true }).click()
  } },
  { name: 'connection-error', setup: page => bridge(page, null) },
  { name: 'long-cards', setup: page => bridge(page, longState()), all: true },
  { name: 'long-compact', setup: page => bridge(page, { ...longState(), settings: { ...longState().settings, overviewLayout: 'compact' } }), run: async page => { await page.locator('.compact-row').filter({ hasText: longLabel }).click() } },
  { name: 'long-settings', setup: page => bridge(page, longState()), run: page => settings(page, 'Accounts') },
  { name: 'long-settings-add', setup: page => bridge(page, longState()), run: async page => {
    await settings(page, 'Accounts')
    await page.getByRole('button', { name: 'Add account', exact: true }).click()
    await page.locator('#account-path').fill('~/Library/Application Support/Claude/configs/client-work-northwind')
    await page.locator('.add-account-form').evaluate(element => element.scrollIntoView({ block: 'end' }))
  } },
  { name: 'long-tray-menu', setup: page => bridge(page, longState()), run: async page => { await settings(page); await page.locator('#tray-account').click() } },
  { name: 'long-make-active', setup: page => bridge(page, longState(), { accountId: 'demo-codex-work', provider: 'codex', kind: 'swap', fingerprint: 'x', target: longEmail, displaced: 'alex.longsurname-hyphenated@studio-northwind.example', folder: '~/.codex-client-work-northwind', running: true } as MakeActivePreview), run: async page => {
    await settings(page, 'Accounts')
    await page.getByRole('button', { name: 'Make Work active', exact: true }).click()
  } },
]

const narrow: Shot[] = [
  { name: 'cards', url: '/?demo=1', all: true },
  { name: 'compact', url: '/?demo=1&state=compact', all: true },
  { name: 'idle', url: '/?demo=1&state=idle', all: true },
  { name: 'retrying-compact', url: '/?demo=1&state=retrying&layout=compact' },
  { name: 'empty', url: '/?demo=1&state=empty&prompt=1' },
  { name: 'settings', url: '/?demo=1', all: true, run: page => settings(page) },
  { name: 'settings-notifications', url: '/?demo=1', run: async page => { await settings(page); await section(page, 'Notifications') } },
  { name: 'settings-accounts', url: '/?demo=1&state=compact', all: true, run: page => settings(page, 'Accounts') },
  { name: 'make-active', url: '/?demo=1&running=1', run: async page => { await settings(page, 'Accounts'); await page.getByRole('button', { name: 'Make Studio active', exact: true }).click() } },
  { name: 'long-cards', setup: page => bridge(page, longState()) },
  { name: 'long-settings-add', setup: page => bridge(page, longState()), run: async page => {
    await settings(page, 'Accounts')
    await page.getByRole('button', { name: 'Add account', exact: true }).click()
    await page.locator('.add-account-form').evaluate(element => element.scrollIntoView({ block: 'end' }))
  } },
]

for (const theme of ['light', 'dark']) {
  test.describe(`walk ${theme}`, () => {
    test.describe.configure({ mode: 'parallel' })
    const variants: [string, Shot[], { width: number; height: number }][] = [['', shots, { width: 420, height: 660 }], ['narrow-', narrow, { width: 280, height: 620 }]]
    for (const [prefix, list, size] of variants) {
      for (const shot of list) {
        test(`${prefix}${shot.name}`, async ({ page }, info) => {
          await page.setViewportSize(size)
          await page.emulateMedia({ colorScheme: theme as 'light' | 'dark' })
          await shot.setup?.(page)
          const url = shot.url ? `${shot.url}&theme=${theme}` : '/'
          await page.goto(url)
          await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
          await expect(page.locator('.popover')).toBeVisible()
          await shot.run?.(page)
          if (shot.all) await snapAll(page, info, theme, `${prefix}${shot.name}`)
          else await snap(page, info, theme, `${prefix}${shot.name}`)
        })
      }
    }
  })
}

const focusPages: [string, string, ((page: Page) => Promise<void>)?][] = [
  ['cards', '/?demo=1&state=idle'],
  ['compact', '/?demo=1&state=signed-out&layout=compact', async page => { await page.locator('.compact-row').nth(1).click() }],
  ['settings', '/?demo=1', page => settings(page)],
  ['accounts', '/?demo=1&state=switch-restore', page => settings(page, 'Accounts')],
]

for (const [name, url, run] of focusPages) {
  test(`focus rings are not clipped: ${name}`, async ({ page }) => {
    await page.goto(url)
    await run?.(page)
    await page.keyboard.press('Shift')
    const clipped = await page.evaluate(() => {
      const found: string[] = []
      const focusable = [...document.querySelectorAll<HTMLElement>('button, [tabindex="0"], input, a[href], [role="tab"]')]
        .filter(element => !element.closest('[aria-hidden="true"], .sr-only') && element.getClientRects().length && !(element as HTMLButtonElement).disabled)
      for (const element of focusable) {
        element.focus()
        if (document.activeElement !== element) continue
        const style = getComputedStyle(element)
        if (style.outlineStyle === 'none') continue
        const reach = parseFloat(style.outlineWidth) + parseFloat(style.outlineOffset)
        const box = element.getBoundingClientRect()
        const ring = { left: box.left - reach, top: box.top - reach, right: box.right + reach, bottom: box.bottom + reach }
        for (let node = element.parentElement; node && node !== document.body; node = node.parentElement) {
          const clip = getComputedStyle(node)
          if (clip.overflowX === 'visible' && clip.overflowY === 'visible') continue
          const edge = node.getBoundingClientRect()
          const x = ring.left < edge.left - 0.5 || ring.right > edge.right + 0.5
          const y = ring.top < edge.top - 0.5 || ring.bottom > edge.bottom + 0.5
          const scrolledAway = box.top < edge.top || box.bottom > edge.bottom || box.left < edge.left || box.right > edge.right
          if ((x || y) && !scrolledAway) found.push(`${element.getAttribute("aria-label") ?? element.getAttribute("aria-labelledby") ?? element.textContent?.trim().slice(0, 30)} in .${[...node.classList].join('.')}`)
          break
        }
      }
      return found
    })
    expect(clipped).toEqual([])
  })
}

test('a centered empty state stays reachable when a banner leaves too little room', async ({ page }) => {
  await page.goto('/?demo=1&state=recovery')
  const scroller = page.locator('.accounts-scroll')
  await scroller.evaluate(element => { element.scrollTop = 0 })
  const [content, viewport] = await Promise.all([page.locator('.empty-state').boundingBox(), scroller.boundingBox()])
  expect(content!.y).toBeGreaterThanOrEqual(viewport!.y - 0.5)
})

test('a long email in the Make active title wraps after the @, not mid-word', async ({ page }) => {
  await bridge(page, longState(), { accountId: 'demo-codex-work', provider: 'codex', kind: 'swap', fingerprint: 'x', target: longEmail, displaced: 'alex@studio.example', folder: '~/.codex-work', running: false } as MakeActivePreview)
  await page.goto('/')
  await settings(page, 'Accounts')
  await page.getByRole('button', { name: 'Make Work active', exact: true }).click()
  const title = page.getByRole('alertdialog').getByRole('heading')
  await expect(title).toHaveText(`Make ${longEmail} active?`)
  const lines = await title.evaluate(heading => {
    const tops = new Map<number, string>()
    const walker = document.createTreeWalker(heading, NodeFilter.SHOW_TEXT)
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = node.textContent ?? ''
      for (let index = 0; index < text.length; index++) {
        const range = document.createRange()
        range.setStart(node, index)
        range.setEnd(node, index + 1)
        const top = Math.round(range.getBoundingClientRect().top)
        tops.set(top, (tops.get(top) ?? '') + text[index])
      }
    }
    return [...tops.entries()].sort(([a], [b]) => a - b).map(([, line]) => line.trim())
  })
  const [local] = longEmail.split('@')
  expect(lines.some(line => line.endsWith(`${local}@`))).toBe(true)
})
