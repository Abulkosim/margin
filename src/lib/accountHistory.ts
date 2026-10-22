import { onUnmounted, ref, watch, type Ref } from 'vue'
import { accountHistorySchema, createDemoHistory, type AccountHistory } from './history'
import type { Account } from './model'

export type HistoryState = { kind: 'loading' } | { kind: 'error'; message: string } | { kind: 'ready'; history: AccountHistory }

export function useAccountHistory(account: () => Account, demo: () => boolean): { state: Ref<HistoryState>; load: () => Promise<void> } {
  const state = ref<HistoryState>({ kind: 'loading' })
  let request = 0
  let destroyed = false

  async function load(): Promise<void> {
    const current = ++request
    const source = account()
    const id = source.id
    try {
      const payload: unknown = demo() ? createDemoHistory(source) : await window.__TAURI__?.core.invoke('get_account_history', { id })
      const history = accountHistorySchema.parse(payload)
      if (history.accountId !== id) throw new Error('Account mismatch')
      if (destroyed || current !== request) return
      state.value = { kind: 'ready', history }
    } catch {
      if (!destroyed && current === request) state.value = { kind: 'error', message: 'Could not load history.' }
    }
  }

  watch([() => account().id, () => account().snapshot?.fetchedAt], () => { void load() }, { immediate: true })
  onUnmounted(() => { destroyed = true; request++ })
  return { state, load }
}
