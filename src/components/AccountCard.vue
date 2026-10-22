<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import type { Account, Settings } from '../lib/model'
import { accountIsStale, formatMoney, providerName, relativeAge, signedOutMessage } from '../lib/format'
import AppIcon from './AppIcon.vue'
import QuotaWindow from './QuotaWindow.vue'
import UsageHistory from './UsageHistory.vue'
import AppTooltip from './ui/AppTooltip.vue'

const props = defineProps<{ account: Account; demo: boolean; settings: Settings; now: number; pinned: boolean; busy: boolean; connectionError: string | null }>()
defineEmits<{ pin: []; refresh: [] }>()
const expanded = ref(false)
const historyOpen = ref(false)
const stale = computed(() => accountIsStale(props.account, props.settings, props.now, Boolean(props.connectionError)))
const refreshing = computed(() => props.account.refreshing && !props.connectionError)
const errorMessage = computed(() => props.connectionError ?? signedOutMessage(props.account, props.now) ?? props.account.error?.message)
const signInCommand = computed(() => props.connectionError ? null : props.account.error?.signInCommand ?? null)
const copied = ref(false)
let copiedTimer: ReturnType<typeof setTimeout> | undefined
onBeforeUnmount(() => clearTimeout(copiedTimer))

async function copyCommand() {
  const command = signInCommand.value
  if (!command) return
  try {
    await navigator.clipboard.writeText(command)
  } catch {
    const field = document.createElement('textarea')
    field.value = command
    field.setAttribute('readonly', '')
    field.style.position = 'fixed'
    field.style.opacity = '0'
    document.body.append(field)
    field.select()
    document.execCommand('copy')
    field.remove()
  }
  copied.value = true
  clearTimeout(copiedTimer)
  copiedTimer = setTimeout(() => { copied.value = false }, 1600)
}
const extraWindows = computed(() => props.account.snapshot?.windows.slice(2) ?? [])
const hasDetails = computed(() => extraWindows.value.length > 0 || props.account.snapshot?.extraUsage)
const plan = computed(() => props.account.identity.plan?.replaceAll('_', ' ').replace(/^./, letter => letter.toUpperCase()))
</script>

<template>
  <article class="account-card" :class="{ 'account-stale': stale, 'account-refreshing': refreshing }" :aria-label="`${account.label}, ${providerName(account.provider)}`">
    <div class="account-heading">
      <span class="provider-mark" :class="account.provider"><AppIcon :name="account.provider" :size="20" /></span>
      <div class="account-heading-copy">
        <AppTooltip :text="account.identity.email ?? account.label"><h2 tabindex="0">{{ account.label }}</h2></AppTooltip>
        <p>{{ providerName(account.provider) }}<template v-if="plan"><span class="middot">·</span>{{ plan }}</template></p>
      </div>
      <span v-if="stale && account.snapshot" class="stale-badge">Last known</span>
      <AppTooltip :text="pinned ? 'Pinned to tray' : 'Show this account in tray'"><button class="icon-button pin-button" :class="{ 'is-pinned': pinned }" :aria-label="pinned ? `Unpin ${account.label} from tray` : `Pin ${account.label} to tray`" :disabled="busy" :aria-pressed="pinned" @click="$emit('pin')"><AppIcon name="pin" :size="14" /></button></AppTooltip>
    </div>

    <template v-if="account.snapshot">
      <div v-if="account.snapshot.windows.length" class="quota-list">
        <QuotaWindow v-for="window in account.snapshot.windows.slice(0, 2)" :key="window.id" :window="window" :now="now" :stale="stale" />
        <template v-if="expanded"><QuotaWindow v-for="window in extraWindows" :key="window.id" :window="window" :now="now" :stale="stale" /></template>
      </div>
      <p v-else class="inline-empty">No usage windows reported by {{ providerName(account.provider) }}.</p>
      <div v-if="expanded && account.snapshot.extraUsage" class="extra-usage">
        <span>Extra usage <span class="muted">this month</span></span>
        <span>{{ formatMoney(account.snapshot.extraUsage.spent, account.snapshot.extraUsage.currency) }}<span v-if="account.snapshot.extraUsage.limit !== null" class="muted"> / {{ formatMoney(account.snapshot.extraUsage.limit, account.snapshot.extraUsage.currency) }}</span></span>
      </div>
      <button v-if="hasDetails" class="details-toggle" :aria-expanded="expanded" @click="expanded = !expanded">
        <span>{{ expanded ? 'Show less' : extraWindows.length ? `${extraWindows.length} more ${extraWindows.length === 1 ? 'window' : 'windows'}` : 'Extra usage' }}</span><AppIcon name="chevron" :size="12" :class="{ rotated: expanded }" />
      </button>
      <div v-if="stale" class="stale-note"><AppIcon name="alert" :size="13" /><p><span>Updated {{ relativeAge(account.snapshot.fetchedAt, now) }}.</span> {{ errorMessage ?? 'Waiting for a fresh reading.' }}</p></div>
      <AppTooltip v-if="stale && signInCommand" :text="signInCommand"><button class="text-button sign-in-copy" :aria-label="copied ? 'Command copied' : `Copy command: ${signInCommand}`" @click="copyCommand">{{ copied ? 'Copied' : 'Copy command' }} <AppIcon :name="copied ? 'check' : 'copy'" :size="12" /></button></AppTooltip>
    </template>

    <div v-else-if="refreshing" class="quota-loading" aria-label="Loading account usage" role="status"><div class="skeleton skeleton-label"/><div class="skeleton skeleton-bar"/><div class="skeleton skeleton-label"/><div class="skeleton skeleton-bar"/></div>
    <div v-else class="account-error">
      <div class="account-error-title"><AppIcon name="alert" :size="15" /><span>{{ errorMessage ? 'Usage unavailable' : 'Waiting for usage' }}</span></div>
      <p>{{ errorMessage ?? 'Refresh to read this account’s usage.' }}</p>
      <div class="account-error-actions">
        <AppTooltip v-if="signInCommand" :text="signInCommand"><button class="text-button" :aria-label="copied ? 'Command copied' : `Copy command: ${signInCommand}`" @click="copyCommand">{{ copied ? 'Copied' : 'Copy command' }} <AppIcon :name="copied ? 'check' : 'copy'" :size="12" /></button></AppTooltip>
        <button class="text-button" :disabled="busy" @click="$emit('refresh')">Try again <AppIcon name="refresh" :size="12" /></button>
      </div>
    </div>
    <button class="details-toggle history-toggle" :aria-expanded="historyOpen" :aria-controls="`history-${account.id}`" @click="historyOpen = !historyOpen"><span>{{ historyOpen ? 'Hide history' : 'History' }}</span><AppIcon name="chevron" :size="12" :class="{ rotated: historyOpen }" /></button>
    <UsageHistory v-if="historyOpen" :id="`history-${account.id}`" :account="account" :demo="demo" :now="now" />
  </article>
</template>
