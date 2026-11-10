import assert from 'node:assert/strict'
import test from 'node:test'
import { platformOf } from '../../src/lib/platform.ts'

test('only Windows reads as Windows; every other browser previews macOS', () => {
  assert.equal(platformOf('Mozilla/5.0 (Windows NT 10.0; Win64; x64)'), 'windows')
  assert.equal(platformOf('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)'), 'mac')
  assert.equal(platformOf('Mozilla/5.0 (X11; Linux x86_64)'), 'mac')
  assert.equal(platformOf(''), 'mac')
})
