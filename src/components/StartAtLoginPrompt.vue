<script setup lang="ts">
import InstallWarning from './InstallWarning.vue'

defineProps<{ warning: boolean; busy: boolean }>()
defineEmits<{ accept: []; dismiss: [] }>()
</script>

<template>
  <aside class="start-prompt" aria-labelledby="start-prompt-title">
    <div class="start-prompt-main">
      <div class="start-prompt-copy">
        <h2 id="start-prompt-title">Start at login</h2>
        <p>Keeps alerts running after a restart.</p>
      </div>
      <div class="start-prompt-actions">
        <button class="text-button" :disabled="busy" @click="$emit('dismiss')">Not now</button>
        <button class="button button-small button-primary" :disabled="busy" @click="$emit('accept')">Turn on</button>
      </div>
    </div>
    <InstallWarning v-if="warning" class="start-prompt-warning"/>
  </aside>
</template>

<style scoped>
.start-prompt { flex-shrink: 0; margin: 0 22px; padding: 11px 12px 11px 14px; border-radius: 10px; background: var(--accent-soft); animation: start-prompt-in .2s ease-out; }
.start-prompt-main { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 6px 12px; }
.start-prompt-copy { flex: 1 1 170px; min-width: 0; }
.start-prompt h2 { margin: 0; font-size: 14px; font-weight: 550; line-height: 1.35; color: var(--ink); }
.start-prompt-copy p { margin: 1px 0 0; font-size: 13px; line-height: 1.4; color: var(--secondary); }
.start-prompt-actions { display: flex; align-items: center; gap: 12px; flex-shrink: 0; }
.start-prompt .start-prompt-warning { margin-top: 10px; padding-top: 9px; border-top: 1px solid var(--line); color: var(--secondary); }
.start-prompt-warning :deep(svg) { color: var(--warning); }
@keyframes start-prompt-in { from { opacity: 0; transform: translateY(4px); } }
@media (prefers-reduced-motion: reduce) { .start-prompt { animation: none; } }
</style>
