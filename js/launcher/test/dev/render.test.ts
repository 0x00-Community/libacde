import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { renderAnswer } from '../../src/dev/render.ts'

describe('the render answer of a page', () => {
  test('moves the page layers onto the screen and names their elements by ref; panel layers keep their own place', () => {
    const window = {
      ref: 'w5', id: 5, kind: 'web', app: 'notes', title: 'Notes', mode: 'window', rect: [100, 50, 400, 300],
      z: 1, focus: true, hidden: false, pid: null,
    } as const
    const page = {
      raster: { path: 'gpu', adapter: 'A', driver: 'd', backend: 'vulkan', software: false },
      composedBy: 'host-window',
      layers: [
        { rect: [10, 20, 30, 40], kind: 'glass', element: 7, drawn: 'gpu-glass' },
        { panel: 2, rect: [1, 2, 3, 4], kind: 'frost', drawn: 'frost' },
      ],
    } as const
    const answer = renderAnswer('notes#1', window as never, page)
    assert.equal(answer.window, 'w5')
    assert.deepEqual(answer.layers[0], { rect: [110, 70, 30, 40], kind: 'glass', element: 'w5.7', drawn: 'gpu-glass' })
    assert.deepEqual(answer.layers[1], { panel: 2, rect: [1, 2, 3, 4], kind: 'frost', drawn: 'frost' })
  })
})
