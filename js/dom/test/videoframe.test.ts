import assert from 'node:assert/strict'
import { test } from 'node:test'
import { page } from './harness.ts'

// Expected values come from headless Chrome 157 running the same calls (2026-11-18). YUV to RGB matches within 4 levels:
// Chrome converts with libyuv's fixed-point coefficients, these are exact BT.709 / BT.601.
const near = (actual: number[], expected: number[], tolerance: number) =>
  assert.ok(actual.length === expected.length && actual.every((v, i) => Math.abs(v - expected[i]!) <= tolerance), `${actual} vs ${expected}`)

test('VideoFrame from an I420 buffer: properties, copies in its format, RGBA conversion, crops', async () => {
  using web = await page()
  const out = JSON.parse(
    await web.run<Promise<string>>(`(async () => {
      const f = new VideoFrame(new Uint8Array([16, 60, 120, 235, 81, 145, 41, 210, 90, 240, 240, 110]), { format: 'I420', codedWidth: 4, codedHeight: 2, timestamp: 1234 })
      const same = new Uint8Array(f.allocationSize()), rgba = new Uint8Array(32), crop = new Uint8Array(6)
      const sameLayout = await f.copyTo(same)
      const rgbaLayout = await f.copyTo(rgba, { format: 'RGBA' })
      await f.copyTo(crop, { rect: { x: 2, y: 0, width: 2, height: 2 } })
      return JSON.stringify({ format: f.format, coded: [f.codedWidth, f.codedHeight], display: [f.displayWidth, f.displayHeight], ts: f.timestamp, duration: f.duration,
        visible: [f.visibleRect.x, f.visibleRect.y, f.visibleRect.width, f.visibleRect.height], space: f.colorSpace.toJSON(), size: f.allocationSize(),
        sameLayout, same: [...same], rgbaLayout, rgba: [...rgba], crop: [...crop] })
    })()`),
  )
  assert.equal(out.format, 'I420')
  assert.deepEqual([out.coded, out.display, out.ts, out.duration, out.visible], [[4, 2], [4, 2], 1234, null, [0, 0, 4, 2]])
  assert.deepEqual(out.space, { primaries: 'bt709', transfer: 'bt709', matrix: 'bt709', fullRange: false })
  assert.equal(out.size, 12)
  assert.deepEqual(out.sameLayout, [{ offset: 0, stride: 4 }, { offset: 8, stride: 2 }, { offset: 10, stride: 2 }])
  assert.deepEqual(out.same, [16, 60, 120, 235, 81, 145, 41, 210, 90, 240, 240, 110])
  assert.deepEqual(out.rgbaLayout, [{ offset: 0, stride: 16 }])
  near(out.rgba, [201, 0, 0, 255, 252, 0, 0, 255, 89, 106, 255, 255, 223, 240, 255, 255, 255, 24, 0, 255, 255, 99, 74, 255, 0, 14, 253, 255, 193, 211, 255, 255], 4)
  assert.deepEqual(out.crop, [120, 235, 41, 210, 240, 110])
})

test('VideoFrame: NV12 with a partial color space, BGRA to RGBA, clone and close, required fields', async () => {
  using web = await page()
  const out = JSON.parse(
    await web.run<Promise<string>>(`(async () => {
      const nv12 = new VideoFrame(new Uint8Array([16, 235, 128, 80, 128, 200, 40, 240]), { format: 'NV12', codedWidth: 2, codedHeight: 2, timestamp: 5, colorSpace: { matrix: 'smpte170m', fullRange: false } })
      const nv = new Uint8Array(16); await nv12.copyTo(nv, { format: 'RGBA' })
      const bgra = new VideoFrame(new Uint8Array([10, 20, 30, 40, 50, 60, 70, 80]), { format: 'BGRA', codedWidth: 2, codedHeight: 1, timestamp: 0 })
      const b = new Uint8Array(8); await bgra.copyTo(b, { format: 'RGBA' })
      const clone = nv12.clone(); nv12.close()
      const error = (f) => { try { f(); return 'no error' } catch (e) { return e.name } }
      return JSON.stringify({ nv: [...nv], nvSpace: clone.colorSpace.toJSON(), bgra: [...b],
        closed: { format: nv12.format, coded: nv12.codedWidth, ts: nv12.timestamp }, cloneTs: clone.timestamp,
        cloneClosed: error(() => nv12.clone()),
        noTimestamp: error(() => new VideoFrame(new Uint8Array(4), { format: 'RGBA', codedWidth: 1, codedHeight: 1 })),
        tooSmall: error(() => new VideoFrame(new Uint8Array(3), { format: 'RGBA', codedWidth: 1, codedHeight: 1, timestamp: 0 })),
        badFormat: error(() => new VideoFrame(new Uint8Array(4), { format: 'YUYV', codedWidth: 1, codedHeight: 1, timestamp: 0 })),
        globals: [typeof VideoFrame, typeof VideoColorSpace] })
    })()`),
  )
  near(out.nv, [115, 0, 0, 255, 255, 196, 255, 255, 245, 72, 130, 255, 189, 16, 74, 255], 4)
  assert.deepEqual(out.nvSpace, { primaries: null, transfer: null, matrix: 'smpte170m', fullRange: false })
  // same color space, so the channels only swap (Chrome 157 moves them a few levels: [32, 19, 13, 40, 70, 61, 51, 80])
  assert.deepEqual(out.bgra, [30, 20, 10, 40, 70, 60, 50, 80])
  assert.deepEqual(out.closed, { format: null, coded: 0, ts: 5 })
  assert.equal(out.cloneTs, 5)
  assert.deepEqual([out.cloneClosed, out.noTimestamp, out.tooSmall, out.badFormat], ['InvalidStateError', 'TypeError', 'TypeError', 'TypeError'])
  assert.deepEqual(out.globals, ['function', 'function'])
})

test('a VideoFrame draws into a 2D canvas as the canvas it stands for', async () => {
  using web = await page('<!doctype html><html><body><canvas id="c" width="4" height="4"></canvas></body></html>')
  const thrown = web.run<string>(`(() => {
    const f = new VideoFrame(new Uint8Array(16).fill(255), { format: 'RGBA', codedWidth: 2, codedHeight: 2, timestamp: 0 })
    try { document.getElementById('c').getContext('2d').drawImage(f, 0, 0); return 'ok' } catch (e) { return String(e) }
  })()`)
  assert.equal(thrown, 'ok')
})
