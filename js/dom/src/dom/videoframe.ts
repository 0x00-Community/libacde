// WebCodecs VideoFrame and VideoColorSpace: frames made from images, canvases, bitmaps, videos and raw pixel buffers, for
// drawImage, WebGL texture uploads, WebGPU copyExternalImageToTexture and createImageBitmap. No codecs (VideoDecoder /
// VideoEncoder): a frame holds either a canvas (image sources) or the raw planes it was built from (buffer sources), and a
// buffer frame turns into a canvas only when something draws it.
import { DOMException } from '../core/exception'
import { tag } from '../core/host'
import { bitmaps, ImageBitmap } from './bitmap'
import { canvasSources, ImageData } from './canvas'
import { DOMRectReadOnly } from './geometry'
import { createHtml } from './html'

const TOKEN = Symbol('token')

interface Canvas {
  width: number
  height: number
  readonly localName: string
  getContext(kind: '2d'): Context2d
}

interface Context2d {
  drawImage(image: unknown, ...a: number[]): void
  putImageData(image: ImageData, x: number, y: number): void
  getImageData(x: number, y: number, w: number, h: number): ImageData
}

type Format = 'I420' | 'I420A' | 'I422' | 'I444' | 'NV12' | 'RGBA' | 'RGBX' | 'BGRA' | 'BGRX'
const FORMATS: readonly Format[] = ['I420', 'I420A', 'I422', 'I444', 'NV12', 'RGBA', 'RGBX', 'BGRA', 'BGRX']
const RGB_FORMATS: readonly string[] = ['RGBA', 'RGBX', 'BGRA', 'BGRX']

interface Rect {
  x: number
  y: number
  width: number
  height: number
}

interface ColorSpaceInit {
  primaries?: string | null
  transfer?: string | null
  matrix?: string | null
  fullRange?: boolean | null
}

export class VideoColorSpace {
  readonly primaries: string | null
  readonly transfer: string | null
  readonly matrix: string | null
  readonly fullRange: boolean | null

  constructor(init: ColorSpaceInit = {}) {
    this.primaries = init.primaries ?? null
    this.transfer = init.transfer ?? null
    this.matrix = init.matrix ?? null
    this.fullRange = init.fullRange ?? null
  }

  toJSON(): ColorSpaceInit {
    return { primaries: this.primaries, transfer: this.transfer, matrix: this.matrix, fullRange: this.fullRange }
  }
}
tag(VideoColorSpace)

const SRGB: ColorSpaceInit = { primaries: 'bt709', transfer: 'iec61966-2-1', matrix: 'rgb', fullRange: true }
// what Chrome reports for YUV buffers given no color space
const REC709: ColorSpaceInit = { primaries: 'bt709', transfer: 'bt709', matrix: 'bt709', fullRange: false }

interface Plane {
  offset: number
  stride: number
  width: number
  height: number
  bytes: number
}

// the planes of a `format` frame of `w`x`h` laid out tightly (no padding), in the order WebCodecs lists them
function planesOf(format: Format, w: number, h: number): Plane[] {
  const cw = Math.ceil(w / 2)
  const ch = Math.ceil(h / 2)
  const sizes: Array<[number, number, number]> = (() => {
    switch (format) {
      case 'I420':
        return [[w, h, 1], [cw, ch, 1], [cw, ch, 1]]
      case 'I420A':
        return [[w, h, 1], [cw, ch, 1], [cw, ch, 1], [w, h, 1]]
      case 'I422':
        return [[w, h, 1], [cw, h, 1], [cw, h, 1]]
      case 'I444':
        return [[w, h, 1], [w, h, 1], [w, h, 1]]
      case 'NV12':
        return [[w, h, 1], [cw, ch, 2]]
      default:
        return [[w, h, 4]]
    }
  })()
  let offset = 0
  return sizes.map(([pw, ph, bpp]) => {
    const plane = { offset, stride: pw * bpp, width: pw, height: ph, bytes: pw * ph * bpp }
    offset += plane.bytes
    return plane
  })
}

function bytesOf(data: unknown): Uint8Array {
  if (data instanceof ArrayBuffer) return new Uint8Array(data)
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength)
  throw new TypeError("Failed to construct 'VideoFrame': The provided value is not of type '(AllowSharedBufferSource or CanvasImageSource)'.")
}

// YUV (8-bit) to RGB with the matrix and range of `space` (BT.709 limited range unless it says otherwise)
function yuvToRgb(space: ColorSpaceInit): (y: number, u: number, v: number, out: Uint8ClampedArray, at: number) => void {
  const [kr, kb] = space.matrix === 'bt470bg' || space.matrix === 'smpte170m' ? [0.299, 0.114] : space.matrix === 'bt2020-ncl' ? [0.2627, 0.0593] : [0.2126, 0.0722]
  const kg = 1 - kr - kb
  const full = space.fullRange === true
  const ys = full ? 1 : 255 / 219
  const cs = full ? 1 : 255 / 224
  return (y, u, v, out, at) => {
    const Y = (y - (full ? 0 : 16)) * ys
    const U = (u - 128) * cs
    const V = (v - 128) * cs
    const r = Y + 2 * (1 - kr) * V
    const b = Y + 2 * (1 - kb) * U
    const g = (Y - kr * r - kb * b) / kg
    out[at] = r
    out[at + 1] = g
    out[at + 2] = b
  }
}

// the RGBA pixels of a buffer frame's visible rect
function toRgba(format: Format, data: Uint8Array, coded: [number, number], planes: Plane[], rect: Rect, space: ColorSpaceInit): Uint8ClampedArray {
  const { x: rx, y: ry, width: w, height: h } = rect
  const out = new Uint8ClampedArray(w * h * 4)
  if (RGB_FORMATS.includes(format)) {
    const p = planes[0]!
    const bgr = format.startsWith('B')
    const opaque = format.endsWith('X')
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        const i = p.offset + (ry + y) * p.stride + (rx + x) * 4
        const o = (y * w + x) * 4
        out[o] = data[i + (bgr ? 2 : 0)]!
        out[o + 1] = data[i + 1]!
        out[o + 2] = data[i + (bgr ? 0 : 2)]!
        out[o + 3] = opaque ? 255 : data[i + 3]!
      }
    }
    return out
  }
  const convert = yuvToRgb(space)
  const [yp, up, vp, ap] = planes
  const sx = format === 'I444' ? 1 : 2
  const sy = format === 'I420' || format === 'I420A' || format === 'NV12' ? 2 : 1
  void coded
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const px = rx + x
      const py = ry + y
      const Y = data[yp!.offset + py * yp!.stride + px]!
      const cx = Math.floor(px / sx)
      const cy = Math.floor(py / sy)
      let U: number
      let V: number
      if (format === 'NV12') {
        const i = up!.offset + cy * up!.stride + cx * 2
        U = data[i]!
        V = data[i + 1]!
      } else {
        U = data[up!.offset + cy * up!.stride + cx]!
        V = data[vp!.offset + cy * vp!.stride + cx]!
      }
      const o = (y * w + x) * 4
      convert(Y, U, V, out, o)
      out[o + 3] = format === 'I420A' ? data[ap!.offset + py * ap!.stride + px]! : 255
    }
  }
  return out
}

interface Init {
  timestamp?: number
  duration?: number
  alpha?: string
  visibleRect?: Partial<Rect>
  displayWidth?: number
  displayHeight?: number
  format?: string
  codedWidth?: number
  codedHeight?: number
  colorSpace?: ColorSpaceInit
  layout?: Array<{ offset: number; stride: number }>
}

interface State {
  canvas: Canvas | null
  drawable: Canvas | null
  data: Uint8Array | null
  format: Format
  planes: Plane[]
  coded: [number, number]
  visible: Rect
  display: [number, number]
  timestamp: number
  duration: number | null
  space: ColorSpaceInit
}

function integer(what: string, value: unknown): number {
  const n = Number(value)
  if (!Number.isFinite(n)) throw new TypeError(`Failed to construct 'VideoFrame': ${what} is not a finite number.`)
  return Math.trunc(n)
}

function sourceSize(source: unknown): [number, number, number | undefined] {
  if (source instanceof ImageBitmap) {
    const canvas = bitmaps.canvas(source)
    if (!canvas) throw new DOMException("Failed to construct 'VideoFrame': The image bitmap is closed.", 'InvalidStateError')
    return [canvas.width, canvas.height, undefined]
  }
  const element = source as { localName?: string; width?: number; height?: number; naturalWidth?: number; naturalHeight?: number; videoWidth?: number; videoHeight?: number; complete?: boolean; currentTime?: number }
  switch (element?.localName) {
    case 'canvas':
      return [element.width ?? 0, element.height ?? 0, undefined]
    case 'img':
      if (!element.complete) throw new DOMException("Failed to construct 'VideoFrame': The image is not decoded.", 'InvalidStateError')
      return [element.naturalWidth ?? 0, element.naturalHeight ?? 0, undefined]
    case 'video':
      return [element.videoWidth ?? 0, element.videoHeight ?? 0, Math.round((element.currentTime ?? 0) * 1e6)]
  }
  throw new TypeError("Failed to construct 'VideoFrame': The provided value is not of type '(AllowSharedBufferSource or CanvasImageSource)'.")
}

/** For the consumers of image sources (drawImage, WebGL, WebGPU, createImageBitmap): the frame's pixels as a canvas. */
export const videoFrames = {} as {
  canvas(frame: VideoFrame): Canvas | null
}

export class VideoFrame {
  #s: State | null
  #times: [number, number | null] = [0, null]

  constructor(source: unknown, init: Init = {}) {
    if (source instanceof VideoFrame) {
      const from = source.#s
      if (!from) throw new DOMException("Failed to construct 'VideoFrame': The VideoFrame has been closed.", 'InvalidStateError')
      this.#s = { ...from, timestamp: init.timestamp ?? from.timestamp, duration: init.duration ?? from.duration }
      return
    }
    if (source instanceof ArrayBuffer || ArrayBuffer.isView(source)) {
      this.#s = VideoFrame.#fromBuffer(bytesOf(source), init)
      return
    }
    const [width, height, time] = sourceSize(source)
    if (!width || !height) throw new DOMException("Failed to construct 'VideoFrame': The source has no pixels.", 'InvalidStateError')
    const timestamp = init.timestamp ?? time
    if (timestamp === undefined) throw new TypeError("Failed to construct 'VideoFrame': timestamp is required.")
    const canvas = createHtml('canvas') as unknown as Canvas
    canvas.width = width
    canvas.height = height
    canvas.getContext('2d').drawImage(canvasSources.resolve(source), 0, 0)
    const visible = VideoFrame.#rect(init.visibleRect, width, height)
    this.#s = {
      canvas,
      drawable: null,
      data: null,
      format: 'RGBA',
      planes: planesOf('RGBA', width, height),
      coded: [width, height],
      visible,
      display: [init.displayWidth ?? visible.width, init.displayHeight ?? visible.height],
      timestamp: integer('timestamp', timestamp),
      duration: init.duration === undefined ? null : integer('duration', init.duration),
      space: SRGB,
    }
  }

  static #rect(rect: Partial<Rect> | undefined, width: number, height: number): Rect {
    if (!rect) return { x: 0, y: 0, width, height }
    const r = { x: integer('visibleRect.x', rect.x ?? 0), y: integer('visibleRect.y', rect.y ?? 0), width: integer('visibleRect.width', rect.width ?? 0), height: integer('visibleRect.height', rect.height ?? 0) }
    if (r.x < 0 || r.y < 0 || r.width <= 0 || r.height <= 0 || r.x + r.width > width || r.y + r.height > height) {
      throw new TypeError("Failed to construct 'VideoFrame': visibleRect is outside the coded size.")
    }
    return r
  }

  static #fromBuffer(bytes: Uint8Array, init: Init): State {
    const format = init.format as Format
    if (!FORMATS.includes(format)) throw new TypeError(`Failed to construct 'VideoFrame': '${init.format}' is not a supported VideoPixelFormat.`)
    if (init.timestamp === undefined) throw new TypeError("Failed to construct 'VideoFrame': timestamp is required.")
    const w = integer('codedWidth', init.codedWidth)
    const h = integer('codedHeight', init.codedHeight)
    if (w <= 0 || h <= 0) throw new TypeError("Failed to construct 'VideoFrame': codedWidth and codedHeight must be positive.")
    const tight = planesOf(format, w, h)
    const planes = init.layout ? tight.map((p, i) => ({ ...p, offset: init.layout![i]?.offset ?? p.offset, stride: init.layout![i]?.stride ?? p.stride })) : tight
    if (init.layout && init.layout.length !== tight.length) throw new TypeError(`Failed to construct 'VideoFrame': ${format} has ${tight.length} planes, the layout ${init.layout.length}.`)
    for (const p of planes) {
      if (p.stride < tight[planes.indexOf(p)]!.stride || p.offset + p.stride * (p.height - 1) + tight[planes.indexOf(p)]!.stride > bytes.byteLength) {
        throw new TypeError("Failed to construct 'VideoFrame': The data is too small for the format and size.")
      }
    }
    const visible = VideoFrame.#rect(init.visibleRect, w, h)
    const yuv = !RGB_FORMATS.includes(format)
    return {
      canvas: null,
      drawable: null,
      data: bytes.slice(),
      format,
      planes,
      coded: [w, h],
      visible,
      display: [init.displayWidth ?? visible.width, init.displayHeight ?? visible.height],
      timestamp: integer('timestamp', init.timestamp),
      duration: init.duration === undefined ? null : integer('duration', init.duration),
      // a given color space is taken as it is, members it leaves out stay null (as in Chrome)
      space: init.colorSpace ? { ...init.colorSpace } : yuv ? REC709 : SRGB,
    }
  }

  get format(): string | null {
    return this.#s?.format ?? null
  }
  get codedWidth(): number {
    return this.#s?.coded[0] ?? 0
  }
  get codedHeight(): number {
    return this.#s?.coded[1] ?? 0
  }
  get codedRect(): DOMRectReadOnly | null {
    return this.#s ? new DOMRectReadOnly(0, 0, this.#s.coded[0], this.#s.coded[1]) : null
  }
  get visibleRect(): DOMRectReadOnly | null {
    const s = this.#s
    return s ? new DOMRectReadOnly(s.visible.x, s.visible.y, s.visible.width, s.visible.height) : null
  }
  get displayWidth(): number {
    return this.#s?.display[0] ?? 0
  }
  get displayHeight(): number {
    return this.#s?.display[1] ?? 0
  }
  get timestamp(): number {
    return this.#s ? this.#s.timestamp : this.#times[0]
  }
  get duration(): number | null {
    return this.#s ? this.#s.duration : this.#times[1]
  }
  get colorSpace(): VideoColorSpace {
    return new VideoColorSpace(this.#s?.space ?? {})
  }
  get rotation(): number {
    return 0
  }
  get flip(): boolean {
    return false
  }

  #open(what: string): State {
    if (!this.#s) throw new DOMException(`Failed to execute '${what}' on 'VideoFrame': The VideoFrame has been closed.`, 'InvalidStateError')
    return this.#s
  }

  // RGBA of the visible rect: from the canvas (image frames) or converted from the planes (buffer frames)
  #rgba(s: State): Uint8ClampedArray {
    if (s.canvas) return s.canvas.getContext('2d').getImageData(s.visible.x, s.visible.y, s.visible.width, s.visible.height).data
    return toRgba(s.format, s.data!, s.coded, s.planes, s.visible, s.space)
  }

  allocationSize(options: { rect?: Partial<Rect>; format?: string } = {}): number {
    const s = this.#open('allocationSize')
    return this.#layoutFor(s, options).size
  }

  #layoutFor(s: State, options: { rect?: Partial<Rect>; layout?: Array<{ offset: number; stride: number }>; format?: string }): { format: Format; rect: Rect; planes: Plane[]; size: number } {
    const rect = options.rect ? VideoFrame.#rect(options.rect, s.coded[0], s.coded[1]) : s.visible
    const format = (options.format ?? s.format) as Format
    if (options.format !== undefined && !RGB_FORMATS.includes(options.format)) throw new TypeError(`copyTo: converting to '${options.format}' is not supported.`)
    let planes = planesOf(format, rect.width, rect.height)
    if (options.layout) {
      if (options.layout.length !== planes.length) throw new TypeError(`copyTo: ${format} has ${planes.length} planes, the layout ${options.layout.length}.`)
      planes = planes.map((p, i) => ({ ...p, offset: options.layout![i]!.offset, stride: options.layout![i]!.stride }))
    }
    const size = Math.max(...planes.map((p) => p.offset + p.stride * (p.height - 1) + (p.bytes / p.height)))
    return { format, rect, planes, size }
  }

  async copyTo(destination: ArrayBuffer | ArrayBufferView, options: { rect?: Partial<Rect>; layout?: Array<{ offset: number; stride: number }>; format?: string } = {}): Promise<Array<{ offset: number; stride: number }>> {
    const s = this.#open('copyTo')
    const out = bytesOf(destination)
    const { format, rect, planes, size } = this.#layoutFor(s, options)
    if (out.byteLength < size) throw new TypeError('copyTo: the destination is too small.')
    const sameFormat = format === s.format && !s.canvas
    if (sameFormat) {
      // copy the source planes row by row into the destination layout
      const sx = format === 'I444' || RGB_FORMATS.includes(format) ? 1 : 2
      const sy = format === 'I420' || format === 'I420A' || format === 'NV12' ? 2 : 1
      planes.forEach((p, i) => {
        const src = s.planes[i]!
        const bpp = RGB_FORMATS.includes(format) ? 4 : format === 'NV12' && i === 1 ? 2 : 1
        const scaled = i === 0 || (format === 'I420A' && i === 3) || RGB_FORMATS.includes(format)
        const x0 = scaled ? rect.x : Math.floor(rect.x / sx)
        const y0 = scaled ? rect.y : Math.floor(rect.y / sy)
        for (let row = 0; row < p.height; row++) {
          const from = src.offset + (y0 + row) * src.stride + x0 * bpp
          out.set(s.data!.subarray(from, from + p.width * bpp), p.offset + row * p.stride)
        }
      })
    } else {
      const rgba = this.#rgbaRect(s, rect)
      const bgr = format.startsWith('B')
      const opaque = format.endsWith('X')
      const p = planes[0]!
      for (let y = 0; y < rect.height; y++) {
        for (let x = 0; x < rect.width; x++) {
          const i = (y * rect.width + x) * 4
          const o = p.offset + y * p.stride + x * 4
          out[o] = rgba[i + (bgr ? 2 : 0)]!
          out[o + 1] = rgba[i + 1]!
          out[o + 2] = rgba[i + (bgr ? 0 : 2)]!
          out[o + 3] = opaque ? 255 : rgba[i + 3]!
        }
      }
    }
    return planes.map((p) => ({ offset: p.offset, stride: p.stride }))
  }

  #rgbaRect(s: State, rect: Rect): Uint8ClampedArray {
    if (s.canvas) return s.canvas.getContext('2d').getImageData(rect.x, rect.y, rect.width, rect.height).data
    return toRgba(s.format, s.data!, s.coded, s.planes, rect, s.space)
  }

  clone(): VideoFrame {
    this.#open('clone')
    return new VideoFrame(this)
  }

  // a closed frame keeps its timestamp and duration (as Chrome does); everything about its pixels reads as empty
  close(): void {
    if (this.#s) this.#times = [this.#s.timestamp, this.#s.duration]
    this.#s = null
  }

  static {
    // the visible rect as a canvas: the frame's own canvas when it shows all of it, else a copy made once and kept
    videoFrames.canvas = (frame) => {
      const s = frame.#s
      if (!s) return null
      const whole = s.visible.x === 0 && s.visible.y === 0 && s.visible.width === s.coded[0] && s.visible.height === s.coded[1]
      if (s.canvas && whole) return s.canvas
      if (!s.drawable) {
        const canvas = createHtml('canvas') as unknown as Canvas
        canvas.width = s.visible.width
        canvas.height = s.visible.height
        canvas.getContext('2d').putImageData(new ImageData(frame.#rgba(s), s.visible.width, s.visible.height), 0, 0)
        s.drawable = canvas
      }
      return s.drawable
    }
  }
}
tag(VideoFrame)

const resolveBefore = canvasSources.resolve
canvasSources.resolve = (image) => {
  if (!(image instanceof VideoFrame)) return resolveBefore(image)
  const canvas = videoFrames.canvas(image)
  if (!canvas) throw new DOMException('The VideoFrame has been closed.', 'InvalidStateError')
  return canvas
}
