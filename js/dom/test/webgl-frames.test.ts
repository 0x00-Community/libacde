import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { WebglNative } from '../src/webgl/index.ts'
import { page } from './harness.ts'

// A WebGL native that draws nothing and hands out the GPU completion of each presented frame for the test to settle.
function gpu(): { native: WebglNative; done: Array<() => void> } {
  const done: Array<() => void> = []
  const calls: Record<string, unknown> = {
    create: () => 1,
    present: () => true,
    done: () => new Promise<void>((resolve) => done.push(resolve)),
    onLost: () => {},
    getError: () => 0,
    supported: () => [],
    drawingSize: () => [300, 150],
  }
  const native = new Proxy(calls, { get: (target, name) => target[name as string] ?? (() => undefined) }) as unknown as WebglNative
  return { native, done }
}

test('webgl: animation frames wait while two presented frames are still on the GPU', async () => {
  const { native, done } = gpu()
  using web = await page('<!doctype html><html><body><canvas id="c"></canvas></body></html>', { webgl: native })
  web.run(`
    const gl = document.getElementById('c').getContext('webgl')
    globalThis.ticks = 0
    const tick = () => { ticks++; gl.clear(gl.COLOR_BUFFER_BIT); requestAnimationFrame(tick) }
    requestAnimationFrame(tick)
  `)
  web.frame()
  web.frame()
  assert.equal(web.run('ticks'), 2)
  assert.equal(done.length, 2, 'each frame that drew asked for its GPU completion')
  web.frame()
  web.frame()
  assert.equal(web.run('ticks'), 2, 'no callbacks while two frames are in flight')
  assert.equal(done.length, 2, 'a frame that ran no callbacks presented nothing')
  done[0]!()
  await web.wait()
  web.frame()
  assert.equal(web.run('ticks'), 3, 'the oldest frame finished on the GPU, so the page draws again')
})
