<script setup lang="ts">
import { formatRemaining, remainingPercent, resetCountdown } from '../lib/format'
import type { UsageWindow } from '../lib/model'

defineProps<{ window: UsageWindow; now: number; stale: boolean }>()
</script>

<template>
  <div class="quota-window" :class="{ 'quota-low': window.usedPercent >= 80, 'quota-empty': window.usedPercent >= 100 }">
    <div class="quota-meta">
      <span class="quota-label">{{ window.label }}</span>
      <span class="quota-reset" :title="window.resetsAt ? new Date(window.resetsAt).toLocaleString() : undefined">{{ resetCountdown(window.resetsAt, now) }}</span>
      <span class="quota-value">{{ formatRemaining(window.usedPercent) }}<span>%</span></span>
    </div>
    <div class="quota-track" role="progressbar" :aria-label="`${window.label} remaining${stale ? ', last known' : ''}`" :aria-valuenow="remainingPercent(window.usedPercent)" :aria-valuemin="0" :aria-valuemax="100" :aria-valuetext="`${formatRemaining(window.usedPercent)} percent remaining${stale ? ', last known' : ''}`">
      <div class="quota-fill" :style="{ width: `${remainingPercent(window.usedPercent)}%` }" />
    </div>
  </div>
</template>
