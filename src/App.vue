<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch, watchEffect } from 'vue'
import { z } from 'zod'
import { TooltipProvider } from 'reka-ui'
import AppTooltip from './components/ui/AppTooltip.vue'
import AccountCard from './components/AccountCard.vue'
import AppIcon from './components/AppIcon.vue'
import SettingsPanel from './components/SettingsPanel.vue'
import { accountIsStale, relativeAge } from './lib/format'
import { useMargin } from './lib/margin'
import type { Account, Provider } from './lib/model'

const { state, loading, connectionError, pending, message, now, demo, desktop, refresh, discover, addAccount, removeAccount, renameAccount, saveSettings, hideWindow } = useMargin()
const popoverPlacementSchema = z.object({ revision: z.number().int().nonnegative(), arrowX: z.number().finite().nullable() })
const popoverPlacement = ref<z.infer<typeof popoverPlacementSchema>>({ revision: 0, arrowX: null })
const viewportWidth = ref(window.innerWidth)
const isWindows = navigator.userAgent.includes('Windows')
const nativeArrowX = computed(() => {
  const x = popoverPlacement.value.arrowX
  if (!desktop || demo || isWindows || x === null) return null
  return x >= 26 && x <= viewportWidth.value - 26 ? x : null
})
const nativeArrowStyle = computed(() => nativeArrowX.value === null ? undefined : { left: `${nativeArrowX.value - 6}px` })
const page = ref<'overview' | 'settings'>('overview')
const settingsTab = ref<'general' | 'accounts'>('general')
const filter = ref<'all' | Provider>('all')
const media = window.matchMedia('(prefers-color-scheme: dark)')
const systemDark = ref(media.matches)
const dark = computed(() => state.value.settings.theme === 'dark' || (state.value.settings.theme === 'system' && systemDark.value))
const busy = computed(() => pending.value !== null)
const refreshing = computed(() => (!connectionError.value && state.value.refreshing) || pending.value === 'refresh_accounts' || pending.value === 'discover_accounts')
const visibleAccounts = computed(() => state.value.accounts.filter(account => filter.value === 'all' || account.provider === filter.value))
const staleCount = computed(() => state.value.accounts.filter(account => accountIsStale(account, state.value.settings, now.value, Boolean(connectionError.value))).length)
const counts = computed(() => ({ all: state.value.accounts.length, claude: state.value.accounts.filter(account => account.provider === 'claude').length, codex: state.value.accounts.filter(account => account.provider === 'codex').length }))
const filterOptions: { id: 'all' | Provider; label: string }[] = [{ id: 'all', label: 'All' }, { id: 'claude', label: 'Claude' }, { id: 'codex', label: 'Codex' }]
let stopSettingsListener: (() => void) | undefined
let stopPlacementListener: (() => void) | undefined
let destroyed = false
const latestFetch = computed(() => {
  const timestamps = state.value.accounts.flatMap(account => account.snapshot ? [account.snapshot.fetchedAt] : [])
  return timestamps.sort((a, b) => Date.parse(b) - Date.parse(a)).at(0)
})
const status = computed(() => {
  if (refreshing.value) return 'Checking usage'
  if (connectionError.value) return 'Connection interrupted'
  if (staleCount.value) return `${staleCount.value} ${staleCount.value === 1 ? 'account needs' : 'accounts need'} attention`
  if (latestFetch.value) return `Updated ${relativeAge(latestFetch.value, now.value)}`
  return 'Ready to connect'
})

watch(counts, value => {
  if (filter.value !== 'all' && (value[filter.value] === 0 || value.all < 2)) filter.value = 'all'
})

watchEffect(() => {
  document.documentElement.dataset.theme = dark.value ? 'dark' : 'light'
  document.documentElement.dataset.platform = isWindows ? 'windows' : 'mac'
  document.documentElement.style.colorScheme = dark.value ? 'dark' : 'light'
})

function openSettings(tab: 'general' | 'accounts' = 'general'): void {
  settingsTab.value = tab
  page.value = 'settings'
}

async function togglePin(account: Account): Promise<void> {
  const pinned = state.value.settings.trayMode === 'pinned' && state.value.settings.pinnedAccountId === account.id
  await saveSettings({ ...state.value.settings, trayMode: pinned ? 'lowest' : 'pinned', pinnedAccountId: pinned ? null : account.id })
}

function syncSystemTheme(event: MediaQueryListEvent): void { systemDark.value = event.matches }
function syncViewportWidth(): void { viewportWidth.value = window.innerWidth }

function acceptPopoverPlacement(payload: unknown): void {
  if (destroyed) return
  const result = popoverPlacementSchema.safeParse(payload)
  if (!result.success) {
    popoverPlacement.value = { ...popoverPlacement.value, arrowX: null }
    return
  }
  if (result.data.revision >= popoverPlacement.value.revision) popoverPlacement.value = result.data
}

async function connectPopoverPlacement(): Promise<void> {
  const tauri = window.__TAURI__
  if (!tauri) return
  try {
    const unlisten = await tauri.event.listen('popover_placement_changed', event => acceptPopoverPlacement(event.payload))
    if (destroyed) { unlisten(); return }
    stopPlacementListener = unlisten
    acceptPopoverPlacement(await tauri.core.invoke('get_popover_placement'))
  } catch {
    if (!destroyed) popoverPlacement.value = { ...popoverPlacement.value, arrowX: null }
  }
}

function onKeydown(event: KeyboardEvent): void {
  if (event.defaultPrevented) return
  if (document.querySelector('[role="alertdialog"]')) {
    if ((event.metaKey || event.ctrlKey) && [',', 'r'].includes(event.key.toLowerCase())) event.preventDefault()
    return
  }
  if (event.key === 'Escape') {
    if (page.value === 'settings') page.value = 'overview'
    else void hideWindow()
  }
  if ((event.metaKey || event.ctrlKey) && event.key === ',') { event.preventDefault(); openSettings() }
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'r') { event.preventDefault(); void refresh() }
}

onMounted(async () => {
  media.addEventListener('change', syncSystemTheme)
  window.addEventListener('keydown', onKeydown)
  window.addEventListener('resize', syncViewportWidth)
  if (!desktop || demo) return
  void connectPopoverPlacement()
  try {
    const unlisten = await window.__TAURI__?.event.listen('open_settings', () => openSettings())
    if (destroyed) unlisten?.()
    else stopSettingsListener = unlisten
  } catch { }
})
onUnmounted(() => {
  destroyed = true
  stopSettingsListener?.()
  stopPlacementListener?.()
  media.removeEventListener('change', syncSystemTheme)
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('resize', syncViewportWidth)
})
</script>

<template>
  <TooltipProvider :delay-duration="350">
  <div class="preview-stage" :class="{ 'desktop-stage': desktop && !demo }">
    <div v-if="!desktop && !demo" class="desktop-gate">
      <span class="gate-logo"><AppIcon name="margin" :size="36"/></span>
      <p class="eyebrow">CLAUDE + CODEX, IN VIEW</p><h1>A little more<br>headroom.</h1>
      <p class="gate-description">Margin lives in your menu bar. Your accounts, usage windows, and reset times. In one quiet place.</p>
      <a class="button button-primary" href="https://github.com/Abulkosim/usagebar/releases" target="_blank" rel="noopener noreferrer">Get the desktop app<AppIcon name="external" :size="14"/></a>
      <a class="preview-link" href="?demo=1">Explore the interface <span>with sample data</span><AppIcon name="chevron" :size="12"/></a>
      <p class="gate-footnote">macOS &amp; Windows · Your credentials stay on your device</p>
    </div>

    <div v-else class="app-shell" :class="{ 'is-preview': demo }">
      <div v-if="demo || nativeArrowX !== null" class="popover-arrow" :style="demo ? undefined : nativeArrowStyle" aria-hidden="true"/>
      <div class="popover">
        <div v-if="state.notice" class="notice-banner" role="alert"><AppIcon name="alert" :size="13"/><span>{{ state.notice }}</span></div>
        <template v-if="page === 'overview'">
          <header class="app-header"><div class="wordmark"><AppIcon name="margin" :size="20"/><span>margin</span></div><div class="header-actions"><AppTooltip :text="refreshing ? 'Checking usage' : `Refresh usage (${isWindows ? 'Ctrl+R' : '⌘R'})`"><button class="icon-button refresh-button" :class="{ pulsing: refreshing }" aria-label="Refresh usage" :disabled="busy || refreshing" @click="refresh"><AppIcon name="refresh" :size="16"/></button></AppTooltip><AppTooltip :text="`Settings (${isWindows ? 'Ctrl+,' : '⌘,'})`"><button class="icon-button" aria-label="Open settings" @click="openSettings()"><AppIcon name="settings" :size="18"/></button></AppTooltip></div></header>
          <div class="overview-heading"><div><h1>Your headroom</h1><p>{{ state.accounts.length ? `${state.accounts.length} ${state.accounts.length === 1 ? 'account' : 'accounts'}` : 'A little clarity for your day' }}<template v-if="state.accounts.length"><span class="middot">·</span>Usage remaining</template></p></div><span v-if="state.accounts.length" class="overview-symbol" aria-hidden="true"><span/><span/><span/></span></div>
          <div v-if="state.accounts.length > 1" class="account-filters" role="group" aria-label="Filter by provider"><button v-for="option in filterOptions" :key="option.id" :class="{ active: filter === option.id }" :aria-pressed="filter === option.id" @click="filter = option.id">{{ option.label }}<span>{{ counts[option.id] }}</span></button></div>

          <main class="accounts-scroll" :class="{ 'empty-scroll': !state.accounts.length }" aria-label="Account usage">
            <div v-if="loading" class="initial-loading" role="status" aria-label="Loading accounts"><div v-for="index in 2" :key="index" class="skeleton-card"><div class="skeleton skeleton-heading"/><div class="skeleton skeleton-label"/><div class="skeleton skeleton-bar"/><div class="skeleton skeleton-label"/><div class="skeleton skeleton-bar"/></div><p>Reading your accounts…</p></div>
            <div v-else-if="connectionError && !state.accounts.length" class="empty-state"><span class="load-error-symbol"><AppIcon name="alert" :size="28"/></span><h2>Couldn’t load your accounts.</h2><p>{{ connectionError }}</p><button class="button button-primary" :disabled="busy" @click="refresh"><AppIcon name="refresh" :size="14"/>Try again</button></div>
            <div v-else-if="!state.accounts.length" class="empty-state"><div class="empty-illustration" aria-hidden="true"><div class="empty-orbit orbit-one"/><div class="empty-orbit orbit-two"/><span class="empty-provider empty-claude"><AppIcon name="claude" :size="24"/></span><span class="empty-provider empty-codex"><AppIcon name="codex" :size="23"/></span><span class="empty-center"><AppIcon name="margin" :size="27"/></span></div><h2>A home for your accounts.</h2><p>See your Claude and Codex limits together.<br>Connect accounts already signed in on this device.</p><button class="button button-primary" :disabled="busy" @click="discover"><AppIcon name="refresh" :size="14"/>Find my accounts</button><button class="text-button" @click="openSettings('accounts')">Add a config folder<AppIcon name="plus" :size="13"/></button><span class="empty-privacy">Read-only. Your credentials stay yours.</span></div>
            <template v-else><AccountCard v-for="account in visibleAccounts" :key="account.id" :account="account" :settings="state.settings" :now="now" :pinned="state.settings.trayMode === 'pinned' && state.settings.pinnedAccountId === account.id" :busy="busy" :connection-error="connectionError" @pin="togglePin(account)" @refresh="refresh"/><div v-if="!visibleAccounts.length" class="filter-empty"><AppIcon :name="filter === 'claude' ? 'claude' : 'codex'" :size="25"/><p>No {{ filter === 'claude' ? 'Claude' : 'Codex' }} accounts yet.</p><button class="text-button" @click="openSettings('accounts')">Add an account<AppIcon name="plus" :size="13"/></button></div></template>
          </main>
          <footer class="app-footer"><span class="sync-status" :class="{ warning: staleCount || connectionError, pulsing: refreshing }" role="status"><span class="status-dot"/>{{ status }}</span><button class="text-button footer-add" @click="openSettings('accounts')"><AppIcon name="plus" :size="13"/>Add account</button></footer>
        </template>

        <SettingsPanel v-else :accounts="state.accounts" :settings="state.settings" :busy="busy" :initial-tab="settingsTab" :save="saveSettings" :add="addAccount" :remove="removeAccount" :rename="renameAccount" @back="page = 'overview'" @discover="discover"/>
        <div v-if="demo" class="preview-ribbon"><span class="preview-dot"/>Interface preview<span>Sample data</span></div>
        <Transition name="toast"><div v-if="message" class="toast-message" :class="{ 'toast-error': message.kind === 'error' }" :role="message.kind === 'error' ? 'alert' : 'status'"><AppIcon :name="message.kind === 'error' ? 'alert' : 'check'" :size="15"/><span>{{ message.text }}</span><button class="icon-button" aria-label="Dismiss message" @click="message = null"><AppIcon name="close" :size="13"/></button></div></Transition>
      </div>
    </div>
  </div>
  </TooltipProvider>
</template>
