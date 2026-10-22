<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogOverlay, AlertDialogPortal, AlertDialogRoot, AlertDialogTitle, TabsContent, TabsList, TabsRoot, TabsTrigger } from 'reka-ui'
import { version } from '../../package.json'
import { accountIdSchema, providerSchema, type Account, type AccountId, type Provider, type Settings } from '../lib/model'
import { accountSource, providerName } from '../lib/format'
import AppIcon from './AppIcon.vue'
import UpdateSettings from './settings/UpdateSettings.vue'
import type { UpdateState } from '../lib/updater'
import AppSelect from './ui/AppSelect.vue'
import AppSwitch from './ui/AppSwitch.vue'
import AppTooltip from './ui/AppTooltip.vue'

const props = defineProps<{
  updateState: UpdateState; updateBusy: boolean; demo: boolean;
  accounts: Account[]; recovered: string[]; settings: Settings; busy: boolean; initialTab: 'general' | 'accounts';
  save: (settings: Settings) => Promise<boolean>;
  add: (provider: Provider, path: string) => Promise<boolean>;
  remove: (id: AccountId) => Promise<boolean>;
  rename: (id: AccountId, label: string) => Promise<boolean>;
}>()
defineEmits<{ back: []; discover: []; checkUpdate: []; installUpdate: [] }>()
const tab = ref(props.initialTab)
const adding = ref(false)
const provider = ref<Provider>('claude')
const path = ref('')
const pathInput = ref<HTMLInputElement | null>(null)
const removeTarget = ref<Account | null>(null)
const removeFailed = ref(false)
const removeButton = ref<HTMLButtonElement | null>(null)
const renameTarget = ref<AccountId | null>(null)
const renameLabel = ref('')
const renameInput = ref<HTMLInputElement | null>(null)
const addAccountButton = ref<HTMLButtonElement | null>(null)
let removeTrigger: HTMLElement | null = null
const formError = ref<string | null>(null)
const selectedMode = computed(() => props.settings.trayMode === 'pinned' ? props.settings.pinnedAccountId ?? 'lowest' : 'lowest')
const trayOptions = computed(() => [
  { value: 'lowest', label: 'Lowest remaining' },
  ...props.accounts.map(account => ({ value: account.id, label: account.label, description: providerName(account.provider) })),
])
const metricOptions: { value: Settings['trayMetric']; label: string }[] = [
  { value: 'core', label: 'Session and weekly' },
  { value: 'session', label: 'Session only' },
  { value: 'weekly', label: 'Weekly only' },
  { value: 'all', label: 'Any window' },
]
const isMac = navigator.userAgent.includes('Mac')
const refreshOptions = computed(() => intervalOptions([30, 60, 120, 300], props.settings.pollIntervalSeconds))
const idleOptions = computed(() => intervalOptions([300, 600, 900], props.settings.idlePollIntervalSeconds))
const themes: { id: Settings['theme']; label: string; icon: 'system' | 'sun' | 'moon' }[] = [
  { id: 'system', label: 'System', icon: 'system' },
  { id: 'light', label: 'Light', icon: 'sun' },
  { id: 'dark', label: 'Dark', icon: 'moon' },
]

watch(() => props.initialTab, value => { tab.value = value })

function intervalOptions(presets: number[], current: number): { value: string; label: string }[] {
  return [...new Set([...presets, current])].sort((a, b) => a - b).map(seconds => ({
    value: String(seconds),
    label: seconds % 60 === 0 ? `${seconds / 60} ${seconds === 60 ? 'minute' : 'minutes'}` : `${seconds} seconds`,
  }))
}

function setTab(value: unknown): void {
  if (value === 'general' || value === 'accounts') tab.value = value
}

function setRenameInput(element: unknown): void {
  renameInput.value = element instanceof HTMLInputElement ? element : null
}

function openRemove(account: Account, event: MouseEvent): void {
  removeFailed.value = false
  removeTrigger = event.currentTarget instanceof HTMLElement ? event.currentTarget : null
  removeTarget.value = account
}

function closeRemove(open: boolean): void {
  if (!open && !props.busy) removeTarget.value = null
}

function escapeRemove(event: KeyboardEvent): void {
  event.preventDefault()
  event.stopPropagation()
  closeRemove(false)
}

function restoreRemoveFocus(event: Event): void {
  event.preventDefault()
  if (removeTrigger?.isConnected) removeTrigger.focus()
  else (addAccountButton.value ?? pathInput.value)?.focus()
  removeTrigger = null
}

async function patchSettings(patch: Partial<Settings>): Promise<void> {
  await props.save({ ...props.settings, ...patch })
}

async function setIntervalSetting(selection: string, key: 'pollIntervalSeconds' | 'idlePollIntervalSeconds'): Promise<void> {
  const value = Number(selection)
  if (Number.isFinite(value)) await patchSettings({ [key]: value })
}

async function setTray(value: string): Promise<void> {
  if (value === 'lowest') await patchSettings({ trayMode: 'lowest', pinnedAccountId: null })
  else {
    const id = accountIdSchema.safeParse(value)
    if (id.success && props.accounts.some(account => account.id === id.data)) await patchSettings({ trayMode: 'pinned', pinnedAccountId: id.data })
  }
}

async function setMetric(value: string): Promise<void> {
  const metric = metricOptions.find(option => option.value === value)
  if (metric) await patchSettings({ trayMetric: metric.value })
}

async function toggleThreshold(value: number): Promise<void> {
  const thresholds = props.settings.thresholds.includes(value)
    ? props.settings.thresholds.filter(threshold => threshold !== value)
    : [...props.settings.thresholds, value].sort((a, b) => a - b)
  if (!thresholds.length) return
  await patchSettings({ thresholds })
}

async function openAdd(): Promise<void> {
  adding.value = true
  formError.value = null
  await nextTick()
  pathInput.value?.focus()
}

async function submitAccount(): Promise<void> {
  const cleanPath = path.value.trim()
  if (!cleanPath) { formError.value = 'Enter the account’s config folder.'; return }
  const parsedProvider = providerSchema.parse(provider.value)
  if (await props.add(parsedProvider, cleanPath)) {
    path.value = ''
    formError.value = null
    adding.value = false
  }
}

async function startRename(account: Account): Promise<void> {
  renameTarget.value = account.id
  renameLabel.value = account.label
  await nextTick()
  renameInput.value?.focus()
  renameInput.value?.select()
}

async function submitRename(): Promise<void> {
  if (!renameTarget.value || !renameLabel.value.trim()) return
  if (await props.rename(renameTarget.value, renameLabel.value.trim())) renameTarget.value = null
}

async function confirmRemove(): Promise<void> {
  if (!removeTarget.value) return
  removeFailed.value = false
  if (await props.remove(removeTarget.value.id)) removeTarget.value = null
  else {
    removeFailed.value = true
    await nextTick()
    removeButton.value?.focus()
  }
}
</script>

<template>
  <AlertDialogRoot :open="removeTarget !== null" @update:open="closeRemove">
    <TabsRoot :model-value="tab" class="settings-view" @update:model-value="setTab">
      <header class="settings-header"><button class="icon-button back-button" aria-label="Back to overview" @click="$emit('back')"><AppIcon name="back" :size="18"/></button><h1>Settings</h1><span class="settings-autosave">Saves automatically</span></header>
      <TabsList class="settings-tabs" aria-label="Settings sections">
        <TabsTrigger value="general">General</TabsTrigger>
        <TabsTrigger value="accounts">Accounts <span>{{ accounts.length }}</span></TabsTrigger>
      </TabsList>

      <TabsContent value="general" class="settings-content">
        <p v-if="recovered.length" class="recovered-note" role="status"><AppIcon name="alert" :size="13"/><span>{{ recovered.join(' ') }} A backup is kept next to the settings file.</span></p>
        <section class="settings-section">
          <h2>Appearance</h2>
          <div class="theme-options"><button v-for="theme in themes" :key="theme.id" class="theme-option" :class="{ selected: settings.theme === theme.id }" :aria-pressed="settings.theme === theme.id" :disabled="busy" @click="patchSettings({ theme: theme.id })"><span class="theme-thumbnail" :class="`thumbnail-${theme.id}`"><span/><span/><span/></span><span><AppIcon :name="theme.icon" :size="13"/>{{ theme.label }}</span><span v-if="settings.theme === theme.id" class="theme-check"><AppIcon name="check" :size="10"/></span></button></div>
        </section>
        <section class="settings-section">
          <h2>Tray &amp; refresh</h2>
          <div class="setting-row">
            <div><label for="tray-account">Tray account</label><p>Which account the icon follows.</p></div>
            <AppSelect id="tray-account" :model-value="selectedMode" :options="trayOptions" :disabled="busy" @update:model-value="setTray"/>
          </div>
          <div class="setting-row">
            <div><label for="tray-metric">Tray shows</label><p>Which limits the icon counts.</p></div>
            <AppSelect id="tray-metric" :model-value="settings.trayMetric" :options="metricOptions" :disabled="busy" @update:model-value="setMetric"/>
          </div>
          <div v-if="isMac" class="setting-row"><span id="tray-percent-label" class="setting-label">Show percentage next to icon</span><AppSwitch :model-value="settings.trayShowPercent" aria-labelledby="tray-percent-label" :disabled="busy" @update:model-value="patchSettings({ trayShowPercent: $event })"/></div>
          <div class="setting-row">
            <label for="refresh-interval">Refresh while open</label>
            <AppSelect id="refresh-interval" :model-value="String(settings.pollIntervalSeconds)" :options="refreshOptions" :disabled="busy" @update:model-value="setIntervalSetting($event, 'pollIntervalSeconds')"/>
          </div>
          <div class="setting-row">
            <label for="idle-interval">Refresh in background</label>
            <AppSelect id="idle-interval" :model-value="String(settings.idlePollIntervalSeconds)" :options="idleOptions" :disabled="busy" @update:model-value="setIntervalSetting($event, 'idlePollIntervalSeconds')"/>
          </div>
        </section>
        <section class="settings-section">
          <h2>Notifications</h2>
          <div class="setting-row"><span id="notification-label" class="setting-label">Usage alerts</span><AppSwitch :model-value="settings.notifications" aria-labelledby="notification-label" :disabled="busy" @update:model-value="patchSettings({ notifications: $event })"/></div>
          <div class="threshold-options" :class="{ 'is-disabled': !settings.notifications }"><span>Notify at</span><button v-for="threshold in [80, 95, 100]" :key="threshold" class="threshold-chip" :class="{ selected: settings.thresholds.includes(threshold) }" :aria-pressed="settings.thresholds.includes(threshold)" :aria-label="`Notify at ${threshold}% used`" :disabled="busy || !settings.notifications" @click="toggleThreshold(threshold)"><AppIcon v-if="settings.thresholds.includes(threshold)" name="check" :size="11"/>{{ threshold }}%</button><span>used</span></div>
        </section>
        <section class="settings-section">
          <h2>Startup</h2>
          <div class="setting-row"><span id="autostart-label" class="setting-label">Launch at login</span><AppSwitch :model-value="settings.autostart" aria-labelledby="autostart-label" :disabled="busy" @update:model-value="patchSettings({ autostart: $event })"/></div>
        </section>
        <UpdateSettings :state="updateState" :busy="updateBusy" :demo="demo" @check="$emit('checkUpdate')" @install="$emit('installUpdate')"/>
        <p class="settings-footnote">Version {{ version }}</p>
      </TabsContent>

      <TabsContent value="accounts" class="settings-content accounts-settings">
        <div class="section-heading"><div><h2>Your accounts</h2><p>From your CLI logins.</p></div><button class="text-button discover-button" :disabled="busy" @click="$emit('discover')"><AppIcon name="refresh" :size="13"/>Find accounts</button></div>
        <div v-if="!accounts.length" class="settings-empty"><AppIcon name="folder" :size="28"/><p>No accounts connected.</p><span>Find a signed-in Claude or Codex account, or add a config folder below.</span></div>
        <article v-for="account in accounts" :key="account.id" class="managed-account">
          <div class="managed-account-heading"><span class="provider-mark small" :class="account.provider"><AppIcon :name="account.provider" :size="17"/></span><div class="managed-account-copy"><h3>{{ account.label }}</h3><p>{{ account.identity.email ?? providerName(account.provider) }}</p></div><AppTooltip text="Rename account"><button class="icon-button" :aria-label="`Rename ${account.label}`" :disabled="busy" @click="startRename(account)"><AppIcon name="edit" :size="14"/></button></AppTooltip><AppTooltip text="Remove account"><button class="icon-button remove-button" :aria-label="`Remove ${account.label}`" :disabled="busy" @click="openRemove(account, $event)"><AppIcon name="trash" :size="14"/></button></AppTooltip></div>
          <form v-if="renameTarget === account.id" class="rename-form" @submit.prevent="submitRename"><label class="sr-only" :for="`rename-${account.id}`">Account name</label><input :id="`rename-${account.id}`" :ref="setRenameInput" v-model="renameLabel" class="text-input" maxlength="48" autocomplete="off" :disabled="busy" @keydown.esc.stop="renameTarget = null"><button class="button button-small" type="submit" :disabled="busy || !renameLabel.trim()">Save</button><button class="icon-button" type="button" aria-label="Cancel rename" @click="renameTarget = null"><AppIcon name="close" :size="14"/></button></form>
          <AppTooltip :text="accountSource(account)"><div class="account-path" tabindex="0"><AppIcon name="folder" :size="12"/><span>{{ accountSource(account) }}</span></div></AppTooltip>
        </article>
        <button v-if="!adding" ref="addAccountButton" class="add-account-button" :disabled="busy" @click="openAdd"><AppIcon name="plus" :size="16"/>Add account</button>
        <form v-else class="add-account-form" @submit.prevent="submitAccount"><div class="form-heading"><h3>Add account</h3><button class="icon-button" type="button" aria-label="Cancel adding account" @click="adding = false"><AppIcon name="close" :size="15"/></button></div><div class="provider-options"><button v-for="choice in providerSchema.options" :key="choice" type="button" :class="{ selected: provider === choice }" :aria-pressed="provider === choice" @click="provider = choice"><AppIcon :name="choice" :size="16"/>{{ providerName(choice) }}</button></div><label for="account-path">Config folder</label><input id="account-path" ref="pathInput" v-model="path" class="text-input" :placeholder="provider === 'claude' ? '~/.claude-work' : '~/.codex-work'" autocomplete="off" spellcheck="false" :disabled="busy" :aria-invalid="Boolean(formError)" :aria-describedby="formError ? 'path-help path-error' : 'path-help'"><p id="path-help">The folder containing {{ provider === 'claude' ? '.credentials.json' : 'auth.json' }}. Sign in with the CLI first.</p><p v-if="formError" id="path-error" class="field-error" role="alert">{{ formError }}</p><button class="button button-primary" type="submit" :disabled="busy">Connect account<AppIcon name="plus" :size="14"/></button></form>
        <div class="privacy-note"><AppIcon name="check" :size="14"/><p>Margin reads your CLI credentials without changing them.</p></div>
      </TabsContent>
    </TabsRoot>

    <AlertDialogPortal to=".popover">
      <AlertDialogOverlay class="modal-backdrop">
        <AlertDialogContent class="confirm-dialog" @escape-key-down="escapeRemove" @close-auto-focus="restoreRemoveFocus">
          <span class="dialog-icon"><AppIcon name="trash" :size="20"/></span>
          <AlertDialogTitle as="h2">Remove {{ removeTarget?.label }}?</AlertDialogTitle>
          <AlertDialogDescription as="p">Removes the account and its usage history from Margin. CLI credentials are not deleted.</AlertDialogDescription>
          <p v-if="removeFailed" class="field-error" role="alert">Couldn’t remove this account. Try again.</p>
          <div class="dialog-actions"><AlertDialogCancel class="button" tabindex="0" :disabled="busy">Keep account</AlertDialogCancel><button ref="removeButton" class="button button-danger" tabindex="0" :disabled="busy" @click="confirmRemove">Remove account</button></div>
        </AlertDialogContent>
      </AlertDialogOverlay>
    </AlertDialogPortal>
  </AlertDialogRoot>
</template>
