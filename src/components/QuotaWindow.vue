<script setup lang="ts">
import { computed } from 'vue'
import { formatRemaining, fullDateTime, remainingPercent, resetClock, resetCountdown, resetPassedText, unreadSinceReset } from '../lib/format'
import type { UsageWindow } from '../lib/model'
import AppTooltip from './ui/AppTooltip.vue'

const props = defineProps<{ window: UsageWindow; fetchedAt: string; now: number; stale: boolean; pace?: string | null }>()
const unread = computed(() => unreadSinceReset(props.window, props.fetchedAt, props.now))
const clock = computed(() => resetClock(props.window.resetsAt, props.now))
const resetTooltip = computed(() => fullDateTime(props.window.resetsAt))
</script>

<template>
  <div v-if="unread" class="quota-window quota-unread">
    <div class="quota-meta">
      <span class="quota-label">{{ window.label }}</span>
      <AppTooltip :text="resetTooltip"><span class="quota-reset">{{ resetPassedText(window, now) }}</span></AppTooltip>
    </div>
    <div class="quota-track" aria-hidden="true" />
  </div>
  <div v-else class="quota-window" :class="{ 'quota-low': window.usedPercent >= 80, 'quota-empty': window.usedPercent >= 100 }">
    <div class="quota-meta">
      <span class="quota-label">{{ window.label }}</span>
      <AppTooltip :text="resetTooltip"><span class="quota-reset">{{ resetCountdown(window.resetsAt, now) }}<span v-if="clock" class="quota-reset-at"> · {{ clock }}</span></span></AppTooltip>
      <span class="quota-value">{{ formatRemaining(window.usedPercent) }}<span>%</span></span>
    </div>
    <div class="quota-track" role="progressbar" :aria-label="`${window.label} remaining${stale ? ', last known' : ''}`" :aria-valuenow="remainingPercent(window.usedPercent)" :aria-valuemin="0" :aria-valuemax="100" :aria-valuetext="`${formatRemaining(window.usedPercent)} percent remaining${stale ? ', last known' : ''}`">
      <div class="quota-fill" :style="{ width: `${remainingPercent(window.usedPercent)}%` }" />
    </div>
    <p v-if="pace" class="quota-pace">{{ pace }}</p>
  </div>
</template>
