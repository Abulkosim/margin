import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate } from 'node:timers/promises'
import { createRenderer, defineComponent, nextTick, ref } from 'vue'
import { useAccountHistory, type HistoryState } from '../../src/lib/accountHistory.ts'
import { createDemoState } from '../../src/lib/demo.ts'
import type { AccountHistory } from '../../src/lib/history.ts'
import type { Account } from '../../src/lib/model.ts'

type HostNode = { parent: HostNode | null; children: HostNode[] }

function mount(invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>, account: () => Account, open: () => boolean) {
  const restore = ['window', 'document'].map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)] as const)
  Object.defineProperty(globalThis, 'window', { configurable: true, value: { __TAURI__: { core: { invoke }, event: { listen: async () => () => {} } } } })
  Object.defineProperty(globalThis, 'document', { configurable: true, value: { visibilityState: 'visible', addEventListener: () => {}, removeEventListener: () => {} } })
  const node = (): HostNode => ({ parent: null, children: [] })
  const renderer = createRenderer<HostNode, HostNode>({
    createElement: node, createText: node, createComment: node, setText: () => {}, setElementText: () => {},
    parentNode: entry => entry.parent, nextSibling: () => null,
    insert: (entry, parent) => { entry.parent = parent; parent.children.push(entry) },
    remove: entry => { entry.parent = null }, patchProp: () => {},
  })
  let state: { value: HistoryState } | undefined
  const app = renderer.createApp(defineComponent({ setup() { state = useAccountHistory(account, () => false, open, () => true).state; return () => null } }))
  app.mount(node())
  assert.ok(state)
  return {
    state,
    close() {
      app.unmount()
      for (const [key, descriptor] of restore) {
        if (descriptor) Object.defineProperty(globalThis, key, descriptor)
        else Reflect.deleteProperty(globalThis, key)
      }
    },
  }
}

test('a new sign-in on the account never shows the previous sign-in’s history while its own loads', async t => {
  const base = createDemoState(null).accounts[0]
  assert.ok(base?.snapshot)
  const account = ref<Account>(base)
  const history = (usedPercent: number): AccountHistory => ({
    accountId: base.id, error: null,
    points: [{ windowId: 'five_hour', at: '2026-10-16T10:00:00Z', usedPercent, resetsAt: null, windowSeconds: 18_000, gapBefore: false }],
  })
  const replies: ((value: unknown) => void)[] = []
  const view = mount(() => new Promise(resolve => { replies.push(resolve) }), () => account.value, () => true)
  t.after(() => view.close())
  replies.shift()?.(history(10))
  await setImmediate()
  assert.equal(view.state.value.kind, 'ready')

  account.value = { ...base, identity: { ...base.identity, email: 'someone-else@example.com' }, snapshot: { ...base.snapshot, fetchedAt: '2026-10-16T10:05:00Z' } }
  await nextTick()
  assert.equal(view.state.value.kind, 'loading')
  replies.shift()?.(history(70))
  await setImmediate()
  // Read through a function: the asserts above narrowed `view.state.value.kind`.
  const state = (): HistoryState => view.state.value
  const loaded = state()
  assert.equal(loaded.kind === 'ready' && loaded.history.points[0]?.usedPercent, 70)
})

test('a newer reading from the same sign-in keeps the loaded history visible while it reloads', async t => {
  const base = createDemoState(null).accounts[0]
  assert.ok(base?.snapshot)
  const account = ref<Account>(base)
  const replies: ((value: unknown) => void)[] = []
  const view = mount(() => new Promise(resolve => { replies.push(resolve) }), () => account.value, () => true)
  t.after(() => view.close())
  replies.shift()?.({ accountId: base.id, error: null, points: [] })
  await setImmediate()
  account.value = { ...base, snapshot: { ...base.snapshot, fetchedAt: '2026-10-16T10:05:00Z' } }
  await nextTick()
  assert.equal(view.state.value.kind, 'ready')
  assert.equal(replies.length, 1)
})
