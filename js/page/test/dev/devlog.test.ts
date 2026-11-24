import assert from 'node:assert/strict'
import { test } from 'node:test'
import { DevLog } from '../../src/devlog.ts'

function logged(count: number): DevLog {
  const log = new DevLog()
  for (let i = 0; i < count; i++) log.take({ name: 'app', app: null }, { devlog: 'console', time: i, level: 'log', text: `line ${i}` } as never)
  return log
}

test('without since the newest entries come back and the older ones are counted as skipped', () => {
  const page = logged(8).console({ limit: 3 })
  assert.deepEqual(page.entries.map((e) => e.text), ['line 5', 'line 6', 'line 7'])
  assert.equal(page.skipped, 5)
  assert.equal(page.more, false)
})

test('from since the entries come in order and next leads on to the rest, so a follower misses none', () => {
  const log = logged(8)
  const first = log.console({ since: 1, limit: 3 })
  assert.deepEqual(first.entries.map((e) => e.text), ['line 0', 'line 1', 'line 2'])
  assert.equal(first.more, true)
  const second = log.console({ since: first.next, limit: 3 })
  assert.deepEqual(second.entries.map((e) => e.text), ['line 3', 'line 4', 'line 5'])
  const last = log.console({ since: second.next, limit: 3 })
  assert.deepEqual(last.entries.map((e) => e.text), ['line 6', 'line 7'])
  assert.equal(last.more, false)
  assert.equal(last.next, 9)
})
