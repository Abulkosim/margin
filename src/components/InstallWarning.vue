<script setup lang="ts">
import AppIcon from './AppIcon.vue'
</script>

<template>
  <p class="install-warning" role="status"><AppIcon name="alert" :size="13"/><span>Move Margin to Applications so Start at login keeps working.</span></p>
</template>

<style scoped>
.install-warning { display: flex; gap: 7px; margin: 0; font-size: 13px; line-height: 1.45; color: var(--warning); }
.install-warning>svg { flex-shrink: 0; margin-top: 2.5px; }
</style>
