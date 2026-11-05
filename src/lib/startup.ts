import type { AppState } from './model.ts'

/** The prompt stays until answered, so a popover closed by accident doesn't use up the one offer. */
export function offersStartAtLogin(state: Pick<AppState, 'accounts' | 'settings' | 'recovery'>, loading: boolean, connectionError: boolean): boolean {
  return !loading && !connectionError && !state.recovery && state.accounts.length > 0
    && !state.settings.autostart && !state.settings.autostartPromptDismissed
}
