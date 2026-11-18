import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { bindNatives } from '../../page/src/sys/natives.ts'
import { DevChannel } from '../src/dev/dev.ts'
import { DevToolsEndpoint, type DevToolsPage, type NativeDevToolsTarget } from '../src/dev/devtools.ts'

const PAGE: DevToolsPage = { name: 'notes#1', app: 'notes', kind: 'app', window: 1, title: 'Notes', url: null }

function native(port: number | null) {
  const published: NativeDevToolsTarget[][] = []
  const reloads: string[] = []
  return {
    published,
    reloads,
    devtools: {
      open: () => (port === null ? null : { port }),
      targets: (t: NativeDevToolsTarget[]) => void published.push(t),
      reloading: (web: string) => void reloads.push(web),
    },
  }
}

describe('the single-app DevTools endpoint', () => {
  test('publishes the one page once until it changes, and forwards a reload', () => {
    const { devtools, published, reloads } = native(9229)
    let page: DevToolsPage | null = PAGE
    const endpoint = new DevToolsEndpoint(devtools, {}, () => page)
    endpoint.publish()
    endpoint.publish()
    assert.deepEqual(published, [[{ id: 'notes#1', kind: 'page', title: 'Notes (notes)', url: 'http://acde.local/' }]])
    page = null
    endpoint.publish()
    assert.deepEqual(published[1], [])
    endpoint.reloading('notes#1')
    assert.deepEqual(reloads, ['notes#1'])
    assert.deepEqual(endpoint.port(), { port: 9229 })
  })

  test('answering the port publishes the page first, so acdev devtools finds it at once', () => {
    const { devtools, published } = native(9229)
    const endpoint = new DevToolsEndpoint(devtools, {}, () => PAGE)
    assert.deepEqual(endpoint.port(), { port: 9229 })
    assert.equal(published.length, 1)
    assert.equal(published[0]![0]!.id, 'notes#1')
  })

  test('the devtools request answers the port, or says no endpoint is served', async () => {
    const answers: { ok: boolean; payload: string }[] = []
    const shell = {
      devRespond: (_id: number, ok: boolean, payload: string) => void answers.push({ ok, payload }),
      screenshot: async () => ({ width: 0, height: 0 }),
    }
    const host = (port: number | null) => ({
      shell,
      devtools: new DevToolsEndpoint(native(port).devtools, {}, () => PAGE),
      page: () => null,
      inspect: () => { throw new Error('no inspect') },
      reload: async () => {},
    })
    const served = new DevChannel(host(9229))
    const none = new DevChannel(host(null))
    await served.request(JSON.stringify({ id: 1, op: 'devtools' }))
    await none.request(JSON.stringify({ id: 2, op: 'devtools' }))
    assert.deepEqual(answers, [{ ok: true, payload: '{"port":9229}' }, { ok: false, payload: 'devtools: this session serves no DevTools endpoint' }])
  })

  test('F12 sends the page to acdev dev as a devtools event', () => {
    const sent: unknown[] = []
    const said: string[] = []
    bindNatives({ host: {}, sys: { platform: 'linux', argv: [], pid: 1, stderr: (text: string) => void said.push(text) } } as never)
    const endpoint = new DevToolsEndpoint(native(null).devtools, { devEvent: (json) => sent.push(JSON.parse(json)) }, () => PAGE)
    endpoint.pressed()
    assert.deepEqual(sent, [{ type: 'event', event: 'devtools', web: 'notes#1', app: 'notes', window: 1 }])
    assert.deepEqual(said, ['acdev: F12 on notes#1: asked acdev dev to open DevTools\n'])
  })
})
