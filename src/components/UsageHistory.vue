<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { TabsContent, TabsList, TabsRoot, TabsTrigger } from 'reka-ui'
import type { Account } from '../lib/model'
import { relativeAge } from '../lib/format'
import { accountHistorySchema, createDemoHistory, historyReadings, historySegments, historyWindows, observedHourChange, type AccountHistory } from '../lib/history'

const props = defineProps<{ account: Account; demo: boolean; now: number }>()
type HistoryState = { kind: 'loading' } | { kind: 'error'; message: string } | { kind: 'ready'; history: AccountHistory }
const state = ref<HistoryState>({ kind: 'loading' })
const selectedWindow = ref('')
const selectedIndex = ref(0)
let request = 0
let destroyed = false
const points = computed(() => state.value.kind === 'ready' ? state.value.history.points : [])
const windows = computed(() => historyWindows(props.account.snapshot?.windows ?? [], points.value))
const readings = computed(() => historyReadings(points.value, selectedWindow.value))
const segments = computed(() => historySegments(readings.value))
const selected = computed(() => readings.value[selectedIndex.value])
const first = computed(() => readings.value.at(0))
const last = computed(() => readings.value.at(-1))
const change = computed(() => observedHourChange(readings.value, props.now))
const timestamp = (time: number): string => new Date(time).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
const percentage = (value: number): string => String(Math.round(value * 10) / 10)
const boundaryLabel = { gap: 'Readings resumed', reset: 'First reading in new period', reading: 'New reading' }
const selectedDescription = computed(() => selected.value ? `${percentage(selected.value.remaining)}% remaining, ${timestamp(selected.value.timestamp)}${selected.value.boundary ? `, ${boundaryLabel[selected.value.boundary]}` : ''}` : '')
const x = (time: number): number => 4 + (time - (first.value?.timestamp ?? time)) / Math.max(1, (last.value?.timestamp ?? time) - (first.value?.timestamp ?? time)) * 276
const y = (remaining: number): number => 8 + (100 - remaining) * 0.96
const path = (segment: typeof readings.value): string => segment.map((point, index) => `${index ? 'L' : 'M'}${x(point.timestamp).toFixed(2)},${y(point.remaining).toFixed(2)}`).join(' ')

const segmentPaths = computed(() => segments.value.map(segment => ({ path: path(segment), isolated: segment.length === 1 ? segment[0] : undefined })))
const markers = computed(() => readings.value.filter(point => point.boundary === 'reset' || point.boundary === 'reading').map(point => ({ at: point.at, path: `M${x(point.timestamp)} 8V104` })))

async function load(): Promise<void> {
  const current = ++request
  const id = props.account.id
  try {
    const payload: unknown = props.demo ? createDemoHistory(props.account) : await window.__TAURI__?.core.invoke('get_account_history', { id })
    const history = accountHistorySchema.parse(payload)
    if (history.accountId !== id) throw new Error('Account mismatch')
    if (destroyed || current !== request) return
    state.value = { kind: 'ready', history }
  } catch {
    if (!destroyed && current === request) state.value = { kind: 'error', message: 'Could not load history.' }
  }
}
watch([() => props.account.id, () => props.account.snapshot?.fetchedAt], () => { void load() }, { immediate: true })
watch(windows, value => {
  if (!value.some(window => window.id === selectedWindow.value)) selectedWindow.value = value[0]?.id ?? ''
}, { immediate: true })
watch(readings, value => { selectedIndex.value = Math.max(0, value.length - 1) }, { immediate: true })
onUnmounted(() => { destroyed = true; request++ })

function inspect(event: KeyboardEvent): void {
  const deltas: Record<string, number> = { ArrowLeft: -1, ArrowDown: -1, ArrowRight: 1, ArrowUp: 1, PageDown: -10, PageUp: 10 }
  if (event.key === 'Home') selectedIndex.value = 0
  else if (event.key === 'End') selectedIndex.value = readings.value.length - 1
  else if (deltas[event.key] !== undefined) selectedIndex.value += deltas[event.key] ?? 0
  else return
  selectedIndex.value = Math.max(0, Math.min(readings.value.length - 1, selectedIndex.value))
  event.preventDefault()
}
function pointAt(event: PointerEvent): void {
  if (!(event.currentTarget instanceof SVGSVGElement) || !first.value || !last.value) return
  const bounds = event.currentTarget.getBoundingClientRect()
  const position = ((event.clientX - bounds.left) / bounds.width * 288 - 4) / 276
  const time = first.value.timestamp + position * (last.value.timestamp - first.value.timestamp)
  let distance = Infinity
  readings.value.forEach((point, index) => {
    if (Math.abs(point.timestamp - time) < distance) { distance = Math.abs(point.timestamp - time); selectedIndex.value = index }
  })
}
</script>

<template>
  <section class="usage-history" aria-label="Usage history">
    <div class="history-heading"><h3>{{ account.label }} history</h3><span>On this device · 7 days</span></div>
    <p v-if="state.kind === 'loading'" class="history-note" role="status">Loading history…</p>
    <div v-else-if="state.kind === 'error'" class="history-note" role="status"><p>{{ state.message }}</p><button class="text-button" @click="load">Try again</button></div>
    <template v-else>
      <p v-if="state.history.error" class="history-note" role="status">{{ state.history.error }}</p>
      <TabsRoot v-model="selectedWindow" class="history-tabs">
        <TabsList class="history-window-list" aria-label="History window"><TabsTrigger v-for="window in windows" :key="window.id" :value="window.id" class="history-window">{{ window.label }}</TabsTrigger></TabsList>
        <TabsContent :value="selectedWindow">
      <div v-if="readings.length < 2" class="history-empty" role="status">
        <p>{{ readings.length ? 'First reading recorded.' : 'No history yet.' }}</p>
        <span>Keep Margin running to record usage over time.</span>
      </div>
      <template v-else>
        <div class="history-reading">
          <strong>{{ selected ? percentage(selected.remaining) : '—' }}<span>% remaining</span></strong>
          <span>{{ selected ? timestamp(selected.timestamp) : '' }}</span>
          <span class="history-boundary">{{ selected?.boundary ? boundaryLabel[selected.boundary] : last ? `Latest reading ${relativeAge(last.at, now)}` : '' }}</span>
        </div>
        <div class="history-plot">
          <div class="history-scale" aria-hidden="true"><span>100</span><span>50</span><span>0</span></div>
          <svg class="history-chart" viewBox="0 0 288 112" role="slider" tabindex="0" aria-label="Recorded usage; use arrow keys to inspect readings" :aria-valuemin="1" :aria-valuemax="readings.length" :aria-valuenow="selectedIndex + 1" :aria-valuetext="selectedDescription" @keydown="inspect" @pointermove="pointAt" @pointerdown="pointAt">
            <g aria-hidden="true">
              <path d="M4 8H280 M4 56H280 M4 104H280" class="history-grid" />
              <template v-for="(segment, index) in segmentPaths" :key="index">
                <path :d="segment.path" class="history-line" />
                <circle v-if="segment.isolated" :cx="x(segment.isolated.timestamp)" :cy="y(segment.isolated.remaining)" r="2" class="history-dot" />
              </template>
              <template v-for="point in markers" :key="point.at"><path :d="point.path" class="history-marker" /></template>
              <template v-if="selected"><path :d="`M${x(selected.timestamp)} 8V104`" class="history-cursor" /><circle :cx="x(selected.timestamp)" :cy="y(selected.remaining)" r="3.5" class="history-dot" /></template>
            </g>
          </svg>
        </div>
        <div class="history-dates"><span>{{ first ? timestamp(first.timestamp) : '' }}</span><span>{{ last ? timestamp(last.timestamp) : '' }}</span></div>
        <p v-if="change !== null" class="history-change">{{ percentage(change) }} percentage points used · ~1h</p>
        <p class="history-note">Gaps: no readings. Dotted lines: new period or changed reading.</p>
      </template>
        </TabsContent>
      </TabsRoot>
    </template>
  </section>
</template>

<style scoped>
.usage-history { margin-top: 14px; padding-top: 12px; border-top: 1px solid var(--track); }
.history-heading { display: flex; flex-wrap: wrap; align-items: baseline; justify-content: space-between; gap: 4px 8px; }
.history-heading h3 { max-width: 100%; overflow-wrap: anywhere; font-size: 14px; font-weight: 600; margin: 0; }
.history-heading > span, .history-note, .history-empty span { font-size: 13px; color: var(--secondary); line-height: 1.5; }
.history-window-list { display: flex; flex-wrap: wrap; gap: 4px; margin: 10px 0; }
.history-window { max-width: 100%; overflow-wrap: anywhere; border-radius: 5px; padding: 6px 9px; color: var(--secondary); font-size: 13px; }
.history-window:hover { background: var(--hover); }
.history-window[data-state=active] { background: var(--accent-soft); color: var(--ink); font-weight: 600; }
.history-reading { display: flex; flex-wrap: wrap; align-items: baseline; justify-content: space-between; gap: 4px 8px; font-variant-numeric: tabular-nums; }
.history-reading strong { font-size: 21px; font-weight: 600; letter-spacing: -.5px; }
.history-reading strong span { font-size: 13px; font-weight: 400; letter-spacing: 0; margin-left: 4px; color: var(--secondary); }
.history-reading > span { color: var(--secondary); font-size: 13px; }
.history-reading .history-boundary { width: 100%; height: 18px; font-size: 13px; }
.history-plot { display: flex; gap: 6px; margin: 4px 0 6px; }
.history-scale { display: flex; flex-direction: column; justify-content: space-between; color: var(--secondary); font-size: 13px; width: 26px; flex-shrink: 0; padding: 1px 0; font-variant-numeric: tabular-nums; }
.history-chart { width: calc(100% - 32px); height: auto; overflow: visible; cursor: crosshair; touch-action: pan-y; }
.history-chart:focus-visible { outline: 2px solid var(--focus); outline-offset: 3px; border-radius: 4px; }
.history-grid { stroke: var(--track); fill: none; stroke-width: 1; }
.history-line { stroke: var(--accent-ink); stroke-width: 2; fill: none; stroke-linejoin: round; stroke-linecap: round; }
.history-marker { stroke: var(--secondary); stroke-width: 1; stroke-dasharray: 2 4; opacity: .7; }
.history-cursor { stroke: var(--secondary); stroke-width: 1; opacity: .4; }
.history-dot { fill: var(--accent-ink); stroke: var(--card); stroke-width: 1.5; }
.history-dates { display: flex; justify-content: space-between; gap: 12px; color: var(--secondary); font-size: 13px; line-height: 1.4; margin-left: 32px; }
.history-dates span:last-child { text-align: right; }
.history-change { font-size: 13px; line-height: 1.5; margin: 8px 0 0; }
.history-note { margin: 6px 0 0; }
.history-note p { margin: 0 0 8px; }
.history-empty { padding: 10px 0; }
.history-empty p { font-size: 14px; margin: 0 0 4px; }
</style>
