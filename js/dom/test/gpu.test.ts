import assert from 'node:assert/strict'
import { test } from 'node:test'
import { installGpu } from '../src/gpu/index.ts'
import { ID } from '../src/gpu/pass.ts'
import type { GpuAdapterInfo, GpuNative } from '../src/native.ts'

type Adapter = { isFallbackAdapter: boolean; info: { isFallbackAdapter: boolean } }

const INFO: GpuAdapterInfo = {
  vendor: 'mesa',
  architecture: '',
  device: '',
  description: 'llvmpipe (LLVM 20.1.8, 256 bits)',
  isFallbackAdapter: true,
  features: [],
  limits: {},
}

async function adapterFor(info: Partial<GpuAdapterInfo>) {
  class Navigator {}
  const global = { navigator: new Navigator() } as unknown as typeof globalThis
  const native = { requestAdapter: async () => ({ ...INFO, ...info }) } as unknown as GpuNative
  installGpu(global, native, () => null)
  const gpu = (global.navigator as unknown as { gpu: { requestAdapter(): Promise<Adapter | null> } }).gpu
  const adapter = await gpu.requestAdapter()
  assert.ok(adapter)
  return adapter
}

test('webgpu: a software (CPU) adapter reports isFallbackAdapter on the adapter and its info', async () => {
  const adapter = await adapterFor({ isFallbackAdapter: true })
  assert.equal(adapter.isFallbackAdapter, true)
  assert.equal(adapter.info.isFallbackAdapter, true)
})

test('webgpu: a hardware adapter is not a fallback adapter', async () => {
  const adapter = await adapterFor({ isFallbackAdapter: false, description: 'NVIDIA GeForce RTX 2080 Ti' })
  assert.equal(adapter.isFallbackAdapter, false)
  assert.equal(adapter.info.isFallbackAdapter, false)
})

test('webgpu: a render pass sends its descriptor as numbers ahead of its commands, and a bad load op is a TypeError at once', async () => {
  class Navigator {}
  const global = { navigator: new Navigator() } as unknown as typeof globalThis
  const passes: { commands: number[]; strings: string[] }[] = []
  let next = 10
  const calls: Record<string, unknown> = {
    requestAdapter: async () => INFO,
    createDevice: () => 1,
    createCommandEncoder: () => 2,
    createTexture: () => ++next,
    createView: () => ++next,
    renderPass: (_device: number, _encoder: number, data: Float64Array, length: number, strings: string[]) => {
      passes.push({ commands: Array.from(data.subarray(0, length)), strings: [...strings] })
    },
  }
  const native = new Proxy(calls, { get: (target, name) => target[name as string] ?? (() => 0) }) as unknown as GpuNative
  installGpu(global, native, () => null)
  const gpu = (global.navigator as unknown as { gpu: any }).gpu
  const device = await (await gpu.requestAdapter()).requestDevice()
  const texture = device.createTexture({ size: [4, 4], format: 'rgba8unorm', usage: 0x10 })
  const view = texture.createView()
  const encoder = device.createCommandEncoder()
  assert.throws(() => encoder.beginRenderPass({ colorAttachments: [{ view, loadOp: 'keep', storeOp: 'store' }] }), TypeError)
  const pass = encoder.beginRenderPass({ label: 'sky', colorAttachments: [{ view, loadOp: 'clear', storeOp: 'store', clearValue: [0.25, 0.5, 0.75, 1] }, null] })
  pass.draw(3)
  pass.end()
  assert.deepEqual(passes, [{
    commands: [0, 2, 1, view[ID], -1, 0, 0.25, 0.5, 0.75, 1, 0, 0, 0, 0, 0, 0, 5, 3, 1, 0, 0],
    strings: ['sky'],
  }])
})
