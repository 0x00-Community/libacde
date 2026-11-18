import assert from 'node:assert/strict'
import { test } from 'node:test'
import { bindHost } from '../../dom/src/core/host.ts'
import { MockHost } from '../../dom/test/mock/host.ts'
import { WebAppPage } from '../src/webapp.ts'

bindHost({}, new MockHost())

function page() {
  const calls: [string, string, unknown[]][] = []
  const window = new EventTarget() as EventTarget & { Event: typeof Event }
  window.Event = Event
  const internals = { env: {} as Record<string, unknown>, window, document: {} }
  const rpc = { call: async (service: string, method: string, args: unknown[]) => void calls.push([service, method, args]) }
  new WebAppPage(internals as never, rpc as never, 'notes#1')
  return { env: internals.env as { navigate(url: string, target: string): void; reload(): void; close(): void; url: string }, calls }
}

const nextTask = () => new Promise((done) => setTimeout(done, 5))

test('a navigation reaches the launcher after the task that asked for it, not inside it', async () => {
  const { env, calls } = page()
  env.navigate('http://acde.local/next.html', '_self')
  assert.deepEqual(calls, [], 'nothing is sent while the task runs: its other replies go out first')
  await nextTask()
  assert.deepEqual(calls, [['window', 'reload', ['http://acde.local/next.html']]])
})

test('reload and close wait for the task to end too', async () => {
  const { env, calls } = page()
  env.url = 'http://acde.local/'
  env.reload()
  assert.deepEqual(calls, [])
  await nextTask()
  assert.deepEqual(calls, [['window', 'reload', ['http://acde.local/']]])
  const other = page()
  other.env.close()
  assert.deepEqual(other.calls, [])
  await nextTask()
  assert.deepEqual(other.calls, [['window', 'close', []]])
})
