import { computed, onMounted, onUnmounted, ref } from 'vue'
import { z } from 'zod'

const counterSchema = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER)
export const updateStateSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('idle') }),
  z.object({ kind: z.literal('checking') }),
  z.object({ kind: z.literal('current') }),
  z.object({ kind: z.literal('available'), version: z.string().min(1).max(100) }),
  z.object({ kind: z.literal('downloading'), version: z.string().min(1).max(100), downloaded: counterSchema, total: counterSchema.nullable() }),
  z.object({ kind: z.literal('installing'), version: z.string().min(1).max(100) }),
  z.object({ kind: z.literal('error'), message: z.string().min(1).max(500) }),
])
export const updateStatusSchema = z.object({ revision: counterSchema, state: updateStateSchema })
export type UpdateState = z.infer<typeof updateStateSchema>

export function updateAvailable(state: UpdateState): boolean {
  return state.kind === 'available' || state.kind === 'downloading' || state.kind === 'installing'
}

export function useUpdater({ demo, desktop, initial = { kind: 'idle' } }: { demo: boolean; desktop: boolean; initial?: UpdateState }) {
  const status = ref<z.infer<typeof updateStatusSchema>>({ revision: 0, state: initial })
  const problem = ref<string | null>(null)
  const pending = ref(false)
  const remoteBusy = computed(() => ['checking', 'downloading', 'installing'].includes(status.value.state.kind))
  const busy = computed(() => pending.value || (!problem.value && remoteBusy.value))
  const state = computed<UpdateState>(() => problem.value ? { kind: 'error', message: problem.value } : status.value.state)
  let destroyed = false
  let stopListening: (() => void) | undefined
  let connecting: Promise<void> | null = null

  function accept(payload: unknown): void {
    if (destroyed) return
    const parsed = updateStatusSchema.safeParse(payload)
    if (!parsed.success) {
      problem.value = 'Couldn’t read the update status. Try again.'
      return
    }
    if (parsed.data.revision < status.value.revision) return
    status.value = parsed.data
    problem.value = null
  }

  async function listen(): Promise<void> {
    if (stopListening || destroyed) return
    if (connecting) return connecting
    connecting = (async () => {
      const tauri = window.__TAURI__
      if (!tauri) throw new Error('Desktop connection unavailable')
      const unlisten = await tauri.event.listen('update_status_changed', event => accept(event.payload))
      if (destroyed) unlisten()
      else stopListening = unlisten
    })()
    try { await connecting }
    finally { connecting = null }
  }

  async function perform(command: 'check_for_update' | 'install_update', args?: { version: string }): Promise<void> {
    if (demo || !desktop || busy.value || destroyed) return
    const recovering = problem.value !== null
    pending.value = true
    problem.value = null
    try {
      await listen()
      if (destroyed) return
      const tauri = window.__TAURI__
      if (!tauri) throw new Error('Desktop connection unavailable')
      if (recovering) {
        accept(await tauri.core.invoke('get_update_status'))
        if (problem.value || remoteBusy.value || destroyed) return
      }
      accept(await tauri.core.invoke(command, args))
    } catch {
      if (!destroyed) problem.value = 'Couldn’t connect to the updater. Try again.'
    } finally {
      pending.value = false
    }
  }

  async function check(): Promise<void> { await perform('check_for_update') }
  async function install(): Promise<void> {
    const current = state.value
    if (current.kind === 'available') await perform('install_update', { version: current.version })
  }

  onMounted(async () => {
    if (!desktop || demo) return
    try {
      await listen()
      if (destroyed) return
      const tauri = window.__TAURI__
      if (!tauri) throw new Error('Desktop connection unavailable')
      accept(await tauri.core.invoke('get_update_status'))
    } catch {
      if (!destroyed) problem.value = 'Couldn’t connect to the updater. Try again.'
    }
  })
  onUnmounted(() => {
    destroyed = true
    stopListening?.()
  })
  const available = computed(() => updateAvailable(state.value))
  return { state, busy, available, check, install }
}
