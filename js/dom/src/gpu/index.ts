import { defineHandlers, Event, EventTarget, fire } from '../core/event'
import { DOMException, guard, nativeException } from '../core/exception'
import { base64 } from '../core/encoding'
import { host, tag } from '../core/host'
import type { GpuAdapterInfo, GpuNative } from '../native'
import { colorOf, GPUComputePassEncoder, GPURenderBundleEncoder, GPURenderPassEncoder, ID, idOf, type Owned } from './pass'

let native: GpuNative = null as unknown as GpuNative

const dropped = new FinalizationRegistry<number>((id) => native.drop(id))

type Dict = Record<string, any>

function extent(e: any): Dict {
  if (e && typeof e === 'object' && Symbol.iterator in e) {
    const [width, height = 1, depthOrArrayLayers = 1] = Array.from(e as Iterable<number>)
    if (width === undefined) throw new TypeError('A GPUExtent3D needs a width')
    return { width, height, depthOrArrayLayers }
  }
  if (!e || e.width === undefined) throw new TypeError('A GPUExtent3D needs a width')
  return { width: e.width, height: e.height ?? 1, depthOrArrayLayers: e.depthOrArrayLayers ?? 1 }
}

function origin(o: any): Dict {
  if (!o) return { x: 0, y: 0, z: 0 }
  if (Symbol.iterator in o) {
    const [x = 0, y = 0, z = 0] = Array.from(o as Iterable<number>)
    return { x, y, z }
  }
  return { x: o.x ?? 0, y: o.y ?? 0, z: o.z ?? 0 }
}

const texelTexture = (d: Dict): Dict => ({ texture: idOf(d.texture), mipLevel: d.mipLevel, origin: d.origin && origin(d.origin), aspect: d.aspect })
const texelBuffer = (d: Dict): Dict => ({ buffer: idOf(d.buffer), offset: d.offset, bytesPerRow: d.bytesPerRow, rowsPerImage: d.rowsPerImage })
const stage = (d: Dict): Dict => ({ module: idOf(d.module), entryPoint: d.entryPoint, constants: d.constants ?? {} })
const viewOf = (v: unknown): number => (v instanceof GPUTexture ? idOf(v.createView()) : idOf(v))

class GPUSupportedFeatures {
  readonly #set: Set<string>
  constructor(names: Iterable<string>) {
    this.#set = new Set(names)
  }
  get size(): number {
    return this.#set.size
  }
  has(name: string): boolean {
    return this.#set.has(name)
  }
  keys(): IterableIterator<string> {
    return this.#set.keys()
  }
  values(): IterableIterator<string> {
    return this.#set.values()
  }
  entries(): IterableIterator<[string, string]> {
    return this.#set.entries()
  }
  forEach(f: (value: string, key: string, set: GPUSupportedFeatures) => void, thisArg?: unknown): void {
    for (const v of this.#set) f.call(thisArg, v, v, this)
  }
  [Symbol.iterator](): IterableIterator<string> {
    return this.#set.values()
  }
}
tag(GPUSupportedFeatures)

class GPUSupportedLimits {
  constructor(values: Record<string, number>) {
    for (const [name, value] of Object.entries(values)) Object.defineProperty(this, name, { value, enumerable: true })
  }
}
tag(GPUSupportedLimits)

class GPUAdapterInfo {
  declare readonly vendor: string
  declare readonly architecture: string
  declare readonly device: string
  declare readonly description: string
  declare readonly isFallbackAdapter: boolean
  constructor({ vendor, architecture, device, description, isFallbackAdapter }: GpuAdapterInfo) {
    Object.assign(this, { vendor, architecture, device, description, subgroupMinSize: 4, subgroupMaxSize: 128, isFallbackAdapter: isFallbackAdapter === true })
  }
}
tag(GPUAdapterInfo)

class GPUError {
  readonly message: string
  constructor(message: string) {
    this.message = String(message)
  }
}
tag(GPUError)
class GPUValidationError extends GPUError {}
tag(GPUValidationError)
class GPUOutOfMemoryError extends GPUError {}
tag(GPUOutOfMemoryError)
class GPUInternalError extends GPUError {}
tag(GPUInternalError)

function gpuError(kind: string, message: string): GPUError {
  if (kind === 'out-of-memory') return new GPUOutOfMemoryError(message)
  if (kind === 'internal') return new GPUInternalError(message)
  return new GPUValidationError(message)
}

class GPUPipelineError extends DOMException {
  readonly reason: string
  constructor(message = '', options: { reason: string } = { reason: 'validation' }) {
    super(message, 'GPUPipelineError')
    this.reason = options.reason
  }
}
tag(GPUPipelineError)

class GPUUncapturedErrorEvent extends Event {
  readonly error: GPUError
  constructor(type: string, init: { error: GPUError; bubbles?: boolean; cancelable?: boolean }) {
    super(type, init)
    this.error = init.error
  }
}
tag(GPUUncapturedErrorEvent)

class GPUDeviceLostInfo {
  constructor(readonly reason: string, readonly message: string) {}
}
tag(GPUDeviceLostInfo)

class GPUCompilationMessage {
  constructor(m: { message: string; type: string; lineNum: number; linePos: number; offset: number; length: number }) {
    Object.assign(this, m)
  }
}
tag(GPUCompilationMessage)

class GPUCompilationInfo {
  readonly messages: readonly GPUCompilationMessage[]
  constructor(messages: GPUCompilationMessage[]) {
    this.messages = Object.freeze(messages)
  }
}
tag(GPUCompilationInfo)

const DEVICE = Symbol('device')

class Base implements Owned {
  readonly [ID]: number
  readonly [DEVICE]: GPUDevice
  label: string

  constructor(device: GPUDevice, id: number, label: unknown) {
    this[ID] = id
    this[DEVICE] = device
    this.label = label === undefined ? '' : String(label)
    if (id) dropped.register(this, id)
  }
}

class GPUBuffer extends Base {
  readonly size: number
  readonly usage: number
  #state: 'unmapped' | 'pending' | 'mapped' = 'unmapped'
  #ranges: [number, ArrayBuffer][] = []
  #pending: Promise<void> | null = null

  constructor(device: GPUDevice, id: number, d: Dict) {
    super(device, id, d.label)
    this.size = Number(d.size)
    this.usage = Number(d.usage)
    if (d.mappedAtCreation) this.#state = 'mapped'
  }

  get mapState(): string {
    return this.#state
  }

  mapAsync(mode: number, offset = 0, size?: number): Promise<void> {
    const length = size ?? Math.max(0, this.size - offset)
    if (this.#state !== 'unmapped') {
      return native.mapAsync(this[DEVICE][ID], this[ID], mode, offset, length).then(() => Promise.reject(new DOMException('The buffer is not unmapped', 'OperationError')))
    }
    this.#state = 'pending'
    const pending = native.mapAsync(this[DEVICE][ID], this[ID], mode, offset, length).then((failure) => {
      if (this.#pending !== pending) throw new DOMException('The mapping was aborted', 'AbortError')
      this.#pending = null
      if (failure !== null) {
        this.#state = 'unmapped'
        throw nativeException(failure, 'OperationError')
      }
      this.#state = 'mapped'
    })
    this.#pending = pending
    return pending
  }

  getMappedRange(offset = 0, size?: number): ArrayBuffer {
    if (this.#state !== 'mapped') throw new DOMException('The buffer is not mapped', 'OperationError')
    const length = size ?? Math.max(0, this.size - offset)
    for (const [at, range] of this.#ranges) {
      if (offset < at + range.byteLength && at < offset + length) throw new DOMException('The range overlaps one already returned', 'OperationError')
    }
    const range = guard(() => native.getMappedRange(this[ID], offset, length))
    this.#ranges.push([offset, range])
    return range
  }

  unmap(): void {
    if (this.#state === 'pending') this.#pending = null
    const written: (number | ArrayBuffer)[] = []
    for (const [at, range] of this.#ranges) written.push(at, range)
    native.unmap(this[DEVICE][ID], this[ID], written)
    this.#ranges = []
    this.#state = 'unmapped'
  }

  destroy(): void {
    if (this.#state !== 'unmapped') this.unmap()
    native.destroyBuffer(this[DEVICE][ID], this[ID])
  }
}
tag(GPUBuffer)

class GPUTexture extends Base {
  readonly width: number
  readonly height: number
  readonly depthOrArrayLayers: number
  readonly mipLevelCount: number
  readonly sampleCount: number
  readonly dimension: string
  readonly format: string
  readonly usage: number

  constructor(device: GPUDevice, id: number, d: Dict) {
    super(device, id, d.label)
    const size = extent(d.size)
    this.width = size['width']
    this.height = size['height']
    this.depthOrArrayLayers = size['depthOrArrayLayers']
    this.mipLevelCount = d.mipLevelCount ?? 1
    this.sampleCount = d.sampleCount ?? 1
    this.dimension = d.dimension ?? '2d'
    this.format = d.format
    this.usage = d.usage
  }

  createView(d: Dict = {}): GPUTextureView {
    const json = JSON.stringify({ ...d, label: d.label, usage: d.usage || undefined })
    return new GPUTextureView(this[DEVICE], guard(() => native.createView(this[DEVICE][ID], this[ID], json)), d.label)
  }

  destroy(): void {
    native.destroyTexture(this[DEVICE][ID], this[ID])
  }
}
tag(GPUTexture)

class GPUTextureView extends Base {}
tag(GPUTextureView)
class GPUSampler extends Base {}
tag(GPUSampler)
class GPUBindGroupLayout extends Base {}
tag(GPUBindGroupLayout)
class GPUPipelineLayout extends Base {}
tag(GPUPipelineLayout)
class GPUBindGroup extends Base {}
tag(GPUBindGroup)
class GPUCommandBuffer extends Base {}
tag(GPUCommandBuffer)
class GPURenderBundle extends Base {}
tag(GPURenderBundle)
class GPUExternalTexture extends Base {}
tag(GPUExternalTexture)

class GPUShaderModule extends Base {
  getCompilationInfo(): Promise<GPUCompilationInfo> {
    const messages = native.compilationInfo(this[ID]).map((m) => new GPUCompilationMessage(m))
    return Promise.resolve(new GPUCompilationInfo(messages))
  }
}
tag(GPUShaderModule)

class Pipeline extends Base {
  getBindGroupLayout(index: number): GPUBindGroupLayout {
    return new GPUBindGroupLayout(this[DEVICE], native.getBindGroupLayout(this[DEVICE][ID], this[ID], index), '')
  }
}
class GPUComputePipeline extends Pipeline {}
tag(GPUComputePipeline)
class GPURenderPipeline extends Pipeline {}
tag(GPURenderPipeline)

class GPUQuerySet extends Base {
  readonly type: string
  readonly count: number
  constructor(device: GPUDevice, id: number, d: Dict) {
    super(device, id, d.label)
    this.type = d.type
    this.count = d.count
  }
  destroy(): void {
    native.destroyQuerySet(this[DEVICE][ID], this[ID])
  }
}
tag(GPUQuerySet)

class GPUCommandEncoder extends Base {
  beginRenderPass(d: Dict): GPURenderPassEncoder {
    const json = JSON.stringify({
      label: d.label,
      colorAttachments: Array.from((d.colorAttachments ?? []) as Iterable<Dict | null>, (a) =>
        a ? { view: viewOf(a.view), depthSlice: a.depthSlice, resolveTarget: a.resolveTarget ? viewOf(a.resolveTarget) : undefined, clearValue: a.clearValue === undefined ? undefined : colorOf(a.clearValue), loadOp: a.loadOp, storeOp: a.storeOp } : null,
      ),
      depthStencilAttachment: d.depthStencilAttachment ? { ...d.depthStencilAttachment, view: viewOf(d.depthStencilAttachment.view) } : undefined,
      occlusionQuerySet: d.occlusionQuerySet ? idOf(d.occlusionQuerySet) : undefined,
      timestampWrites: d.timestampWrites ? { ...d.timestampWrites, querySet: idOf(d.timestampWrites.querySet) } : undefined,
    })
    return new GPURenderPassEncoder(native, this[DEVICE][ID], this[ID], json, d.label ?? '')
  }

  beginComputePass(d: Dict = {}): GPUComputePassEncoder {
    const json = JSON.stringify({ label: d.label, timestampWrites: d.timestampWrites ? { ...d.timestampWrites, querySet: idOf(d.timestampWrites.querySet) } : undefined })
    return new GPUComputePassEncoder(native, this[DEVICE][ID], this[ID], json, d.label ?? '')
  }

  copyBufferToBuffer(source: unknown, a: number, b?: unknown, c?: number, d?: number): void {
    if (typeof a !== 'number') {
      const destination = a as unknown as GPUBuffer
      const size = (b as number | undefined) ?? (source as GPUBuffer).size
      return native.copyBufferToBuffer(this[DEVICE][ID], this[ID], idOf(source), 0, idOf(destination), 0, size)
    }
    native.copyBufferToBuffer(this[DEVICE][ID], this[ID], idOf(source), a, idOf(b), c ?? 0, d ?? Math.max(0, (source as GPUBuffer).size - a))
  }

  copyBufferToTexture(source: Dict, destination: Dict, size: unknown): void {
    native.copyBufferToTexture(this[DEVICE][ID], this[ID], JSON.stringify({ source: texelBuffer(source), destination: texelTexture(destination), size: extent(size) }))
  }

  copyTextureToBuffer(source: Dict, destination: Dict, size: unknown): void {
    native.copyTextureToBuffer(this[DEVICE][ID], this[ID], JSON.stringify({ source: texelTexture(source), destination: texelBuffer(destination), size: extent(size) }))
  }

  copyTextureToTexture(source: Dict, destination: Dict, size: unknown): void {
    native.copyTextureToTexture(this[DEVICE][ID], this[ID], JSON.stringify({ source: texelTexture(source), destination: texelTexture(destination), size: extent(size) }))
  }

  clearBuffer(buffer: GPUBuffer, offset = 0, size?: number): void {
    native.clearBuffer(this[DEVICE][ID], this[ID], idOf(buffer), offset, size ?? Math.max(0, buffer.size - offset))
  }

  resolveQuerySet(querySet: GPUQuerySet, firstQuery: number, queryCount: number, destination: GPUBuffer, destinationOffset: number): void {
    native.resolveQuerySet(this[DEVICE][ID], this[ID], idOf(querySet), firstQuery, queryCount, idOf(destination), destinationOffset)
  }

  pushDebugGroup(label: string): void {
    native.encoderDebug(this[DEVICE][ID], this[ID], 0, String(label))
  }

  popDebugGroup(): void {
    native.encoderDebug(this[DEVICE][ID], this[ID], 1, '')
  }

  insertDebugMarker(label: string): void {
    native.encoderDebug(this[DEVICE][ID], this[ID], 2, String(label))
  }

  finish(d: Dict = {}): GPUCommandBuffer {
    return new GPUCommandBuffer(this[DEVICE], native.finishEncoder(this[DEVICE][ID], this[ID], d.label ?? ''), d.label)
  }
}
tag(GPUCommandEncoder)

interface ExternalPixels {
  (source: unknown): { width: number; height: number; rgba: Uint8Array } | null
}

let externalPixels: ExternalPixels = () => null

class GPUQueue extends Base {
  submit(buffers: Iterable<GPUCommandBuffer>): void {
    native.submit(this[DEVICE][ID], Array.from(buffers, idOf))
  }

  onSubmittedWorkDone(): Promise<void> {
    return native.onSubmittedWorkDone(this[DEVICE][ID])
  }

  writeBuffer(buffer: GPUBuffer, bufferOffset: number, data: ArrayBufferView | ArrayBuffer, dataOffset = 0, size?: number): void {
    const view = ArrayBuffer.isView(data) ? data : new Uint8Array(data)
    const element = view instanceof DataView ? 1 : (view as unknown as { BYTES_PER_ELEMENT: number }).BYTES_PER_ELEMENT
    const count = view.byteLength / element
    const length = size ?? count - dataOffset
    if (dataOffset < 0 || length < 0 || dataOffset + length > count) throw new DOMException('writeBuffer: the data range is outside the data', 'OperationError')
    native.writeBuffer(this[DEVICE][ID], idOf(buffer), bufferOffset, new Uint8Array(view.buffer, view.byteOffset + dataOffset * element, length * element))
  }

  writeTexture(destination: Dict, data: ArrayBufferView | ArrayBuffer, layout: Dict, size: unknown): void {
    const json = JSON.stringify({ destination: texelTexture(destination), layout: { offset: layout.offset, bytesPerRow: layout.bytesPerRow, rowsPerImage: layout.rowsPerImage }, size: extent(size) })
    native.writeTexture(this[DEVICE][ID], json, data)
  }

  copyExternalImageToTexture(source: Dict, destination: Dict, copySize: unknown): void {
    const pixels = externalPixels(source.source)
    if (!pixels) throw new DOMException('copyExternalImageToTexture: the source has no pixels', 'OperationError')
    const size = extent(copySize)
    const { x: sx, y: sy } = origin(source.origin) as { x: number; y: number }
    const { width, height } = size as { width: number; height: number }
    if (sx + width > pixels.width || sy + height > pixels.height) throw new DOMException('copyExternalImageToTexture: the copy is outside the source', 'OperationError')
    const out = new Uint8Array(width * height * 4)
    const premultiply = !!destination.premultipliedAlpha
    for (let y = 0; y < height; y++) {
      const from = ((source.flipY ? pixels.height - 1 - (sy + y) : sy + y) * pixels.width + sx) * 4
      const row = pixels.rgba.subarray(from, from + width * 4)
      if (!premultiply) {
        out.set(row, y * width * 4)
        continue
      }
      for (let x = 0; x < width * 4; x += 4) {
        const a = row[x + 3]! / 255, o = y * width * 4 + x
        out[o] = row[x]! * a
        out[o + 1] = row[x + 1]! * a
        out[o + 2] = row[x + 2]! * a
        out[o + 3] = row[x + 3]!
      }
    }
    this.writeTexture(destination, out, { bytesPerRow: width * 4, rowsPerImage: height }, size)
  }
}
tag(GPUQueue)

class GPUDevice extends EventTarget implements Owned {
  readonly [ID]: number
  label = ''
  readonly features: GPUSupportedFeatures
  readonly limits: GPUSupportedLimits
  readonly adapterInfo: GPUAdapterInfo
  readonly queue: GPUQueue
  readonly lost: Promise<GPUDeviceLostInfo>
  #lose!: (info: GPUDeviceLostInfo) => void

  constructor(adapter: GPUAdapter, features: string[], limits: Record<string, number>) {
    super()
    this.lost = new Promise((resolve) => (this.#lose = resolve))
    const self = new WeakRef(this)
    this[ID] = guard(() =>
      native.createDevice(
        features,
        limits,
        (kind, message) => {
          const device = self.deref()
          if (device) device.#uncaptured(kind, message)
        },
        (reason, message) => {
          const device = self.deref()
          if (device) device.#lose(new GPUDeviceLostInfo(reason, message))
        },
      ),
    )
    dropped.register(this, this[ID])
    this.features = new GPUSupportedFeatures(features)
    this.limits = adapter.limits
    this.adapterInfo = adapter.info
    this.queue = new GPUQueue(this, 0, '')
  }

  #uncaptured(kind: string, message: string): void {
    const error = gpuError(kind, message)
    if (fire(this, new GPUUncapturedErrorEvent('uncapturederror', { error, cancelable: true }))) host.print(1, `WebGPU ${kind} error: ${message}`)
  }

  destroy(): void {
    native.destroyDevice(this[ID])
    this.#lose(new GPUDeviceLostInfo('destroyed', 'the device was destroyed'))
  }

  createBuffer(d: Dict): GPUBuffer {
    if (d.mappedAtCreation && d.size % 4 !== 0) throw new RangeError('A buffer mapped at creation needs a size that is a multiple of 4')
    return new GPUBuffer(this, native.createBuffer(this[ID], JSON.stringify({ label: d.label, size: d.size, usage: d.usage, mappedAtCreation: !!d.mappedAtCreation })), d)
  }

  createTexture(d: Dict): GPUTexture {
    const json = JSON.stringify({ label: d.label, size: extent(d.size), mipLevelCount: d.mipLevelCount, sampleCount: d.sampleCount, dimension: d.dimension, format: d.format, usage: d.usage, viewFormats: d.viewFormats ? Array.from(d.viewFormats as Iterable<string>) : [] })
    return new GPUTexture(this, guard(() => native.createTexture(this[ID], json)), d)
  }

  createSampler(d: Dict = {}): GPUSampler {
    return new GPUSampler(this, guard(() => native.createSampler(this[ID], JSON.stringify(d))), d.label)
  }

  importExternalTexture(): never {
    throw new DOMException('External textures are not supported', 'NotSupportedError')
  }

  createBindGroupLayout(d: Dict): GPUBindGroupLayout {
    const json = JSON.stringify({ label: d.label, entries: Array.from(d.entries as Iterable<Dict>) })
    return new GPUBindGroupLayout(this, guard(() => native.createBindGroupLayout(this[ID], json)), d.label)
  }

  createPipelineLayout(d: Dict): GPUPipelineLayout {
    const json = JSON.stringify({ label: d.label, bindGroupLayouts: Array.from(d.bindGroupLayouts as Iterable<unknown>, (l) => (l ? idOf(l) : null)) })
    return new GPUPipelineLayout(this, guard(() => native.createPipelineLayout(this[ID], json)), d.label)
  }

  createBindGroup(d: Dict): GPUBindGroup {
    const entries = Array.from(d.entries as Iterable<Dict>, (e) => {
      const r = e.resource
      let resource: Dict
      if (r instanceof GPUSampler) resource = { sampler: idOf(r) }
      else if (r instanceof GPUTextureView || r instanceof GPUTexture) resource = { view: viewOf(r) }
      else if (r instanceof GPUBuffer) resource = { buffer: idOf(r) }
      else if (r instanceof GPUExternalTexture) throw new TypeError('External textures are not supported')
      else if (r && typeof r === 'object' && r.buffer) resource = { buffer: idOf(r.buffer), offset: r.offset ?? 0, size: r.size }
      else throw new TypeError(`Bind group entry ${e.binding} has no resource`)
      return { binding: e.binding, resource }
    })
    return new GPUBindGroup(this, guard(() => native.createBindGroup(this[ID], JSON.stringify({ label: d.label, layout: idOf(d.layout), entries }))), d.label)
  }

  createShaderModule(d: Dict): GPUShaderModule {
    return new GPUShaderModule(this, native.createShaderModule(this[ID], String(d.code), d.label ?? ''), d.label)
  }

  #compute(d: Dict): string {
    return JSON.stringify({ label: d.label, layout: d.layout === 'auto' ? null : idOf(d.layout), compute: stage(d.compute) })
  }

  #render(d: Dict): string {
    return JSON.stringify({
      label: d.label,
      layout: d.layout === 'auto' ? null : idOf(d.layout),
      vertex: { ...stage(d.vertex), buffers: d.vertex.buffers ? Array.from(d.vertex.buffers as Iterable<Dict | null>, (b) => (b ? { ...b, attributes: Array.from(b.attributes as Iterable<Dict>) } : null)) : [] },
      primitive: d.primitive,
      depthStencil: d.depthStencil,
      multisample: d.multisample,
      fragment: d.fragment ? { ...stage(d.fragment), targets: Array.from(d.fragment.targets as Iterable<Dict | null>) } : undefined,
    })
  }

  createComputePipeline(d: Dict): GPUComputePipeline {
    return new GPUComputePipeline(this, guard(() => native.createComputePipeline(this[ID], this.#compute(d), false)) as number, d.label)
  }

  createRenderPipeline(d: Dict): GPURenderPipeline {
    return new GPURenderPipeline(this, guard(() => native.createRenderPipeline(this[ID], this.#render(d), false)) as number, d.label)
  }

  async createComputePipelineAsync(d: Dict): Promise<GPUComputePipeline> {
    const made = guard(() => native.createComputePipeline(this[ID], this.#compute(d), true))
    if (typeof made === 'string') throw new GPUPipelineError(made, { reason: 'validation' })
    return new GPUComputePipeline(this, made, d.label)
  }

  async createRenderPipelineAsync(d: Dict): Promise<GPURenderPipeline> {
    const made = guard(() => native.createRenderPipeline(this[ID], this.#render(d), true))
    if (typeof made === 'string') throw new GPUPipelineError(made, { reason: 'validation' })
    return new GPURenderPipeline(this, made, d.label)
  }

  createCommandEncoder(d: Dict = {}): GPUCommandEncoder {
    return new GPUCommandEncoder(this, native.createCommandEncoder(this[ID], d.label ?? ''), d.label)
  }

  createRenderBundleEncoder(d: Dict): GPURenderBundleEncoder {
    const json = JSON.stringify({ ...d, colorFormats: Array.from(d.colorFormats as Iterable<string | null>) })
    return new GPURenderBundleEncoder(native, this[ID], json, d.label ?? '', (bundle, label) => new GPURenderBundle(this, bundle, label))
  }

  createQuerySet(d: Dict): GPUQuerySet {
    return new GPUQuerySet(this, guard(() => native.createQuerySet(this[ID], JSON.stringify(d))), d)
  }

  pushErrorScope(filter: string): void {
    guard(() => native.pushErrorScope(this[ID], filter))
  }

  popErrorScope(): Promise<GPUError | null> {
    const popped = native.popErrorScope(this[ID])
    if (popped === undefined) return Promise.reject(new DOMException('There is no error scope to pop', 'OperationError'))
    return Promise.resolve(popped && gpuError(popped.kind, popped.message))
  }
}
defineHandlers(GPUDevice.prototype, ['uncapturederror'])
tag(GPUDevice)

class GPUAdapter {
  readonly features: GPUSupportedFeatures
  readonly limits: GPUSupportedLimits
  readonly info: GPUAdapterInfo
  readonly isFallbackAdapter: boolean
  #used = false

  constructor(info: GpuAdapterInfo) {
    this.features = new GPUSupportedFeatures(info.features)
    this.limits = new GPUSupportedLimits(info.limits)
    this.info = new GPUAdapterInfo(info)
    this.isFallbackAdapter = info.isFallbackAdapter === true
  }

  async requestDevice(d: Dict = {}): Promise<GPUDevice> {
    if (this.#used) throw new DOMException('The adapter already gave a device', 'OperationError')
    this.#used = true
    const features = d.requiredFeatures ? Array.from(d.requiredFeatures as Iterable<string>, String) : []
    const limits: Record<string, number> = {}
    for (const [k, v] of Object.entries((d.requiredLimits ?? {}) as Record<string, unknown>)) if (v !== undefined) limits[k] = Number(v)
    return new GPUDevice(this, features, limits)
  }

  async requestAdapterInfo(): Promise<GPUAdapterInfo> {
    return this.info
  }
}
tag(GPUAdapter)

class GPU {
  readonly wgslLanguageFeatures = new GPUSupportedFeatures([])

  async requestAdapter(_options?: unknown): Promise<GPUAdapter | null> {
    const info = await native.requestAdapter()
    return info ? new GPUAdapter(info) : null
  }

  getPreferredCanvasFormat(): string {
    return native.preferredFormat()
  }
}
tag(GPU)

interface CanvasLike {
  readonly width: number
  readonly height: number
}

let present: (context: GPUCanvasContext) => GPUDevice | null
let flushed: (context: GPUCanvasContext) => number

export class GPUCanvasContext {
  readonly canvas: CanvasLike
  readonly #node: number
  #config: Dict | null = null
  #texture: GPUTexture | null = null

  constructor(canvas: CanvasLike, node: number) {
    this.canvas = canvas
    this.#node = node
  }

  configure(d: Dict): void {
    const device = d.device as GPUDevice
    if (!(device instanceof GPUDevice)) throw new TypeError('configure: device is not a GPUDevice')
    const config: Dict = { ...d, usage: d.usage ?? 0x10, alphaMode: d.alphaMode ?? 'opaque', viewFormats: Array.from((d.viewFormats ?? []) as Iterable<string>), colorSpace: d.colorSpace ?? 'srgb', toneMapping: d.toneMapping ?? { mode: 'standard' } }
    guard(() => native.configure(this.#node, device[ID], JSON.stringify({ format: config.format, usage: config.usage, alphaMode: config.alphaMode, viewFormats: config.viewFormats })))
    this.#config = config
    this.#texture = null
  }

  unconfigure(): void {
    native.unconfigure(this.#node)
    this.#config = null
    this.#texture = null
  }

  getConfiguration(): Dict | null {
    return this.#config && { ...this.#config }
  }

  getCurrentTexture(): GPUTexture {
    const config = this.#config
    if (!config) throw new DOMException('The canvas context is not configured', 'InvalidStateError')
    const width = Math.max(1, this.canvas.width), height = Math.max(1, this.canvas.height)
    if (this.#texture) {
      if (this.#texture.width === width && this.#texture.height === height) return this.#texture
      present(this)
    }
    const id = native.currentTexture(this.#node, width, height)
    this.#texture = new GPUTexture(config.device as GPUDevice, id, { size: [width, height], format: config.format, usage: config.usage, label: '' })
    contexts.waiting.add(this)
    return this.#texture
  }

  static {
    present = (context) => {
      if (!context.#texture) return null
      context.#texture = null
      native.present(context.#node)
      return context.#config?.device as GPUDevice
    }
    flushed = (context) => {
      present(context)
      return context.#node
    }
  }
}
tag(GPUCanvasContext)

const inFlight = new Set<{ at: number }>()
const IN_FLIGHT_MS = 1000

const byCanvas = new WeakMap<object, GPUCanvasContext>()

export const contexts = {
  has(canvas: object): boolean {
    return byCanvas.has(canvas)
  },
  pixels(canvas: object): { width: number; height: number; rgba: Uint8Array } | null | undefined {
    const context = byCanvas.get(canvas)
    return context && native.pixels(flushed(context))
  },
  dataURL(canvas: CanvasLike): string | null {
    const context = byCanvas.get(canvas)
    return context ? `data:image/png;base64,${base64(native.png(flushed(context), canvas.width, canvas.height))}` : null
  },
  png(canvas: CanvasLike): Promise<Uint8Array> | null {
    const context = byCanvas.get(canvas)
    return context ? native.pngAsync(flushed(context), canvas.width, canvas.height) : null
  },
  waiting: new Set<GPUCanvasContext>(),
  behind(): boolean {
    if (inFlight.size < 2) return false
    const now = host.now()
    let count = 0
    for (const { at } of inFlight) if (now - at < IN_FLIGHT_MS) count++
    return count >= 2
  },
  of(canvas: CanvasLike, node: number): GPUCanvasContext | null {
    if (!native) return null
    let context = byCanvas.get(canvas)
    if (!context) byCanvas.set(canvas, (context = new GPUCanvasContext(canvas, node)))
    return context
  },
  present(): void {
    if (contexts.waiting.size === 0) return
    let device: GPUDevice | null = null
    for (const context of contexts.waiting) device = present(context) ?? device
    contexts.waiting.clear()
    if (!device) return
    const entry = { at: host.now() }
    inFlight.add(entry)
    const done = () => inFlight.delete(entry)
    native.onSubmittedWorkDone(device[ID]).then(done, done)
  },
}

const BUFFER_USAGE = { MAP_READ: 0x1, MAP_WRITE: 0x2, COPY_SRC: 0x4, COPY_DST: 0x8, INDEX: 0x10, VERTEX: 0x20, UNIFORM: 0x40, STORAGE: 0x80, INDIRECT: 0x100, QUERY_RESOLVE: 0x200 }
const TEXTURE_USAGE = { COPY_SRC: 0x1, COPY_DST: 0x2, TEXTURE_BINDING: 0x4, STORAGE_BINDING: 0x8, RENDER_ATTACHMENT: 0x10 }

export function installGpu(global: typeof globalThis, gpu: GpuNative, pixels: ExternalPixels): Record<string, unknown> {
  native = gpu
  externalPixels = pixels
  const instance = new GPU()
  const navigator = (global as unknown as { navigator: object }).navigator
  Object.defineProperty(Object.getPrototypeOf(navigator), 'gpu', { get: () => instance, enumerable: true, configurable: true })
  return {
    GPU, GPUAdapter, GPUAdapterInfo, GPUDevice, GPUQueue, GPUBuffer, GPUTexture, GPUTextureView, GPUSampler, GPUBindGroupLayout,
    GPUPipelineLayout, GPUBindGroup, GPUShaderModule, GPUComputePipeline, GPURenderPipeline, GPUCommandEncoder, GPUCommandBuffer,
    GPURenderPassEncoder, GPUComputePassEncoder, GPURenderBundleEncoder, GPURenderBundle, GPUQuerySet, GPUCanvasContext,
    GPUExternalTexture, GPUSupportedFeatures, GPUSupportedLimits, GPUDeviceLostInfo, GPUCompilationInfo, GPUCompilationMessage,
    GPUError, GPUValidationError, GPUOutOfMemoryError, GPUInternalError, GPUPipelineError, GPUUncapturedErrorEvent,
    GPUBufferUsage: Object.freeze({ ...BUFFER_USAGE }),
    GPUTextureUsage: Object.freeze({ ...TEXTURE_USAGE }),
    GPUShaderStage: Object.freeze({ VERTEX: 0x1, FRAGMENT: 0x2, COMPUTE: 0x4 }),
    GPUMapMode: Object.freeze({ READ: 0x1, WRITE: 0x2 }),
    GPUColorWrite: Object.freeze({ RED: 0x1, GREEN: 0x2, BLUE: 0x4, ALPHA: 0x8, ALL: 0xf }),
  }
}
