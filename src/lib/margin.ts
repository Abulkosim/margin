import { onMounted, onUnmounted, ref } from 'vue'
import { createDemoState } from './demo.ts'
import { accountSchema, appStateSchema, defaultSettings, type AccountId, type AppState, type Provider, type Settings } from './model.ts'

declare global {
  interface Window {
    __TAURI__?: {
      core: { invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown> }
      event: { listen: (event: string, handler: (event: { payload: unknown }) => void) => Promise<() => void> }
    }
  }
}

type Command = 'refresh_accounts' | 'discover_accounts' | 'add_account' | 'remove_account' | 'rename_account' | 'save_settings'

export function useMargin() {
  const query = new URLSearchParams(window.location.search)
  const demo = query.get('demo') === '1'
  const desktop = typeof window.__TAURI__?.core.invoke === 'function'
  const state = ref<AppState>(demo ? createDemoState(query.get('state')) : { revision: 0, accounts: [], settings: { ...defaultSettings }, refreshing: false, notice: null })
  const loading = ref(desktop && !demo)
  const connectionError = ref<string | null>(null)
  const pending = ref<Command | null>(null)
  const message = ref<{ kind: 'error' | 'success'; text: string } | null>(null)
  const now = ref(Date.now())
  let stopListening: (() => void) | undefined
  let connectingListener: Promise<void> | null = null
  let destroyed = false
  let toastTimer: ReturnType<typeof setTimeout> | undefined
  const clock = window.setInterval(() => { now.value = Date.now() }, 1000)

  const theme = query.get('theme')
  if (demo && (theme === 'light' || theme === 'dark')) state.value.settings.theme = theme

  function notify(text: string, kind: 'error' | 'success' = 'success'): void {
    clearTimeout(toastTimer)
    message.value = { kind, text }
    toastTimer = setTimeout(() => { message.value = null }, kind === 'error' ? 9000 : 3500)
  }

  function acceptState(payload: unknown): boolean {
    const result = appStateSchema.safeParse(payload)
    if (result.success) {
      if (result.data.revision < state.value.revision) return false
      state.value = result.data
      connectionError.value = null
      return true
    } else {
      connectionError.value = 'Margin received an unreadable response. Refresh to try again.'
      throw new Error(connectionError.value)
    }
  }

  async function listenToState(): Promise<void> {
    if (stopListening || destroyed) return
    if (connectingListener) return connectingListener
    connectingListener = (async () => {
      const tauri = window.__TAURI__
      if (!tauri) throw new Error('Desktop connection unavailable')
      const unlisten = await tauri.event.listen('state_changed', event => {
        try { acceptState(event.payload) }
        catch { notify(connectionError.value ?? 'Could not read the latest account state.', 'error') }
      })
      if (destroyed) unlisten()
      else stopListening = unlisten
    })()
    try { await connectingListener }
    finally { connectingListener = null }
  }

  async function reload(): Promise<void> {
    let payload: unknown
    try {
      await listenToState()
      const tauri = window.__TAURI__
      if (!tauri) throw new Error('Desktop connection unavailable')
      payload = await tauri.core.invoke('get_state')
    } catch {
      connectionError.value = 'Could not read your accounts. Refresh to reconnect.'
      throw new Error(connectionError.value)
    }
    const accepted = acceptState(payload)
    if (!accepted && connectionError.value) throw new Error(connectionError.value)
  }

  async function perform(command: Command, args?: Record<string, unknown>, preview?: () => void): Promise<boolean> {
    if (pending.value) return false
    pending.value = command
    try {
      if (demo) {
        preview?.()
        state.value.revision++
      }
      else {
        const tauri = window.__TAURI__
        if (!tauri) throw new Error('Open the desktop app to manage your accounts.')
        await tauri.core.invoke(command, args)
        await reload()
      }
      return true
    } catch (error: unknown) {
      notify(error instanceof Error ? error.message : typeof error === 'string' ? error : 'That did not work. Please try again.', 'error')
      return false
    } finally {
      pending.value = null
    }
  }

  async function refresh(): Promise<void> {
    await perform('refresh_accounts', undefined, () => {
      state.value.refreshing = false
      state.value.accounts = state.value.accounts.map(account => ({
        ...account, refreshing: false,
        snapshot: account.snapshot ? { ...account.snapshot, fetchedAt: new Date().toISOString() } : null,
      }))
      notify('Preview refreshed. Values are sample data.')
    })
  }

  async function discover(): Promise<void> {
    if (await perform('discover_accounts', undefined, () => {
      if (!state.value.accounts.length) state.value.accounts = createDemoState(null).accounts
    })) notify(demo ? 'Sample accounts found.' : 'Account discovery complete.')
  }

  async function addAccount(provider: Provider, path: string): Promise<boolean> {
    const added = await perform('add_account', { provider, path }, () => {
      state.value.accounts.push(accountSchema.parse({
        id: `demo-${crypto.randomUUID()}`, provider,
        label: path.split(/[\\/]/).filter(Boolean).at(-1) ?? 'New account', color: '#b8d1bf',
        source: { kind: 'configDir', path }, identity: { email: null, plan: null },
        snapshot: null, refreshing: false,
        error: { kind: 'preview', message: 'This is a preview. The desktop app reads usage for accounts added here.', retryAfterSeconds: null },
      }))
    })
    if (added) notify(demo ? 'Account added to preview.' : 'Account added.')
    return added
  }

  async function removeAccount(id: AccountId): Promise<boolean> {
    const removed = await perform('remove_account', { id }, () => {
      state.value.accounts = state.value.accounts.filter(account => account.id !== id)
      if (state.value.settings.pinnedAccountId === id) state.value.settings = {
        ...state.value.settings,
        pinnedAccountId: state.value.accounts.at(0)?.id ?? null,
      }
    })
    if (removed) notify('Account removed from Margin.')
    return removed
  }

  async function renameAccount(id: AccountId, label: string): Promise<boolean> {
    return perform('rename_account', { id, label }, () => {
      state.value.accounts = state.value.accounts.map(account => account.id === id ? { ...account, label } : account)
    })
  }

  async function saveSettings(settings: Settings): Promise<boolean> {
    return perform('save_settings', { settings }, () => { state.value.settings = settings })
  }

  async function hideWindow(): Promise<void> {
    if (!desktop || demo) return
    try {
      const tauri = window.__TAURI__
      if (!tauri) throw new Error('Desktop connection unavailable')
      await tauri.core.invoke('hide_window')
    } catch {
      notify('Could not hide the window. Try again.', 'error')
    }
  }

  onMounted(async () => {
    if (!desktop || demo) return
    try {
      await reload()
    } catch {
      connectionError.value ??= 'Could not connect to Margin. Close and reopen the app to try again.'
      notify(connectionError.value, 'error')
    } finally {
      loading.value = false
    }
  })

  onUnmounted(() => {
    destroyed = true
    stopListening?.()
    clearInterval(clock)
    clearTimeout(toastTimer)
  })

  return { state, loading, connectionError, pending, message, now, demo, desktop, refresh, discover, addAccount, removeAccount, renameAccount, saveSettings, hideWindow }
}
