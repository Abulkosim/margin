import { computed } from 'vue'
import { accountIsStale, idleStatus, planName, retryingMessage } from './format.ts'
import type { Account, Settings } from './model.ts'

export type AccountStatusSource = { account: Account; settings: Settings; now: number; connectionError: string | null }

/** A lost desktop connection outranks the account's own state, which may be out of date. */
export function useAccountStatus(props: AccountStatusSource) {
  const interrupted = (): boolean => Boolean(props.connectionError)
  return {
    stale: computed(() => accountIsStale(props.account, props.settings, props.now, interrupted())),
    retrying: computed(() => retryingMessage(props.account, props.settings, props.now, interrupted())),
    refreshing: computed(() => props.account.refreshing && !interrupted()),
    idle: computed(() => interrupted() ? null : idleStatus(props.account, props.now)),
    plan: computed(() => planName(props.account.identity.plan)),
  }
}
