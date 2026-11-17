import assert from 'node:assert/strict'
import { test } from 'node:test'
import { installGpu } from '../src/gpu/index.ts'
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
