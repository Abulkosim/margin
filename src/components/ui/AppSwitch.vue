<script setup lang="ts">
import { SwitchRoot, SwitchThumb } from 'reka-ui'
import { nextTick, ref, watch } from 'vue'

defineOptions({ inheritAttrs: false })
const props = withDefaults(defineProps<{ modelValue: boolean; disabled?: boolean }>(), { disabled: false })
const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>()
const control = ref<HTMLButtonElement | null>(null)
let restoreFocusAfterSave = false

function setControl(value: unknown) {
  control.value = value && typeof value === 'object' && '$el' in value && value.$el instanceof HTMLButtonElement ? value.$el : null
}

function updateValue(value: unknown) {
  if (typeof value === 'boolean') emit('update:modelValue', value)
}

watch(() => props.disabled, async disabled => {
  if (disabled) {
    restoreFocusAfterSave = document.activeElement === control.value
    return
  }
  if (!restoreFocusAfterSave) return
  restoreFocusAfterSave = false
  await nextTick()
  if (document.activeElement === document.body) control.value?.focus({ preventScroll: true })
})
</script>

<template>
  <SwitchRoot :ref="setControl" v-bind="$attrs" :model-value="modelValue" :disabled="disabled" class="switch app-switch" @update:model-value="updateValue">
    <SwitchThumb class="app-switch-thumb" />
  </SwitchRoot>
</template>
