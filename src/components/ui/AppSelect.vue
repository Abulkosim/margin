<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import {
  SelectContent,
  SelectIcon,
  SelectItem,
  SelectItemIndicator,
  SelectItemText,
  SelectPortal,
  SelectRoot,
  SelectTrigger,
  SelectValue,
  SelectViewport,
} from 'reka-ui'
import AppIcon from '../AppIcon.vue'

defineOptions({ inheritAttrs: false })

const props = withDefaults(defineProps<{
  modelValue: string
  options: { value: string; label: string; description?: string }[]
  id?: string
  label?: string
  disabled?: boolean
  placeholder?: string
}>(), { disabled: false, placeholder: 'Choose an option' })

const emit = defineEmits<{ 'update:modelValue': [value: string] }>()
const open = ref(false)
const trigger = ref<HTMLButtonElement | null>(null)
let restoreFocusAfterSave = false

function setTrigger(value: unknown) {
  trigger.value = value && typeof value === 'object' && '$el' in value && value.$el instanceof HTMLButtonElement ? value.$el : null
}

function updateValue(value: unknown) {
  if (typeof value === 'string') emit('update:modelValue', value)
}

function dismissOnEscape(event: KeyboardEvent) {
  if (event.key !== 'Escape') return
  event.preventDefault()
  event.stopPropagation()
  open.value = false
}

function restoreTriggerFocus(event: Event) {
  if (!props.disabled) return
  event.preventDefault()
  restoreFocusAfterSave = true
}

watch(() => props.disabled, async disabled => {
  if (disabled) {
    open.value = false
    return
  }
  if (!restoreFocusAfterSave) return
  restoreFocusAfterSave = false
  await nextTick()
  if (document.activeElement === document.body) trigger.value?.focus({ preventScroll: true })
})

watch(open, (isOpen, _wasOpen, onCleanup) => {
  if (!isOpen) return
  document.addEventListener('keydown', dismissOnEscape, true)
  onCleanup(() => document.removeEventListener('keydown', dismissOnEscape, true))
})
</script>

<template>
  <SelectRoot v-model:open="open" :model-value="modelValue" :disabled="disabled" @update:model-value="updateValue">
    <SelectTrigger :ref="setTrigger" :id="id" :aria-label="label" v-bind="$attrs" class="app-select-trigger">
      <span class="app-select-value"><SelectValue :placeholder="placeholder" /></span>
      <SelectIcon class="app-select-icon"><AppIcon name="chevron" :size="14" /></SelectIcon>
    </SelectTrigger>
    <SelectPortal to="body">
      <SelectContent
        class="app-select-content"
        position="popper"
        align="end"
        :side-offset="6"
        :collision-padding="12"
        @escape-key-down="dismissOnEscape"
        @close-auto-focus="restoreTriggerFocus"
      >
        <SelectViewport class="app-select-viewport">
          <SelectItem v-for="option in options" :key="option.value" :value="option.value" :text-value="option.label" class="app-select-item">
            <span class="app-select-option-copy">
              <SelectItemText>{{ option.label }}</SelectItemText>
              <span v-if="option.description" class="app-select-description">{{ option.description }}</span>
            </span>
            <SelectItemIndicator class="app-select-indicator"><AppIcon name="check" :size="14" /></SelectItemIndicator>
          </SelectItem>
        </SelectViewport>
      </SelectContent>
    </SelectPortal>
  </SelectRoot>
</template>
