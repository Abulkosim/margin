import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { commands } from '../../src/lib/tauri.ts'

test('the command list matches the Rust handler list', () => {
  const source = readFileSync(new URL('../../src-tauri/src/lib.rs', import.meta.url), 'utf8')
  const handlers = /generate_handler!\[([^\]]*)\]/.exec(source)?.[1]
  assert.ok(handlers)
  const names = handlers.split(',').map(entry => entry.trim().split('::').at(-1)).filter(Boolean)
  assert.deepEqual([...commands].sort(), names.sort())
})
