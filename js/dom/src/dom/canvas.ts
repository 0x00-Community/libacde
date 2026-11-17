import { serializable } from '../core/clone'
import { DOMException } from '../core/exception'
import { host, tag } from '../core/host'
import { DOMMatrix } from './geometry'
import type { Element } from './element'
import { dom, ID } from './realm'

const finite = Number.isFinite

export interface CanvasElement extends Element {
  width: number
  height: number
}

export class TextMetrics {
  readonly width: number
  readonly actualBoundingBoxLeft: number
  readonly actualBoundingBoxRight: number
  readonly actualBoundingBoxAscent: number
  readonly actualBoundingBoxDescent: number
  readonly fontBoundingBoxAscent: number
  readonly fontBoundingBoxDescent: number
  readonly emHeightAscent: number
  readonly emHeightDescent: number
  readonly hangingBaseline = 0
  readonly alphabeticBaseline = 0
  readonly ideographicBaseline = 0

  constructor(m: ArrayLike<number>) {
    this.width = m[0] ?? 0
    this.actualBoundingBoxLeft = m[1] ?? 0
    this.actualBoundingBoxRight = m[2] ?? 0
    this.actualBoundingBoxAscent = m[3] ?? 0
    this.actualBoundingBoxDescent = m[4] ?? 0
    this.fontBoundingBoxAscent = m[5] ?? 0
    this.fontBoundingBoxDescent = m[6] ?? 0
    this.emHeightAscent = this.fontBoundingBoxAscent
    this.emHeightDescent = this.fontBoundingBoxDescent
  }
}
tag(TextMetrics)

export const canvasSources = {
  present(_canvas: Element): void {},
  resolve(image: unknown): unknown {
    return image
  },
}

export class ImageData {
  readonly width: number
  readonly height: number
  readonly data: Uint8ClampedArray
  readonly colorSpace = 'srgb'

  constructor(a: number | Uint8ClampedArray, b: number, c?: number) {
    if (typeof a === 'number') {
      if (!(a > 0) || !(b > 0)) throw new DOMException('The source width or height is 0.', 'IndexSizeError')
      this.width = a
      this.height = b
      this.data = new Uint8ClampedArray(a * b * 4)
    } else {
      const height = c ?? a.length / 4 / b
      if (a.length !== b * height * 4) throw new DOMException('The input data length is not equal to (4 * width * height).', 'IndexSizeError')
      this.data = a
      this.width = b
      this.height = height
    }
  }
}
tag(ImageData)

type Rgba = [number, number, number, number]

const colors = new Map<string, Rgba | null>()

const FILTERS: Record<string, number> = {
  blur: 0, brightness: 1, contrast: 2, grayscale: 3, 'hue-rotate': 4, invert: 5, opacity: 6, saturate: 7, sepia: 8, 'drop-shadow': 9,
}

function parseFilter(text: string): number[][] | null {
  let rest = text.trim()
  if (rest.toLowerCase() === 'none') return []
  const out: number[][] = []
  while (rest) {
    const open = /^([a-z-]+)\(/i.exec(rest)
    if (!open) return null
    const kind = FILTERS[open[1]!.toLowerCase()]
    if (kind === undefined) return null
    let depth = 0
    let end = -1
    for (let i = open[0].length - 1; i < rest.length && end < 0; i++) {
      if (rest[i] === '(') depth++
      else if (rest[i] === ')' && --depth === 0) end = i
    }
    if (end < 0) return null
    const op = filterOp(kind, rest.slice(open[0].length, end).trim())
    if (!op) return null
    out.push(op)
    rest = rest.slice(end + 1).trim()
  }
  return out.length ? out : null
}

function filterLength(text: string): number | null {
  const m = /^(-?[\d.]+(?:e-?\d+)?)(px)?$/i.exec(text)
  if (!m || (!m[2] && Number(m[1]) !== 0)) return null
  const v = Number(m[1])
  return Number.isFinite(v) ? v : null
}

function filterOp(kind: number, args: string): number[] | null {
  if (kind === 9) {
    const tokens = args.match(/[^\s(]+(?:\([^)]*\))?|\([^)]*\)/g) ?? []
    const lengths: number[] = []
    let rgba: Rgba | null = null
    let closed = false
    for (const token of tokens) {
      const length = filterLength(token)
      if (length === null) {
        if (rgba) return null
        rgba = parseColor(token)
        if (!rgba) return null
        closed = lengths.length > 0
      } else {
        if (closed) return null
        lengths.push(length)
      }
    }
    if (lengths.length < 2 || lengths.length > 3 || (lengths[2] ?? 0) < 0) return null
    return [9, lengths[0]!, lengths[1]!, lengths[2] ?? 0, ...(rgba ?? [0, 0, 0, 1])]
  }
  if (kind === 0) {
    const sigma = args ? filterLength(args) : 0
    return sigma === null || sigma < 0 ? null : [0, sigma]
  }
  if (kind === 4) {
    if (!args) return [4, 0]
    const m = /^(-?[\d.]+)(deg|grad|rad|turn)?$/i.exec(args)
    if (!m || (!m[2] && Number(m[1]) !== 0)) return null
    const turns = { deg: 1 / 360, grad: 1 / 400, rad: 1 / (2 * Math.PI), turn: 1 }[(m[2] ?? 'deg').toLowerCase() as 'deg']
    return [4, Number(m[1]) * turns * 2 * Math.PI]
  }
  let amount = 1
  if (args) {
    const m = /^([\d.]+(?:e-?\d+)?)(%)?$/.exec(args)
    if (!m || !Number.isFinite(Number(m[1]))) return null
    amount = Number(m[1]) / (m[2] ? 100 : 1)
  }
  if (kind === 3 || kind === 5 || kind === 6 || kind === 8) amount = Math.min(amount, 1)
  return [kind, amount]
}

function parseColor(text: string): Rgba | null {
  let rgba = colors.get(text)
  if (rgba === undefined) {
    const parsed = dom.parseColor(text)
    rgba = parsed ? [parsed[0]!, parsed[1]!, parsed[2]!, parsed[3]!] : null
    if (colors.size > 1024) colors.clear()
    colors.set(text, rgba)
  }
  return rgba
}

const PAINT = Symbol('paint')
const COMMANDS = Symbol('commands')

interface GradientPaint {
  version: number
  readonly kind: 1 | 2
  readonly coords: readonly number[]
  readonly stops: [number, Rgba][]
}

export class CanvasGradient {
  readonly [PAINT]: GradientPaint

  constructor(kind: 1 | 2, coords: readonly number[]) {
    this[PAINT] = { version: 0, kind, coords, stops: [] }
  }

  addColorStop(offset: number, color: string): void {
    const rgba = parseColor(String(color))
    if (!rgba || !(offset >= 0 && offset <= 1)) throw new DOMException(`addColorStop(${offset}, ${color})`, 'SyntaxError')
    const paint = this[PAINT]
    paint.stops.push([offset, rgba])
    paint.stops.sort((a, b) => a[0] - b[0])
    paint.version++
  }
}

class CommandWriter {
  #buf = new Uint8Array(1024)
  #view = new DataView(this.#buf.buffer)
  len = 0

  u8(v: number): this {
    this.#reserve(1)
    this.#buf[this.len++] = v
    return this
  }
  u32(v: number): this {
    this.#reserve(4)
    this.#view.setUint32(this.len, v, true)
    this.len += 4
    return this
  }
  f(v: number): this {
    this.#reserve(4)
    this.#view.setFloat32(this.len, v, true)
    this.len += 4
    return this
  }
  f32(...vs: number[]): this {
    this.#reserve(4 * vs.length)
    for (const v of vs) {
      this.#view.setFloat32(this.len, v, true)
      this.len += 4
    }
    return this
  }
  str(s: string): this {
    const bytes = host.encodeUtf8(s)
    this.#reserve(4 + bytes.length)
    this.#view.setUint32(this.len, bytes.length, true)
    this.#buf.set(bytes, this.len + 4)
    this.len += 4 + bytes.length
    return this
  }
  bytes(b: Uint8Array): this {
    this.#reserve(b.length)
    this.#buf.set(b, this.len)
    this.len += b.length
    return this
  }
  take(): Uint8Array {
    const out = this.#buf.slice(0, this.len)
    this.len = 0
    return out
  }
  #reserve(n: number): void {
    if (this.len + n <= this.#buf.length) return
    const grown = new Uint8Array(Math.max(this.#buf.length * 2, this.len + n))
    grown.set(this.#buf.subarray(0, this.len))
    this.#buf = grown
    this.#view = new DataView(grown.buffer)
  }
}

type Matrix = [number, number, number, number, number, number]

const REPEATS = ['repeat', 'repeat-x', 'repeat-y', 'no-repeat']

interface PatternPaint {
  version: number
  readonly source: Element
  readonly repetition: number
  m: Matrix
}

export class CanvasPattern {
  readonly [PAINT]: PatternPaint

  constructor(source: Element, repetition: number) {
    this[PAINT] = { version: 0, source, repetition, m: [1, 0, 0, 1, 0, 0] }
  }

  setTransform(t: { a?: number; b?: number; c?: number; d?: number; e?: number; f?: number } = {}): void {
    const paint = this[PAINT]
    paint.m = [t.a ?? 1, t.b ?? 0, t.c ?? 0, t.d ?? 1, t.e ?? 0, t.f ?? 0]
    paint.version++
  }
}

type Style = string | CanvasGradient | CanvasPattern | object

interface State {
  m: Matrix
  fill: Style
  stroke: Style
  lineWidth: number
  cap: string
  join: string
  miter: number
  dash: number[]
  dashOffset: number
  alpha: number
  gco: string
  font: string
  align: string
  baseline: string
  direction: string
  shadow: { color: string; rgba: Rgba; blur: number; x: number; y: number }
  filter: string
  filters: number[][]
}

const M = 1, FILL = 2, STROKE = 4, LINE_WIDTH = 8, LINE = 16, DASH = 32, ALPHA = 64, GCO = 128, FONT = 256, TEXT = 512, SHADOW = 1024, FILTER = 2048
const ALL = 4095
const LINES = LINE_WIDTH | LINE | DASH
const NEEDS_FILL = M | FILL | ALPHA | GCO | SHADOW | FILTER
const NEEDS_STROKE = M | STROKE | LINES | ALPHA | GCO | SHADOW | FILTER
const NEEDS_IMAGE = M | ALPHA | GCO | SHADOW | FILTER

interface Sent {
  dirty: number
  fill: number
  stroke: number
}

function styleVersion(style: Style): number {
  return style instanceof CanvasGradient || style instanceof CanvasPattern ? style[PAINT].version : -1
}

const Sub = {
  Save: 1, Restore: 2, SetTransform: 3, FillStyle: 4, StrokeStyle: 5, LineWidth: 6, Line: 7, Dash: 8, Alpha: 9, Composite: 10, Font: 11, Text: 12, Shadow: 13, Filter: 14,
  BeginPath: 20, MoveTo: 21, LineTo: 22, Quadratic: 23, Bezier: 24, Arc: 25, ArcTo: 26, Ellipse: 27, Rect: 28, RoundRect: 29, ClosePath: 30, SvgPath: 31,
  Fill: 40, Stroke: 41, Clip: 42, Rects: 43, DrawText: 44, DrawImage: 45, PutImageData: 46,
} as const

const CAPS = ['butt', 'round', 'square'], JOINS = ['miter', 'round', 'bevel']
const ALIGNS = ['start', 'end', 'left', 'right', 'center'], BASELINES = ['alphabetic', 'top', 'hanging', 'middle', 'ideographic', 'bottom']
const DIRECTIONS = ['inherit', 'ltr', 'rtl']
const COMPOSITES = ['source-over', 'source-in', 'source-out', 'source-atop', 'destination-over', 'destination-in', 'destination-out', 'destination-atop',
  'lighter', 'copy', 'xor', 'multiply', 'screen', 'overlay', 'darken', 'lighten', 'color-dodge', 'color-burn', 'hard-light', 'soft-light',
  'difference', 'exclusion', 'hue', 'saturation', 'color', 'luminosity']

let takeCommands: (context: CanvasContext2d) => Uint8Array
let hasCommands: (context: CanvasContext2d) => boolean

export class CanvasContext2d {
  #state: State = CanvasContext2d.#initial()
  #stack: State[] = []
  #sent: Sent = { dirty: ALL, fill: -1, stroke: -1 }
  #sentStack: Sent[] = []
  readonly #out = new CommandWriter()
  #size: [number, number]
  imageSmoothingEnabled = true
  imageSmoothingQuality = 'low'

  constructor(readonly canvas: CanvasElement) {
    this.#size = [canvas.width, canvas.height]
  }

  static {
    takeCommands = (context) => context.#out.take()
    hasCommands = (context) => context.#out.len > 0
  }

  static #initial(): State {
    return { m: [1, 0, 0, 1, 0, 0], fill: '#000000', stroke: '#000000', lineWidth: 1, cap: 'butt', join: 'miter', miter: 10, dash: [], dashOffset: 0, alpha: 1, gco: 'source-over', font: '10px sans-serif', align: 'start', baseline: 'alphabetic', direction: 'inherit', shadow: { color: 'rgba(0, 0, 0, 0)', rgba: [0, 0, 0, 0], blur: 0, x: 0, y: 0 }, filter: 'none', filters: [] }
  }

  #changed(bits: number): void {
    this.#sent.dirty |= bits
  }
  #shadow(key: 'blur' | 'x' | 'y', v: number): void {
    if (Number.isFinite(v) && (key !== 'blur' || v >= 0) && v !== this.#state.shadow[key]) {
      this.#state.shadow = { ...this.#state.shadow, [key]: v }
      this.#changed(SHADOW)
    }
  }
  #set<K extends keyof State>(key: K, v: State[K], bits: number, valid: () => unknown): void {
    if (v !== this.#state[key] && valid()) {
      this.#state[key] = v
      this.#changed(bits)
    }
  }

  get filter(): string {
    return this.#state.filter
  }
  set filter(v: string) {
    const text = String(v)
    const filters = parseFilter(text)
    if (!filters || text === this.#state.filter) return
    this.#state.filter = text
    this.#state.filters = filters
    this.#changed(FILTER)
  }

  get shadowColor(): string {
    return this.#state.shadow.color
  }
  set shadowColor(v: string) {
    const rgba = parseColor(String(v))
    if (rgba) {
      this.#state.shadow = { ...this.#state.shadow, color: String(v), rgba }
      this.#changed(SHADOW)
    }
  }
  get shadowBlur(): number {
    return this.#state.shadow.blur
  }
  set shadowBlur(v: number) {
    this.#shadow('blur', v)
  }
  get shadowOffsetX(): number {
    return this.#state.shadow.x
  }
  set shadowOffsetX(v: number) {
    this.#shadow('x', v)
  }
  get shadowOffsetY(): number {
    return this.#state.shadow.y
  }
  set shadowOffsetY(v: number) {
    this.#shadow('y', v)
  }

  get fillStyle(): Style {
    return this.#state.fill
  }
  set fillStyle(v: Style) {
    this.#set('fill', v, FILL, () => (typeof v === 'string' ? parseColor(v) : v instanceof CanvasGradient || v instanceof CanvasPattern))
  }
  get strokeStyle(): Style {
    return this.#state.stroke
  }
  set strokeStyle(v: Style) {
    this.#set('stroke', v, STROKE, () => (typeof v === 'string' ? parseColor(v) : v instanceof CanvasGradient || v instanceof CanvasPattern))
  }
  get lineWidth(): number {
    return this.#state.lineWidth
  }
  set lineWidth(v: number) {
    this.#set('lineWidth', v, LINE_WIDTH, () => v > 0 && Number.isFinite(v))
  }
  get lineCap(): string {
    return this.#state.cap
  }
  set lineCap(v: string) {
    this.#set('cap', v, LINE, () => CAPS.includes(v))
  }
  get lineJoin(): string {
    return this.#state.join
  }
  set lineJoin(v: string) {
    this.#set('join', v, LINE, () => JOINS.includes(v))
  }
  get miterLimit(): number {
    return this.#state.miter
  }
  set miterLimit(v: number) {
    this.#set('miter', v, LINE, () => v > 0 && Number.isFinite(v))
  }
  get lineDashOffset(): number {
    return this.#state.dashOffset
  }
  set lineDashOffset(v: number) {
    this.#set('dashOffset', v, DASH, () => Number.isFinite(v))
  }
  setLineDash(segments: number[]): void {
    if (segments.some((s) => !(s >= 0) || !Number.isFinite(s))) return
    this.#state.dash = segments.length % 2 ? [...segments, ...segments] : [...segments]
    this.#changed(DASH)
  }
  getLineDash(): number[] {
    return [...this.#state.dash]
  }
  get globalAlpha(): number {
    return this.#state.alpha
  }
  set globalAlpha(v: number) {
    this.#set('alpha', v, ALPHA, () => v >= 0 && v <= 1)
  }
  get globalCompositeOperation(): string {
    return this.#state.gco
  }
  set globalCompositeOperation(v: string) {
    this.#set('gco', v, GCO, () => COMPOSITES.includes(v))
  }
  get font(): string {
    return this.#state.font
  }
  set font(v: string) {
    this.#set('font', v, FONT, () => typeof v === 'string' && v.trim())
  }
  get textAlign(): string {
    return this.#state.align
  }
  set textAlign(v: string) {
    this.#set('align', v, TEXT, () => ALIGNS.includes(v))
  }
  get textBaseline(): string {
    return this.#state.baseline
  }
  set textBaseline(v: string) {
    this.#set('baseline', v, TEXT, () => BASELINES.includes(v))
  }
  get direction(): string {
    return this.#state.direction
  }
  set direction(v: string) {
    this.#set('direction', v, TEXT, () => DIRECTIONS.includes(v))
  }

  save(): void {
    this.#check()
    this.#stack.push({ ...this.#state, m: [...this.#state.m] as Matrix, dash: [...this.#state.dash] })
    this.#sentStack.push({ ...this.#sent })
    this.#cmd().u8(Sub.Save)
  }
  restore(): void {
    this.#check()
    const state = this.#stack.pop()
    if (!state) return
    this.#state = state
    this.#sent = this.#sentStack.pop() ?? { dirty: ALL, fill: -1, stroke: -1 }
    this.#cmd().u8(Sub.Restore)
  }
  reset(): void {
    this.#check()
    while (this.#stack.length) this.restore()
    this.#state = CanvasContext2d.#initial()
    this.#changed(ALL)
    this.#sync(M)
    this.#cmd().u8(Sub.Rects).u8(2).f32(0, 0, this.canvas.width, this.canvas.height)
  }

  setTransform(a: number | { a?: number; b?: number; c?: number; d?: number; e?: number; f?: number } = {}, b?: number, c?: number, d?: number, e?: number, f?: number): void {
    const m = typeof a === 'object' ? [a.a ?? 1, a.b ?? 0, a.c ?? 0, a.d ?? 1, a.e ?? 0, a.f ?? 0] : [a, b!, c!, d!, e!, f!]
    if (m.every(Number.isFinite)) {
      this.#state.m = m as Matrix
      this.#changed(M)
    }
  }
  resetTransform(): void {
    this.#state.m = [1, 0, 0, 1, 0, 0]
    this.#changed(M)
  }
  transform(a: number, b: number, c: number, d: number, e: number, f: number): void {
    if (![a, b, c, d, e, f].every(Number.isFinite)) return
    const [A, B, C, D, E, F] = this.#state.m
    this.#state.m = [A * a + C * b, B * a + D * b, A * c + C * d, B * c + D * d, A * e + C * f + E, B * e + D * f + F]
    this.#changed(M)
  }
  translate(x: number, y: number): void {
    this.transform(1, 0, 0, 1, x, y)
  }
  scale(x: number, y: number): void {
    this.transform(x, 0, 0, y, 0, 0)
  }
  rotate(angle: number): void {
    const cos = Math.cos(angle), sin = Math.sin(angle)
    this.transform(cos, sin, -sin, cos, 0, 0)
  }
  getTransform(): DOMMatrix {
    return new DOMMatrix(this.#state.m)
  }

  beginPath(): void {
    this.#check()
    this.#cmd().u8(Sub.BeginPath)
  }
  closePath(): void {
    this.#check()
    this.#cmd().u8(Sub.ClosePath)
  }
  moveTo(x: number, y: number): void {
    this.#path(Sub.MoveTo, finite(x) && finite(y))?.f(x).f(y)
  }
  lineTo(x: number, y: number): void {
    this.#path(Sub.LineTo, finite(x) && finite(y))?.f(x).f(y)
  }
  quadraticCurveTo(cx: number, cy: number, x: number, y: number): void {
    this.#path(Sub.Quadratic, finite(cx) && finite(cy) && finite(x) && finite(y))?.f(cx).f(cy).f(x).f(y)
  }
  bezierCurveTo(c1x: number, c1y: number, c2x: number, c2y: number, x: number, y: number): void {
    this.#path(Sub.Bezier, finite(c1x) && finite(c1y) && finite(c2x) && finite(c2y) && finite(x) && finite(y))?.f(c1x).f(c1y).f(c2x).f(c2y).f(x).f(y)
  }
  arc(x: number, y: number, r: number, a0: number, a1: number, ccw = false): void {
    if (r < 0) throw new DOMException(`arc radius ${r}`, 'IndexSizeError')
    this.#path(Sub.Arc, finite(x) && finite(y) && finite(r) && finite(a0) && finite(a1))?.f(x).f(y).f(r).f(a0).f(a1).u8(ccw ? 1 : 0)
  }
  arcTo(x1: number, y1: number, x2: number, y2: number, r: number): void {
    if (r < 0) throw new DOMException(`arcTo radius ${r}`, 'IndexSizeError')
    this.#path(Sub.ArcTo, finite(x1) && finite(y1) && finite(x2) && finite(y2) && finite(r))?.f(x1).f(y1).f(x2).f(y2).f(r)
  }
  ellipse(x: number, y: number, rx: number, ry: number, rotation: number, a0: number, a1: number, ccw = false): void {
    if (rx < 0 || ry < 0) throw new DOMException(`ellipse radii ${rx} ${ry}`, 'IndexSizeError')
    this.#path(Sub.Ellipse, finite(x) && finite(y) && finite(rx) && finite(ry) && finite(rotation) && finite(a0) && finite(a1))?.f(x).f(y).f(rx).f(ry).f(rotation).f(a0).f(a1).u8(ccw ? 1 : 0)
  }
  rect(x: number, y: number, w: number, h: number): void {
    this.#path(Sub.Rect, finite(x) && finite(y) && finite(w) && finite(h))?.f(x).f(y).f(w).f(h)
  }
  roundRect(x: number, y: number, w: number, h: number, radii: number | { x?: number } | (number | { x?: number })[] = 0): void {
    const list = (Array.isArray(radii) ? radii : [radii]).map((r) => (typeof r === 'number' ? r : (r?.x ?? 0)))
    const [tl, tr = tl, br = tl, bl = tr] = list.length === 3 ? [list[0]!, list[1]!, list[2]!, list[1]!] : (list as [number, number?, number?, number?])
    this.#path(Sub.RoundRect, finite(x) && finite(y) && finite(w) && finite(h) && finite(tl) && finite(tr) && finite(br) && finite(bl))?.f(x).f(y).f(w).f(h).f(tl!).f(tr!).f(br!).f(bl!)
  }

  fill(rule?: unknown, rule2?: string): void {
    this.#area(Sub.Fill, NEEDS_FILL, rule, rule2)
  }
  stroke(path?: unknown): void {
    if (path instanceof Path2D) {
      this.#replay(path)
      return this.stroke()
    }
    if (path !== undefined) return
    this.#check()
    this.#sync(NEEDS_STROKE)
    this.#cmd().u8(Sub.Stroke)
  }
  clip(rule?: unknown, rule2?: string): void {
    this.#area(Sub.Clip, M, rule, rule2)
  }
  #area(sub: number, needs: number, rule: unknown = 'nonzero', rule2?: string): void {
    if (rule instanceof Path2D) {
      this.#replay(rule)
      rule = rule2 ?? 'nonzero'
    }
    if (typeof rule !== 'string') return
    this.#check()
    this.#sync(needs)
    this.#cmd().u8(sub).u8(rule === 'evenodd' ? 1 : 0)
  }
  fillRect(x: number, y: number, w: number, h: number): void {
    this.#rects(0, x, y, w, h)
  }
  strokeRect(x: number, y: number, w: number, h: number): void {
    this.#rects(1, x, y, w, h)
  }
  clearRect(x: number, y: number, w: number, h: number): void {
    this.#rects(2, x, y, w, h)
  }
  fillText(text: string, x: number, y: number, maxWidth?: number): void {
    this.#text(0, text, x, y, maxWidth)
  }
  strokeText(text: string, x: number, y: number, maxWidth?: number): void {
    this.#text(1, text, x, y, maxWidth)
  }
  measureText(text: string): TextMetrics {
    return new TextMetrics(dom.measureText(this.canvas[ID], this.#state.font, String(text)))
  }
  drawImage(source: Element, ...a: number[]): void {
    this.#check()
    const image = canvasSources.resolve(source) as Element
    const tag = (image as Element | null)?.localName
    if (tag !== 'img' && tag !== 'canvas') return
    const id = image[ID]
    const iw = tag === 'canvas' ? (image as unknown as CanvasElement).width : (image as unknown as { naturalWidth: number }).naturalWidth || NaN
    const ih = tag === 'canvas' ? (image as unknown as CanvasElement).height : (image as unknown as { naturalHeight: number }).naturalHeight || NaN
    const [sx, sy, sw, sh, dx, dy, dw, dh] =
      a.length >= 8 ? a : a.length >= 4 ? [0, 0, iw, ih, a[0]!, a[1]!, a[2]!, a[3]!] : [0, 0, iw, ih, a[0]!, a[1]!, iw, ih]
    if (![sx, sy, dx, dy].every(Number.isFinite)) return
    if (tag === 'canvas') canvasSources.present(image)
    this.#sync(NEEDS_IMAGE)
    this.#cmd().u8(Sub.DrawImage).u8(tag === 'img' ? 0 : 1).u32(id).f32(sx!, sy!, sw!, sh!, dx!, dy!, dw!, dh!)
  }

  createLinearGradient(x0: number, y0: number, x1: number, y1: number): CanvasGradient {
    return new CanvasGradient(1, [x0, y0, x1, y1])
  }
  createRadialGradient(x0: number, y0: number, r0: number, x1: number, y1: number, r1: number): CanvasGradient {
    return new CanvasGradient(2, [x0, y0, r0, x1, y1, r1])
  }
  createConicGradient(): CanvasGradient {
    return new CanvasGradient(1, [0, 0, 0, 0])
  }
  createPattern(image: Element, repetition: string | null = 'repeat'): CanvasPattern | null {
    const tag = image?.localName
    if (tag !== 'img' && tag !== 'canvas') throw new TypeError('createPattern: an <img> or <canvas>')
    const rep = repetition === null || repetition === '' ? 0 : REPEATS.indexOf(String(repetition))
    if (rep < 0) throw new DOMException(`createPattern repetition ${repetition}`, 'SyntaxError')
    if (tag === 'canvas' && (!(image as unknown as CanvasElement).width || !(image as unknown as CanvasElement).height)) throw new DOMException('createPattern of an empty canvas', 'InvalidStateError')
    if (tag === 'img' && !(image as unknown as { complete: boolean }).complete) return null
    return new CanvasPattern(image, rep)
  }
  getImageData(sx: number, sy: number, sw: number, sh: number): ImageData {
    if (![sx, sy, sw, sh].every(Number.isFinite)) throw new TypeError('getImageData: non-finite argument')
    if (!sw || !sh) throw new DOMException('getImageData: zero width or height', 'IndexSizeError')
    const width = Math.abs(Math.trunc(sw)), height = Math.abs(Math.trunc(sh))
    const bytes = canvases.pixels(this.canvas, sx, sy, sw, sh)
    const data = bytes && bytes.length === width * height * 4 ? new Uint8ClampedArray(bytes.buffer, bytes.byteOffset, bytes.length) : new Uint8ClampedArray(width * height * 4)
    return new ImageData(data, width, height)
  }
  createImageData(w: number | { width: number; height: number }, h?: number): ImageData {
    const [width, height] = typeof w === 'number' ? [w, h ?? 0] : [w.width, w.height]
    return new ImageData(Math.abs(Math.trunc(width)), Math.abs(Math.trunc(height)))
  }
  putImageData(image: { width: number; height: number; data: ArrayLike<number> & { buffer?: ArrayBufferLike; byteOffset?: number; byteLength?: number } }, dx: number, dy: number, dirtyX = 0, dirtyY = 0, dirtyW?: number, dirtyH?: number): void {
    this.#check()
    const { width, height, data } = image
    if (![dx, dy, dirtyX, dirtyY].every(Number.isFinite) || data.length !== width * height * 4) return
    const raw = data.buffer ? new Uint8Array(data.buffer, data.byteOffset ?? 0, data.byteLength ?? data.length) : Uint8Array.from(data)
    this.#cmd().u8(Sub.PutImageData).f32(dx, dy, dirtyX, dirtyY, dirtyW ?? width, dirtyH ?? height).u32(width).u32(height).bytes(raw)
  }
  isPointInPath(): boolean {
    return false
  }

  #replay(path: Path2D): void {
    this.beginPath()
    for (const command of path[COMMANDS]) {
      if (command[0] === 'svg') this.#svgPath(command[1] as string, command.slice(2) as number[])
      else (this as unknown as Record<string, (...a: number[]) => void>)[command[0]]!(...(command.slice(1) as number[]))
    }
  }

  #svgPath(d: string, m: number[]): void {
    this.#check()
    if (!m.every(Number.isFinite)) return
    this.#sync(M)
    this.#cmd().u8(Sub.SvgPath).str(d).f32(...m)
  }
  isPointInStroke(): boolean {
    return false
  }
  getContextAttributes(): { alpha: boolean; desynchronized: boolean } {
    return { alpha: true, desynchronized: false }
  }
  drawFocusIfNeeded(): void {}

  #check(): void {
    const { width, height } = this.canvas
    if (width === this.#size[0] && height === this.#size[1]) return
    this.resized()
  }

  resized(): void {
    this.#size = [this.canvas.width, this.canvas.height]
    this.#state = CanvasContext2d.#initial()
    this.#stack = []
    this.#sent = { dirty: ALL, fill: -1, stroke: -1 }
    this.#sentStack = []
    this.#out.take()
  }

  #cmd(): CommandWriter {
    canvases.writing(this)
    return this.#out
  }

  #path(code: number, ok: boolean): CommandWriter | null {
    this.#check()
    if (!ok) return null
    this.#sync(M)
    return this.#cmd().u8(code)
  }

  #rects(kind: 0 | 1 | 2, x: number, y: number, w: number, h: number): void {
    this.#check()
    if (!(finite(x) && finite(y) && finite(w) && finite(h))) return
    this.#sync(kind === 0 ? NEEDS_FILL : kind === 1 ? NEEDS_STROKE : M)
    this.#cmd().u8(Sub.Rects).u8(kind).f(x).f(y).f(w).f(h)
  }

  #text(kind: 0 | 1, text: string, x: number, y: number, maxWidth?: number): void {
    this.#check()
    if (!(finite(x) && finite(y))) return
    this.#sync((kind ? NEEDS_STROKE : NEEDS_FILL) | FONT | TEXT)
    this.#cmd().u8(Sub.DrawText).u8(kind).str(String(text)).f32(x, y, maxWidth === undefined || !(maxWidth >= 0) ? NaN : maxWidth)
  }

  #sync(need: number): void {
    const s = this.#state, sent = this.#sent
    let send = need & sent.dirty
    if (need & FILL && !(send & FILL) && typeof s.fill !== 'string' && styleVersion(s.fill) !== sent.fill) send |= FILL
    if (need & STROKE && !(send & STROKE) && typeof s.stroke !== 'string' && styleVersion(s.stroke) !== sent.stroke) send |= STROKE
    if (send === 0) return
    sent.dirty &= ~send
    canvases.writing(this)
    const out = this.#out
    if (send & M) out.u8(Sub.SetTransform).f32(...s.m)
    if (send & FILL) {
      this.#paint(out.u8(Sub.FillStyle), s.fill)
      sent.fill = styleVersion(s.fill)
    }
    if (send & STROKE) {
      this.#paint(out.u8(Sub.StrokeStyle), s.stroke)
      sent.stroke = styleVersion(s.stroke)
    }
    if (send & LINE_WIDTH) out.u8(Sub.LineWidth).f32(s.lineWidth)
    if (send & LINE) out.u8(Sub.Line).u8(CAPS.indexOf(s.cap)).u8(JOINS.indexOf(s.join)).f32(s.miter)
    if (send & DASH) out.u8(Sub.Dash).u32(s.dash.length).f32(...s.dash, s.dashOffset)
    if (send & ALPHA) out.u8(Sub.Alpha).f32(s.alpha)
    if (send & GCO) out.u8(Sub.Composite).u8(COMPOSITES.indexOf(s.gco))
    if (send & FONT) out.u8(Sub.Font).str(s.font)
    if (send & SHADOW) {
      const sh = s.shadow
      out.u8(Sub.Shadow).f32(...sh.rgba, sh.blur, sh.x, sh.y)
    }
    if (send & FILTER) {
      out.u8(Sub.Filter).u32(s.filters.length)
      for (const [kind, ...values] of s.filters) out.u8(kind!).f32(...values)
    }
    if (send & TEXT) out.u8(Sub.Text).u8(ALIGNS.indexOf(s.align)).u8(BASELINES.indexOf(s.baseline)).u8(DIRECTIONS.indexOf(s.direction))
  }

  #paint(out: CommandWriter, style: Style): void {
    if (style instanceof CanvasPattern) {
      const { source, repetition, m } = style[PAINT]
      return void out.u8(3).u8(source.localName === 'img' ? 0 : 1).u32(source[ID]).u8(repetition).f32(...m)
    }
    if (style instanceof CanvasGradient) {
      const { kind, coords, stops } = style[PAINT]
      out.u8(kind).f32(...coords).u32(stops.length)
      for (const [offset, rgba] of stops) out.f32(offset, ...rgba)
      return
    }
    out.u8(0).f32(...((typeof style === 'string' && parseColor(style)) || [0, 0, 0, 0]))
  }
}

class CanvasContexts {
  readonly #contexts = new WeakMap<object, CanvasContext2d>()
  readonly #dirty = new Set<CanvasContext2d>()
  #writer: CanvasContext2d | null = null

  context(canvas: CanvasElement): CanvasContext2d {
    let context = this.#contexts.get(canvas)
    if (!context) this.#contexts.set(canvas, (context = new CanvasContext2d(canvas)))
    return context
  }

  has(canvas: CanvasElement): boolean {
    return this.#contexts.has(canvas)
  }

  resized(canvas: CanvasElement): void {
    this.#contexts.get(canvas)?.resized()
  }

  writing(context: CanvasContext2d): void {
    if (this.#writer !== context) {
      const last = this.#writer
      if (last && hasCommands(last)) this.#send(last)
      this.#writer = context
    }
    this.#dirty.add(context)
  }

  #send(context: CanvasContext2d): void {
    const bytes = takeCommands(context)
    if (bytes.length) dom.canvas(context.canvas[ID], bytes)
  }

  flush(): void {
    if (this.#dirty.size === 0) return
    for (const context of this.#dirty) if (hasCommands(context)) this.#send(context)
    this.#dirty.clear()
    this.#writer = null
  }

  pixels(canvas: CanvasElement, sx: number, sy: number, sw: number, sh: number): Uint8Array | null {
    this.flush()
    return dom.getImageData(canvas[ID], sx, sy, sw, sh)
  }

  toDataURL(canvas: CanvasElement, type: string): string {
    this.flush()
    return dom.toDataURL(canvas[ID], type)
  }
}

export const canvases = new CanvasContexts()

type PathCommand = [string, ...unknown[]]

export class Path2D {
  readonly [COMMANDS]: PathCommand[] = []

  constructor(init?: Path2D | string) {
    if (init instanceof Path2D) this[COMMANDS].push(...init[COMMANDS].map((c) => [...c] as PathCommand))
    else if (typeof init === 'string') this[COMMANDS].push(['svg', init, 1, 0, 0, 1, 0, 0])
  }

  addPath(path: Path2D, transform?: { a?: number; b?: number; c?: number; d?: number; e?: number; f?: number }): void {
    if (!transform) {
      this[COMMANDS].push(...path[COMMANDS])
      return
    }
    const m = DOMMatrix.fromMatrix(transform)
    const p = (x: number, y: number) => [m.a * x + m.c * y + m.e, m.b * x + m.d * y + m.f]
    for (const command of path[COMMANDS]) {
      if (command[0] === 'svg') {
        const [, d, a, b, c, dd, e, f] = command as [string, string, number, number, number, number, number, number]
        const t = m.multiply({ a, b, c, d: dd, e, f })
        this[COMMANDS].push(['svg', d, t.a, t.b, t.c, t.d, t.e, t.f])
        continue
      }
      const [op, ...a] = command as [string, ...number[]]
      if (op === 'moveTo' || op === 'lineTo') this[COMMANDS].push([op, ...p(a[0]!, a[1]!)])
      else if (op === 'quadraticCurveTo') this[COMMANDS].push([op, ...p(a[0]!, a[1]!), ...p(a[2]!, a[3]!)])
      else if (op === 'bezierCurveTo') this[COMMANDS].push([op, ...p(a[0]!, a[1]!), ...p(a[2]!, a[3]!), ...p(a[4]!, a[5]!)])
      else this[COMMANDS].push([op, ...a])
    }
  }

  roundRect(x: number, y: number, w: number, h: number, radii: unknown = 0): void {
    this[COMMANDS].push(['roundRect', x, y, w, h, Array.isArray(radii) ? [...radii] : radii])
  }
}
for (const op of ['closePath', 'moveTo', 'lineTo', 'quadraticCurveTo', 'bezierCurveTo', 'arc', 'arcTo', 'ellipse', 'rect']) {
  Object.defineProperty(Path2D.prototype, op, { value(this: Path2D, ...args: unknown[]) { this[COMMANDS].push([op, ...args]) }, writable: true, enumerable: true, configurable: true })
}
tag(Path2D)

serializable(ImageData, 'ImageData', (image) => [image.data.slice(), image.width, image.height], ([data, width, height]) => new ImageData(data as Uint8ClampedArray, width as number, height as number))
