<script setup lang="ts">
import { computed } from 'vue'
import { AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogOverlay, AlertDialogPortal, AlertDialogRoot, AlertDialogTitle } from 'reka-ui'
import type { MakeActivePreview } from '../../lib/model'
import AppIcon from '../AppIcon.vue'

const props = defineProps<{ preview: MakeActivePreview | null; busy: boolean }>()
const emit = defineEmits<{ confirm: []; close: []; closed: [event: Event] }>()
const cli = computed(() => props.preview?.provider === 'codex' ? 'codex' : 'claude')
const codexRunning = computed(() => props.preview?.provider === 'codex' && props.preview.running)

function update(open: boolean): void {
  if (!open && !props.busy) emit('close')
}

function escape(event: KeyboardEvent): void {
  event.preventDefault()
  event.stopPropagation()
  update(false)
}
</script>

<template>
  <AlertDialogRoot :open="preview !== null" @update:open="update">
    <AlertDialogPortal to=".popover">
      <AlertDialogOverlay class="modal-backdrop">
        <AlertDialogContent v-if="preview" class="confirm-dialog make-active-dialog" @escape-key-down="escape" @close-auto-focus="emit('closed', $event)">
          <span class="dialog-icon"><AppIcon name="swap" :size="19"/></span>
          <AlertDialogTitle as="h2">Make {{ preview.target }} active?</AlertDialogTitle>
          <AlertDialogDescription as="p">
            <code>{{ cli }}</code> will use {{ preview.target }}.
            <template v-if="preview.kind === 'swap'"> {{ preview.displaced }} moves to <code>{{ preview.folder }}</code>.</template>
            <template v-else> <code>{{ preview.folder }}</code> will be signed out.</template>
          </AlertDialogDescription>
          <p v-if="preview.running" class="switch-warning" :class="{ strong: codexRunning }"><AppIcon name="alert" :size="14"/><span v-if="codexRunning">Codex is running. Quit it first, or it may undo the switch.</span><span v-else>Claude Code is running. Open sessions switch within a minute. Restart them to be sure.</span></p>
          <div class="dialog-actions">
            <template v-if="codexRunning">
              <button class="button" tabindex="0" :disabled="busy" @click="emit('confirm')">Switch anyway</button>
              <AlertDialogCancel class="button button-primary" tabindex="0" :disabled="busy">Cancel</AlertDialogCancel>
            </template>
            <template v-else>
              <AlertDialogCancel class="button" tabindex="0" :disabled="busy">Cancel</AlertDialogCancel>
              <button class="button button-primary" tabindex="0" :disabled="busy" @click="emit('confirm')">Make active</button>
            </template>
          </div>
        </AlertDialogContent>
      </AlertDialogOverlay>
    </AlertDialogPortal>
  </AlertDialogRoot>
</template>

<style scoped>
.make-active-dialog code {
  font: 500 12.5px/1.5 ui-monospace,"SF Mono",SFMono-Regular,Menlo,Consolas,monospace;
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--card);
  color: var(--ink);
  display: inline-block;
  max-width: 100%;
  vertical-align: bottom;
  overflow-wrap: anywhere;
}
.switch-warning {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  padding: 9px 11px;
  border-radius: 8px;
  background: var(--card);
  font-size: 13px;
  line-height: 1.5;
  margin-top: 14px;
}
.switch-warning>svg {
  flex-shrink: 0;
  margin-top: 2px;
  color: var(--tertiary);
}
.switch-warning.strong {
  background: var(--warning-soft);
  color: var(--ink);
}
.switch-warning.strong>svg {
  color: var(--warning);
}
</style>
