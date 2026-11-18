import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { bindNatives } from '../../page/src/sys/natives.ts'
import { DevChannel } from '../src/dev/dev.ts'
import { DevToolsEndpoint, type DevToolsPage } from '../src/dev/devtools.ts'
import { managerRelay } from '../src/dev/relay.ts'

const PAGE: DevToolsPage = { name: 'notes#1', app: 'notes', kind: 'app', window: 1, title: 'Notes', url: null }

function manager() {
  const frames: object[] = []
  return { frames, link: { send: (frame: object) => void frames.push(frame) } }
}

describe('dev requests the session manager passes on', () => {
  test('no manager, or one that cannot send, gives no relay', () => {
    assert.equal(managerRelay(null), null)
    assert.equal(managerRelay({}), null)
  })

  test('an answer goes back to the manager under the id it asked with', async () => {
    const { frames, link } = manager()
    const relay = managerRelay(link)!
    const devRespond = () => assert.fail('the host shell answers nothing when a manager asked')
    const dev = new DevChannel({
      shell: { devRespond, screenshot: async () => ({ width: 0, height: 0 }) },
      page: () => null,
      inspect: () => { throw new Error('no inspect') },
      reload: async () => {},
      respond: relay.respond,
    })
    await dev.request(JSON.stringify({ type: 'request', id: 7, op: 'webs' }))
    await dev.request(JSON.stringify({ type: 'request', id: 8, op: 'fly' }))
    assert.deepEqual(frames[0], { devresponse: { id: 7, ok: true, payload: '{"webs":[]}' } })
    assert.match(JSON.stringify(frames[1]), /"id":8,"ok":false/)
  })

  test('the DevTools port is announced once it is served, and F12 becomes a devevent', () => {
    const { frames, link } = manager()
    const relay = managerRelay(link)!
    bindNatives({ host: {}, sys: { platform: 'linux', argv: [], pid: 1, stderr: () => {} } } as never)
    const devtools = { open: () => ({ port: 9229 }), targets: () => {} }
    const endpoint = new DevToolsEndpoint(devtools, relay, () => PAGE, relay.announce)
    endpoint.pressed()
    assert.deepEqual(frames[0], { devtools: { port: 9229 } })
    assert.deepEqual(JSON.parse((frames[1] as { devevent: string }).devevent), { type: 'event', event: 'devtools', web: 'notes#1', app: 'notes', window: 1 })
  })
})
