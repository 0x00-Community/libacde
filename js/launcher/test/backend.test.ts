import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { BackendPlace } from '../src/backend/backend.ts'
import { bindNatives } from '../../page/src/sys/natives.ts'

bindNatives({ host: {}, sys: { platform: 'linux', argv: [], pid: 1, env: () => ({}), stderr: () => undefined } } as never)

describe('the debugger of an app backend', () => {
  test('only a dev session that names a port asks for one: 0 is any free port, anything else is refused', () => {
    assert.equal(BackendPlace.inspector({}), null)
    assert.equal(BackendPlace.inspector({ ACDE_INSPECT_BACKEND: '9229' }), null)
    assert.equal(BackendPlace.inspector({ ACDE_DEV: '/tmp/dev.sock' }), null)
    assert.equal(BackendPlace.inspector({ ACDE_DEV: '/tmp/dev.sock', ACDE_INSPECT_BACKEND: '9229' }), 9229)
    assert.equal(BackendPlace.inspector({ ACDE_DEV: '/tmp/dev.sock', ACDE_INSPECT_BACKEND: '0' }), 0)
    for (const bad of ['', '70000', '-1', '127.0.0.1:9229', 'yes']) {
      assert.match(String(BackendPlace.inspector({ ACDE_DEV: '1', ACDE_INSPECT_BACKEND: bad })), /is not a port \(0 to 65535\); no debugger$/)
    }
  })

  test('the inspector flag goes right after bun or node, on the loopback address only', () => {
    const bun = ['/usr/bin/bun', '--preload', '/a/acde-listen.js', '/a/server.js']
    assert.deepEqual(BackendPlace.inspected(bun, null), bun)
    assert.deepEqual(BackendPlace.inspected(bun, 0), ['/usr/bin/bun', '--inspect=127.0.0.1:0', '--preload', '/a/acde-listen.js', '/a/server.js'])
    const node = ['/usr/bin/node', '--import', 'file:///a/acde-listen.js', '/a/server.js']
    assert.deepEqual(BackendPlace.inspected(node, 9229), ['/usr/bin/node', '--inspect=127.0.0.1:9229', '--import', 'file:///a/acde-listen.js', '/a/server.js'])
    assert.equal(BackendPlace.mode(BackendPlace.inspected(node, 9229)), 'channel')
  })
})
