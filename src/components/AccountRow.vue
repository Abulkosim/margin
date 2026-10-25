<script setup lang="ts">
import { computed } from 'vue'
import type { Account, Settings, UsageWindow } from '../lib/model'
import { accountIsStale, coreSummary, formatRemaining, providerName, resetClock, resetIn, retryingMessage } from '../lib/format'
import AppIcon from './AppIcon.vue'

const props = defineProps<{ account: Account; settings: Settings; now: number; pinned: boolean; expanded: boolean; connectionError: string | null; controls: string }>()
defineEmits<{ toggle: [] }>()

const stale = computed(() => accountIsStale(props.account, props.settings, props.now, Boolean(props.connectionError)))
const retrying = computed(() => retryingMessage(props.account, props.settings, props.now, Boolean(props.connectionError)))
const refreshing = computed(() => props.account.refreshing && !props.connectionError)
const summary = computed(() => coreSummary(props.account.snapshot?.windows ?? []))
const hasValues = computed(() => Boolean(props.account.snapshot && (summary.value.session || summary.value.weekly)))
const reset = computed(() => summary.value.limiting ? resetIn(summary.value.limiting.resetsAt, props.now) : null)
const plan = computed(() => props.account.identity.plan?.replaceAll('_', ' ').replace(/^./, letter => letter.toUpperCase()))
const status = computed(() => {
  const error = props.account.error
  if (!props.account.snapshot) {
    if (refreshing.value) return 'Checking'
    if (!props.connectionError && error?.kind === 'signedOut') return 'Signed out'
    return props.connectionError || error ? 'Unavailable' : 'Waiting'
  }
  if (stale.value) return !props.connectionError && error?.kind === 'signedOut' ? 'Signed out' : 'Last known'
  return retrying.value ? 'Retrying' : null
})
const tone = (window: UsageWindow | null) => ({ 'quota-low': (window?.usedPercent ?? 0) >= 80, 'quota-empty': (window?.usedPercent ?? 0) >= 100, limiting: window !== null && window === summary.value.limiting })
const value = (window: UsageWindow | null): string => window ? `${formatRemaining(window.usedPercent)}%` : '–'
const limitingName = computed(() => summary.value.limiting === summary.value.session ? 'session' : 'weekly')
const resetLabel = computed(() => {
  if (!reset.value) return null
  if (reset.value === 'Pending') return `${limitingName.value} reset pending`
  const clock = resetClock(summary.value.limiting?.resetsAt ?? null, props.now)
  return `${limitingName.value} resets in ${reset.value}${clock ? `, ${clock}` : ''}`
})
const label = computed(() => {
  const parts = [`${props.account.label}, ${providerName(props.account.provider)}`]
  if (props.account.identity.email) parts.push(props.account.identity.email)
  if (hasValues.value) {
    if (summary.value.session) parts.push(`session ${formatRemaining(summary.value.session.usedPercent)} percent left`)
    if (summary.value.weekly) parts.push(`weekly ${formatRemaining(summary.value.weekly.usedPercent)} percent left`)
    if (resetLabel.value) parts.push(resetLabel.value)
  }
  if (status.value) parts.push(status.value.toLowerCase())
  if (props.pinned) parts.push('pinned to tray')
  return parts.join(', ')
})
</script>

<template>
  <button class="compact-row" :class="{ 'account-stale': stale, 'account-refreshing': refreshing, 'no-values': !hasValues, expanded }" :aria-expanded="expanded" :aria-controls="controls" :aria-label="label" @click="$emit('toggle')">
    <span class="provider-mark compact-mark" :class="account.provider"><AppIcon :name="account.provider" :size="16"/></span>
    <span class="compact-identity">
      <span class="compact-name"><span class="compact-label">{{ account.label }}</span><AppIcon v-if="pinned" name="pin" :size="11" class="compact-pin"/></span>
      <span v-if="status" class="compact-status" :title="retrying ?? undefined">{{ status }}</span>
      <span v-else class="compact-plan">{{ providerName(account.provider) }}<template v-if="plan"> · {{ plan }}</template></span>
    </span>
    <template v-if="hasValues">
      <span class="compact-value" :class="tone(summary.session)">{{ value(summary.session) }}</span>
      <span class="compact-value" :class="tone(summary.weekly)">{{ value(summary.weekly) }}</span>
      <span class="compact-reset">{{ reset ?? '–' }}</span>
    </template>
    <span v-else class="compact-empty">–</span>
    <AppIcon name="chevron" :size="12" class="compact-chevron" :class="{ rotated: expanded }"/>
  </button>
</template>
