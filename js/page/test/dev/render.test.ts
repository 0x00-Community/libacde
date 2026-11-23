import assert from 'node:assert/strict'
import { describe, test } from 'node:test'
import { PageRenderReport } from '../../src/dev/render.ts'

const fact = (extra: Record<string, unknown> = {}) => ({
  panel: null, rect: [10, 20, 100, 50], glass: true, refracts: true,
  group: null, pieces: 1, leftOut: false, ...extra,
})

function report(raster: Record<string, unknown>, backdrops: unknown[], hosted: boolean) {
  const dom = {
    renderReport: () => JSON.stringify({ raster, backdrops }),
    hitTestAll: () => new Uint32Array([31, 30]),
    computedStyle: (id: number) => (id === 30 ? 'blur(12px)' : 'none'),
  }
  return new PageRenderReport(dom as never, [0, 32], hosted).report()
}

describe('the render path a page reports', () => {
  test('a host window on the GPU draws glass itself; the layer is placed under the title bar and found by its backdrop-filter', () => {
    const gpu = { path: 'gpu', adapter: 'A770', driver: 'mesa', backend: 'vulkan', software: false }
    const shown = report(gpu, [fact()], true)
    assert.deepEqual(shown.raster, gpu)
    assert.equal(shown.composedBy, 'host-window')
    assert.deepEqual(shown.layers, [{ rect: [10, 52, 100, 50], kind: 'glass', element: 30, drawn: 'gpu-glass' }])
  })

  test('on the CPU a lens glass is frosted and says why, and the raster says no-gpu', () => {
    const shown = report({ path: 'cpu', canvases: null }, [fact(), fact({ refracts: false }), fact({ glass: false, refracts: false })], true)
    assert.equal(shown.raster.degraded, 'no-gpu')
    assert.deepEqual(shown.layers.map((l) => [l.drawn, l.degraded ?? null]), [
      ['cpu-glass-frosted', 'cpu-cannot-refract'],
      ['cpu-glass', null],
      ['frost', null],
    ])
  })

  test('in a session the compositor draws the glass; a piece left out of a full group and a panel layer say so', () => {
    const shown = report({ path: 'cpu', canvases: 'llvmpipe' }, [fact({ group: 'g0', pieces: 16 }), fact({ leftOut: true }), fact({ panel: 4 })], false)
    assert.equal(shown.composedBy, 'compositor')
    assert.deepEqual(shown.layers[0], { rect: [10, 52, 100, 50], kind: 'glass', group: 'g0', pieces: 16, element: 30, drawn: 'compositor' })
    assert.equal(shown.layers[1]!.degraded, 'group-full')
    assert.deepEqual(shown.layers[2], { panel: 4, rect: [10, 20, 100, 50], kind: 'glass', drawn: 'compositor' })
  })
})
