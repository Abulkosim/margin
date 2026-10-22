import assert from 'node:assert/strict'
import test from 'node:test'
import { setImmediate } from 'node:timers/promises'
import { createRenderer, defineComponent } from 'vue'
import { useUpdater, updateStatusSchema } from '../../src/lib/updater.ts'

type HostNode = { parent: HostNode | null; children: HostNode[]; text: string }
type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>
type EventHandler = (event: { payload: unknown }) => void

function hostNode(text = ''): HostNode { return { parent: null, children: [], text } }

async function mountUpdater(options: { invoke?: Invoke; demo?: boolean; failListener?: () => boolean; beforeListener?: () => Promise<void> }) {
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
  let controller: ReturnType<typeof useUpdater> | undefined
  const app = renderer.createApp(defineComponent({
    setup() { controller = useUpdater({ demo: Boolean(options.demo), desktop: Boolean(options.invoke) }); return () => null },
  }))
  app.mount(hostNode())
  await setImmediate()
  assert.ok(controller)
  let closed = false
  return {
    controller,
    emit(payload: unknown) {
      const handler = handlers.get('update_status_changed')
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

test('mount only reads local status; checks and installation require separate explicit actions', async t => {
  const calls: { command: string; args?: Record<string, unknown> }[] = []
  const harness = await mountUpdater({ invoke: async (command, args) => {
    calls.push({ command, args })
    if (command === 'check_for_update') return { revision: 1, state: { kind: 'available', version: '0.3.0' } }
    if (command === 'install_update') return { revision: 2, state: { kind: 'installing', version: '0.3.0' } }
    return { revision: 0, state: { kind: 'idle' } }
  } })
  t.after(() => harness.close())
  assert.deepEqual(calls.map(call => call.command), ['get_update_status'])
  await harness.controller.install()
  assert.equal(calls.length, 1)
  await harness.controller.check()
  assert.deepEqual(harness.controller.state.value, { kind: 'available', version: '0.3.0' })
  assert.equal(calls.length, 2)
  await harness.controller.install()
  assert.deepEqual(calls.at(-1), { command: 'install_update', args: { version: '0.3.0' } })
  assert.equal(harness.controller.busy.value, true)
})

test('duplicate actions are blocked during requests and download events', async t => {
  let finishCheck: ((payload: unknown) => void) | undefined
  let checks = 0
  const harness = await mountUpdater({ invoke: async command => {
    if (command === 'check_for_update') {
      checks++
      return new Promise(resolve => { finishCheck = resolve })
    }
    return { revision: 0, state: { kind: 'idle' } }
  } })
  t.after(() => harness.close())
  const checking = harness.controller.check()
  await harness.controller.check()
  await setImmediate()
  assert.equal(checks, 1)
  assert.ok(finishCheck)
  harness.emit({ revision: 2, state: { kind: 'downloading', version: '0.3.0', downloaded: 100, total: null } })
  finishCheck({ revision: 1, state: { kind: 'available', version: '0.3.0' } })
  await checking
  assert.equal(harness.controller.state.value.kind, 'downloading')
  await harness.controller.check()
  await harness.controller.install()
  assert.equal(checks, 1)
})

test('older replies and events cannot overwrite progress or clear a boundary error', async t => {
  let finishRead: ((payload: unknown) => void) | undefined
  const harness = await mountUpdater({ invoke: async () => new Promise(resolve => { finishRead = resolve }) })
  t.after(() => harness.close())
  const latest = { revision: 4, state: { kind: 'downloading', version: '0.3.0', downloaded: 500, total: 1000 } }
  harness.emit(latest)
  assert.ok(finishRead)
  finishRead({ revision: 0, state: { kind: 'idle' } })
  await setImmediate()
  assert.deepEqual(harness.controller.state.value, latest.state)
  assert.doesNotThrow(() => harness.emit({ revision: 5, state: { kind: 'downloading', downloaded: -1 } }))
  assert.equal(harness.controller.state.value.kind, 'error')
  harness.emit({ revision: 2, state: { kind: 'current' } })
  assert.equal(harness.controller.state.value.kind, 'error')
  harness.emit(latest)
  assert.deepEqual(harness.controller.state.value, latest.state)
})

test('malformed check result fails closed and allows checking again', async t => {
  let malformed = true
  const harness = await mountUpdater({ invoke: async command => command === 'get_update_status'
    ? { revision: 0, state: { kind: 'idle' } }
    : malformed ? { revision: 1, state: { kind: 'available' } } : { revision: 2, state: { kind: 'current' } } })
  t.after(() => harness.close())
  await harness.controller.check()
  assert.equal(harness.controller.state.value.kind, 'error')
  assert.equal(harness.controller.busy.value, false)
  malformed = false
  await harness.controller.check()
  assert.equal(harness.controller.state.value.kind, 'current')
})

test('listener failure recovers on an explicit retry and never exposes raw IPC errors', async t => {
  let failing = true
  const harness = await mountUpdater({ failListener: () => failing, invoke: async () => ({ revision: 1, state: { kind: 'current' } }) })
  t.after(() => harness.close())
  assert.deepEqual(harness.controller.state.value, { kind: 'error', message: 'Couldn’t connect to the updater. Try again.' })
  failing = false
  await harness.controller.check()
  assert.equal(harness.controller.state.value.kind, 'current')
  harness.close()
  assert.equal(harness.unlistened, 1)
})

test('preview never invokes desktop update commands or subscribes', async t => {
  const harness = await mountUpdater({ demo: true, invoke: async () => { assert.fail('Preview must not reach desktop updater') } })
  t.after(() => harness.close())
  await harness.controller.check()
  await harness.controller.install()
  assert.equal(harness.controller.state.value.kind, 'idle')
  harness.close()
  assert.equal(harness.unlistened, 0)
})

test('unmount during listener registration releases the eventual subscription without reading status', async t => {
  let finishListener: (() => void) | undefined
  const gate = new Promise<void>(resolve => { finishListener = resolve })
  const harness = await mountUpdater({ beforeListener: () => gate, invoke: async () => { assert.fail('Unmounted updater must not issue commands') } })
  t.after(() => harness.close())
  harness.close()
  assert.ok(finishListener)
  finishListener()
  await setImmediate()
  assert.equal(harness.unlistened, 1)
})

test('update boundary rejects unsafe counters, missing versions, and unknown state variants', () => {
  for (const payload of [
    { revision: Number.MAX_SAFE_INTEGER + 1, state: { kind: 'idle' } },
    { revision: 1, state: { kind: 'available', version: '' } },
    { revision: 1, state: { kind: 'downloading', version: '0.3.0', downloaded: -1, total: null } },
    { revision: 1, state: { kind: 'installing' } },
    { revision: 1, state: { kind: 'unexpected' } },
  ]) assert.equal(updateStatusSchema.safeParse(payload).success, false)
})

test('retry reconciles a lost install response without starting a second download', async t => {
  let latest: unknown = { revision: 0, state: { kind: 'idle' } }
  let installs = 0
  let checks = 0
  let failInstall: (() => void) | undefined
  const harness = await mountUpdater({ invoke: async command => {
    if (command === 'get_update_status') return latest
    if (command === 'check_for_update') {
      checks++
      return { revision: 1, state: { kind: 'available', version: '0.3.0' } }
    }
    installs++
    return new Promise((_resolve, reject) => { failInstall = () => reject(new Error('Raw transport details')) })
  } })
  t.after(() => harness.close())
  await harness.controller.check()
  const installing = harness.controller.install()
  await setImmediate()
  latest = { revision: 2, state: { kind: 'downloading', version: '0.3.0', downloaded: 200, total: 1000 } }
  harness.emit(latest)
  assert.ok(failInstall)
  failInstall()
  await installing
  assert.deepEqual(harness.controller.state.value, { kind: 'error', message: 'Couldn’t connect to the updater. Try again.' })
  assert.equal(harness.controller.busy.value, false)
  await harness.controller.check()
  assert.equal(harness.controller.state.value.kind, 'downloading')
  assert.equal(harness.controller.busy.value, true)
  assert.equal(checks, 1)
  assert.equal(installs, 1)
})

test('only states with a newer version count as an available update', async () => {
  const { updateAvailable } = await import('../../src/lib/updater.ts')
  const { createDemoUpdateState } = await import('../../src/lib/demo.ts')
  assert.equal(updateAvailable({ kind: 'available', version: '0.9.0' }), true)
  assert.equal(updateAvailable({ kind: 'downloading', version: '0.9.0', downloaded: 1, total: null }), true)
  assert.equal(updateAvailable({ kind: 'installing', version: '0.9.0' }), true)
  for (const kind of ['idle', 'checking', 'current'] as const) assert.equal(updateAvailable({ kind }), false)
  assert.equal(updateAvailable({ kind: 'error', message: 'Couldn’t check.' }), false)
  assert.equal(updateAvailable(createDemoUpdateState('update-available')), true)
  assert.equal(updateAvailable(createDemoUpdateState(null)), false)
})
