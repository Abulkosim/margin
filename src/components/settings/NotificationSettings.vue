<script setup lang="ts">
import { computed } from 'vue'
import type { Settings } from '../../lib/model'
import AppIcon from '../AppIcon.vue'
import AppSwitch from '../ui/AppSwitch.vue'

const props = defineProps<{ settings: Settings; busy: boolean }>()
const emit = defineEmits<{ change: [patch: Partial<Settings>] }>()
const presets = [50, 80, 90, 95, 100]
const choices = computed(() => [...new Set([...presets, ...props.settings.thresholds])].sort((a, b) => a - b))
const systemSettings = navigator.userAgent.includes('Mac') ? 'System Settings' : 'Windows Settings'
const blocked = computed(() => props.settings.notifications && props.settings.notificationPermission === 'denied')

function selected(threshold: number): boolean {
  return props.settings.thresholds.includes(threshold)
}

function locked(threshold: number): boolean {
  return selected(threshold) && props.settings.thresholds.length === 1
}

function toggle(threshold: number): void {
  if (locked(threshold)) return
  const thresholds = selected(threshold)
    ? props.settings.thresholds.filter(value => value !== threshold)
    : [...props.settings.thresholds, threshold].sort((a, b) => a - b)
  emit('change', { thresholds })
}
</script>

<template>
  <section class="settings-section">
    <h2>Notifications</h2>
    <div class="setting-row"><span id="notification-label" class="setting-label">Usage alerts</span><AppSwitch :model-value="settings.notifications" aria-labelledby="notification-label" :disabled="busy" @update:model-value="emit('change', { notifications: $event })"/></div>
    <p v-if="blocked" class="notification-blocked" role="status"><AppIcon name="alert" :size="13"/><span>Notifications are off in {{ systemSettings }}.</span></p>
    <div class="threshold-options" :class="{ 'is-disabled': !settings.notifications }"><span>Notify at</span><button v-for="threshold in choices" :key="threshold" class="threshold-chip" :class="{ selected: selected(threshold) }" :aria-pressed="selected(threshold)" :aria-label="`Notify at ${threshold}% used`" :disabled="busy || !settings.notifications || locked(threshold)" @click="toggle(threshold)"><AppIcon v-if="selected(threshold)" name="check" :size="11"/>{{ threshold }}%</button></div>
    <div class="setting-row reset-row" :class="{ 'is-disabled': !settings.notifications }"><div><span id="reset-alert-label" class="setting-label">Notify when limits reset</span><p>Session and weekly, after a usage alert.</p></div><AppSwitch :model-value="settings.notifyOnReset" aria-labelledby="reset-alert-label" :disabled="busy || !settings.notifications" @update:model-value="emit('change', { notifyOnReset: $event })"/></div>
  </section>
</template>

<style scoped>
.notification-blocked { display: flex; align-items: center; gap: 6px; margin: 2px 0 0; font-size: 13px; color: var(--secondary); }
.notification-blocked>svg { flex-shrink: 0; color: var(--warning); }
.threshold-chip:disabled { cursor: default; }
.reset-row { margin-top: 13px; transition: opacity .18s; }
.reset-row.is-disabled { opacity: .5; }
</style>
