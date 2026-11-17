import { DOMException } from '../core/exception'
import { tag } from '../core/host'
import type { GpuNative } from '../native'

export const ID = Symbol('gpu id')

export interface Owned {
  readonly [ID]: number
}

class Recorder {
  #data = new Float64Array(256)
  length = 0
  strings: string[] = []

  op(...values: number[]): void {
    const end = this.length + values.length
    if (end > this.#data.length) {
      const bigger = new Float64Array(Math.max(end, this.#data.length * 2))
      bigger.set(this.#data.subarray(0, this.length))
      this.#data = bigger
    }
    this.#data.set(values, this.length)
    this.length = end
  }

  string(text: unknown): number {
    this.strings.push(String(text))
    return this.strings.length - 1
  }

  get data(): Float64Array {
    return this.#data
  }
}

export const idOf = (object: unknown): number => (object && typeof object === 'object' && ID in object ? (object as Owned)[ID] : 0)

function offsets(dynamic?: Iterable<number> | ArrayLike<number>, start = 0, length?: number): number[] {
  if (!dynamic) return []
  const list = Array.from(dynamic as ArrayLike<number>)
  return length === undefined ? list.slice(start) : list.slice(start, start + length)
}

let recording: (encoding: Encoding) => Recorder | null
let ending: (encoding: Encoding) => Recorder | null

abstract class Encoding {
  label: string
  readonly #rec = new Recorder()
  readonly #submit: ((r: Recorder) => void) | undefined
  #ended = false

  constructor(label: string, submit?: (r: Recorder) => void) {
    this.label = label
    this.#submit = submit
  }

  static {
    recording = (encoding) => (encoding.#ended ? null : encoding.#rec)
    ending = (encoding) => {
      if (encoding.#ended) return null
      encoding.#ended = true
      encoding.#submit?.(encoding.#rec)
      return encoding.#rec
    }
  }

  setBindGroup(index: number, group: unknown, dynamicOffsets?: Iterable<number> | ArrayLike<number>, start?: number, length?: number): void {
    const o = offsets(dynamicOffsets, start, length)
    recording(this)?.op(2, index, idOf(group), o.length, ...o)
  }

  pushDebugGroup(label: string): void {
    const r = recording(this)
    r?.op(16, r.string(label))
  }

  popDebugGroup(): void {
    recording(this)?.op(17)
  }

  insertDebugMarker(label: string): void {
    const r = recording(this)
    r?.op(18, r.string(label))
  }
}

abstract class RenderEncoding extends Encoding {
  setPipeline(pipeline: unknown): void {
    recording(this)?.op(1, idOf(pipeline))
  }

  setVertexBuffer(slot: number, buffer: unknown, offset = 0, size?: number): void {
    recording(this)?.op(3, slot, idOf(buffer), offset, size ?? -1)
  }

  setIndexBuffer(buffer: unknown, format: string, offset = 0, size?: number): void {
    if (format !== 'uint16' && format !== 'uint32') throw new TypeError(`'${format}' is not a GPUIndexFormat`)
    recording(this)?.op(4, idOf(buffer), format === 'uint32' ? 1 : 0, offset, size ?? -1)
  }

  draw(vertexCount: number, instanceCount = 1, firstVertex = 0, firstInstance = 0): void {
    recording(this)?.op(5, vertexCount, instanceCount, firstVertex, firstInstance)
  }

  drawIndexed(indexCount: number, instanceCount = 1, firstIndex = 0, baseVertex = 0, firstInstance = 0): void {
    recording(this)?.op(6, indexCount, instanceCount, firstIndex, baseVertex, firstInstance)
  }

  drawIndirect(buffer: unknown, offset: number): void {
    recording(this)?.op(7, idOf(buffer), offset)
  }

  drawIndexedIndirect(buffer: unknown, offset: number): void {
    recording(this)?.op(8, idOf(buffer), offset)
  }
}

export class GPURenderPassEncoder extends RenderEncoding {
  constructor(gpu: GpuNative, device: number, encoder: number, descriptor: string, label: string) {
    super(label, (r) => gpu.renderPass(device, encoder, descriptor, r.data, r.length, r.strings))
  }

  setViewport(x: number, y: number, width: number, height: number, minDepth: number, maxDepth: number): void {
    recording(this)?.op(9, x, y, width, height, minDepth, maxDepth)
  }

  setScissorRect(x: number, y: number, width: number, height: number): void {
    recording(this)?.op(10, x, y, width, height)
  }

  setBlendConstant(color: unknown): void {
    const [r, g, b, a] = colorOf(color)
    recording(this)?.op(11, r, g, b, a)
  }

  setStencilReference(reference: number): void {
    recording(this)?.op(12, reference)
  }

  beginOcclusionQuery(index: number): void {
    recording(this)?.op(13, index)
  }

  endOcclusionQuery(): void {
    recording(this)?.op(14)
  }

  executeBundles(bundles: Iterable<unknown>): void {
    const ids = Array.from(bundles, idOf)
    recording(this)?.op(15, ids.length, ...ids)
  }

  end(): void {
    ending(this)
  }
}
tag(GPURenderPassEncoder)

export class GPUComputePassEncoder extends Encoding {
  constructor(gpu: GpuNative, device: number, encoder: number, descriptor: string, label: string) {
    super(label, (r) => gpu.computePass(device, encoder, descriptor, r.data, r.length, r.strings))
  }

  setPipeline(pipeline: unknown): void {
    recording(this)?.op(1, idOf(pipeline))
  }

  dispatchWorkgroups(x: number, y = 1, z = 1): void {
    recording(this)?.op(19, x, y, z)
  }

  dispatchWorkgroupsIndirect(buffer: unknown, offset: number): void {
    recording(this)?.op(20, idOf(buffer), offset)
  }

  end(): void {
    ending(this)
  }
}
tag(GPUComputePassEncoder)

export class GPURenderBundleEncoder extends RenderEncoding {
  readonly #finish: (r: Recorder, label: string) => unknown

  constructor(gpu: GpuNative, device: number, descriptor: string, label: string, make: (bundle: number, label: string) => unknown) {
    super(label)
    this.#finish = (r, label) => make(gpu.createRenderBundle(device, descriptor, r.data, r.length, r.strings), label)
  }

  finish(descriptor: { label?: string } = {}): unknown {
    const r = ending(this)
    if (!r) throw new DOMException('The render bundle encoder is finished', 'InvalidStateError')
    return this.#finish(r, descriptor.label ?? '')
  }
}
tag(GPURenderBundleEncoder)

export function colorOf(color: unknown): [number, number, number, number] {
  if (color && typeof color === 'object' && Symbol.iterator in color) {
    const list = Array.from(color as Iterable<number>, Number)
    if (list.length !== 4) throw new TypeError('A GPUColor sequence has 4 numbers')
    return list as [number, number, number, number]
  }
  const c = (color ?? {}) as { r?: number; g?: number; b?: number; a?: number }
  for (const k of ['r', 'g', 'b', 'a'] as const) if (c[k] === undefined) throw new TypeError(`GPUColor is missing '${k}'`)
  return [Number(c.r), Number(c.g), Number(c.b), Number(c.a)]
}
