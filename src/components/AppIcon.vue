<script setup lang="ts">
defineProps<{ name: 'margin' | 'refresh' | 'settings' | 'plus' | 'back' | 'chevron' | 'check' | 'copy' | 'close' | 'pin' | 'claude' | 'codex' | 'folder' | 'edit' | 'trash' | 'alert' | 'sun' | 'moon' | 'system' | 'external' | 'swap'; size?: number }>()
</script>

<template>
  <svg :width="size ?? 18" :height="size ?? 18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.65" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <template v-if="name === 'margin'"><path d="M19.15 6.3a9 9 0 1 0 1.45 8.3" stroke-width="2.6"/><path d="M8.5 12h7" stroke-width="2.6"/></template>
    <template v-else-if="name === 'refresh'"><path d="M20 10a8.1 8.1 0 0 0-14-4L3 9m0-6v6h6M4 14a8.1 8.1 0 0 0 14 4l3-3m0 6v-6h-6"/></template>
    <template v-else-if="name === 'settings'"><path d="m9 3-.7 2.2-2 .9-2.1-.5L2.7 8.2l1.5 1.7v2.3l-1.5 1.7 1.5 2.6 2.1-.5 2 .9L9 19h3l.7-2.1 2-.9 2.1.5 1.5-2.6-1.5-1.7V9.9l1.5-1.7-1.5-2.6-2.1.5-2-.9L12 3z" transform="translate(1.5 1)"/><circle cx="12" cy="12" r="3"/></template>
    <path v-else-if="name === 'plus'" d="M12 5v14M5 12h14"/>
    <path v-else-if="name === 'back'" d="m14 6-6 6 6 6M8 12h12"/>
    <path v-else-if="name === 'chevron'" d="m8 10 4 4 4-4"/>
    <path v-else-if="name === 'check'" d="m5 12 4 4L19 6"/>
    <template v-else-if="name === 'copy'"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/></template>
    <path v-else-if="name === 'close'" d="m6 6 12 12M6 18 18 6"/>
    <path v-else-if="name === 'pin'" d="m16 3 5 5-4 1-4 5-1 4-6-6 4-1 5-4zM8 16l-5 5"/>
    <g v-else-if="name === 'claude'" stroke-width="1.8"><path d="m12 3 .2 18M3 12l18-.3M5.8 5.7l12.4 12.5M5.6 18.1 18.4 5.7M8.6 3.8l6.8 16.4M3.8 15.4l16.4-6.8M3.7 8.6l16.6 6.8M8.6 20.3l6.8-16.6"/></g>
    <template v-else-if="name === 'codex'"><path d="m8 7-5 5 5 5m8-10 5 5-5 5m-3-12-2 14" stroke-width="1.9"/></template>
    <path v-else-if="name === 'folder'" d="M3 7V5a1 1 0 0 1 1-1h5l2 3h9a1 1 0 0 1 1 1v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z"/>
    <template v-else-if="name === 'edit'"><path d="m15 4 5 5M4 20l5-1L21 7a2 2 0 0 0-5-4L4 15z"/></template>
    <template v-else-if="name === 'trash'"><path d="M3 6h18M8 6V3h8v3M5 6l1 14h12l1-14M10 10v6m4-6v6"/></template>
    <template v-else-if="name === 'alert'"><circle cx="12" cy="12" r="9"/><path d="M12 7v6m0 4h.01"/></template>
    <template v-else-if="name === 'sun'"><circle cx="12" cy="12" r="4"/><path d="M12 2v2m0 16v2M2 12h2m16 0h2M5 5l1 1m12 12 1 1M5 19l1-1M18 6l1-1"/></template>
    <path v-else-if="name === 'moon'" d="M20 14a8 8 0 0 1-10-10A9 9 0 1 0 20 14Z"/>
    <template v-else-if="name === 'system'"><rect x="3" y="4" width="18" height="13" rx="2"/><path d="M8 21h8m-4-4v4"/></template>
    <path v-else-if="name === 'swap'" d="m16 3 4 4-4 4M20 7H8M8 21l-4-4 4-4M4 17h12"/>
    <template v-else-if="name === 'external'"><path d="M14 3h7v7M21 3l-11 11M10 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-5"/></template>
  </svg>
</template>
