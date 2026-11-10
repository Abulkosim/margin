import { onUnmounted, ref, watch, type Ref } from 'vue'
import { accountHistorySchema, createDemoHistory, type AccountHistory } from './history'
import type { Account } from './model'
import { invoke } from './tauri'

export type HistoryState = { kind: 'loading' } | { kind: 'error'; message: string } | { kind: 'ready'; history: AccountHistory }

/**
 * Loads the account's history when its reading changes. While the History panel is closed, a new
 * reading from the same sign-in reloads only if `covers` says the loaded history can't serve the card.
 */
export function useAccountHistory(account: () => Account, demo: () => boolean, open: () => boolean, covers: (history: AccountHistory) => boolean): { state: Ref<HistoryState>; load: () => Promise<void> } {
  const state = ref<HistoryState>({ kind: 'loading' })
  let request = 0
  let destroyed = false
  let outdated = false
  let loaded: { reading: string; email: string | null } | null = null
  const readingKey = (source: Account): string => JSON.stringify([source.id, source.identity.email, source.snapshot?.fetchedAt])

  async function load(): Promise<void> {
    const current = ++request
    outdated = false
    const source = account()
    const id = source.id
    loaded = { reading: readingKey(source), email: source.identity.email }
    try {
      const payload: unknown = demo() ? createDemoHistory(source) : await invoke('get_account_history', { id })
      const history = accountHistorySchema.parse(payload)
      if (history.accountId !== id) throw new Error('Account mismatch')
      if (destroyed || current !== request) return
      state.value = { kind: 'ready', history }
    } catch {
      if (!destroyed && current === request) state.value = { kind: 'error', message: 'Couldn’t load history.' }
    }
  }

  function refresh(): void {
    if (!demo() && document.visibilityState === 'hidden') {
      outdated = true
      request++
      return
    }
    void load()
  }

  function visibilityChanged(): void {
    if (outdated && document.visibilityState === 'visible') void load()
  }

  function sync(): void {
    const source = account()
    if (loaded?.reading === readingKey(source)) return
    const history = state.value.kind === 'ready' ? state.value.history : null
    const sameSignIn = loaded?.email === source.identity.email
    if (!outdated && !open() && sameSignIn && history?.accountId === source.id && covers(history)) return
    refresh()
  }

  watch([() => readingKey(account()), open], sync, { immediate: true })
  document.addEventListener('visibilitychange', visibilityChanged)
  onUnmounted(() => {
    destroyed = true
    request++
    document.removeEventListener('visibilitychange', visibilityChanged)
  })
  return { state, load }
}
