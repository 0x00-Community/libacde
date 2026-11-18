import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { DevChannel, type DevHost, type DevTarget } from '../src/dev.ts'

function channel(evaluate: (code: string) => unknown = () => null) {
  const answers: { id: number; ok: boolean; payload: unknown }[] = []
  const posted: Record<string, unknown>[] = []
  const calls: [string, string, readonly unknown[]][] = []
  const page: DevTarget = {
    name: 'notes#1', app: 'notes', kind: 'app', window: 1, title: 'Notes', frame: 32,
    call: async (service, method, args) => {
      calls.push([service, method, args])
      return service === 'debug' ? evaluate(String(args[0])) : { clock: method }
    },
    post: (message) => void posted.push(message),
  }
  let reloads = 0
  const host: DevHost = {
    shell: {
      devRespond: (id, ok, payload) => void answers.push({ id, ok, payload: ok ? JSON.parse(payload) : payload }),
      screenshot: async () => ({ width: 10, height: 20 }),
    },
    page: () => page,
    inspect: () => { throw new Error('no inspect in this test') },
    reload: async () => void reloads++,
  }
  const dev = new DevChannel(host)
  const ask = async (request: Record<string, unknown>) => {
    await dev.request(JSON.stringify({ type: 'request', ...request }))
    return answers.at(-1)!
  }
  return { dev, answers, posted, calls, ask, reloads: () => reloads }
}

describe('the single-app dev channel', () => {
  test('webs lists the one page, eval answers through the page, console reads what it posted', async () => {
    const { dev, ask } = channel((code) => (code === 'document.title' ? 'Notes' : undefined))
    assert.equal(dev.take({ devlog: 'console', time: 1, level: 'log', text: 'notes ready' }), true)
    assert.equal(dev.take({ rpc: 'call' }), false)
    const webs = [{ name: 'notes#1', app: 'notes', kind: 'app', window: 1, title: 'Notes' }]
    assert.deepEqual(await ask({ id: 1, op: 'webs' }), { id: 1, ok: true, payload: { webs } })
    assert.deepEqual(await ask({ id: 2, op: 'eval', web: 'notes', code: 'document.title' }), { id: 2, ok: true, payload: { value: 'Notes' } })
    const read = (await ask({ id: 3, op: 'console' })).payload as { entries: { text: string; web: string; app: string }[] }
    assert.deepEqual(read.entries.map((e) => [e.text, e.web, e.app]), [['notes ready', 'notes#1', 'notes']])
  })

  test('an unknown web, an op that does not exist, open and a request that is not JSON', async () => {
    const { dev, answers, ask } = channel()
    assert.match(String((await ask({ id: 4, op: 'eval', web: 'other', code: '1' })).payload), /no web other \(webs: notes#1/)
    assert.match(String((await ask({ id: 5, op: 'fly' })).payload), /no op fly in a single-app session/)
    assert.match(String((await ask({ id: 6, op: 'open', name: 'files' })).payload), /single-app session .* has no other apps/)
    await dev.request('not json')
    assert.equal(answers.length, 3)
    assert.ok(answers.every((a) => !a.ok))
  })

  test('permissions has nothing pending, answer finds no prompt, revoke revokes nothing', async () => {
    const { ask } = channel()
    assert.deepEqual((await ask({ id: 7, op: 'permissions' })).payload, { pending: [], decisions: [] })
    assert.equal((await ask({ id: 8, op: 'answer', prompt: 1, allow: true })).ok, false)
    assert.deepEqual((await ask({ id: 9, op: 'revoke', app: 'notes', capability: 'camera' })).payload, { app: 'notes', capability: 'camera', revoked: false })
  })

  test('routes go to the page now and to the next page through prepare; a used one-shot route goes away', async () => {
    const { dev, posted, ask } = channel()
    const routed = (await ask({ id: 10, op: 'route', url: '/api/*', status: 200, body: 'x', times: 1 })).payload as { id: number }
    assert.equal(posted.at(-1)!['type'], 'devroutes')
    assert.equal((posted.at(-1)!['routes'] as unknown[]).length, 1)
    assert.equal(dev.prepare('notes#1', 'notes').devRoutes.length, 1)
    assert.equal(dev.take({ devroute: routed.id }), true)
    assert.equal(dev.prepare('notes#1', 'notes').devRoutes.length, 0)
    assert.deepEqual((await ask({ id: 11, op: 'unroute' })).payload, { removed: 0, left: 0 })
  })

  test('init scripts reach the next page, once means once; reload, dialog, clock and emulate reach the page', async () => {
    const { dev, posted, calls, ask, reloads } = channel()
    await ask({ id: 12, op: 'init', code: 'window.a = 1', once: true })
    await ask({ id: 13, op: 'init', code: 'window.b = 2', web: 'notes' })
    assert.deepEqual(dev.prepare('notes#1', 'notes').devInit, ['window.a = 1', 'window.b = 2'])
    assert.deepEqual(dev.prepare('notes#1', 'notes').devInit, ['window.b = 2'])
    assert.deepEqual((await ask({ id: 14, op: 'reload', web: 'notes' })).payload, { web: 'notes#1' })
    assert.equal(reloads(), 1)
    assert.deepEqual((await ask({ id: 15, op: 'dialog', accept: false })).payload, { web: 'notes#1', accept: false })
    assert.deepEqual(posted.at(-1), { type: 'devdialog', accept: false })
    await ask({ id: 16, op: 'clock', action: 'runFor', ms: 50 })
    assert.deepEqual(calls.at(-1), ['dev', 'clock', [{ action: 'runFor', ms: 50 }]])
    assert.equal((await ask({ id: 17, op: 'clock', action: 'runFor' })).ok, false)
    assert.deepEqual((await ask({ id: 18, op: 'emulate', colorScheme: 'dark' })).payload, { colorScheme: 'dark', emulated: true })
    assert.deepEqual(posted.at(-1), { type: 'colorscheme', dark: true })
  })

  test('screen wants an absolute path; resize and the host-window-only ops say when the window cannot', async () => {
    const { ask } = channel()
    assert.equal((await ask({ id: 19, op: 'screen', path: 'shot.png' })).ok, false)
    assert.deepEqual((await ask({ id: 20, op: 'screen', path: '/tmp/shot.png' })).payload, { width: 10, height: 20, path: '/tmp/shot.png' })
    assert.match(String((await ask({ id: 21, op: 'resize', width: 400, height: 300 })).payload), /only host windows/)
    assert.match(String((await ask({ id: 22, op: 'clipboard' })).payload), /host windows only/)
    assert.match(String((await ask({ id: 23, op: 'pointer', x: 1, y: 2 })).payload), /no injected input/)
  })
})
