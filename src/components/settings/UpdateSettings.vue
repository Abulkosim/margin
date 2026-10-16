<script setup lang="ts">
import { computed } from 'vue'
import { version } from '../../../package.json'
import type { UpdateState } from '../../lib/updater'
import AppIcon from '../AppIcon.vue'

const props = defineProps<{ state: UpdateState; busy: boolean; demo: boolean }>()
const emit = defineEmits<{ check: []; install: [] }>()
const progress = computed(() => props.state.kind === 'downloading' && props.state.total
  ? Math.min(100, Math.round(props.state.downloaded / props.state.total * 100)) : undefined)
const copy = computed(() => {
  if (props.demo) return 'Update checks require the desktop app.'
  const state = props.state
  switch (state.kind) {
    case 'idle': return 'Updates are checked manually.'
    case 'checking': return 'Checking for updates…'
    case 'current': return 'Up to date.'
    case 'available': return `Version ${state.version} available.`
    case 'downloading': return `Downloading ${state.version}…`
    case 'installing': return 'Installing. Margin will restart.'
    case 'error': return state.message
    default: { const exhaustive: never = state; return exhaustive }
  }
})
const action = computed(() => {
  if (props.state.kind === 'available') return 'Install and restart'
  if (props.state.kind === 'downloading') return progress.value === undefined ? 'Downloading…' : `${progress.value}% downloaded`
  if (props.state.kind === 'installing') return 'Installing…'
  if (props.busy) return 'Checking…'
  return 'Check for updates'
})
function activate(): void {
  if (props.busy || props.demo) return
  if (props.state.kind === 'available') emit('install')
  else emit('check')
}
</script>

<template>
  <section class="settings-section update-settings" aria-labelledby="updates-title">
    <h2 id="updates-title">Updates</h2>
    <div class="update-card">
      <div class="update-heading"><span class="update-mark"><AppIcon name="margin" :size="19"/></span><div><span class="setting-label">Margin</span><span class="update-version">Version {{ version }}</span></div><AppIcon v-if="state.kind === 'current' && !demo" class="update-current" name="check" :size="16"/></div>
      <p class="update-message" :class="{ 'update-error': state.kind === 'error' }" :role="state.kind === 'error' ? 'alert' : 'status'">{{ copy }}</p>
      <div v-if="state.kind === 'downloading'" class="update-progress" :class="{ indeterminate: progress === undefined }" role="progressbar" aria-label="Update download" :aria-valuenow="progress" :aria-valuemin="0" :aria-valuemax="100"><span :style="{ width: progress === undefined ? '35%' : `${progress}%` }"/></div>
      <button class="button button-small update-action" :class="{ 'button-primary': state.kind === 'available' }" :aria-disabled="busy || demo" :aria-busy="busy" @click="activate">{{ action }}</button>
    </div>
  </section>
</template>

<style scoped>
.update-card { background: var(--card); padding: 14px; border-radius: 9px; }
.update-heading { display: flex; align-items: center; gap: 9px; }
.update-heading>div { display: flex; flex-direction: column; gap: 3px; }
.update-mark { display: grid; place-items: center; width: 33px; height: 33px; background: var(--accent-soft); color: var(--accent-ink); border-radius: 8px; }
.update-version { font-size: 9px; color: var(--tertiary); font-variant-numeric: tabular-nums; }
.update-current { margin-left: auto; color: var(--accent); }
.update-message { font-size: 11px; line-height: 1.5; margin: 12px 0; color: var(--secondary); overflow-wrap: anywhere; }
.update-error { color: var(--danger); }
.update-action { min-height: 30px; }
.update-action[aria-disabled="true"] { opacity: .55; cursor: default; }
.update-progress { overflow: hidden; height: 4px; background: var(--track); border-radius: 3px; margin: 0 0 12px; }
.update-progress>span { display: block; height: 100%; background: var(--accent); border-radius: inherit; transition: width .2s ease; }
.update-progress.indeterminate>span { animation: update-travel 1.6s ease-in-out infinite alternate; }
@keyframes update-travel { to { transform: translateX(185%); } }
@media (prefers-reduced-motion: reduce) { .update-progress>span { transition: none; animation: none !important; } }
</style>
