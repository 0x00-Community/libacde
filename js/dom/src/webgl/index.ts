import { Event, fire, type EventTarget } from '../core/event'
import { DOMException } from '../core/exception'
import { constants, host, tag } from '../core/host'
import { bitmaps, ImageBitmap } from '../dom/bitmap'
import { VideoFrame, videoFrames } from '../dom/videoframe'
import { canvases, canvasSources, ImageData } from '../dom/canvas'
import { ID } from '../dom/realm'
import { WEBGL1, WEBGL2 } from './constants'
import { gpuFrames } from '../gpu/frames'

export interface WebglNative {
  create(node: number, webgl2: boolean, attributes: object, width: number, height: number): number | null
  release(context: number): void
  present(context: number): boolean
  done(): Promise<void>
  pixels(context: number): { width: number; height: number; rgba: Uint8Array } | null
  onLost(callback: (context: number) => void): void
  watch?(callback: (kind: 'lost' | 'restored') => void): void
  texImageSource(...args: unknown[]): void
  error(context: number, code: number): void
  uniform(...args: unknown[]): void
  draw(...args: unknown[]): void
  clear(context: number, mask: number): void
  readPixels(...args: unknown[]): void
  png(context: number): { rgba: Uint8Array } | null
  resize(context: number, width: number, height: number): void
  getError(context: number): number
  [name: string]: ((...args: any[]) => any) | undefined
}

let native: WebglNative = null as unknown as WebglNative

const TOKEN = Symbol('token')
const OBJECT = Symbol('object')
const OWNER = Symbol('owner')
// the context generation an object belongs to: objects made before a context was restored are invalid after it
const GENERATION = Symbol('generation')
const CONTEXT = Symbol('context')

type Base = WebGLContextBase

export class WebGLObject {
  readonly [OBJECT]: number
  readonly [OWNER]: Base
  [GENERATION] = 0
  deleted = false
  constructor(token: symbol, owner: Base, id: number) {
    if (token !== TOKEN) throw new TypeError('Illegal constructor')
    this[OWNER] = owner
    this[OBJECT] = id
  }
}
tag(WebGLObject)

export class WebGLBuffer extends WebGLObject {}
export class WebGLTexture extends WebGLObject {}
export class WebGLFramebuffer extends WebGLObject {}
export class WebGLRenderbuffer extends WebGLObject {}
export class WebGLShader extends WebGLObject {
  compiled = false
  attached = 0
}
export class WebGLProgram extends WebGLObject {
  linked = false
  validated = false
  serial = 0
  varyings: string[] = []
  bufferMode = WEBGL2.INTERLEAVED_ATTRIBS
}
export class WebGLVertexArrayObject extends WebGLObject {}
export class WebGLVertexArrayObjectOES extends WebGLObject {}
export class WebGLQuery extends WebGLObject {
  target = 0
  ended = -1
}
export class WebGLSampler extends WebGLObject {}
export class WebGLSync extends WebGLObject {
  created = -1
}

let epoch = 0
export class WebGLTransformFeedback extends WebGLObject {}
for (const cls of [WebGLBuffer, WebGLTexture, WebGLFramebuffer, WebGLRenderbuffer, WebGLShader, WebGLProgram, WebGLVertexArrayObject, WebGLVertexArrayObjectOES, WebGLQuery, WebGLSampler, WebGLSync, WebGLTransformFeedback]) tag(cls)

export class WebGLUniformLocation {
  readonly program: WebGLProgram
  readonly index: number
  readonly serial: number
  constructor(token: symbol, program: WebGLProgram, index: number, serial: number) {
    if (token !== TOKEN) throw new TypeError('Illegal constructor')
    this.program = program
    this.index = index
    this.serial = serial
  }
}
tag(WebGLUniformLocation)

export class WebGLActiveInfo {
  readonly size: number
  readonly type: number
  readonly name: string
  constructor(token: symbol, size: number, type: number, name: string) {
    if (token !== TOKEN) throw new TypeError('Illegal constructor')
    this.size = size
    this.type = type
    this.name = name
  }
}
tag(WebGLActiveInfo)

export class WebGLShaderPrecisionFormat {
  readonly rangeMin: number
  readonly rangeMax: number
  readonly precision: number
  constructor(token: symbol, rangeMin: number, rangeMax: number, precision: number) {
    if (token !== TOKEN) throw new TypeError('Illegal constructor')
    this.rangeMin = rangeMin
    this.rangeMax = rangeMax
    this.precision = precision
  }
}
tag(WebGLShaderPrecisionFormat)

export class WebGLContextEvent extends Event {
  readonly statusMessage: string
  constructor(type: string, init: { statusMessage?: string; bubbles?: boolean; cancelable?: boolean; composed?: boolean } = {}) {
    super(type, init)
    this.statusMessage = String(init.statusMessage ?? '')
  }
}
tag(WebGLContextEvent)

interface CanvasLike {
  readonly width: number
  readonly height: number
}

type View = ArrayBufferView | ArrayBuffer
type Uniform = Float32Array | Int32Array | Uint32Array | ArrayLike<number>

const C = { ...WEBGL1, ...WEBGL2 }

function bytesOf(data: View | null | undefined, offset = 0, length = 0): Uint8Array | null {
  if (data == null) return null
  if (data instanceof ArrayBuffer || (typeof SharedArrayBuffer !== 'undefined' && data instanceof SharedArrayBuffer)) return new Uint8Array(data as ArrayBuffer)
  if (!ArrayBuffer.isView(data)) throw new TypeError('expected an ArrayBufferView')
  const size = (data as unknown as { BYTES_PER_ELEMENT?: number }).BYTES_PER_ELEMENT ?? 1
  const count = data.byteLength / size
  if (offset > count) return null
  const n = length ? length : count - offset
  if (offset + n > count) return null
  return new Uint8Array(data.buffer, data.byteOffset + offset * size, n * size)
}

function typed<T extends Float32Array | Int32Array | Uint32Array>(Kind: { new (v: ArrayLike<number> | ArrayBufferLike, o?: number, n?: number): T; BYTES_PER_ELEMENT: number }, data: Uniform, offset = 0, length = 0): T {
  const array = data instanceof Kind ? data : new Kind(Array.from(data as ArrayLike<number>))
  const n = length || array.length - offset
  return new Kind(array.buffer, array.byteOffset + offset * Kind.BYTES_PER_ELEMENT, Math.max(0, n))
}

function sourceOf(source: unknown): [number, number, number, Uint8Array | null] | null {
  canvases.flush()
  if (source instanceof ImageBitmap) {
    const canvas = bitmaps.canvas(source)
    if (!canvas) throw new DOMException('The image bitmap is closed', 'InvalidStateError')
    return [bitmaps.premultiplied(source) ? 3 : 2, (canvas as unknown as Record<symbol, number>)[ID] ?? 0, 0, null]
  }
  if (source instanceof ImageData) return [1, source.width, source.height, new Uint8Array(source.data.buffer, source.data.byteOffset, source.data.byteLength)]
  if (source instanceof VideoFrame) {
    const canvas = videoFrames.canvas(source)
    if (!canvas) throw new DOMException('The VideoFrame has been closed', 'InvalidStateError')
    return [2, (canvas as unknown as Record<symbol, number>)[ID] ?? 0, 0, null]
  }
  if (source && typeof source === 'object' && ID in source) return [0, (source as Record<symbol, number>)[ID] ?? 0, 0, null]
  return null
}

const webglHooks = {} as {
  objects(gl: WebGLContextBase, id: number): WebGLObject | null
  wrap<T extends WebGLObject>(gl: WebGLContextBase, Kind: new (token: symbol, owner: Base, id: number) => T, id: number | null): T | null
  id<T extends WebGLObject>(gl: WebGLContextBase, object: T | null | undefined, Kind: abstract new (...args: never[]) => T): number | undefined
  shaderTypes(gl: WebGLContextBase): Map<number, number>
  draw(gl: WebGLContextBase, mode: number, first: number, count: number, instances: number, type: number, offset: number): void
  webgl2(gl: WebGLContextBase): boolean
}

export const contextHooks = {} as {
  present(gl: WebGLContextBase): boolean
  drawn(gl: WebGLContextBase): boolean
  pixels(gl: WebGLContextBase): { width: number; height: number; rgba: Uint8Array } | null
  png(gl: WebGLContextBase): Uint8Array | null
  resize(gl: WebGLContextBase): void
  lose(gl: WebGLContextBase): void
  restore(gl: WebGLContextBase): void
  id(gl: WebGLContextBase): number
  request?: () => void
}

const byCanvas = new WeakMap<object, WebGLContextBase>()
const live = new Set<WebGLContextBase>()
// the canvas node a context was made for, to make it again when it is restored
const contextNodes = new WeakMap<WebGLContextBase, number>()

export class WebGLContextBase {
  readonly canvas: CanvasLike
  #c: number
  #webgl2: boolean
  #lost = false
  // the page cancelled webglcontextlost, so the context comes back once a GPU device is there again (WebGL 1.0 5.15.3)
  #restorable = false
  #generation = 0
  #drawn = false
  #attributes: Record<string, unknown>
  #objects = new Map<number, WebGLObject>()
  #extensions = new Map<string, object>()
  #program: WebGLProgram | null = null

  constructor(token: symbol, canvas: CanvasLike, context: number, webgl2: boolean, attributes: Record<string, unknown>) {
    if (token !== TOKEN) throw new TypeError('Illegal constructor')
    this.canvas = canvas
    this.#c = context
    this.#webgl2 = webgl2
    this.#attributes = attributes
  }

  static {
    contextHooks.present = (gl) => {
      if (gl.#lost || !gl.#drawn) return false
      gl.#drawn = false
      return native.present(gl.#c)
    }
    contextHooks.drawn = (gl) => gl.#drawn
    contextHooks.pixels = (gl) => (gl.#lost ? null : native.pixels(gl.#c))
    contextHooks.png = (gl) => (gl.#lost ? null : (native.png(gl.#c)?.rgba ?? null))
    contextHooks.resize = (gl) => {
      if (gl.#lost) return
      native.resize(gl.#c, gl.canvas.width, gl.canvas.height)
      gl.#drawn = true
    }
    contextHooks.lose = (gl) => {
      if (gl.#lost) return
      gl.#lost = true
      const event = new WebGLContextEvent('webglcontextlost', { cancelable: true, statusMessage: '' })
      fire(gl.canvas as unknown as EventTarget, event)
      gl.#restorable = event.defaultPrevented
    }
    // a new native context on the current device, the same attributes and size; every object and extension of the lost
    // context stays invalid; then webglcontextrestored. Without a device yet it stays lost until the next "restored".
    contextHooks.restore = (gl) => {
      if (!gl.#lost || !gl.#restorable) return
      const node = contextNodes.get(gl)
      if (node === undefined) return
      const id = native.create(node, gl.#webgl2, gl.#attributes, Math.max(1, gl.canvas.width), Math.max(1, gl.canvas.height))
      if (id == null) return
      native.release(gl.#c)
      gl.#c = id
      gl.#generation++
      gl.#objects.clear()
      gl.#extensions.clear()
      gl.#program = null
      gl.#lost = false
      gl.#restorable = false
      gl.#drawn = true
      fire(gl.canvas as unknown as EventTarget, new WebGLContextEvent('webglcontextrestored', { cancelable: false, statusMessage: '' }))
    }
    contextHooks.id = (gl) => gl.#c
  }

  #id<T extends WebGLObject>(object: T | null | undefined, Kind: abstract new (...args: never[]) => T): number | undefined {
    if (object == null) return 0
    if (!(object instanceof Kind)) throw new TypeError(`expected ${Kind.name}`)
    if (object[OWNER] !== this || object[GENERATION] !== this.#generation) {
      native.error(this.#c, C.INVALID_OPERATION)
      return undefined
    }
    return object[OBJECT]
  }

  #wrap<T extends WebGLObject>(Kind: new (token: symbol, owner: Base, id: number) => T, id: number | null): T | null {
    if (!id) return null
    const object = new Kind(TOKEN, this, id)
    object[GENERATION] = this.#generation
    this.#objects.set(id, object)
    return object
  }

  #object(value: unknown): unknown {
    if (value && typeof value === 'object' && 'kind' in value && 'id' in value) return this.#objects.get((value as { id: number }).id) ?? null
    return value
  }

  get [CONTEXT](): number {
    return this.#lost ? -1 : this.#c
  }

  get drawingBufferWidth(): number {
    return this.#lost ? 0 : (((this.#lost ? null : native.drawingSize!(this.#c)) as number[])[0] ?? 0)
  }
  get drawingBufferHeight(): number {
    return this.#lost ? 0 : (((this.#lost ? null : native.drawingSize!(this.#c)) as number[])[1] ?? 0)
  }
  get drawingBufferColorSpace(): string {
    return 'srgb'
  }
  set drawingBufferColorSpace(_: string) {}
  get unpackColorSpace(): string {
    return 'srgb'
  }
  set unpackColorSpace(_: string) {}
  drawingBufferStorage(): void {
    throw new DOMException('drawingBufferStorage is not supported', 'NotSupportedError')
  }

  getContextAttributes(): Record<string, unknown> | null {
    return this.#lost ? null : { ...this.#attributes }
  }
  isContextLost(): boolean {
    return this.#lost
  }
  makeXRCompatible(): Promise<void> {
    return Promise.reject(new DOMException('WebXR is not supported', 'InvalidStateError'))
  }

  getSupportedExtensions(): string[] | null {
    return this.#lost ? null : ((this.#lost ? null : native.supported!(this.#c)) as string[])
  }

  getExtension(name: string): object | null {
    if (this.#lost) return null
    const supported = this.getSupportedExtensions() ?? []
    const found = supported.find((n) => n.toLowerCase() === String(name).toLowerCase())
    if (!found) return null
    let ext = this.#extensions.get(found)
    if (!ext) {
      if (!this.#lost) native.enableExtension!(this.#c, found)
      ext = extension(this, found)
      this.#extensions.set(found, ext)
    }
    return ext
  }

  getError(): number {
    if (this.#lost) return C.CONTEXT_LOST_WEBGL
    return (this.#lost ? null : native.getError!(this.#c)) as number
  }

  activeTexture(texture: number): void {
    if (!this.#lost) native.activeTexture!(this.#c, texture)
  }
  blendColor(r: number, g: number, b: number, a: number): void {
    if (!this.#lost) native.blendColor!(this.#c, r, g, b, a)
  }
  blendEquation(mode: number): void {
    if (!this.#lost) native.blendEquation!(this.#c, mode, mode)
  }
  blendEquationSeparate(rgb: number, alpha: number): void {
    if (!this.#lost) native.blendEquation!(this.#c, rgb, alpha)
  }
  blendFunc(src: number, dst: number): void {
    if (!this.#lost) native.blendFunc!(this.#c, src, dst, src, dst)
  }
  blendFuncSeparate(srcRgb: number, dstRgb: number, srcAlpha: number, dstAlpha: number): void {
    if (!this.#lost) native.blendFunc!(this.#c, srcRgb, dstRgb, srcAlpha, dstAlpha)
  }
  clearColor(r: number, g: number, b: number, a: number): void {
    if (!this.#lost) native.clearColor!(this.#c, r, g, b, a)
  }
  clearDepth(d: number): void {
    if (!this.#lost) native.clearDepth!(this.#c, d)
  }
  clearStencil(s: number): void {
    if (!this.#lost) native.clearStencil!(this.#c, s)
  }
  colorMask(r: boolean, g: boolean, b: boolean, a: boolean): void {
    if (!this.#lost) native.colorMask!(this.#c, !!r, !!g, !!b, !!a)
  }
  cullFace(mode: number): void {
    if (!this.#lost) native.cullFace!(this.#c, mode)
  }
  depthFunc(f: number): void {
    if (!this.#lost) native.depthFunc!(this.#c, f)
  }
  depthMask(on: boolean): void {
    if (!this.#lost) native.depthMask!(this.#c, !!on)
  }
  depthRange(near: number, far: number): void {
    if (!this.#lost) native.depthRange!(this.#c, near, far)
  }
  disable(cap: number): void {
    if (!this.#lost) native.enable!(this.#c, cap, false)
  }
  enable(cap: number): void {
    if (!this.#lost) native.enable!(this.#c, cap, true)
  }
  isEnabled(cap: number): boolean {
    return !!(this.#lost ? null : native.isEnabled!(this.#c, cap))
  }
  frontFace(mode: number): void {
    if (!this.#lost) native.frontFace!(this.#c, mode)
  }
  hint(target: number, mode: number): void {
    if (!this.#lost) native.hint!(this.#c, target, mode)
  }
  lineWidth(w: number): void {
    if (!this.#lost) native.lineWidth!(this.#c, w)
  }
  pixelStorei(name: number, value: number | boolean): void {
    if (!this.#lost) native.pixelStorei!(this.#c, name, Number(value))
  }
  polygonOffset(factor: number, units: number): void {
    if (!this.#lost) native.polygonOffset!(this.#c, factor, units)
  }
  sampleCoverage(value: number, invert: boolean): void {
    if (!this.#lost) native.sampleCoverage!(this.#c, value, !!invert)
  }
  scissor(x: number, y: number, w: number, h: number): void {
    if (!this.#lost) native.scissor!(this.#c, x, y, w, h)
  }
  viewport(x: number, y: number, w: number, h: number): void {
    if (!this.#lost) native.viewport!(this.#c, x, y, w, h)
  }
  stencilFunc(func: number, ref: number, mask: number): void {
    if (!this.#lost) native.stencilFunc!(this.#c, C.FRONT_AND_BACK, func, ref, mask >>> 0)
  }
  stencilFuncSeparate(face: number, func: number, ref: number, mask: number): void {
    if (!this.#lost) native.stencilFunc!(this.#c, face, func, ref, mask >>> 0)
  }
  stencilMask(mask: number): void {
    if (!this.#lost) native.stencilMask!(this.#c, C.FRONT_AND_BACK, mask >>> 0)
  }
  stencilMaskSeparate(face: number, mask: number): void {
    if (!this.#lost) native.stencilMask!(this.#c, face, mask >>> 0)
  }
  stencilOp(fail: number, zfail: number, zpass: number): void {
    if (!this.#lost) native.stencilOp!(this.#c, C.FRONT_AND_BACK, fail, zfail, zpass)
  }
  stencilOpSeparate(face: number, fail: number, zfail: number, zpass: number): void {
    if (!this.#lost) native.stencilOp!(this.#c, face, fail, zfail, zpass)
  }

  flush(): void {
    if (!this.#lost) native.flush!(this.#c)
  }
  finish(): void {
    if (!this.#lost) native.flush!(this.#c)
  }

  createBuffer(): WebGLBuffer | null {
    return this.#wrap(WebGLBuffer, (this.#lost ? null : native.createBuffer!(this.#c)))
  }
  deleteBuffer(buffer: WebGLBuffer | null): void {
    const id = this.#id(buffer, WebGLBuffer)
    if (!id || !buffer || buffer.deleted) return
    buffer.deleted = true
    if (!this.#lost) native.deleteBuffer!(this.#c, id)
  }
  isBuffer(buffer: WebGLBuffer | null): boolean {
    return buffer instanceof WebGLBuffer && buffer[OWNER] === this && !buffer.deleted && !!(this.#lost ? null : native.exists!(this.#c, buffer[OBJECT]))
  }
  bindBuffer(target: number, buffer: WebGLBuffer | null): void {
    const id = this.#id(buffer, WebGLBuffer)
    if (id === undefined) return
    if (buffer?.deleted) return void native.error(this.#c, C.INVALID_OPERATION)
    if (!this.#lost) native.bindBuffer!(this.#c, target, id)
  }
  bufferData(target: number, data: number | View | null, usage: number, srcOffset = 0, length = 0): void {
    if (typeof data === 'number') {
      if (data < 0) return void native.error(this.#c, C.INVALID_VALUE)
      return void (this.#lost ? null : native.bufferData!(this.#c, target, null, data, usage))
    }
    if (data == null) return void native.error(this.#c, C.INVALID_VALUE)
    const bytes = bytesOf(data, srcOffset, length)
    if (!bytes) return void native.error(this.#c, C.INVALID_VALUE)
    if (!this.#lost) native.bufferData!(this.#c, target, bytes, bytes.byteLength, usage)
  }
  bufferSubData(target: number, offset: number, data: View, srcOffset = 0, length = 0): void {
    const bytes = bytesOf(data, srcOffset, length)
    if (!bytes) return void native.error(this.#c, C.INVALID_VALUE)
    if (!this.#lost) native.bufferSubData!(this.#c, target, offset, bytes)
  }
  getBufferParameter(target: number, name: number): unknown {
    return (this.#lost ? null : native.bufferParameter!(this.#c, target, name))
  }

  createTexture(): WebGLTexture | null {
    return this.#wrap(WebGLTexture, (this.#lost ? null : native.createTexture!(this.#c)))
  }
  deleteTexture(texture: WebGLTexture | null): void {
    const id = this.#id(texture, WebGLTexture)
    if (!id || !texture || texture.deleted) return
    texture.deleted = true
    if (!this.#lost) native.deleteTexture!(this.#c, id)
  }
  isTexture(texture: WebGLTexture | null): boolean {
    return texture instanceof WebGLTexture && texture[OWNER] === this && !texture.deleted && !!(this.#lost ? null : native.exists!(this.#c, texture[OBJECT]))
  }
  bindTexture(target: number, texture: WebGLTexture | null): void {
    const id = this.#id(texture, WebGLTexture)
    if (id === undefined) return
    if (texture?.deleted) return void native.error(this.#c, C.INVALID_OPERATION)
    if (!this.#lost) native.bindTexture!(this.#c, target, id)
  }
  texParameterf(target: number, name: number, value: number): void {
    if (!this.#lost) native.texParameter!(this.#c, target, name, value)
  }
  texParameteri(target: number, name: number, value: number): void {
    if (!this.#lost) native.texParameter!(this.#c, target, name, value)
  }
  getTexParameter(target: number, name: number): unknown {
    const v = (this.#lost ? null : native.getTexParameter!(this.#c, target, name))
    if (v === null) return null
    if (name === C.TEXTURE_IMMUTABLE_FORMAT) return !!v
    return v
  }
  generateMipmap(target: number): void {
    if (!this.#lost) native.generateMipmap!(this.#c, target)
  }

  texImage2D(target: number, level: number, internal: number, ...rest: unknown[]): void {
    if (rest.length === 3) {
      const [format, type, source] = rest as [number, number, unknown]
      return this.#source(target, level, internal, format, type, source, null)
    }
    const [width, height, border, format, type, pixels, srcOffset] = rest as [number, number, number, number, number, unknown, number?]
    if (typeof pixels === 'number') return void native.error(this.#c, C.INVALID_OPERATION)
    if (pixels != null && !ArrayBuffer.isView(pixels)) return this.#source(target, level, internal, format, type, pixels, null, [width, height])
    const bytes = pixels == null ? null : bytesOf(pixels as View, srcOffset ?? 0)
    if (!this.#lost) native.texImage!(this.#c, target, level, internal, width, height, 1, border, format, type, bytes)
  }
  texSubImage2D(target: number, level: number, x: number, y: number, ...rest: unknown[]): void {
    if (rest.length === 3) {
      const [format, type, source] = rest as [number, number, unknown]
      return this.#source(target, level, 0, format, type, source, [x, y, 0])
    }
    const [width, height, format, type, pixels, srcOffset] = rest as [number, number, number, number, unknown, number?]
    if (typeof pixels === 'number') return void native.error(this.#c, C.INVALID_OPERATION)
    if (pixels != null && !ArrayBuffer.isView(pixels)) return this.#source(target, level, 0, format, type, pixels, [x, y, 0], [width, height])
    if (pixels == null) throw new TypeError('texSubImage2D: pixels is null')
    if (!this.#lost) native.texSubImage!(this.#c, target, level, x, y, 0, width, height, 1, format, type, bytesOf(pixels as View, srcOffset ?? 0))
  }
  #source(target: number, level: number, internal: number, format: number, type: number, source: unknown, sub: number[] | null, _size?: number[]): void {
    const s = sourceOf(source)
    if (!s) throw new TypeError('texImage: not an image source')
    const [kind, a, b, data] = s
    const [x, y, z] = sub ?? [null, null, null]
    native.texImageSource(this.#c, target, level, internal, format, type, kind, a, b, data, x, y, z)
  }
  compressedTexImage2D(target: number, level: number, internal: number, width: number, height: number, border: number, data: View, srcOffset = 0, length = 0): void {
    if (border !== 0) return void native.error(this.#c, C.INVALID_VALUE)
    if (!this.#lost) native.compressedTexImage!(this.#c, target, level, internal, width, height, 1, bytesOf(data, srcOffset, length), null)
  }
  compressedTexSubImage2D(target: number, level: number, x: number, y: number, width: number, height: number, format: number, data: View, srcOffset = 0, length = 0): void {
    if (!this.#lost) native.compressedTexImage!(this.#c, target, level, format, width, height, 1, bytesOf(data, srcOffset, length), x, y, 0)
  }
  copyTexImage2D(target: number, level: number, internal: number, x: number, y: number, width: number, height: number, border: number): void {
    if (border !== 0) return void native.error(this.#c, C.INVALID_VALUE)
    if (!this.#lost) native.copyTexImage!(this.#c, target, level, internal || C.RGBA, 0, 0, 0, x, y, width, height)
  }
  copyTexSubImage2D(target: number, level: number, xoffset: number, yoffset: number, x: number, y: number, width: number, height: number): void {
    if (!this.#lost) native.copyTexImage!(this.#c, target, level, 0, xoffset, yoffset, 0, x, y, width, height)
  }

  createFramebuffer(): WebGLFramebuffer | null {
    return this.#wrap(WebGLFramebuffer, (this.#lost ? null : native.createFramebuffer!(this.#c)))
  }
  deleteFramebuffer(fb: WebGLFramebuffer | null): void {
    const id = this.#id(fb, WebGLFramebuffer)
    if (!id || !fb || fb.deleted) return
    fb.deleted = true
    if (!this.#lost) native.deleteFramebuffer!(this.#c, id)
  }
  isFramebuffer(fb: WebGLFramebuffer | null): boolean {
    return fb instanceof WebGLFramebuffer && fb[OWNER] === this && !fb.deleted && !!(this.#lost ? null : native.exists!(this.#c, fb[OBJECT]))
  }
  bindFramebuffer(target: number, fb: WebGLFramebuffer | null): void {
    const id = this.#id(fb, WebGLFramebuffer)
    if (id === undefined) return
    if (fb?.deleted) return void native.error(this.#c, C.INVALID_OPERATION)
    if (!this.#lost) native.bindFramebuffer!(this.#c, target, id)
  }
  createRenderbuffer(): WebGLRenderbuffer | null {
    return this.#wrap(WebGLRenderbuffer, (this.#lost ? null : native.createRenderbuffer!(this.#c)))
  }
  deleteRenderbuffer(rb: WebGLRenderbuffer | null): void {
    const id = this.#id(rb, WebGLRenderbuffer)
    if (!id || !rb || rb.deleted) return
    rb.deleted = true
    if (!this.#lost) native.deleteRenderbuffer!(this.#c, id)
  }
  isRenderbuffer(rb: WebGLRenderbuffer | null): boolean {
    return rb instanceof WebGLRenderbuffer && rb[OWNER] === this && !rb.deleted && !!(this.#lost ? null : native.exists!(this.#c, rb[OBJECT]))
  }
  bindRenderbuffer(target: number, rb: WebGLRenderbuffer | null): void {
    const id = this.#id(rb, WebGLRenderbuffer)
    if (id === undefined) return
    if (!this.#lost) native.bindRenderbuffer!(this.#c, target, id)
  }
  renderbufferStorage(target: number, internal: number, width: number, height: number): void {
    if (!this.#lost) native.renderbufferStorage!(this.#c, target, 0, internal, width, height)
  }
  getRenderbufferParameter(target: number, name: number): unknown {
    return (this.#lost ? null : native.renderbufferParameter!(this.#c, target, name))
  }
  framebufferRenderbuffer(target: number, attachment: number, rbtarget: number, rb: WebGLRenderbuffer | null): void {
    const id = this.#id(rb, WebGLRenderbuffer)
    if (id === undefined) return
    if (!this.#lost) native.framebufferRenderbuffer!(this.#c, target, attachment, rbtarget, id)
  }
  framebufferTexture2D(target: number, attachment: number, textarget: number, texture: WebGLTexture | null, level: number): void {
    const id = this.#id(texture, WebGLTexture)
    if (id === undefined) return
    if (!this.#lost) native.framebufferTexture!(this.#c, target, attachment, textarget, id, level, 0)
  }
  checkFramebufferStatus(target: number): number {
    return this.#lost ? 0 : ((this.#lost ? null : native.checkFramebufferStatus!(this.#c, target)) as number)
  }
  getFramebufferAttachmentParameter(target: number, attachment: number, name: number): unknown {
    return this.#object((this.#lost ? null : native.attachmentParameter!(this.#c, target, attachment, name)))
  }

  createShader(type: number): WebGLShader | null {
    const shader = this.#wrap(WebGLShader, (this.#lost ? null : native.createShader!(this.#c, type)))
    if (shader) this.#shaderTypes.set(shader[OBJECT], type)
    return shader
  }
  deleteShader(shader: WebGLShader | null): void {
    const id = this.#id(shader, WebGLShader)
    if (!id || !shader || shader.deleted) return
    shader.deleted = true
    if (!this.#lost) native.deleteShader!(this.#c, id)
  }
  isShader(shader: WebGLShader | null): boolean {
    return shader instanceof WebGLShader && shader[OWNER] === this && !(shader.deleted && shader.attached === 0) && !!(this.#lost ? null : native.exists!(this.#c, shader[OBJECT]))
  }
  shaderSource(shader: WebGLShader, source: string): void {
    const id = this.#id(shader, WebGLShader)
    if (id) (this.#lost ? null : native.shaderSource!(this.#c, id, String(source)))
  }
  getShaderSource(shader: WebGLShader): string | null {
    const id = this.#id(shader, WebGLShader)
    return id ? ((this.#lost ? null : native.getShaderSource!(this.#c, id)) as string | null) : null
  }
  compileShader(shader: WebGLShader): void {
    const id = this.#id(shader, WebGLShader)
    if (!id) return
    if (!this.#lost) native.compileShader!(this.#c, id)
    shader.compiled = (this.#lost ? null : native.shaderInfoLog!(this.#c, id)) === ''
  }
  getShaderInfoLog(shader: WebGLShader): string | null {
    const id = this.#id(shader, WebGLShader)
    return id ? ((this.#lost ? null : native.shaderInfoLog!(this.#c, id)) as string | null) : null
  }
  getShaderParameter(shader: WebGLShader, name: number): unknown {
    const id = this.#id(shader, WebGLShader)
    if (!id) return null
    switch (name) {
      case C.SHADER_TYPE:
        return shader instanceof WebGLShader ? (this.#shaderTypes.get(id) ?? null) : null
      case C.DELETE_STATUS:
        return shader.deleted
      case C.COMPILE_STATUS:
        return shader.compiled
      case 0x91b1:
        return true
    }
    native.error(this.#c, C.INVALID_ENUM)
    return null
  }
  #shaderTypes = new Map<number, number>()
  getShaderPrecisionFormat(shaderType: number, precision: number): WebGLShaderPrecisionFormat | null {
    if (shaderType !== C.VERTEX_SHADER && shaderType !== C.FRAGMENT_SHADER) return void native.error(this.#c, C.INVALID_ENUM), null
    switch (precision) {
      case C.LOW_FLOAT:
      case C.MEDIUM_FLOAT:
      case C.HIGH_FLOAT:
        return new WebGLShaderPrecisionFormat(TOKEN, 127, 127, 23)
      case C.LOW_INT:
      case C.MEDIUM_INT:
      case C.HIGH_INT:
        return new WebGLShaderPrecisionFormat(TOKEN, 31, 30, 0)
    }
    native.error(this.#c, C.INVALID_ENUM)
    return null
  }

  createProgram(): WebGLProgram | null {
    return this.#wrap(WebGLProgram, (this.#lost ? null : native.createProgram!(this.#c)))
  }
  deleteProgram(program: WebGLProgram | null): void {
    const id = this.#id(program, WebGLProgram)
    if (!id || !program || program.deleted) return
    program.deleted = true
    if (!this.#lost) native.deleteProgram!(this.#c, id)
  }
  isProgram(program: WebGLProgram | null): boolean {
    return program instanceof WebGLProgram && program[OWNER] === this && !!(this.#lost ? null : native.exists!(this.#c, program[OBJECT]))
  }
  attachShader(program: WebGLProgram, shader: WebGLShader): void {
    const p = this.#id(program, WebGLProgram)
    const s = this.#id(shader, WebGLShader)
    if (!p || !s) return
    if (!this.#lost) native.attachShader!(this.#c, p, s)
    shader.attached++
  }
  detachShader(program: WebGLProgram, shader: WebGLShader): void {
    const p = this.#id(program, WebGLProgram)
    const s = this.#id(shader, WebGLShader)
    if (!p || !s) return
    if (!this.#lost) native.detachShader!(this.#c, p, s)
    shader.attached = Math.max(0, shader.attached - 1)
  }
  getAttachedShaders(program: WebGLProgram): WebGLShader[] | null {
    const id = this.#id(program, WebGLProgram)
    if (!id) return null
    return ((this.#lost ? null : native.attachedShaders!(this.#c, id)) as number[]).map((s) => this.#objects.get(s) as WebGLShader)
  }
  bindAttribLocation(program: WebGLProgram, index: number, name: string): void {
    const id = this.#id(program, WebGLProgram)
    if (id) (this.#lost ? null : native.bindAttribLocation!(this.#c, id, index, String(name)))
  }
  linkProgram(program: WebGLProgram): void {
    const id = this.#id(program, WebGLProgram)
    if (!id) return
    if (!this.#lost) native.linkProgram!(this.#c, id)
    program.linked = (this.#lost ? null : native.programInfoLog!(this.#c, id)) === ''
    program.serial = (this.#lost ? null : native.programSerial!(this.#c, id)) as number
  }
  validateProgram(program: WebGLProgram): void {
    const id = this.#id(program, WebGLProgram)
    if (!id) return
    if (!this.#lost) native.validateProgram!(this.#c, id)
    program.validated = program.linked
  }
  getProgramInfoLog(program: WebGLProgram): string | null {
    const id = this.#id(program, WebGLProgram)
    return id ? ((this.#lost ? null : native.programInfoLog!(this.#c, id)) as string | null) : null
  }
  getProgramParameter(program: WebGLProgram, name: number): unknown {
    const id = this.#id(program, WebGLProgram)
    if (!id) return null
    switch (name) {
      case C.DELETE_STATUS:
        return program.deleted
      case C.LINK_STATUS:
        return program.linked
      case C.VALIDATE_STATUS:
        return program.validated
      case C.ATTACHED_SHADERS:
        return ((this.#lost ? null : native.attachedShaders!(this.#c, id)) as number[]).length
      case C.ACTIVE_ATTRIBUTES:
        return (this.#lost ? null : native.activeAttribCount!(this.#c, id))
      case C.ACTIVE_UNIFORMS:
        return (this.#lost ? null : native.activeUniformCount!(this.#c, id))
      case 0x91b1:
        return true
    }
    if (this.#webgl2) {
      switch (name) {
        case C.ACTIVE_UNIFORM_BLOCKS: {
          let n = 0
          while ((this.#lost ? null : native.blockInfo!(this.#c, id, n))) n++
          return n
        }
        case C.TRANSFORM_FEEDBACK_BUFFER_MODE:
          return program.bufferMode
        case C.TRANSFORM_FEEDBACK_VARYINGS:
          return program.varyings.length
      }
    }
    native.error(this.#c, C.INVALID_ENUM)
    return null
  }
  useProgram(program: WebGLProgram | null): void {
    const id = this.#id(program, WebGLProgram)
    if (id === undefined) return
    if (program?.deleted) return void native.error(this.#c, C.INVALID_OPERATION)
    if ((this.#lost ? null : native.useProgram!(this.#c, id))) this.#program = program
  }
  getAttribLocation(program: WebGLProgram, name: string): number {
    const id = this.#id(program, WebGLProgram)
    return id ? ((this.#lost ? null : native.attribLocation!(this.#c, id, String(name))) as number) : -1
  }
  getActiveAttrib(program: WebGLProgram, index: number): WebGLActiveInfo | null {
    const id = this.#id(program, WebGLProgram)
    const info = id ? ((this.#lost ? null : native.activeAttrib!(this.#c, id, index)) as { name: string; size: number; type: number } | null) : null
    return info && new WebGLActiveInfo(TOKEN, info.size, info.type, info.name)
  }
  getActiveUniform(program: WebGLProgram, index: number): WebGLActiveInfo | null {
    const id = this.#id(program, WebGLProgram)
    const info = id ? ((this.#lost ? null : native.activeUniform!(this.#c, id, index)) as { name: string; size: number; type: number } | null) : null
    return info && new WebGLActiveInfo(TOKEN, info.size, info.type, info.name)
  }
  getUniformLocation(program: WebGLProgram, name: string): WebGLUniformLocation | null {
    const id = this.#id(program, WebGLProgram)
    if (!id) return null
    const index = (this.#lost ? null : native.uniformLocation!(this.#c, id, String(name))) as number
    return index < 0 ? null : new WebGLUniformLocation(TOKEN, program, index, program.serial)
  }
  getUniform(program: WebGLProgram, location: WebGLUniformLocation): unknown {
    const id = this.#id(program, WebGLProgram)
    if (!id || !(location instanceof WebGLUniformLocation)) return null
    const r = (this.#lost ? null : native.getUniform!(this.#c, id, location.index, location.serial)) as { type: number; value: ArrayLike<number> | boolean[] } | null
    if (!r) return null
    const v = r.value
    const scalar = [C.FLOAT, C.INT, C.UNSIGNED_INT, C.BOOL].includes(r.type) || r.type >= C.SAMPLER_2D && r.type <= C.SAMPLER_CUBE || isSampler(r.type)
    return scalar ? (v as ArrayLike<number>)[0] : v
  }
  _uniform(location: WebGLUniformLocation | null, kind: number, n: number, m: number, transpose: boolean, data: Float32Array | Int32Array | Uint32Array): void {
    if (location == null || this.#lost) return
    if (!(location instanceof WebGLUniformLocation)) throw new TypeError('expected WebGLUniformLocation')
    if (location.program[OWNER] !== this) return void native.error(this.#c, C.INVALID_OPERATION)
    native.uniform(this.#c, location.program[OBJECT], location.index, location.serial, kind, n, m, transpose, new Uint8Array(data.buffer, data.byteOffset, data.byteLength))
  }

  createVertexArray(): WebGLVertexArrayObject | null {
    return this.#wrap(WebGLVertexArrayObject, (this.#lost ? null : native.createVertexArray!(this.#c)))
  }
  _createVertexArrayOES(): WebGLVertexArrayObjectOES | null {
    return this.#wrap(WebGLVertexArrayObjectOES, (this.#lost ? null : native.createVertexArray!(this.#c)))
  }
  deleteVertexArray(vao: WebGLObject | null): void {
    if (vao == null || vao.deleted || vao[OWNER] !== this) return
    vao.deleted = true
    if (!this.#lost) native.deleteVertexArray!(this.#c, vao[OBJECT])
  }
  isVertexArray(vao: WebGLObject | null): boolean {
    return vao instanceof WebGLObject && vao[OWNER] === this && !vao.deleted && !!(this.#lost ? null : native.isVertexArray!(this.#c, vao[OBJECT]))
  }
  bindVertexArray(vao: WebGLObject | null): void {
    if (vao != null && (vao[OWNER] !== this || vao.deleted)) return void native.error(this.#c, C.INVALID_OPERATION)
    if (!this.#lost) native.bindVertexArray!(this.#c, vao ? vao[OBJECT] : 0)
  }

  enableVertexAttribArray(index: number): void {
    if (!this.#lost) native.enableVertexAttrib!(this.#c, index, true)
  }
  disableVertexAttribArray(index: number): void {
    if (!this.#lost) native.enableVertexAttrib!(this.#c, index, false)
  }
  vertexAttribPointer(index: number, size: number, type: number, normalized: boolean, stride: number, offset: number): void {
    if (!this.#lost) native.vertexAttribPointer!(this.#c, index, size, type, !!normalized, stride, offset, false)
  }
  vertexAttribIPointer(index: number, size: number, type: number, stride: number, offset: number): void {
    if (!this.#lost) native.vertexAttribPointer!(this.#c, index, size, type, false, stride, offset, true)
  }
  vertexAttribDivisor(index: number, divisor: number): void {
    if (!this.#lost) native.vertexAttribDivisor!(this.#c, index, divisor)
  }
  vertexAttrib1f(i: number, x: number): void {
    if (!this.#lost) native.vertexAttrib!(this.#c, i, 0, x, 0, 0, 1)
  }
  vertexAttrib2f(i: number, x: number, y: number): void {
    if (!this.#lost) native.vertexAttrib!(this.#c, i, 0, x, y, 0, 1)
  }
  vertexAttrib3f(i: number, x: number, y: number, z: number): void {
    if (!this.#lost) native.vertexAttrib!(this.#c, i, 0, x, y, z, 1)
  }
  vertexAttrib4f(i: number, x: number, y: number, z: number, w: number): void {
    if (!this.#lost) native.vertexAttrib!(this.#c, i, 0, x, y, z, w)
  }
  vertexAttrib1fv(i: number, v: ArrayLike<number>): void {
    if (!this.#lost) native.vertexAttrib!(this.#c, i, 0, v[0], 0, 0, 1)
  }
  vertexAttrib2fv(i: number, v: ArrayLike<number>): void {
    if (!this.#lost) native.vertexAttrib!(this.#c, i, 0, v[0], v[1], 0, 1)
  }
  vertexAttrib3fv(i: number, v: ArrayLike<number>): void {
    if (!this.#lost) native.vertexAttrib!(this.#c, i, 0, v[0], v[1], v[2], 1)
  }
  vertexAttrib4fv(i: number, v: ArrayLike<number>): void {
    if (!this.#lost) native.vertexAttrib!(this.#c, i, 0, v[0], v[1], v[2], v[3])
  }
  vertexAttribI4i(i: number, x: number, y: number, z: number, w: number): void {
    if (!this.#lost) native.vertexAttrib!(this.#c, i, 1, x, y, z, w)
  }
  vertexAttribI4ui(i: number, x: number, y: number, z: number, w: number): void {
    if (!this.#lost) native.vertexAttrib!(this.#c, i, 2, x, y, z, w)
  }
  vertexAttribI4iv(i: number, v: ArrayLike<number>): void {
    if (!this.#lost) native.vertexAttrib!(this.#c, i, 1, v[0], v[1], v[2], v[3])
  }
  vertexAttribI4uiv(i: number, v: ArrayLike<number>): void {
    if (!this.#lost) native.vertexAttrib!(this.#c, i, 2, v[0], v[1], v[2], v[3])
  }
  getVertexAttrib(index: number, name: number): unknown {
    const s = (this.#lost ? null : native.vertexAttribState!(this.#c, index)) as Record<string, unknown> | null
    if (!s) return null
    switch (name) {
      case C.VERTEX_ATTRIB_ARRAY_BUFFER_BINDING:
        return this.#objects.get(s.buffer as number) ?? null
      case C.VERTEX_ATTRIB_ARRAY_ENABLED:
        return s.enabled
      case C.VERTEX_ATTRIB_ARRAY_SIZE:
        return s.size
      case C.VERTEX_ATTRIB_ARRAY_STRIDE:
        return s.stride
      case C.VERTEX_ATTRIB_ARRAY_TYPE:
        return s.type
      case C.VERTEX_ATTRIB_ARRAY_NORMALIZED:
        return s.normalized
      case C.CURRENT_VERTEX_ATTRIB:
        return s.current
      case C.VERTEX_ATTRIB_ARRAY_INTEGER:
        if (this.#webgl2) return s.integer
        break
      case C.VERTEX_ATTRIB_ARRAY_DIVISOR:
        if (this.#webgl2 || this.#extensions.has('ANGLE_instanced_arrays')) return s.divisor
        break
    }
    native.error(this.#c, C.INVALID_ENUM)
    return null
  }
  getVertexAttribOffset(index: number, name: number): number {
    if (name !== C.VERTEX_ATTRIB_ARRAY_POINTER) return void native.error(this.#c, C.INVALID_ENUM), 0
    const s = (this.#lost ? null : native.vertexAttribState!(this.#c, index)) as { offset: number } | null
    return s?.offset ?? 0
  }

  #draw(mode: number, first: number, count: number, instances: number, type: number, offset: number): void {
    if (this.#lost) return
    this.#drawn = true
    contextHooks.request?.()
    native.draw(this.#c, mode, first, count, instances, type, offset)
  }
  drawArrays(mode: number, first: number, count: number): void {
    this.#draw(mode, first, count, 1, 0, 0)
  }
  drawElements(mode: number, count: number, type: number, offset: number): void {
    if (!type) return void native.error(this.#c, C.INVALID_ENUM)
    this.#draw(mode, 0, count, 1, type, offset)
  }
  drawArraysInstanced(mode: number, first: number, count: number, instances: number): void {
    this.#draw(mode, first, count, instances, 0, 0)
  }
  drawElementsInstanced(mode: number, count: number, type: number, offset: number, instances: number): void {
    if (!type) return void native.error(this.#c, C.INVALID_ENUM)
    this.#draw(mode, 0, count, instances, type, offset)
  }
  clear(mask: number): void {
    if (this.#lost) return
    this.#drawn = true
    contextHooks.request?.()
    native.clear(this.#c, mask)
  }

  readPixels(x: number, y: number, width: number, height: number, format: number, type: number, pixels: ArrayBufferView | number | null, dstOffset = 0): void {
    if (this.#lost) return
    if (pixels == null) return void native.error(this.#c, C.INVALID_VALUE)
    if (typeof pixels === 'number') return void native.error(this.#c, C.INVALID_OPERATION)
    const size = (pixels as unknown as { BYTES_PER_ELEMENT?: number }).BYTES_PER_ELEMENT ?? 1
    const view = new Uint8Array(pixels.buffer, pixels.byteOffset + dstOffset * size, pixels.byteLength - dstOffset * size)
    native.readPixels(this.#c, x, y, width, height, format, type, view)
  }

  getParameter(name: number): unknown {
    if (this.#lost) return null
    const v = (this.#lost ? null : native.parameter!(this.#c, name))
    if (name === C.CURRENT_PROGRAM) return this.#program
    return this.#object(v)
  }

  static {
    webglHooks.objects = (gl, id) => gl.#objects.get(id) ?? null
    webglHooks.wrap = (gl, Kind, id) => gl.#wrap(Kind, id)
    webglHooks.id = (gl, object, Kind) => gl.#id(object, Kind)
    webglHooks.shaderTypes = (gl) => gl.#shaderTypes
    webglHooks.draw = (gl, mode, first, count, instances, type, offset) => gl.#draw(mode, first, count, instances, type, offset)
    webglHooks.webgl2 = (gl) => gl.#webgl2
  }
}

function isSampler(type: number): boolean {
  return [C.SAMPLER_2D, C.SAMPLER_CUBE, C.SAMPLER_3D, C.SAMPLER_2D_SHADOW, C.SAMPLER_2D_ARRAY, C.SAMPLER_2D_ARRAY_SHADOW, C.SAMPLER_CUBE_SHADOW, C.INT_SAMPLER_2D, C.INT_SAMPLER_3D, C.INT_SAMPLER_CUBE, C.INT_SAMPLER_2D_ARRAY, C.UNSIGNED_INT_SAMPLER_2D, C.UNSIGNED_INT_SAMPLER_3D, C.UNSIGNED_INT_SAMPLER_CUBE, C.UNSIGNED_INT_SAMPLER_2D_ARRAY].includes(type)
}

const proto = WebGLContextBase.prototype as unknown as Record<string, unknown>
for (const [suffix, kind, Kind] of [['f', 0, Float32Array], ['i', 1, Int32Array], ['ui', 2, Uint32Array]] as const) {
  for (let n = 1; n <= 4; n++) {
    proto[`uniform${n}${suffix}`] = function (this: WebGLContextBase, location: WebGLUniformLocation | null, ...values: number[]) {
      this._uniform(location, kind, n, 0, false, new Kind(values.slice(0, n)))
    }
    proto[`uniform${n}${suffix}v`] = function (this: WebGLContextBase, location: WebGLUniformLocation | null, data: Uniform, srcOffset = 0, srcLength = 0) {
      this._uniform(location, kind, n, 0, false, typed(Kind as never, data, srcOffset, srcLength))
    }
  }
}
for (const [c, r] of [[2, 2], [3, 3], [4, 4], [2, 3], [2, 4], [3, 2], [3, 4], [4, 2], [4, 3]] as const) {
  const name = c === r ? `uniformMatrix${c}fv` : `uniformMatrix${c}x${r}fv`
  proto[name] = function (this: WebGLContextBase, location: WebGLUniformLocation | null, transpose: boolean, data: Uniform, srcOffset = 0, srcLength = 0) {
    this._uniform(location, 3, c, r, !!transpose, typed(Float32Array, data, srcOffset, srcLength))
  }
}

export class WebGLRenderingContext extends WebGLContextBase {}
constants(WebGLRenderingContext, WEBGL1)
tag(WebGLRenderingContext)

export class WebGL2RenderingContext extends WebGLContextBase {
  #error(code: number): void {
    native.error(contextHooks.id(this), code)
  }
  #id<T extends WebGLObject>(object: T | null | undefined, Kind: abstract new (...args: never[]) => T): number | undefined {
    return webglHooks.id(this, object, Kind)
  }

  bindBufferBase(target: number, index: number, buffer: WebGLBuffer | null): void {
    const id = this.#id(buffer, WebGLBuffer)
    if (id !== undefined) (this[CONTEXT] < 0 ? null : native.bindBufferRange!(this[CONTEXT], target, index, id, 0, -1))
  }
  bindBufferRange(target: number, index: number, buffer: WebGLBuffer | null, offset: number, size: number): void {
    const id = this.#id(buffer, WebGLBuffer)
    if (id !== undefined) (this[CONTEXT] < 0 ? null : native.bindBufferRange!(this[CONTEXT], target, index, id, offset, size))
  }
  copyBufferSubData(read: number, write: number, readOffset: number, writeOffset: number, size: number): void {
    if (this[CONTEXT] >= 0) native.copyBufferSubData!(this[CONTEXT], read, write, readOffset, writeOffset, size)
  }
  getBufferSubData(target: number, offset: number, dst: ArrayBufferView, dstOffset = 0, length = 0): void {
    const bytes = bytesOf(dst, dstOffset, length)
    if (!bytes) return this.#error(C.INVALID_VALUE)
    if (this[CONTEXT] >= 0) native.getBufferSubData!(this[CONTEXT], target, offset, bytes)
  }
  getIndexedParameter(target: number, index: number): unknown {
    const v = (this[CONTEXT] < 0 ? null : native.indexedParameter!(this[CONTEXT], target, index))
    return v && typeof v === 'object' && 'id' in v ? webglHooks.objects(this, (v as { id: number }).id) : v
  }

  texImage3D(target: number, level: number, internal: number, width: number, height: number, depth: number, border: number, format: number, type: number, pixels: unknown, srcOffset = 0): void {
    if (typeof pixels === 'number') return this.#error(C.INVALID_OPERATION)
    if (pixels != null && !ArrayBuffer.isView(pixels)) return this.#error(C.INVALID_OPERATION)
    if (this[CONTEXT] >= 0) native.texImage!(this[CONTEXT], target, level, internal, width, height, depth, border, format, type, pixels == null ? null : bytesOf(pixels as View, srcOffset))
  }
  texSubImage3D(target: number, level: number, x: number, y: number, z: number, width: number, height: number, depth: number, format: number, type: number, pixels: unknown, srcOffset = 0): void {
    if (pixels == null || typeof pixels === 'number') return this.#error(C.INVALID_OPERATION)
    if (!ArrayBuffer.isView(pixels)) {
      const s = sourceOf(pixels)
      if (!s) throw new TypeError('texSubImage3D: not an image source')
      native.texImageSource(contextHooks.id(this), target, level, 0, format, type, s[0], s[1], s[2], s[3], x, y, z)
      return
    }
    if (this[CONTEXT] >= 0) native.texSubImage!(this[CONTEXT], target, level, x, y, z, width, height, depth, format, type, bytesOf(pixels as View, srcOffset))
  }
  texStorage2D(target: number, levels: number, internal: number, width: number, height: number): void {
    if (this[CONTEXT] >= 0) native.texStorage!(this[CONTEXT], target, levels, internal, width, height, 1)
  }
  texStorage3D(target: number, levels: number, internal: number, width: number, height: number, depth: number): void {
    if (this[CONTEXT] >= 0) native.texStorage!(this[CONTEXT], target, levels, internal, width, height, depth)
  }
  copyTexSubImage3D(target: number, level: number, xoffset: number, yoffset: number, zoffset: number, x: number, y: number, width: number, height: number): void {
    if (this[CONTEXT] >= 0) native.copyTexImage!(this[CONTEXT], target, level, 0, xoffset, yoffset, zoffset, x, y, width, height)
  }
  compressedTexImage3D(target: number, level: number, internal: number, width: number, height: number, depth: number, border: number, data: View, srcOffset = 0, length = 0): void {
    if (border !== 0) return this.#error(C.INVALID_VALUE)
    if (this[CONTEXT] >= 0) native.compressedTexImage!(this[CONTEXT], target, level, internal, width, height, depth, bytesOf(data, srcOffset, length), null)
  }
  compressedTexSubImage3D(target: number, level: number, x: number, y: number, z: number, width: number, height: number, depth: number, format: number, data: View, srcOffset = 0, length = 0): void {
    if (this[CONTEXT] >= 0) native.compressedTexImage!(this[CONTEXT], target, level, format, width, height, depth, bytesOf(data, srcOffset, length), x, y, z)
  }
  framebufferTextureLayer(target: number, attachment: number, texture: WebGLTexture | null, level: number, layer: number): void {
    const id = this.#id(texture, WebGLTexture)
    if (id === undefined) return
    if (this[CONTEXT] >= 0) native.framebufferTexture!(this[CONTEXT], target, attachment, 0, id, level, layer)
  }
  invalidateFramebuffer(_target: number, _attachments: number[]): void {}
  invalidateSubFramebuffer(_target: number, _attachments: number[], _x: number, _y: number, _w: number, _h: number): void {}
  readBuffer(src: number): void {
    if (this[CONTEXT] >= 0) native.readBuffer!(this[CONTEXT], src)
  }
  drawBuffers(buffers: Iterable<number>): void {
    if (this[CONTEXT] >= 0) native.drawBuffers!(this[CONTEXT], new Uint8Array(new Uint32Array(Array.from(buffers)).buffer))
  }
  blitFramebuffer(sx0: number, sy0: number, sx1: number, sy1: number, dx0: number, dy0: number, dx1: number, dy1: number, mask: number, filter: number): void {
    contextHooks.request?.()
    if (this[CONTEXT] >= 0) native.blitFramebuffer!(this[CONTEXT], sx0, sy0, sx1, sy1, dx0, dy0, dx1, dy1, mask, filter)
  }
  renderbufferStorageMultisample(target: number, samples: number, internal: number, width: number, height: number): void {
    if (this[CONTEXT] >= 0) native.renderbufferStorage!(this[CONTEXT], target, samples, internal, width, height)
  }
  getInternalformatParameter(target: number, internal: number, name: number): Int32Array | null {
    if (target !== C.RENDERBUFFER) return this.#error(C.INVALID_ENUM), null
    if (name !== C.SAMPLES) return this.#error(C.INVALID_ENUM), null
    const integer = [C.R8I, C.R8UI, C.R16I, C.R16UI, C.R32I, C.R32UI, C.RG8I, C.RG8UI, C.RG16I, C.RG16UI, C.RG32I, C.RG32UI, C.RGBA8I, C.RGBA8UI, C.RGBA16I, C.RGBA16UI, C.RGBA32I, C.RGBA32UI, C.RGB10_A2UI].includes(internal)
    return new Int32Array(integer ? [] : [4])
  }
  getFragDataLocation(program: WebGLProgram, name: string): number {
    const id = this.#id(program, WebGLProgram)
    return id ? ((this[CONTEXT] < 0 ? null : native.fragDataLocation!(this[CONTEXT], id, String(name))) as number) : -1
  }
  clearBufferfv(buffer: number, drawbuffer: number, values: Uniform, srcOffset = 0): void {
    const v = typed(Float32Array, values, srcOffset)
    contextHooks.request?.()
    if (this[CONTEXT] >= 0) native.clearBuffer!(this[CONTEXT], buffer, drawbuffer, 0, new Uint8Array(v.buffer, v.byteOffset, 16), v[0] ?? 0, 0)
  }
  clearBufferiv(buffer: number, drawbuffer: number, values: Uniform, srcOffset = 0): void {
    const v = typed(Int32Array, values, srcOffset)
    contextHooks.request?.()
    if (this[CONTEXT] >= 0) native.clearBuffer!(this[CONTEXT], buffer, drawbuffer, 1, new Uint8Array(v.buffer, v.byteOffset, 16), 0, v[0] ?? 0)
  }
  clearBufferuiv(buffer: number, drawbuffer: number, values: Uniform, srcOffset = 0): void {
    const v = typed(Uint32Array, values, srcOffset)
    contextHooks.request?.()
    if (this[CONTEXT] >= 0) native.clearBuffer!(this[CONTEXT], buffer, drawbuffer, 2, new Uint8Array(v.buffer, v.byteOffset, 16), 0, 0)
  }
  clearBufferfi(buffer: number, drawbuffer: number, depth: number, stencil: number): void {
    contextHooks.request?.()
    if (this[CONTEXT] >= 0) native.clearBuffer!(this[CONTEXT], buffer, drawbuffer, 0, new Uint8Array(16), depth, stencil)
  }
  drawRangeElements(mode: number, _start: number, _end: number, count: number, type: number, offset: number): void {
    this.drawElements(mode, count, type, offset)
  }

  createQuery(): WebGLQuery | null {
    return webglHooks.wrap(this, WebGLQuery, (this[CONTEXT] < 0 ? null : native.createQuery!(this[CONTEXT])))
  }
  deleteQuery(query: WebGLQuery | null): void {
    const id = this.#id(query, WebGLQuery)
    if (id && query && !query.deleted) {
      query.deleted = true
      if (this[CONTEXT] >= 0) native.deleteQuery!(this[CONTEXT], id)
    }
  }
  isQuery(query: WebGLQuery | null): boolean {
    return query instanceof WebGLQuery && !query.deleted && query.target !== 0
  }
  beginQuery(target: number, query: WebGLQuery): void {
    const id = this.#id(query, WebGLQuery)
    if (!id) return this.#error(C.INVALID_OPERATION)
    if (this[CONTEXT] >= 0) native.beginQuery!(this[CONTEXT], target, id)
    query.target = target
    query.ended = -2
    this.#queries.add(query)
    this.#current.set(target, query)
  }
  endQuery(target: number): void {
    if (this[CONTEXT] >= 0) native.endQuery!(this[CONTEXT], target)
    for (const q of this.#queries) if (q.target === target && q.ended === -2) q.ended = epoch
    this.#current.delete(target)
  }
  #queries = new Set<WebGLQuery>()
  #current = new Map<number, WebGLQuery>()
  getQuery(target: number, name: number): WebGLQuery | null {
    if (name !== C.CURRENT_QUERY) return this.#error(C.INVALID_ENUM), null
    return this.#current.get(target) ?? null
  }
  getQueryParameter(query: WebGLQuery, name: number): unknown {
    const id = this.#id(query, WebGLQuery)
    if (!id) return null
    if (query.ended === epoch) {
      if (name === C.QUERY_RESULT_AVAILABLE) return false
      if (name === C.QUERY_RESULT) return 0
    }
    const v = (this[CONTEXT] < 0 ? null : native.queryResult!(this[CONTEXT], id, name))
    if (v == null) return null
    if (name === C.QUERY_RESULT_AVAILABLE) return !!v
    return query.target === C.ANY_SAMPLES_PASSED || query.target === C.ANY_SAMPLES_PASSED_CONSERVATIVE ? !!v : v
  }

  createSampler(): WebGLSampler | null {
    return webglHooks.wrap(this, WebGLSampler, (this[CONTEXT] < 0 ? null : native.createSampler!(this[CONTEXT])))
  }
  deleteSampler(sampler: WebGLSampler | null): void {
    const id = this.#id(sampler, WebGLSampler)
    if (id && sampler && !sampler.deleted) {
      sampler.deleted = true
      if (this[CONTEXT] >= 0) native.deleteSampler!(this[CONTEXT], id)
    }
  }
  isSampler(sampler: WebGLSampler | null): boolean {
    return sampler instanceof WebGLSampler && !sampler.deleted
  }
  bindSampler(unit: number, sampler: WebGLSampler | null): void {
    const id = this.#id(sampler, WebGLSampler)
    if (id !== undefined) (this[CONTEXT] < 0 ? null : native.bindSampler!(this[CONTEXT], unit, id))
  }
  samplerParameteri(sampler: WebGLSampler, name: number, value: number): void {
    const id = this.#id(sampler, WebGLSampler)
    if (id) (this[CONTEXT] < 0 ? null : native.samplerParameter!(this[CONTEXT], id, name, value))
  }
  samplerParameterf(sampler: WebGLSampler, name: number, value: number): void {
    const id = this.#id(sampler, WebGLSampler)
    if (id) (this[CONTEXT] < 0 ? null : native.samplerParameter!(this[CONTEXT], id, name, value))
  }
  getSamplerParameter(sampler: WebGLSampler, name: number): unknown {
    const id = this.#id(sampler, WebGLSampler)
    return id ? (this[CONTEXT] < 0 ? null : native.getSamplerParameter!(this[CONTEXT], id, name)) : null
  }

  fenceSync(condition: number, flags: number): WebGLSync | null {
    if (condition !== C.SYNC_GPU_COMMANDS_COMPLETE || flags !== 0) return this.#error(condition !== C.SYNC_GPU_COMMANDS_COMPLETE ? C.INVALID_ENUM : C.INVALID_VALUE), null
    const sync = webglHooks.wrap(this, WebGLSync, (this[CONTEXT] < 0 ? null : native.fenceSync!(this[CONTEXT])))
    if (sync) sync.created = epoch
    return sync
  }
  isSync(sync: WebGLSync | null): boolean {
    return sync instanceof WebGLSync && !sync.deleted
  }
  deleteSync(sync: WebGLSync | null): void {
    const id = this.#id(sync, WebGLSync)
    if (id && sync) {
      sync.deleted = true
      if (this[CONTEXT] >= 0) native.deleteSync!(this[CONTEXT], id)
    }
  }
  clientWaitSync(sync: WebGLSync, _flags: number, _timeout: number): number {
    const id = this.#id(sync, WebGLSync)
    if (!id) return C.WAIT_FAILED
    if (sync.created === epoch) return C.TIMEOUT_EXPIRED
    return (this[CONTEXT] < 0 ? null : native.syncStatus!(this[CONTEXT], id)) ? C.CONDITION_SATISFIED : C.TIMEOUT_EXPIRED
  }
  waitSync(_sync: WebGLSync, _flags: number, _timeout: number): void {}
  getSyncParameter(sync: WebGLSync, name: number): unknown {
    const id = this.#id(sync, WebGLSync)
    if (!id) return null
    switch (name) {
      case C.OBJECT_TYPE:
        return C.SYNC_FENCE
      case C.SYNC_STATUS:
        return sync.created !== epoch && (this[CONTEXT] < 0 ? null : native.syncStatus!(this[CONTEXT], id)) ? C.SIGNALED : C.UNSIGNALED
      case C.SYNC_CONDITION:
        return C.SYNC_GPU_COMMANDS_COMPLETE
      case C.SYNC_FLAGS:
        return 0
    }
    this.#error(C.INVALID_ENUM)
    return null
  }

  createTransformFeedback(): WebGLTransformFeedback | null {
    return webglHooks.wrap(this, WebGLTransformFeedback, (this[CONTEXT] < 0 ? null : native.createTransformFeedback!(this[CONTEXT])))
  }
  deleteTransformFeedback(tf: WebGLTransformFeedback | null): void {
    const id = this.#id(tf, WebGLTransformFeedback)
    if (id && tf) {
      tf.deleted = true
      if (this[CONTEXT] >= 0) native.deleteTransformFeedback!(this[CONTEXT], id)
    }
  }
  isTransformFeedback(tf: WebGLTransformFeedback | null): boolean {
    return tf instanceof WebGLTransformFeedback && !tf.deleted
  }
  bindTransformFeedback(_target: number, tf: WebGLTransformFeedback | null): void {
    const id = this.#id(tf, WebGLTransformFeedback)
    if (id !== undefined) (this[CONTEXT] < 0 ? null : native.bindTransformFeedback!(this[CONTEXT], id))
  }
  beginTransformFeedback(_mode: number): void {
    if (this[CONTEXT] >= 0) native.beginTransformFeedback!(this[CONTEXT])
  }
  endTransformFeedback(): void {
    this.#error(C.INVALID_OPERATION)
  }
  pauseTransformFeedback(): void {
    this.#error(C.INVALID_OPERATION)
  }
  resumeTransformFeedback(): void {
    this.#error(C.INVALID_OPERATION)
  }
  transformFeedbackVaryings(program: WebGLProgram, varyings: Iterable<string>, mode: number): void {
    const id = this.#id(program, WebGLProgram)
    if (!id) return
    program.varyings = Array.from(varyings, String)
    program.bufferMode = mode
    if (this[CONTEXT] >= 0) native.transformFeedbackVaryings!(this[CONTEXT], id, program.varyings, mode)
  }
  getTransformFeedbackVarying(_program: WebGLProgram, _index: number): WebGLActiveInfo | null {
    this.#error(C.INVALID_VALUE)
    return null
  }

  getUniformBlockIndex(program: WebGLProgram, name: string): number {
    const id = this.#id(program, WebGLProgram)
    return id ? ((this[CONTEXT] < 0 ? null : native.uniformBlockIndex!(this[CONTEXT], id, String(name))) as number) : C.INVALID_INDEX
  }
  uniformBlockBinding(program: WebGLProgram, index: number, binding: number): void {
    const id = this.#id(program, WebGLProgram)
    if (id) (this[CONTEXT] < 0 ? null : native.uniformBlockBinding!(this[CONTEXT], id, index, binding))
  }
  getActiveUniformBlockName(program: WebGLProgram, index: number): string | null {
    const id = this.#id(program, WebGLProgram)
    const info = id ? ((this[CONTEXT] < 0 ? null : native.blockInfo!(this[CONTEXT], id, index)) as { name: string } | null) : null
    if (!info) this.#error(C.INVALID_VALUE)
    return info?.name ?? null
  }
  getActiveUniformBlockParameter(program: WebGLProgram, index: number, name: number): unknown {
    const id = this.#id(program, WebGLProgram)
    const info = id ? ((this[CONTEXT] < 0 ? null : native.blockInfo!(this[CONTEXT], id, index)) as { size: number; binding: number; count: number; indices: Uint32Array; vertex: boolean; fragment: boolean } | null) : null
    if (!info) return this.#error(C.INVALID_VALUE), null
    switch (name) {
      case C.UNIFORM_BLOCK_BINDING:
        return info.binding
      case C.UNIFORM_BLOCK_DATA_SIZE:
        return info.size
      case C.UNIFORM_BLOCK_ACTIVE_UNIFORMS:
        return info.count
      case C.UNIFORM_BLOCK_ACTIVE_UNIFORM_INDICES:
        return info.indices
      case C.UNIFORM_BLOCK_REFERENCED_BY_VERTEX_SHADER:
        return info.vertex
      case C.UNIFORM_BLOCK_REFERENCED_BY_FRAGMENT_SHADER:
        return info.fragment
    }
    this.#error(C.INVALID_ENUM)
    return null
  }
  getUniformIndices(program: WebGLProgram, names: Iterable<string>): number[] | null {
    const id = this.#id(program, WebGLProgram)
    return id ? ((this[CONTEXT] < 0 ? null : native.uniformIndices!(this[CONTEXT], id, Array.from(names, String))) as number[] | null) : null
  }
  getActiveUniforms(program: WebGLProgram, indices: Iterable<number>, name: number): unknown[] | null {
    const id = this.#id(program, WebGLProgram)
    if (!id) return null
    const field: Record<number, string> = {
      [C.UNIFORM_TYPE]: 'type',
      [C.UNIFORM_SIZE]: 'size',
      [C.UNIFORM_BLOCK_INDEX]: 'block',
      [C.UNIFORM_OFFSET]: 'offset',
      [C.UNIFORM_ARRAY_STRIDE]: 'arrayStride',
      [C.UNIFORM_MATRIX_STRIDE]: 'matrixStride',
    }
    if (name === C.UNIFORM_IS_ROW_MAJOR) return Array.from(indices, () => false)
    const key = field[name]
    if (!key) return this.#error(C.INVALID_ENUM), null
    const out: unknown[] = []
    for (const i of indices) {
      const m = (this[CONTEXT] < 0 ? null : native.blockMember!(this[CONTEXT], id, i)) as Record<string, number> | null
      if (!m) return this.#error(C.INVALID_VALUE), null
      out.push(m[key])
    }
    return out
  }

  vertexAttribDivisorANGLE(index: number, divisor: number): void {
    this.vertexAttribDivisor(index, divisor)
  }
}
constants(WebGL2RenderingContext, { ...WEBGL1, ...WEBGL2 })
tag(WebGL2RenderingContext)

function plain(name: string, table: Record<string, number> = {}): object {
  const ext = Object.create(null)
  for (const [k, v] of Object.entries(table)) Object.defineProperty(ext, k, { value: v, enumerable: true })
  Object.defineProperty(ext, Symbol.toStringTag, { value: name })
  return ext
}

function extension(gl: WebGLContextBase, name: string): object {
  switch (name) {
    case 'ANGLE_instanced_arrays':
      return Object.assign(plain('ANGLEInstancedArrays', { VERTEX_ATTRIB_ARRAY_DIVISOR_ANGLE: 0x88fe }), {
        drawArraysInstancedANGLE: (m: number, f: number, c: number, p: number) => webglHooks.draw(gl, m, f, c, p, 0, 0),
        drawElementsInstancedANGLE: (m: number, c: number, t: number, o: number, p: number) => webglHooks.draw(gl, m, 0, c, p, t, o),
        vertexAttribDivisorANGLE: (i: number, d: number) => { if (gl[CONTEXT] >= 0) native.vertexAttribDivisor!(gl[CONTEXT], i, d) },
      })
    case 'OES_vertex_array_object':
      return Object.assign(plain('OESVertexArrayObject', { VERTEX_ARRAY_BINDING_OES: 0x85b5 }), {
        createVertexArrayOES: () => (gl as unknown as { _createVertexArrayOES(): unknown })._createVertexArrayOES(),
        deleteVertexArrayOES: (v: WebGLObject | null) => gl.deleteVertexArray(v),
        isVertexArrayOES: (v: WebGLObject | null) => gl.isVertexArray(v),
        bindVertexArrayOES: (v: WebGLObject | null) => gl.bindVertexArray(v),
      })
    case 'WEBGL_draw_buffers': {
      const table: Record<string, number> = { MAX_COLOR_ATTACHMENTS_WEBGL: 0x8cdf, MAX_DRAW_BUFFERS_WEBGL: 0x8824 }
      for (let i = 0; i < 16; i++) {
        table[`COLOR_ATTACHMENT${i}_WEBGL`] = 0x8ce0 + i
        table[`DRAW_BUFFER${i}_WEBGL`] = 0x8825 + i
      }
      return Object.assign(plain('WEBGLDrawBuffers', table), {
        drawBuffersWEBGL: (buffers: Iterable<number>) => { if (gl[CONTEXT] >= 0) native.drawBuffers!(gl[CONTEXT], new Uint8Array(new Uint32Array(Array.from(buffers)).buffer)) },
      })
    }
    case 'WEBGL_lose_context':
      return Object.assign(plain('WEBGLLoseContext'), {
        loseContext: () => {
          if (gl[CONTEXT] >= 0) native.loseContext!(gl[CONTEXT])
          contextHooks.lose(gl)
        },
        // restoring asks the GPU device again, in a later task (as browsers do); a context that is not lost is an error
        restoreContext: () => {
          if (gl[CONTEXT] >= 0) return native.error(gl[CONTEXT], C.INVALID_OPERATION)
          host.setTimer(0, () => contextHooks.restore(gl), true)
        },
      })
    case 'WEBGL_multi_draw':
      return Object.assign(plain('WEBGLMultiDraw'), {
        multiDrawArraysWEBGL: (m: number, f: ArrayLike<number>, fo: number, c: ArrayLike<number>, co: number, n: number) => {
          for (let i = 0; i < n; i++) webglHooks.draw(gl, m, (f[fo + i] ?? 0), (c[co + i] ?? 0), 1, 0, 0)
        },
        multiDrawElementsWEBGL: (m: number, c: ArrayLike<number>, co: number, t: number, o: ArrayLike<number>, oo: number, n: number) => {
          for (let i = 0; i < n; i++) webglHooks.draw(gl, m, 0, (c[co + i] ?? 0), 1, t, (o[oo + i] ?? 0))
        },
        multiDrawArraysInstancedWEBGL: (m: number, f: ArrayLike<number>, fo: number, c: ArrayLike<number>, co: number, p: ArrayLike<number>, po: number, n: number) => {
          for (let i = 0; i < n; i++) webglHooks.draw(gl, m, (f[fo + i] ?? 0), (c[co + i] ?? 0), (p[po + i] ?? 0), 0, 0)
        },
        multiDrawElementsInstancedWEBGL: (m: number, c: ArrayLike<number>, co: number, t: number, o: ArrayLike<number>, oo: number, p: ArrayLike<number>, po: number, n: number) => {
          for (let i = 0; i < n; i++) webglHooks.draw(gl, m, 0, (c[co + i] ?? 0), (p[po + i] ?? 0), t, (o[oo + i] ?? 0))
        },
      })
    case 'OES_texture_half_float':
      return plain('OESTextureHalfFloat', { HALF_FLOAT_OES: 0x8d61 })
    case 'WEBGL_depth_texture':
      return plain('WEBGLDepthTexture', { UNSIGNED_INT_24_8_WEBGL: 0x84fa })
    case 'EXT_blend_minmax':
      return plain('EXTBlendMinMax', { MIN_EXT: 0x8007, MAX_EXT: 0x8008 })
    case 'EXT_texture_filter_anisotropic':
      return plain('EXTTextureFilterAnisotropic', { TEXTURE_MAX_ANISOTROPY_EXT: 0x84fe, MAX_TEXTURE_MAX_ANISOTROPY_EXT: 0x84ff })
    case 'WEBGL_debug_renderer_info':
      return plain('WEBGLDebugRendererInfo', { UNMASKED_VENDOR_WEBGL: 0x9245, UNMASKED_RENDERER_WEBGL: 0x9246 })
    case 'EXT_sRGB':
      return plain('EXTsRGB', { SRGB_EXT: 0x8c40, SRGB_ALPHA_EXT: 0x8c42, SRGB8_ALPHA8_EXT: 0x8c43, FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING_EXT: 0x8210 })
    case 'OES_standard_derivatives':
      return plain('OESStandardDerivatives', { FRAGMENT_SHADER_DERIVATIVE_HINT_OES: 0x8b8b })
    case 'EXT_color_buffer_half_float':
      return plain('EXTColorBufferHalfFloat', { RGBA16F_EXT: 0x881a, RGB16F_EXT: 0x881b, FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE_EXT: 0x8211, UNSIGNED_NORMALIZED_EXT: 0x8c17 })
    case 'WEBGL_color_buffer_float':
      return plain('WEBGLColorBufferFloat', { RGBA32F_EXT: 0x8814, FRAMEBUFFER_ATTACHMENT_COMPONENT_TYPE_EXT: 0x8211, UNSIGNED_NORMALIZED_EXT: 0x8c17 })
    case 'KHR_parallel_shader_compile':
      return plain('KHRParallelShaderCompile', { COMPLETION_STATUS_KHR: 0x91b1 })
    case 'EXT_texture_norm16':
      return plain('EXTTextureNorm16', { R16_EXT: 0x822a, RG16_EXT: 0x822c, RGB16_EXT: 0x8054, RGBA16_EXT: 0x805b, R16_SNORM_EXT: 0x8f98, RG16_SNORM_EXT: 0x8f99, RGB16_SNORM_EXT: 0x8f9a, RGBA16_SNORM_EXT: 0x8f9b })
    case 'WEBGL_compressed_texture_s3tc':
      return plain('WEBGLCompressedTextureS3TC', { COMPRESSED_RGB_S3TC_DXT1_EXT: 0x83f0, COMPRESSED_RGBA_S3TC_DXT1_EXT: 0x83f1, COMPRESSED_RGBA_S3TC_DXT3_EXT: 0x83f2, COMPRESSED_RGBA_S3TC_DXT5_EXT: 0x83f3 })
    case 'WEBGL_compressed_texture_s3tc_srgb':
      return plain('WEBGLCompressedTextureS3TCsRGB', { COMPRESSED_SRGB_S3TC_DXT1_EXT: 0x8c4c, COMPRESSED_SRGB_ALPHA_S3TC_DXT1_EXT: 0x8c4d, COMPRESSED_SRGB_ALPHA_S3TC_DXT3_EXT: 0x8c4e, COMPRESSED_SRGB_ALPHA_S3TC_DXT5_EXT: 0x8c4f })
    case 'EXT_texture_compression_rgtc':
      return plain('EXTTextureCompressionRGTC', { COMPRESSED_RED_RGTC1_EXT: 0x8dbb, COMPRESSED_SIGNED_RED_RGTC1_EXT: 0x8dbc, COMPRESSED_RED_GREEN_RGTC2_EXT: 0x8dbd, COMPRESSED_SIGNED_RED_GREEN_RGTC2_EXT: 0x8dbe })
    case 'EXT_texture_compression_bptc':
      return plain('EXTTextureCompressionBPTC', { COMPRESSED_RGBA_BPTC_UNORM_EXT: 0x8e8c, COMPRESSED_SRGB_ALPHA_BPTC_UNORM_EXT: 0x8e8d, COMPRESSED_RGB_BPTC_SIGNED_FLOAT_EXT: 0x8e8e, COMPRESSED_RGB_BPTC_UNSIGNED_FLOAT_EXT: 0x8e8f })
    case 'WEBGL_compressed_texture_etc':
      return plain('WEBGLCompressedTextureETC', {
        COMPRESSED_R11_EAC: 0x9270, COMPRESSED_SIGNED_R11_EAC: 0x9271, COMPRESSED_RG11_EAC: 0x9272, COMPRESSED_SIGNED_RG11_EAC: 0x9273, COMPRESSED_RGB8_ETC2: 0x9274, COMPRESSED_SRGB8_ETC2: 0x9275,
        COMPRESSED_RGB8_PUNCHTHROUGH_ALPHA1_ETC2: 0x9276, COMPRESSED_SRGB8_PUNCHTHROUGH_ALPHA1_ETC2: 0x9277, COMPRESSED_RGBA8_ETC2_EAC: 0x9278, COMPRESSED_SRGB8_ALPHA8_ETC2_EAC: 0x9279,
      })
    case 'WEBGL_compressed_texture_astc': {
      const sizes = ['4x4', '5x4', '5x5', '6x5', '6x6', '8x5', '8x6', '8x8', '10x5', '10x6', '10x8', '10x10', '12x10', '12x12']
      const table: Record<string, number> = {}
      sizes.forEach((s, i) => {
        table[`COMPRESSED_RGBA_ASTC_${s}_KHR`] = 0x93b0 + i
        table[`COMPRESSED_SRGB8_ALPHA8_ASTC_${s}_KHR`] = 0x93d0 + i
      })
      return Object.assign(plain('WEBGLCompressedTextureASTC', table), { getSupportedProfiles: () => ['ldr'] })
    }
    default:
      return plain(name.replace(/_/g, ''))
  }
}

export const webglContexts = {
  has(canvas: object): boolean {
    return byCanvas.has(canvas)
  },
  of(canvas: CanvasLike, node: number, kind: string, options: unknown): WebGLContextBase | null {
    const webgl2 = kind === 'webgl2'
    const existing = byCanvas.get(canvas)
    if (existing) return (existing instanceof WebGL2RenderingContext) === webgl2 ? existing : null
    if (!native) return null
    const o = (options && typeof options === 'object' ? options : {}) as Record<string, unknown>
    const flag = (name: string, value: boolean) => (o[name] === undefined ? value : !!o[name])
    const attributes = {
      alpha: flag('alpha', true),
      depth: flag('depth', true),
      stencil: flag('stencil', false),
      antialias: flag('antialias', true),
      premultipliedAlpha: flag('premultipliedAlpha', true),
      preserveDrawingBuffer: flag('preserveDrawingBuffer', false),
      powerPreference: typeof o.powerPreference === 'string' ? o.powerPreference : 'default',
      failIfMajorPerformanceCaveat: flag('failIfMajorPerformanceCaveat', false),
      desynchronized: flag('desynchronized', false),
      xrCompatible: false,
    }
    const id = native.create(node, webgl2, attributes, Math.max(1, canvas.width), Math.max(1, canvas.height))
    if (id == null) {
      fire(canvas as unknown as EventTarget, new WebGLContextEvent('webglcontextcreationerror', { statusMessage: 'no GPU' }))
      return null
    }
    const Kind = webgl2 ? WebGL2RenderingContext : WebGLRenderingContext
    const gl = new Kind(TOKEN, canvas, id, webgl2, attributes)
    byCanvas.set(canvas, gl)
    contextNodes.set(gl, node)
    live.add(gl)
    return gl
  },
  resized(canvas: object): void {
    const gl = byCanvas.get(canvas)
    if (gl) contextHooks.resize(gl)
  },
  present(): void {
    let shown = false
    for (const gl of live) shown = contextHooks.present(gl) || shown
    if (shown) gpuFrames.submitted(native.done())
  },
  endTask(): void {
    epoch++
  },
  drawn(): boolean {
    for (const gl of live) if (contextHooks.drawn(gl)) return true
    return false
  },
  pixels(canvas: object): { width: number; height: number; rgba: Uint8Array } | null | undefined {
    const gl = byCanvas.get(canvas)
    return gl && contextHooks.pixels(gl)
  },
  png(canvas: object): Uint8Array | null | undefined {
    const gl = byCanvas.get(canvas)
    return gl && contextHooks.png(gl)
  },
}

export function installWebgl(webgl: WebglNative, request: () => void): Record<string, unknown> {
  native = webgl
  contextHooks.request = request
  canvasSources.present = (canvas) => {
    const gl = byCanvas.get(canvas)
    if (gl) contextHooks.present(gl)
  }
  native.onLost((id) => {
    for (const gl of live) if (contextHooks.id(gl) === id) contextHooks.lose(gl)
  })
  native.watch?.((kind) => {
    if (kind === 'restored') for (const gl of live) contextHooks.restore(gl)
  })
  return {
    WebGLRenderingContext,
    WebGL2RenderingContext,
    WebGLObject,
    WebGLBuffer,
    WebGLTexture,
    WebGLFramebuffer,
    WebGLRenderbuffer,
    WebGLShader,
    WebGLProgram,
    WebGLUniformLocation,
    WebGLActiveInfo,
    WebGLShaderPrecisionFormat,
    WebGLVertexArrayObject,
    WebGLQuery,
    WebGLSampler,
    WebGLSync,
    WebGLTransformFeedback,
    WebGLContextEvent,
  }
}
