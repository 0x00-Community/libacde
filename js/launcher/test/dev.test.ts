import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { DevChannel, type DevTarget } from '../src/dev.ts'

function channel(evaluate: (code: string) => unknown = () => null) {
  const answers: { id: number; ok: boolean; payload: unknown }[] = []
  const page: DevTarget = {
    name: 'notes#1', app: 'notes', kind: 'app', window: 1, title: 'Notes',
    call: async (service, method, args) => {
      assert.equal(service, 'debug')
      assert.equal(method, 'eval')
      return evaluate(String(args[0]))
    },
  }
  const shell = { devRespond: (id: number, ok: boolean, payload: string) => void answers.push({ id, ok, payload: ok ? JSON.parse(payload) : payload }) }
  return { dev: new DevChannel(shell, () => page), answers }
}

describe('the single-app dev channel', () => {
  test('webs lists the one page, eval answers through the page, console reads what it posted', async () => {
    const { dev, answers } = channel((code) => (code === 'document.title' ? 'Notes' : undefined))
    assert.equal(dev.take({ devlog: 'console', time: 1, level: 'log', text: 'notes ready' }), true)
    assert.equal(dev.take({ rpc: 'call' }), false)
    await dev.request(JSON.stringify({ type: 'request', id: 1, op: 'webs' }))
    await dev.request(JSON.stringify({ type: 'request', id: 2, op: 'eval', web: 'notes', code: 'document.title' }))
    await dev.request(JSON.stringify({ type: 'request', id: 3, op: 'console' }))
    assert.deepEqual(answers[0], { id: 1, ok: true, payload: { webs: [{ name: 'notes#1', app: 'notes', kind: 'app', window: 1, title: 'Notes' }] } })
    assert.deepEqual(answers[1], { id: 2, ok: true, payload: { value: 'Notes' } })
    const read = answers[2]!.payload as { entries: { text: string; web: string; app: string }[] }
    assert.deepEqual(read.entries.map((e) => [e.text, e.web, e.app]), [['notes ready', 'notes#1', 'notes']])
  })

  test('an unknown web, an op not answered yet and a request that is not JSON', async () => {
    const { dev, answers } = channel()
    await dev.request(JSON.stringify({ id: 4, op: 'eval', web: 'other', code: '1' }))
    await dev.request(JSON.stringify({ id: 5, op: 'snapshot' }))
    await dev.request('not json')
    assert.equal(answers.length, 2)
    assert.equal(answers[0]!.ok, false)
    assert.match(String(answers[0]!.payload), /no web other \(webs: notes#1/)
    assert.equal(answers[1]!.ok, false)
    assert.match(String(answers[1]!.payload), /snapshot: not answered in a single-app session yet/)
  })

  test('permissions has nothing pending: a single-app session asks no one', async () => {
    const { dev, answers } = channel()
    await dev.request(JSON.stringify({ id: 6, op: 'permissions' }))
    assert.deepEqual(answers[0], { id: 6, ok: true, payload: { pending: [], decisions: [] } })
  })
})
