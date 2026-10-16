<script setup lang="ts">
import { ref, watch } from 'vue'
import { TooltipContent, TooltipPortal, TooltipRoot, TooltipTrigger } from 'reka-ui'

defineOptions({ inheritAttrs: false })
const props = defineProps<{ text: string }>()
const open = ref(false)

function dismissOnEscape(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  event.preventDefault()
  event.stopPropagation()
  open.value = false
}

watch(() => props.text, text => {
  if (!text) open.value = false
})

watch(open, (isOpen, _wasOpen, onCleanup) => {
  if (!isOpen) return
  document.addEventListener('keydown', dismissOnEscape, true)
  onCleanup(() => document.removeEventListener('keydown', dismissOnEscape, true))
})
</script>

<template>
  <TooltipRoot v-model:open="open" :disabled="!text">
    <TooltipTrigger as-child v-bind="$attrs"><slot /></TooltipTrigger>
    <TooltipPortal to="body">
      <TooltipContent class="app-tooltip-content" :side-offset="6" :collision-padding="12" @escape-key-down="dismissOnEscape">
        {{ text }}
      </TooltipContent>
    </TooltipPortal>
  </TooltipRoot>
</template>
