import { readFileSync, readdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

/** Lines of the top-level `key:` block, without the key line. */
function block(lines, key, indent = '') {
  const start = lines.findIndex(line => line.startsWith(`${indent}${key}:`))
  if (start === -1) return null
  const inline = lines[start].slice(indent.length + key.length + 1).trim()
  const body = []
  for (const line of lines.slice(start + 1)) {
    if (line.trim() && !line.startsWith(`${indent} `) && !line.trimStart().startsWith('#')) break
    body.push(line)
  }
  return { inline, body }
}

/**
 * Problems with one workflow: actions not pinned to a commit with a version comment, write
 * permissions at the workflow level, jobs without a timeout, pull request workflows without a
 * concurrency group, and triggers that run pull request code with secrets.
 */
export function workflowProblems(name, text) {
  const problems = []
  const lines = text.split('\n')
  for (const [index, line] of lines.entries()) {
    const use = line.match(/^\s*(?:-\s*)?uses:\s*([^\s#]+)(.*)$/)
    if (!use || use[1].startsWith('./') || use[1].startsWith('docker://')) continue
    if (!/@[0-9a-f]{40}$/.test(use[1]) || !/^\s+#\s*\S/.test(use[2])) {
      problems.push(`${name}:${index + 1}: pin ${use[1]} to a commit SHA with a # version comment`)
    }
  }
  const on = block(lines, 'on')
  const triggers = on ? [on.inline, ...on.body].join('\n') : ''
  for (const trigger of ['pull_request_target', 'workflow_run']) {
    if (new RegExp(`\\b${trigger}\\b`).test(triggers)) problems.push(`${name}: ${trigger} runs untrusted code with secrets; use pull_request`)
  }
  const permissions = block(lines, 'permissions')
  if (!permissions) {
    problems.push(`${name}: set top-level permissions`)
  } else if (permissions.inline && permissions.inline !== 'read-all' && permissions.inline !== '{}') {
    problems.push(`${name}: top-level permissions must be read-only`)
  } else {
    for (const line of permissions.body) {
      const entry = line.match(/^\s+([\w-]+):\s*(\w+)/)
      if (entry && entry[2] === 'write') problems.push(`${name}: move ${entry[1]}: write from the workflow to the job that needs it`)
    }
  }
  if (/\bpull_request\b/.test(triggers) && !block(lines, 'concurrency')) {
    problems.push(`${name}: pull request workflows need a concurrency group`)
  }
  const jobs = block(lines, 'jobs')
  for (const [index, line] of (jobs?.body ?? []).entries()) {
    const job = line.match(/^ {2}([\w-]+):\s*$/)
    if (!job) continue
    const rest = jobs.body.slice(index + 1)
    const end = rest.findIndex(next => /^ {2}\S/.test(next))
    const body = end === -1 ? rest : rest.slice(0, end)
    if (!body.some(next => /^ {4}timeout-minutes:\s*\d+/.test(next))) problems.push(`${name}: job ${job[1]} needs timeout-minutes`)
  }
  return problems
}

export function checkWorkflows(root = '.') {
  const folder = join(root, '.github/workflows')
  return readdirSync(folder)
    .filter(name => /\.ya?ml$/.test(name))
    .sort()
    .flatMap(name => workflowProblems(name, readFileSync(join(folder, name), 'utf8')))
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const problems = checkWorkflows(resolve(dirname(process.argv[1]), '..'))
  for (const problem of problems) console.error(problem)
  if (problems.length) process.exit(1)
  console.log('Workflows are pinned, least-privilege and bounded.')
}
