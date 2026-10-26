<script setup lang="ts">
import { computed, ref } from 'vue'
import type { Account, Settings, UsageWindow } from '../lib/model'
import { accountIsStale, balanceLines, formatRemaining, groupWindows, modelWindowSummary, paceText, providerName, relativeAge, remainingPercent, resetCountdown, resetGrantsText, retryingMessage, signedOutMessage } from '../lib/format'
import { useAccountHistory } from '../lib/accountHistory'
import { fitPace, paceWindowIds, projectPace } from '../lib/pace'
import AppIcon from './AppIcon.vue'
import QuotaWindow from './QuotaWindow.vue'
import UsageHistory from './UsageHistory.vue'
import AppTooltip from './ui/AppTooltip.vue'
import CopyCommand from './ui/CopyCommand.vue'

const props = defineProps<{ account: Account; demo: boolean; settings: Settings; now: number; pinned: boolean; active?: boolean; busy: boolean; connectionError: string | null }>()
defineEmits<{ pin: []; refresh: [] }>()
const expanded = ref(false)
const historyOpen = ref(false)
const { state: historyState, load: loadHistory } = useAccountHistory(() => props.account, () => props.demo)
const paceFits = computed(() => {
  const points = historyState.value.kind === 'ready' ? historyState.value.history.points : []
  return new Map((props.account.snapshot?.windows ?? []).filter(window => paceWindowIds.has(window.id)).map(window => [window.id, fitPace(points.filter(point => point.windowId === window.id))]))
})
const pace = (window: UsageWindow): string | null => stale.value || !paceFits.value.has(window.id) ? null : paceText(projectPace(paceFits.value.get(window.id) ?? null, window, props.now), props.now)
const stale = computed(() => accountIsStale(props.account, props.settings, props.now, Boolean(props.connectionError)))
const retrying = computed(() => retryingMessage(props.account, props.settings, props.now, Boolean(props.connectionError)))
const refreshing = computed(() => props.account.refreshing && !props.connectionError)
const signedOut = computed(() => props.connectionError ? null : signedOutMessage(props.account, props.now))
const errorMessage = computed(() => props.connectionError ?? signedOut.value ?? props.account.error?.message)
const showUpdated = computed(() => !signedOut.value || !props.account.error?.signedOutSince)
const signInCommand = computed(() => props.connectionError ? null : props.account.error?.signInCommand ?? null)
const groups = computed(() => groupWindows(props.account.snapshot?.windows ?? []))
const extraWindows = computed(() => groups.value.more)
const spokenReset = (window: UsageWindow): string => resetCountdown(window.resetsAt, props.now).toLowerCase()
const resetGrants = computed(() => resetGrantsText(props.account.snapshot?.resetGrants, props.now))
const balances = computed(() => balanceLines(props.account.snapshot, props.now))
const hasDetails = computed(() => extraWindows.value.length > 0 || balances.value.length > 0)
const detailsLabel = computed(() => {
  const count = extraWindows.value.length
  if (count) return `${count} more ${count === 1 ? 'window' : 'windows'}`
  return props.account.snapshot?.balance ? 'Balance' : 'Extra usage'
})
const plan = computed(() => props.account.identity.plan?.replaceAll('_', ' ').replace(/^./, letter => letter.toUpperCase()))
</script>

<template>
  <article class="account-card" tabindex="-1" :data-account-id="account.id" :class="{ 'account-stale': stale, 'account-refreshing': refreshing }" :aria-label="`${account.label}, ${providerName(account.provider)}`">
    <div class="account-heading">
      <span class="provider-mark" :class="account.provider"><AppIcon :name="account.provider" :size="20" /></span>
      <div class="account-heading-copy">
        <div class="account-title"><h2>{{ account.label }}</h2><span v-if="active" class="active-tag">Active</span></div>
        <p class="account-subline"><span class="account-email">{{ account.identity.email ?? providerName(account.provider) }}</span><template v-if="plan"><span class="middot">·</span><span class="account-plan">{{ plan }}</span></template></p>
      </div>
      <span v-if="stale && account.snapshot" class="stale-badge">Last known</span>
      <AppTooltip :text="pinned ? 'Pinned to tray' : 'Show this account in tray'"><button class="icon-button pin-button" :class="{ 'is-pinned': pinned }" :aria-label="pinned ? `Unpin ${account.label} from tray` : `Pin ${account.label} to tray`" :disabled="busy" :aria-pressed="pinned" @click="$emit('pin')"><AppIcon name="pin" :size="14" /></button></AppTooltip>
    </div>

    <template v-if="account.snapshot">
      <div v-if="account.snapshot.windows.length" class="quota-list">
        <QuotaWindow v-for="window in groups.rows" :key="window.id" :window="window" :now="now" :stale="stale" :pace="pace(window)" />
        <ul v-if="groups.models.length" class="model-chips" aria-label="Model limits">
          <li v-for="window in groups.models" :key="window.id">
            <AppTooltip :text="modelWindowSummary(window, now)">
              <span class="model-chip" :class="{ 'quota-low': window.usedPercent >= 80, 'quota-empty': window.usedPercent >= 100 }" tabindex="0" role="progressbar" :aria-label="`${window.label} weekly remaining${stale ? ', last known' : ''}`" :aria-valuenow="remainingPercent(window.usedPercent)" :aria-valuemin="0" :aria-valuemax="100" :aria-valuetext="`${formatRemaining(window.usedPercent)} percent remaining, ${spokenReset(window)}${stale ? ', last known' : ''}`">
                <span class="model-chip-name">{{ window.label }}</span>
                <span class="model-chip-value">{{ formatRemaining(window.usedPercent) }}%</span>
                <span class="model-chip-meter"><span :style="{ width: `${remainingPercent(window.usedPercent)}%` }" /></span>
              </span>
            </AppTooltip>
          </li>
        </ul>
        <template v-if="expanded"><QuotaWindow v-for="window in extraWindows" :key="window.id" :window="window" :now="now" :stale="stale" :pace="pace(window)" /></template>
      </div>
      <p v-else class="inline-empty">No usage windows reported by {{ providerName(account.provider) }}.</p>
      <p v-if="resetGrants" class="reset-grants">{{ resetGrants }}</p>
      <div v-if="expanded && balances.length" class="balance-lines">
        <p v-for="line in balances" :key="line.label" class="balance-line" :class="{ 'balance-warn': line.warn }"><span class="balance-label">{{ line.label }}</span> <span class="balance-value">{{ line.value }}</span></p>
      </div>
      <button v-if="hasDetails" class="details-toggle" :aria-expanded="expanded" @click="expanded = !expanded">
        <span>{{ expanded ? 'Show less' : detailsLabel }}</span><AppIcon name="chevron" :size="12" :class="{ rotated: expanded }" />
      </button>
      <p v-if="retrying" class="retry-note" role="status"><AppIcon name="refresh" :size="12" />{{ retrying }}</p>
      <div v-if="stale" class="stale-note"><AppIcon name="alert" :size="13" /><p><span v-if="showUpdated">Updated {{ relativeAge(account.snapshot.fetchedAt, now) }}. </span><span v-if="signedOut">{{ signedOut }}</span><template v-else>{{ errorMessage ?? 'Waiting for a fresh reading.' }}</template></p></div>
      <CopyCommand v-if="stale && signInCommand" class="sign-in-copy" :command="signInCommand" />
    </template>

    <div v-else-if="refreshing" class="quota-loading" aria-label="Loading account usage" role="status"><div class="skeleton skeleton-label"/><div class="skeleton skeleton-bar"/><div class="skeleton skeleton-label"/><div class="skeleton skeleton-bar"/></div>
    <div v-else class="account-error">
      <div class="account-error-title"><AppIcon name="alert" :size="15" /><span>{{ errorMessage ? 'Usage unavailable' : 'Waiting for usage' }}</span></div>
      <p>{{ errorMessage ?? 'Refresh to read this account’s usage.' }}</p>
      <CopyCommand v-if="signInCommand" :command="signInCommand" />
      <div class="account-error-actions">
        <button class="text-button" :disabled="busy" @click="$emit('refresh')">Try again <AppIcon name="refresh" :size="12" /></button>
      </div>
    </div>
    <button class="details-toggle history-toggle" :aria-expanded="historyOpen" :aria-controls="`history-${account.id}`" @click="historyOpen = !historyOpen"><span>{{ historyOpen ? 'Hide history' : 'History' }}</span><AppIcon name="chevron" :size="12" :class="{ rotated: historyOpen }" /></button>
    <UsageHistory v-if="historyOpen" :id="`history-${account.id}`" :account="account" :state="historyState" :now="now" @retry="loadHistory" />
  </article>
</template>
