import { onUnmounted } from 'vue'

type TauriApi = {
  core: { invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown> }
  event: { listen: (event: string, handler: (event: { payload: unknown }) => void) => Promise<() => void> }
}

declare global {
  interface Window {
    __TAURI__?: TauriApi
  }
}

/** Must match `generate_handler!` in src-tauri/src/lib.rs; a unit test compares them. */
export const commands = [
  'get_state', 'get_account_history', 'get_popover_placement', 'fit_popover', 'refresh_accounts', 'send_test_notification',
  'discover_accounts', 'add_account', 'remove_account', 'restore_ignored_accounts', 'rename_account', 'save_settings',
  'reset_store', 'hide_window', 'get_install_warning', 'get_problem_report', 'open_problem_report', 'preview_make_active',
  'make_active', 'restore_switch_backup', 'dismiss_switch_restore', 'get_update_status', 'check_for_update', 'install_update',
] as const
export type Command = (typeof commands)[number] | 'plugin:dialog|open'
export type TauriEvent = 'state_changed' | 'update_status_changed' | 'popover_placement_changed' | 'popover_shown' | 'open_settings'

export function hasDesktop(): boolean {
  return typeof window.__TAURI__?.core.invoke === 'function'
}

const unavailableText = 'Desktop connection unavailable'

function desktopApi(unavailable = unavailableText): TauriApi {
  const tauri = window.__TAURI__
  if (!tauri) throw new Error(unavailable)
  return tauri
}

/** Rejects with `unavailable` outside the desktop app. */
export async function invoke(command: Command, args?: Record<string, unknown>, unavailable = unavailableText): Promise<unknown> {
  return desktopApi(unavailable).core.invoke(command, args)
}

/**
 * Returns `connect`, which starts listening once; concurrent calls share the attempt and a failed one
 * can be retried. Listening stops when the calling component unmounts, even mid-connect.
 */
export function useTauriEvent(name: TauriEvent, handler: (payload: unknown) => void): () => Promise<void> {
  let stop: (() => void) | undefined
  let connecting: Promise<void> | null = null
  let destroyed = false

  async function connect(): Promise<void> {
    if (stop || destroyed) return
    if (connecting) return connecting
    connecting = (async () => {
      const unlisten = await desktopApi().event.listen(name, event => handler(event.payload))
      if (destroyed) unlisten()
      else stop = unlisten
    })()
    try { await connecting }
    finally { connecting = null }
  }

  onUnmounted(() => {
    destroyed = true
    stop?.()
  })
  return connect
}
