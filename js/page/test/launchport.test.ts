import assert from 'node:assert/strict'
import { test } from 'node:test'
import { LaunchPort } from '../src/launchport.ts'

test('the launch port: the page gets one message event with the data and the port, in a later task; frames reach the line port', () => {
  const posted: Record<string, unknown>[] = []
  const received: (string | null)[] = []
  let closed = 0
  let sides: { send(line: string): void; closed(): void } | null = null
  const fake = { port: 'the port', receive: (line: string | null) => void received.push(line), close: () => void closed++ }
  const make = (line: { send(line: string): void; closed(): void }) => {
    sides = line
    return fake as never
  }
  const launched = new LaunchPort({ type: 'x:init' }, make, (message) => void posted.push(message))
  sides!.send('{"a":1}')
  sides!.closed()
  assert.deepEqual(posted, [{ portline: '{"a":1}' }, { portclosed: true }])
  assert.equal(launched.take({ type: 'portline', line: '[1]' }), true)
  launched.take({ type: 'portline', line: 5 })
  launched.take({ type: 'portclosed' })
  assert.equal(launched.take({ type: 'screen' }), false)
  assert.deepEqual(received, ['[1]', null])
  assert.equal(closed, 1)
  const tasks: (() => void)[] = []
  const events: unknown[] = []
  class MessageEvent {
    constructor(readonly type: string, readonly init: unknown) {}
  }
  launched.hand({ MessageEvent, dispatchEvent: (event) => events.push(event) > 0, setTimeout: (run) => tasks.push(run) })
  assert.deepEqual(events, [])
  tasks[0]!()
  assert.deepEqual(events, [new MessageEvent('message', { data: { type: 'x:init' }, ports: ['the port'] })])
})
