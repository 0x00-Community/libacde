import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { DevChannel } from '../../src/dev/dev.ts'
import { DevTrace, type DevTraceClock } from '../../src/devtrace.ts'

function traced() {
  const lines: string[] = []
  let now = Date.UTC(2026, 10, 24, 8, 0, 0)
  let tick: (() => void) | null = null
  const clock: DevTraceClock = {
    now: () => now,
    every: (_, run) => {
      tick = run
      return () => {
        tick = null
      }
    },
  }
  const trace = new DevTrace((line) => void lines.push(line), clock)
  const pass = (ms: number) => {
    now += ms
  }
  return { lines, trace, pass, fire: () => tick?.(), ticking: () => tick !== null }
}

describe('the dev diagnostics on stderr', () => {
  test('they are on only in a dev session', () => {
    assert.equal(DevTrace.of({}, () => {}), null)
    assert.ok(DevTrace.of({ ACDE_DEV: 'guest' }, () => {}))
  })

  test('every line has the prefix and the time; a request is logged when it comes and when it is answered', () => {
    const { lines, trace, pass } = traced()
    const since = trace.request('webs', 7)
    pass(42)
    trace.answered('webs', 7, since, true)
    assert.deepEqual(lines, [
      'acde_dev: 2026-11-24T08:00:00.000Z devrequest webs id=7 received\n',
      'acde_dev: 2026-11-24T08:00:00.042Z devrequest webs id=7 answered ok in 42 ms\n',
    ])
  })

  test('wakes are summed every five seconds, and the end and the leaving of the loop say why', () => {
    const { lines, trace, fire, ticking } = traced()
    trace.start()
    trace.notified()
    trace.woke()
    trace.notified()
    trace.notified()
    trace.woke()
    fire()
    fire()
    trace.ended(0, 'the manager closed the channel')
    trace.left(0)
    assert.equal(ticking(), false)
    assert.deepEqual(lines.map((l) => l.replace(/^acde_dev: \S+ /, '')), [
      'run loop woke 2 time(s) on 3 event notice(s) in the last 5 s\n',
      'run loop woke 0 time(s) on 0 event notice(s) in the last 5 s\n',
      'launcher ends (code 0): the manager closed the channel\n',
      'run loop woke 0 time(s) on 0 event notice(s) in the last 5 s\n',
      'run loop left (code 0)\n',
    ])
  })

  test('the dev channel logs what it was asked and how it answered', async () => {
    const { lines, trace } = traced()
    const answers: unknown[] = []
    const dev = new DevChannel({
      shell: { screenshot: async () => ({ width: 0, height: 0 }) },
      page: () => null,
      inspect: () => {
        throw new Error('no inspect')
      },
      reload: async () => {},
      respond: (id, ok) => void answers.push([id, ok]),
      trace,
    })
    await dev.request(JSON.stringify({ type: 'request', id: 3, op: 'webs' }))
    await dev.request(JSON.stringify({ type: 'request', id: 4, op: 'fly' }))
    assert.deepEqual(answers, [[3, true], [4, false]])
    assert.deepEqual(lines.map((l) => l.replace(/^acde_dev: \S+ /, '')), [
      'devrequest webs id=3 received\n',
      'devrequest webs id=3 answer ready in 0 ms, sending 11 bytes\n',
      'devrequest webs id=3 answered ok in 0 ms\n',
      'devrequest fly id=4 received\n',
      'devrequest fly id=4 answer ready in 0 ms, sending 33 bytes\n',
      'devrequest fly id=4 answered error in 0 ms\n',
    ])
  })
})
