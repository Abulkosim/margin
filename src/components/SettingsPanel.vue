<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { accountIdSchema, providerSchema, type Account, type AccountId, type Provider, type Settings } from '../lib/model'
import { accountSource, providerName } from '../lib/format'
import AppIcon from './AppIcon.vue'

const props = defineProps<{
  accounts: Account[]; settings: Settings; busy: boolean; initialTab: 'general' | 'accounts';
  save: (settings: Settings) => Promise<boolean>;
  add: (provider: Provider, path: string) => Promise<boolean>;
  remove: (id: AccountId) => Promise<boolean>;
  rename: (id: AccountId, label: string) => Promise<boolean>;
}>()
defineEmits<{ back: []; discover: [] }>()
const tab = ref(props.initialTab)
const adding = ref(false)
const provider = ref<Provider>('claude')
const path = ref('')
const pathInput = ref<HTMLInputElement | null>(null)
const removeTarget = ref<Account | null>(null)
const renameTarget = ref<AccountId | null>(null)
const renameLabel = ref('')
const renameInput = ref<HTMLInputElement | null>(null)
const keepAccountButton = ref<HTMLButtonElement | null>(null)
const dialog = ref<HTMLElement | null>(null)
const formError = ref<string | null>(null)
const selectedMode = computed(() => props.settings.trayMode === 'pinned' ? props.settings.pinnedAccountId ?? 'lowest' : 'lowest')
const themes: { id: Settings['theme']; label: string; icon: 'system' | 'sun' | 'moon' }[] = [
  { id: 'system', label: 'System', icon: 'system' },
  { id: 'light', label: 'Light', icon: 'sun' },
  { id: 'dark', label: 'Dark', icon: 'moon' },
]

watch(() => props.initialTab, value => { tab.value = value })
watch(removeTarget, async value => {
  if (value) { await nextTick(); keepAccountButton.value?.focus() }
})

function setRenameInput(element: unknown): void {
  renameInput.value = element instanceof HTMLInputElement ? element : null
}

function trapDialogFocus(event: KeyboardEvent): void {
  if (event.key !== 'Tab') return
  const buttons = dialog.value?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')
  const first = buttons?.item(0)
  const last = buttons?.item(buttons.length - 1)
  if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus() }
  else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus() }
}

async function patchSettings(patch: Partial<Settings>): Promise<void> {
  await props.save({ ...props.settings, ...patch })
}

async function setIntervalSetting(event: Event, key: 'pollIntervalSeconds' | 'idlePollIntervalSeconds'): Promise<void> {
  if (!(event.target instanceof HTMLSelectElement)) return
  const value = Number(event.target.value)
  if (Number.isFinite(value)) await patchSettings({ [key]: value })
}

async function setTray(event: Event): Promise<void> {
  if (!(event.target instanceof HTMLSelectElement)) return
  const value = event.target.value
  if (value === 'lowest') await patchSettings({ trayMode: 'lowest', pinnedAccountId: null })
  else {
    const id = accountIdSchema.safeParse(value)
    if (id.success && props.accounts.some(account => account.id === id.data)) await patchSettings({ trayMode: 'pinned', pinnedAccountId: id.data })
  }
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
  if (await props.remove(removeTarget.value.id)) removeTarget.value = null
}
</script>

<template>
  <div class="settings-view">
    <header class="settings-header"><button class="icon-button back-button" aria-label="Back to overview" @click="$emit('back')"><AppIcon name="back" :size="18"/></button><h1>Settings</h1><span class="settings-autosave">Saved automatically</span></header>
    <div class="settings-tabs" role="tablist" aria-label="Settings sections"><button id="general-tab" role="tab" :aria-selected="tab === 'general'" aria-controls="general-panel" :class="{ active: tab === 'general' }" @click="tab = 'general'">General</button><button id="accounts-tab" role="tab" :aria-selected="tab === 'accounts'" aria-controls="accounts-panel" :class="{ active: tab === 'accounts' }" @click="tab = 'accounts'">Accounts <span>{{ accounts.length }}</span></button></div>

    <div v-if="tab === 'general'" id="general-panel" class="settings-content" role="tabpanel" aria-labelledby="general-tab">
      <section class="settings-section"><h2>Appearance</h2><div class="theme-options"><button v-for="theme in themes" :key="theme.id" class="theme-option" :class="{ selected: settings.theme === theme.id }" :aria-pressed="settings.theme === theme.id" :disabled="busy" @click="patchSettings({ theme: theme.id })"><span class="theme-thumbnail" :class="`thumbnail-${theme.id}`"><span/><span/><span/></span><span><AppIcon :name="theme.icon" :size="13"/>{{ theme.label }}</span><span v-if="settings.theme === theme.id" class="theme-check"><AppIcon name="check" :size="10"/></span></button></div></section>
      <section class="settings-section"><h2>Menu bar &amp; refresh</h2><div class="setting-row"><div><label for="tray-account">Tray account</label><p>Shows the lowest remaining window.</p></div><select id="tray-account" :value="selectedMode" :disabled="busy" @change="setTray"><option value="lowest">Lowest remaining</option><option v-for="account in accounts" :key="account.id" :value="account.id">{{ account.label }}</option></select></div><div class="setting-row"><label for="refresh-interval">Refresh while open</label><select id="refresh-interval" :value="settings.pollIntervalSeconds" :disabled="busy" @change="setIntervalSetting($event, 'pollIntervalSeconds')"><option :value="30">30 seconds</option><option :value="60">1 minute</option><option :value="120">2 minutes</option><option :value="300">5 minutes</option></select></div><div class="setting-row"><label for="idle-interval">Refresh in background</label><select id="idle-interval" :value="settings.idlePollIntervalSeconds" :disabled="busy" @change="setIntervalSetting($event, 'idlePollIntervalSeconds')"><option :value="300">5 minutes</option><option :value="600">10 minutes</option><option :value="900">15 minutes</option></select></div></section>
      <section class="settings-section"><h2>Notifications</h2><div class="setting-row"><div><span id="notification-label" class="setting-label">Usage reminders</span><p>A heads-up as a window fills up.</p></div><button class="switch" role="switch" :aria-checked="settings.notifications" aria-labelledby="notification-label" :disabled="busy" @click="patchSettings({ notifications: !settings.notifications })"><span/></button></div><div class="threshold-options" :class="{ 'is-disabled': !settings.notifications }"><span>Notify at</span><button v-for="threshold in [80, 95, 100]" :key="threshold" class="threshold-chip" :class="{ selected: settings.thresholds.includes(threshold) }" :aria-pressed="settings.thresholds.includes(threshold)" :aria-label="`Notify at ${threshold}% used`" :disabled="busy || !settings.notifications" @click="toggleThreshold(threshold)"><AppIcon v-if="settings.thresholds.includes(threshold)" name="check" :size="11"/>{{ threshold }}%</button><span>used</span></div></section>
      <section class="settings-section"><h2>Startup</h2><div class="setting-row"><div><span id="autostart-label" class="setting-label">Launch at login</span><p>Ready when you are.</p></div><button class="switch" role="switch" :aria-checked="settings.autostart" aria-labelledby="autostart-label" :disabled="busy" @click="patchSettings({ autostart: !settings.autostart })"><span/></button></div></section>
      <p class="settings-footnote"><AppIcon name="margin" :size="13"/>Margin 0.1.0<span>Made for a little more clarity.</span></p>
    </div>

    <div v-else id="accounts-panel" class="settings-content accounts-settings" role="tabpanel" aria-labelledby="accounts-tab">
      <div class="section-heading"><div><h2>Your accounts</h2><p>Connected through your local CLI.</p></div><button class="text-button discover-button" :disabled="busy" @click="$emit('discover')"><AppIcon name="refresh" :size="13"/>Find accounts</button></div>
      <div v-if="!accounts.length" class="settings-empty"><AppIcon name="folder" :size="28"/><p>No accounts connected.</p><span>Find a signed-in Claude or Codex account, or add a config folder below.</span></div>
      <article v-for="account in accounts" :key="account.id" class="managed-account">
        <div class="managed-account-heading"><span class="provider-mark small" :class="account.provider"><AppIcon :name="account.provider" :size="17"/></span><div class="managed-account-copy"><h3>{{ account.label }}</h3><p>{{ account.identity.email ?? providerName(account.provider) }}</p></div><button class="icon-button" :aria-label="`Rename ${account.label}`" :disabled="busy" @click="startRename(account)"><AppIcon name="edit" :size="14"/></button><button class="icon-button remove-button" :aria-label="`Remove ${account.label}`" :disabled="busy" @click="removeTarget = account"><AppIcon name="trash" :size="14"/></button></div>
        <form v-if="renameTarget === account.id" class="rename-form" @submit.prevent="submitRename"><label class="sr-only" :for="`rename-${account.id}`">Account name</label><input :id="`rename-${account.id}`" :ref="setRenameInput" v-model="renameLabel" maxlength="48" autocomplete="off" @keydown.esc.stop="renameTarget = null"><button class="button button-small" type="submit" :disabled="busy || !renameLabel.trim()">Save</button><button class="icon-button" type="button" aria-label="Cancel rename" @click="renameTarget = null"><AppIcon name="close" :size="14"/></button></form>
        <div class="account-path" :title="accountSource(account)"><AppIcon name="folder" :size="12"/><span>{{ accountSource(account) }}</span></div>
      </article>
      <button v-if="!adding" class="add-account-button" @click="openAdd"><AppIcon name="plus" :size="16"/>Add account</button>
      <form v-else class="add-account-form" @submit.prevent="submitAccount"><div class="form-heading"><h3>Add account</h3><button class="icon-button" type="button" aria-label="Cancel adding account" @click="adding = false"><AppIcon name="close" :size="15"/></button></div><div class="provider-options"><button v-for="choice in providerSchema.options" :key="choice" type="button" :class="{ selected: provider === choice }" :aria-pressed="provider === choice" @click="provider = choice"><AppIcon :name="choice" :size="16"/>{{ providerName(choice) }}</button></div><label for="account-path">Config folder</label><input id="account-path" ref="pathInput" v-model="path" :placeholder="provider === 'claude' ? '~/.claude-work' : '~/.codex-work'" autocomplete="off" spellcheck="false" :aria-invalid="Boolean(formError)" aria-describedby="path-help"><p id="path-help">The folder containing {{ provider === 'claude' ? '.credentials.json' : 'auth.json' }}. Sign in with the CLI first.</p><p v-if="formError" class="field-error" role="alert">{{ formError }}</p><button class="button button-primary" type="submit" :disabled="busy">Connect account<AppIcon name="plus" :size="14"/></button></form>
      <div class="privacy-note"><AppIcon name="check" :size="14"/><p>Your credentials stay on this device. Margin reads usage without changing your CLI accounts.</p></div>
    </div>

    <div v-if="removeTarget" class="modal-backdrop" @keydown.esc.stop="removeTarget = null" @keydown="trapDialogFocus"><section ref="dialog" class="confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby="remove-title" aria-describedby="remove-description"><span class="dialog-icon"><AppIcon name="trash" :size="20"/></span><h2 id="remove-title">Remove {{ removeTarget.label }}?</h2><p id="remove-description">This removes the account from Margin. Your CLI credentials stay untouched.</p><div class="dialog-actions"><button ref="keepAccountButton" class="button" @click="removeTarget = null">Keep account</button><button class="button button-danger" :disabled="busy" @click="confirmRemove">Remove account</button></div></section></div>
  </div>
</template>
