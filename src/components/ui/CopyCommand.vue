<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue'
import AppIcon from '../AppIcon.vue'

const props = defineProps<{ command: string }>()
const copied = ref(false)
let timer: ReturnType<typeof setTimeout> | undefined
onBeforeUnmount(() => clearTimeout(timer))

async function copy(): Promise<void> {
  try {
    await navigator.clipboard.writeText(props.command)
  } catch {
    const field = document.createElement('textarea')
    field.value = props.command
    field.setAttribute('readonly', '')
    field.style.position = 'fixed'
    field.style.opacity = '0'
    document.body.append(field)
    field.select()
    document.execCommand('copy')
    field.remove()
  }
  copied.value = true
  clearTimeout(timer)
  timer = setTimeout(() => { copied.value = false }, 1500)
}
</script>

<template>
  <button type="button" class="command-copy" :class="{ copied }" :aria-label="copied ? 'Command copied' : `Copy command: ${command}`" @click="copy">
    <code>{{ command }}</code>
    <span class="command-copy-state" aria-hidden="true"><span v-if="copied">Copied</span><AppIcon :name="copied ? 'check' : 'copy'" :size="13"/></span>
  </button>
</template>
