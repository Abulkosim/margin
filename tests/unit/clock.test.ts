import assert from 'node:assert/strict'
import test from 'node:test'
import { createRenderer, defineComponent } from 'vue'
import { useMargin } from '../../src/lib/margin.ts'

type HostNode = { parent: HostNode | null; children: HostNode[] }

test('the countdown clock pauses while hidden and ticks on return', t => {
  const intervals = new Set<number>()
  let nextId = 1
  let visibility: 'visible' | 'hidden' = 'visible'
  const listeners = new Set<() => void>()
  const restore = ['window', 'document'].map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)] as const)
  const clear = globalThis.clearInterval
  globalThis.clearInterval = ((id?: number) => { if (id !== undefined) intervals.delete(id) }) as typeof clearInterval
  Object.defineProperty(globalThis, 'window', { configurable: true, value: { location: { search: '?demo=1' }, setInterval: () => { const id = nextId++; intervals.add(id); return id } } })
  Object.defineProperty(globalThis, 'document', {
    configurable: true,
    value: {
      get visibilityState() { return visibility },
      addEventListener: (_: string, listener: () => void) => { listeners.add(listener) },
      removeEventListener: (_: string, listener: () => void) => { listeners.delete(listener) },
    },
  })
  const node = (): HostNode => ({ parent: null, children: [] })
  const renderer = createRenderer<HostNode, HostNode>({
    createElement: node, createText: node, createComment: node, setText: () => {}, setElementText: () => {},
    parentNode: entry => entry.parent, nextSibling: () => null,
    insert: (entry, parent) => { entry.parent = parent; parent.children.push(entry) },
    remove: entry => { entry.parent = null }, patchProp: () => {},
  })
  let controller: ReturnType<typeof useMargin> | undefined
  const app = renderer.createApp(defineComponent({ setup() { controller = useMargin(); return () => null } }))
  app.mount(node())
  let mounted = true
  t.after(() => {
    if (mounted) app.unmount()
    globalThis.clearInterval = clear
    for (const [key, descriptor] of restore) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  })
  assert.ok(controller)
  assert.equal(intervals.size, 1)
  visibility = 'hidden'
  for (const listener of listeners) listener()
  assert.equal(intervals.size, 0)
  controller.now.value = 0
  visibility = 'visible'
  for (const listener of listeners) listener()
  assert.equal(intervals.size, 1)
  assert.ok(controller.now.value > 0)
  app.unmount()
  mounted = false
  assert.equal(intervals.size, 0)
  assert.equal(listeners.size, 0)
})
