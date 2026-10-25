<script setup lang="ts">
import { formatRemaining, remainingPercent, resetClock, resetCountdown } from '../lib/format'
import type { UsageWindow } from '../lib/model'
import AppTooltip from './ui/AppTooltip.vue'

defineProps<{ window: UsageWindow; now: number; stale: boolean; pace?: string | null }>()
</script>

<template>
  <div class="quota-window" :class="{ 'quota-low': window.usedPercent >= 80, 'quota-empty': window.usedPercent >= 100 }">
    <div class="quota-meta">
      <span class="quota-label">{{ window.label }}</span>
      <AppTooltip :text="window.resetsAt ? new Date(window.resetsAt).toLocaleString() : ''"><span class="quota-reset">{{ resetCountdown(window.resetsAt, now) }}<span v-if="resetClock(window.resetsAt, now)" class="quota-reset-at"> · {{ resetClock(window.resetsAt, now) }}</span></span></AppTooltip>
      <span class="quota-value">{{ formatRemaining(window.usedPercent) }}<span>%</span></span>
    </div>
    <div class="quota-track" role="progressbar" :aria-label="`${window.label} remaining${stale ? ', last known' : ''}`" :aria-valuenow="remainingPercent(window.usedPercent)" :aria-valuemin="0" :aria-valuemax="100" :aria-valuetext="`${formatRemaining(window.usedPercent)} percent remaining${stale ? ', last known' : ''}`">
      <div class="quota-fill" :style="{ width: `${remainingPercent(window.usedPercent)}%` }" />
    </div>
    <p v-if="pace" class="quota-pace">{{ pace }}</p>
  </div>
</template>
