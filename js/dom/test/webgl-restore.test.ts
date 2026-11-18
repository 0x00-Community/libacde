import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { WebglNative } from '../src/webgl/index.ts'
import { page } from './harness.ts'

// A WebGL native that hands out context ids, remembers errors and lets the test send the GPU watch's notices.
function gpu() {
  const state = { next: 1, created: [] as number[], released: [] as number[], errors: [] as number[], watch: null as ((kind: 'lost' | 'restored') => void) | null, lost: null as ((id: number) => void) | null }
  const calls: Record<string, unknown> = {
    create: () => {
      const id = state.next++
      state.created.push(id)
      return id
    },
    release: (id: number) => state.released.push(id),
    present: () => true,
    done: () => Promise.resolve(),
    onLost: (f: (id: number) => void) => (state.lost = f),
    watch: (f: (kind: 'lost' | 'restored') => void) => (state.watch = f),
    error: (_: number, code: number) => state.errors.push(code),
    getError: () => 0,
    supported: () => ['WEBGL_lose_context'],
    enableExtension: () => true,
    drawingSize: () => [300, 150],
    createBuffer: () => 7,
  }
  const native = new Proxy(calls, { get: (target, name) => target[name as string] ?? (() => undefined) }) as unknown as WebglNative
  return { native, state }
}

test('webgl: a cancelled webglcontextlost restores on a new device; old objects are invalid after', async () => {
  const { native, state } = gpu()
  using web = await page('<!doctype html><html><body><canvas id="c"></canvas></body></html>', { webgl: native })
  web.run(`
    globalThis.events = []
    const canvas = document.getElementById('c')
    globalThis.gl = canvas.getContext('webgl')
    canvas.addEventListener('webglcontextlost', (e) => { events.push('lost'); e.preventDefault() })
    canvas.addEventListener('webglcontextrestored', () => events.push('restored'))
    globalThis.old = gl.createBuffer()
  `)
  state.lost!(1)
  assert.deepEqual(web.json('events'), ['lost'])
  assert.equal(web.run('gl.isContextLost()'), true)
  state.watch!('restored')
  assert.deepEqual(web.json('events'), ['lost', 'restored'])
  assert.equal(web.run('gl.isContextLost()'), false)
  assert.deepEqual([state.created, state.released], [[1, 2], [1]], 'a new native context replaced the lost one')
  web.run('gl.bindBuffer(gl.ARRAY_BUFFER, old)')
  assert.deepEqual(state.errors, [0x0502], 'a buffer from before the loss is an INVALID_OPERATION')
})

test('webgl: an uncancelled loss stays lost; restoreContext needs a lost context', async () => {
  const { native, state } = gpu()
  using web = await page('<!doctype html><html><body><canvas id="c"></canvas></body></html>', { webgl: native })
  web.run(`
    globalThis.events = []
    const canvas = document.getElementById('c')
    globalThis.gl = canvas.getContext('webgl')
    canvas.addEventListener('webglcontextrestored', () => events.push('restored'))
    globalThis.ext = gl.getExtension('WEBGL_lose_context')
    ext.restoreContext()
  `)
  assert.deepEqual(state.errors, [0x0502], 'restoring a context that is not lost is an INVALID_OPERATION')
  state.lost!(1)
  state.watch!('restored')
  assert.equal(web.run('gl.isContextLost()'), true, 'the page did not cancel webglcontextlost')
  assert.deepEqual(web.json('events'), [])
})

test('webgl: WEBGL_lose_context round trip restores in a later task', async () => {
  const { native } = gpu()
  using web = await page('<!doctype html><html><body><canvas id="c"></canvas></body></html>', { webgl: native })
  web.run(`
    globalThis.events = []
    const canvas = document.getElementById('c')
    globalThis.gl = canvas.getContext('webgl')
    canvas.addEventListener('webglcontextlost', (e) => { events.push('lost'); e.preventDefault() })
    canvas.addEventListener('webglcontextrestored', () => events.push('restored'))
    const ext = gl.getExtension('WEBGL_lose_context')
    ext.loseContext()
    ext.restoreContext()
  `)
  assert.deepEqual(web.json('events'), ['lost'], 'not restored synchronously')
  await web.wait(5)
  assert.deepEqual(web.json('events'), ['lost', 'restored'])
})
