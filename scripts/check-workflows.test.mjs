import assert from 'node:assert/strict'
import { dirname, resolve } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'
import { checkWorkflows, workflowProblems } from './check-workflows.mjs'

const SHA = 'a'.repeat(40)

const good = `name: Good
on:
  push:
    branches: [main]
  pull_request:
permissions:
  contents: read
concurrency:
  group: \${{ github.workflow }}-\${{ github.ref }}
  cancel-in-progress: true
jobs:
  build:
    runs-on: ubuntu-24.04
    timeout-minutes: 10
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@${SHA} # v7.0.1
      - uses: ./local-action
      - run: echo ok
`

test('accepts a pinned, read-only, bounded workflow', () => {
  assert.deepEqual(workflowProblems('good.yml', good), [])
})

test('flags unpinned or uncommented actions', () => {
  const text = good.replace(`actions/checkout@${SHA} # v7.0.1`, 'actions/checkout@v7\n      - uses: dtolnay/rust-toolchain@' + SHA)
  assert.deepEqual(workflowProblems('w.yml', text), [
    'w.yml:18: pin actions/checkout@v7 to a commit SHA with a # version comment',
    `w.yml:19: pin dtolnay/rust-toolchain@${SHA} to a commit SHA with a # version comment`,
  ])
})

test('flags write permissions at the workflow level and missing permissions', () => {
  assert.deepEqual(workflowProblems('w.yml', good.replace('permissions:\n  contents: read\n', 'permissions:\n  contents: read\n  pages: write\n')),
    ['w.yml: move pages: write from the workflow to the job that needs it'])
  assert.deepEqual(workflowProblems('w.yml', good.replace('permissions:\n  contents: read\n', 'permissions: write-all\n')),
    ['w.yml: top-level permissions must be read-only'])
  assert.deepEqual(workflowProblems('w.yml', good.replace('permissions:\n  contents: read\n', '')),
    ['w.yml: set top-level permissions'])
})

test('flags pull_request_target, missing concurrency and missing job timeouts', () => {
  assert.deepEqual(workflowProblems('w.yml', good.replace('  pull_request:\n', '  pull_request_target:\n')),
    ['w.yml: pull_request_target runs untrusted code with secrets; use pull_request'])
  assert.deepEqual(workflowProblems('w.yml', good.replace(/concurrency:\n.*\n.*\n/, '')),
    ['w.yml: pull request workflows need a concurrency group'])
  assert.deepEqual(workflowProblems('w.yml', good.replace('    timeout-minutes: 10\n', '')),
    ['w.yml: job build needs timeout-minutes'])
})

test('the repository workflows pass', () => {
  assert.deepEqual(checkWorkflows(resolve(dirname(fileURLToPath(import.meta.url)), '..')), [])
})
