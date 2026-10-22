import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate } from 'node:timers/promises'
import { createRenderer, defineComponent } from 'vue'
import { createDemoState } from '../../src/lib/demo.ts'
import { accountIsStale } from '../../src/lib/format.ts'
import { useMargin } from '../../src/lib/margin.ts'

type HostNode = { parent: HostNode | null; children: HostNode[]; text: string }
type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>
type EventHandler = (event: { payload: unknown }) => void

function hostNode(text = ''): HostNode { return { parent: null, children: [], text } }

async function mountMargin(options: { invoke?: Invoke; demo?: boolean; failListener?: () => boolean; beforeListener?: () => Promise<void> }) {
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, 'window')
  const handlers = new Map<string, EventHandler>()
  let unlistened = 0
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      location: { search: options.demo ? '?demo=1' : '' },
      setInterval,
      __TAURI__: options.invoke ? {
        core: { invoke: options.invoke },
        event: {
          listen: async (name: string, handler: EventHandler) => {
            if (options.failListener?.()) throw new Error('Native listener unavailable')
            await options.beforeListener?.()
            handlers.set(name, handler)
            return () => { handlers.delete(name); unlistened++ }
          },
        },
      } : undefined,
    },
  })
  const renderer = createRenderer<HostNode, HostNode>({
    createElement: () => hostNode(),
    createText: hostNode,
    createComment: hostNode,
    setText: (node, text) => { node.text = text },
    setElementText: (node, text) => { node.text = text },
    parentNode: node => node.parent,
    nextSibling: node => {
      if (!node.parent) return null
      return node.parent.children.at(node.parent.children.indexOf(node) + 1) ?? null
    },
    insert: (node, parent) => { node.parent = parent; parent.children.push(node) },
    remove: node => {
      if (node.parent) node.parent.children = node.parent.children.filter(child => child !== node)
      node.parent = null
    },
    patchProp: () => {},
  })
  let controller: ReturnType<typeof useMargin> | undefined
  const app = renderer.createApp(defineComponent({
    setup() { controller = useMargin(); return () => null },
  }))
  app.mount(hostNode())
  await setImmediate()
  assert.ok(controller)
  let closed = false
  return {
    controller,
    emit(payload: unknown) {
      const handler = handlers.get('state_changed')
      assert.ok(handler)
      handler({ payload })
    },
    get unlistened() { return unlistened },
    close() {
      if (closed) return
      closed = true
      app.unmount()
      if (originalWindow) Object.defineProperty(globalThis, 'window', originalWindow)
      else Reflect.deleteProperty(globalThis, 'window')
    },
  }
}

test('invalid state after a mutation fails the action and preserves marked-stale readings', async t => {
  const fresh = createDemoState(null)
  let payload: unknown = fresh
  const harness = await mountMargin({ invoke: async command => command === 'get_state' ? payload : undefined })
  t.after(() => harness.close())
  const { controller } = harness
  assert.equal(controller.connectionError.value, null)
  const before = controller.state.value.accounts.at(0)
  assert.ok(before)

  payload = { ...fresh, accounts: [{ ...before, provider: 'unsupported' }] }
  assert.equal(await controller.addAccount('claude', '/example/.claude-work'), false)
  assert.equal(controller.message.value?.kind, 'error')
  assert.match(controller.message.value?.text ?? '', /unreadable response/)
  assert.equal(controller.state.value.accounts.length, fresh.accounts.length)
  assert.equal(controller.state.value.accounts.at(0)?.snapshot?.fetchedAt, before.snapshot?.fetchedAt)
  assert.equal(accountIsStale(before, controller.state.value.settings, Date.now(), Boolean(controller.connectionError.value)), true)

  await controller.discover()
  assert.equal(controller.message.value?.kind, 'error')
  assert.doesNotMatch(controller.message.value?.text ?? '', /discovery complete/i)

  payload = fresh
  await controller.refresh()
  assert.equal(controller.connectionError.value, null)
  assert.equal(accountIsStale(before, controller.state.value.settings, Date.now(), Boolean(controller.connectionError.value)), false)
})

test('malformed state events fail closed without escaping the native event callback', async t => {
  const fresh = createDemoState(null)
  const harness = await mountMargin({ invoke: async () => fresh })
  t.after(() => harness.close())
  const { controller } = harness
  assert.doesNotThrow(() => harness.emit({ accounts: null }))
  assert.match(controller.connectionError.value ?? '', /unreadable response/)
  assert.equal(controller.message.value?.kind, 'error')
  assert.equal(controller.state.value.accounts.length, 3)
  harness.emit(fresh)
  assert.equal(controller.connectionError.value, null)
  harness.close()
  assert.equal(harness.unlistened, 1)
})

test('a failed state read cannot produce a success toast or discard cached usage', async t => {
  const fresh = createDemoState(null)
  let rejectRead = false
  const harness = await mountMargin({ invoke: async command => {
    if (command === 'get_state' && rejectRead) throw new Error('Native connection closed')
    return fresh
  } })
  t.after(() => harness.close())
  rejectRead = true
  await harness.controller.discover()
  assert.equal(harness.controller.message.value?.kind, 'error')
  assert.match(harness.controller.connectionError.value ?? '', /Refresh to reconnect/)
  assert.equal(harness.controller.state.value.accounts.length, 3)
  assert.equal(harness.controller.pending.value, null)
})

test('refresh reconnects a failed native event subscription', async t => {
  const fresh = createDemoState(null)
  let rejectListener = true
  const harness = await mountMargin({ invoke: async () => fresh, failListener: () => rejectListener })
  t.after(() => harness.close())
  assert.ok(harness.controller.connectionError.value)
  rejectListener = false
  await harness.controller.refresh()
  assert.equal(harness.controller.connectionError.value, null)
  assert.doesNotThrow(() => harness.emit(fresh))
})

test('hiding the window handles native failures without an unhandled rejection', async t => {
  const fresh = createDemoState(null)
  const harness = await mountMargin({ invoke: async command => {
    if (command === 'hide_window') throw new Error('Window missing')
    return fresh
  } })
  t.after(() => harness.close())
  await assert.doesNotReject(harness.controller.hideWindow())
  assert.equal(harness.controller.message.value?.kind, 'error')
  assert.equal(harness.controller.message.value?.text, 'Could not hide the window. Try again.')
})

test('preview removal advances the pinned account and preserves pinned mode', async t => {
  const harness = await mountMargin({ demo: true })
  t.after(() => harness.close())
  const { controller } = harness
  const accounts = [...controller.state.value.accounts]
  const first = accounts.at(0)
  const second = accounts.at(1)
  assert.ok(first)
  assert.ok(second)
  await controller.saveSettings({ ...controller.state.value.settings, trayMode: 'pinned', pinnedAccountId: first.id })
  assert.equal(await controller.removeAccount(first.id), true)
  assert.equal(controller.state.value.settings.pinnedAccountId, second.id)
  assert.equal(controller.state.value.settings.trayMode, 'pinned')
  for (const account of controller.state.value.accounts.slice()) await controller.removeAccount(account.id)
  assert.equal(controller.state.value.settings.pinnedAccountId, null)
  assert.equal(controller.state.value.settings.trayMode, 'pinned')
})

test('a delayed get_state response cannot overwrite a newer state event', async t => {
  const older = createDemoState(null)
  older.revision = 1
  const newer = createDemoState(null)
  newer.revision = 2
  newer.accounts = newer.accounts.map(account => ({ ...account, label: `Updated ${account.label}` }))
  let reads = 0
  let completeRead: ((payload: unknown) => void) | undefined
  const harness = await mountMargin({ invoke: async command => {
    if (command !== 'get_state') return undefined
    if (++reads === 1) return older
    return new Promise<unknown>(resolve => { completeRead = resolve })
  } })
  t.after(() => harness.close())
  const refreshing = harness.controller.refresh()
  await setImmediate()
  assert.ok(completeRead)
  harness.emit(newer)
  completeRead(older)
  await refreshing
  assert.equal(harness.controller.state.value.revision, 2)
  assert.equal(harness.controller.state.value.accounts.at(0)?.label, 'Updated Personal')
  assert.equal(harness.controller.connectionError.value, null)

  harness.emit({ accounts: null })
  assert.ok(harness.controller.connectionError.value)
  harness.emit(older)
  assert.equal(harness.controller.state.value.revision, 2)
  assert.ok(harness.controller.connectionError.value)
  harness.emit(newer)
  assert.equal(harness.controller.connectionError.value, null)
})

test('refresh during startup shares one native subscription and cleans it up', async t => {
  let completeListener: (() => void) | undefined
  const gate = new Promise<void>(resolve => { completeListener = resolve })
  let subscriptions = 0
  const harness = await mountMargin({
    invoke: async () => createDemoState(null),
    beforeListener: () => { subscriptions++; return gate },
  })
  t.after(() => harness.close())
  const refreshing = harness.controller.refresh()
  await setImmediate()
  assert.equal(subscriptions, 1)
  assert.ok(completeListener)
  completeListener()
  await refreshing
  assert.equal(harness.controller.connectionError.value, null)
  harness.close()
  assert.equal(harness.unlistened, 1)
})

test('reset invokes the native command and leaves recovery', async t => {
  let payload: unknown = { ...createDemoState('recovery') }
  const calls: string[] = []
  const harness = await mountMargin({ invoke: async command => {
    calls.push(command)
    if (command === 'reset_store') payload = { ...createDemoState(null), revision: 1 }
    return command === 'get_state' ? payload : undefined
  } })
  t.after(() => harness.close())
  assert.equal(harness.controller.state.value.recovery, true)
  assert.equal(await harness.controller.resetStore(), true)
  assert.ok(calls.includes('reset_store'))
  assert.equal(harness.controller.state.value.recovery, false)
  assert.equal(harness.controller.state.value.accounts.length, 3)
  assert.match(harness.controller.message.value?.text ?? '', /backup/)
})
