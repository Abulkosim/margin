<script setup lang="ts">
import { nextTick, ref } from 'vue'
import { version } from '../../../package.json'
import type { ProblemReport } from '../../lib/report'
import AppIcon from '../AppIcon.vue'

const props = defineProps<{ busy: boolean; preview: (version: string) => Promise<ProblemReport | null>; open: () => Promise<boolean> }>()
const report = ref<ProblemReport | null>(null)
const trigger = ref<HTMLButtonElement | null>(null)
const preview = ref<HTMLElement | null>(null)
const openButton = ref<HTMLButtonElement | null>(null)

async function review(): Promise<void> {
  report.value = await props.preview(version)
  if (!report.value) return
  await nextTick()
  preview.value?.scrollIntoView?.({ block: 'nearest' })
  openButton.value?.focus({ preventScroll: true })
}

async function close(): Promise<void> {
  report.value = null
  await nextTick()
  trigger.value?.focus()
}

async function send(): Promise<void> {
  if (await props.open()) await close()
}
</script>

<template>
  <section class="settings-section" aria-labelledby="support-title">
    <h2 id="support-title">Support</h2>
    <div v-if="!report" class="setting-row report-row">
      <p>Review the details, then open an issue.</p>
      <button ref="trigger" class="button button-small" :disabled="busy" @click="review">Report a problem</button>
    </div>
    <div v-else ref="preview" class="report-preview" role="group" aria-label="Report preview">
      <p class="report-note">Opening the page sends these details to GitHub. They become public only if you submit the issue.</p>
      <dl class="report-fields">
        <div><dt>Margin version</dt><dd>{{ report.version }}</dd></div>
        <div><dt>Operating system</dt><dd>{{ report.system }}</dd></div>
        <div class="report-diagnostics"><dt>Diagnostics</dt><dd data-testid="report-diagnostics">{{ report.diagnostics }}</dd></div>
      </dl>
      <p class="report-note">Emails, account names, folder paths, tokens, and error messages are left out.</p>
      <div class="report-actions">
        <button ref="openButton" class="button button-small button-primary" :disabled="busy" @click="send">Open GitHub issue<AppIcon name="external" :size="13"/></button>
        <button class="text-button" :disabled="busy" @click="close">Cancel</button>
      </div>
    </div>
  </section>
</template>

<style scoped>
.report-row { margin-top: 0; }
.report-row>p { margin: 0; }
.report-row>.button { flex-shrink: 0; }
.report-preview { padding: 14px; border-radius: 9px; background: var(--card); }
.report-note { margin: 0; font-size: 13px; line-height: 1.5; color: var(--tertiary); }
.report-fields { margin: 12px 0; font-size: 13px; line-height: 1.5; }
.report-fields>div { display: flex; justify-content: space-between; gap: 12px; padding: 5px 0; }
.report-fields dt { color: var(--secondary); }
.report-fields dd { margin: 0; color: var(--ink); text-align: right; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
.report-fields>.report-diagnostics { display: block; margin-top: 4px; }
.report-diagnostics dd { margin-top: 6px; padding: 9px 11px; border-radius: 7px; background: var(--surface); text-align: left; white-space: pre-line; }
.report-actions { display: flex; align-items: center; gap: 14px; margin-top: 13px; }
.report-actions .button { gap: 6px; }
</style>
