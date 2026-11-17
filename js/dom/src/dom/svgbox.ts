import { NodeType } from '../native'
import { computedStyleOf } from './layout'
import { dom } from './realm'

export type Box = [number, number, number, number]
export type Affine = { a: number; b: number; c: number; d: number; e: number; f: number }

const IDENTITY: Affine = { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 }

function times(m: Affine, n: Affine): Affine {
  return {
    a: m.a * n.a + m.c * n.b,
    b: m.b * n.a + m.d * n.b,
    c: m.a * n.c + m.c * n.d,
    d: m.b * n.c + m.d * n.d,
    e: m.a * n.e + m.c * n.f + m.e,
    f: m.b * n.e + m.d * n.f + m.f,
  }
}

function viewBoxOf(id: number): Box | null {
  const box = (dom.getAttribute(id, 'viewBox') ?? '').trim().split(/[\s,]+/).map(Number)
  return box.length === 4 && box.every(Number.isFinite) && box[2]! > 0 && box[3]! > 0 ? (box as Box) : null
}

export function fitViewBox(svg: number, width: number, height: number): Affine {
  const box = viewBoxOf(svg)
  if (!box) return IDENTITY
  const [vx, vy, vw, vh] = box
  let sx = width / vw
  let sy = height / vh
  let tx = 0
  let ty = 0
  const aspect = (dom.getAttribute(svg, 'preserveAspectRatio') ?? 'xMidYMid meet').trim()
  if (!aspect.startsWith('none')) {
    const s = aspect.includes('slice') ? Math.max(sx, sy) : Math.min(sx, sy)
    const align = aspect.split(/\s+/)[0]!
    const free = (size: number, content: number, part: string) => (part === 'Min' ? 0 : part === 'Max' ? size - content : (size - content) / 2)
    tx = free(width, vw * s, /xMin/.test(align) ? 'Min' : /xMax/.test(align) ? 'Max' : 'Mid')
    ty = free(height, vh * s, /YMin/.test(align) ? 'Min' : /YMax/.test(align) ? 'Max' : 'Mid')
    sx = sy = s
  }
  return { a: sx, b: 0, c: 0, d: sy, e: tx - vx * sx, f: ty - vy * sy }
}

function nestedSvg(id: number): Affine {
  const size = (name: string, fallback: number) => {
    const text = (dom.getAttribute(id, name) ?? '').trim()
    const value = parseFloat(text)
    return text.endsWith('%') || !Number.isFinite(value) ? fallback : value
  }
  const box = viewBoxOf(id)
  const fit = fitViewBox(id, size('width', box?.[2] ?? 0), size('height', box?.[3] ?? 0))
  return times({ ...IDENTITY, e: length(id, 'x'), f: length(id, 'y') }, fit)
}

const CONTAINERS = new Set(['g', 'svg', 'a', 'switch'])
const NUMBER = /[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?/y
const SPACE = /[\s,]*/y

class Bounds {
  box: Box = [Infinity, Infinity, -Infinity, -Infinity]

  add(x: number, y: number): void {
    const b = this.box
    if (!Number.isFinite(x) || !Number.isFinite(y)) return
    b[0] = Math.min(b[0], x)
    b[1] = Math.min(b[1], y)
    b[2] = Math.max(b[2], x)
    b[3] = Math.max(b[3], y)
  }

  get empty(): boolean {
    return this.box[0] > this.box[2]
  }

  cubic(x0: number, y0: number, x1: number, y1: number, x2: number, y2: number, x3: number, y3: number): void {
    this.add(x3, y3)
    const at = (t: number, a: number, b: number, c: number, d: number) => {
      const u = 1 - t
      return u * u * u * a + 3 * u * u * t * b + 3 * u * t * t * c + t * t * t * d
    }
    for (const [a, b, c, d] of [
      [x0, x1, x2, x3],
      [y0, y1, y2, y3],
    ] as const) {
      const qa = -a + 3 * b - 3 * c + d
      const qb = 2 * (a - 2 * b + c)
      const qc = b - a
      const roots: number[] = []
      if (Math.abs(qa) < 1e-12) {
        if (Math.abs(qb) > 1e-12) roots.push(-qc / qb)
      } else {
        const disc = qb * qb - 4 * qa * qc
        if (disc >= 0) roots.push((-qb + Math.sqrt(disc)) / (2 * qa), (-qb - Math.sqrt(disc)) / (2 * qa))
      }
      for (const t of roots) if (t > 0 && t < 1) this.add(at(t, x0, x1, x2, x3), at(t, y0, y1, y2, y3))
    }
  }

  arc(x0: number, y0: number, rx: number, ry: number, angle: number, large: boolean, sweep: boolean, x: number, y: number): void {
    this.add(x, y)
    rx = Math.abs(rx)
    ry = Math.abs(ry)
    if (!rx || !ry || (x0 === x && y0 === y)) return
    const phi = (angle * Math.PI) / 180
    const [cos, sin] = [Math.cos(phi), Math.sin(phi)]
    const dx = (x0 - x) / 2
    const dy = (y0 - y) / 2
    const px = cos * dx + sin * dy
    const py = -sin * dx + cos * dy
    const scale = Math.max(1, Math.sqrt((px * px) / (rx * rx) + (py * py) / (ry * ry)))
    rx *= scale
    ry *= scale
    const num = rx * rx * ry * ry - rx * rx * py * py - ry * ry * px * px
    const den = rx * rx * py * py + ry * ry * px * px
    const k = (large === sweep ? -1 : 1) * Math.sqrt(Math.max(0, num / den))
    const cpx = (k * rx * py) / ry
    const cpy = (-k * ry * px) / rx
    const cx = cos * cpx - sin * cpy + (x0 + x) / 2
    const cy = sin * cpx + cos * cpy + (y0 + y) / 2
    const angleOf = (ux: number, uy: number) => Math.atan2(uy, ux)
    const start = angleOf((px - cpx) / rx, (py - cpy) / ry)
    let delta = angleOf((-px - cpx) / rx, (-py - cpy) / ry) - start
    if (sweep && delta < 0) delta += 2 * Math.PI
    if (!sweep && delta > 0) delta -= 2 * Math.PI
    const point = (t: number) => {
      const [ex, ey] = [rx * Math.cos(t), ry * Math.sin(t)]
      this.add(cx + cos * ex - sin * ey, cy + sin * ex + cos * ey)
    }
    const tx = Math.atan2(-ry * sin, rx * cos)
    const ty = Math.atan2(ry * cos, rx * sin)
    for (const base of [tx, ty]) {
      for (let n = -4; n <= 4; n++) {
        const t = base + n * Math.PI
        const offset = (t - start) / delta
        if (offset > 0 && offset < 1) point(t)
      }
    }
  }
}

class PathData {
  #at = 0

  constructor(readonly text: string) {}

  #skip(): void {
    SPACE.lastIndex = this.#at
    SPACE.exec(this.text)
    this.#at = SPACE.lastIndex
  }

  done(): boolean {
    this.#skip()
    return this.#at >= this.text.length
  }

  command(): string | null {
    this.#skip()
    const c = this.text[this.#at]
    if (c && /[MmLlHhVvCcSsQqTtAaZz]/.test(c)) {
      this.#at++
      return c
    }
    return null
  }

  number(): number {
    this.#skip()
    NUMBER.lastIndex = this.#at
    const m = NUMBER.exec(this.text)
    if (!m) throw new SyntaxError('number expected')
    this.#at = NUMBER.lastIndex
    return Number(m[0])
  }

  flag(): boolean {
    this.#skip()
    const c = this.text[this.#at]
    if (c !== '0' && c !== '1') throw new SyntaxError('flag expected')
    this.#at++
    return c === '1'
  }

  more(): boolean {
    this.#skip()
    return /[-+.\d]/.test(this.text[this.#at] ?? '')
  }
}

export function pathBox(d: string): Box | null {
  const bounds = new Bounds()
  const p = new PathData(d)
  let [x, y, sx, sy, cx, cy] = [0, 0, 0, 0, 0, 0]
  let last = ''
  try {
    while (!p.done()) {
      const command = p.command() ?? (last && last !== 'Z' && last !== 'z' ? last : null)
      if (!command) break
      const rel = command === command.toLowerCase()
      const ox = rel ? x : 0
      const oy = rel ? y : 0
      const upper = command.toUpperCase()
      let [nx, ny] = [x, y]
      let [ncx, ncy] = [NaN, NaN]
      switch (upper) {
        case 'M':
          nx = ox + p.number()
          ny = oy + p.number()
          ;[sx, sy] = [nx, ny]
          bounds.add(nx, ny)
          break
        case 'L':
          nx = ox + p.number()
          ny = oy + p.number()
          bounds.add(x, y)
          bounds.add(nx, ny)
          break
        case 'H':
          nx = ox + p.number()
          bounds.add(x, y)
          bounds.add(nx, ny)
          break
        case 'V':
          ny = oy + p.number()
          bounds.add(x, y)
          bounds.add(nx, ny)
          break
        case 'C':
        case 'S': {
          const [x1, y1] = upper === 'C' ? [ox + p.number(), oy + p.number()] : /[CcSs]/.test(last) ? [2 * x - cx, 2 * y - cy] : [x, y]
          const [x2, y2] = [ox + p.number(), oy + p.number()]
          nx = ox + p.number()
          ny = oy + p.number()
          bounds.add(x, y)
          bounds.cubic(x, y, x1, y1, x2, y2, nx, ny)
          ;[ncx, ncy] = [x2, y2]
          break
        }
        case 'Q':
        case 'T': {
          const [qx, qy] = upper === 'Q' ? [ox + p.number(), oy + p.number()] : /[QqTt]/.test(last) ? [2 * x - cx, 2 * y - cy] : [x, y]
          nx = ox + p.number()
          ny = oy + p.number()
          bounds.add(x, y)
          bounds.cubic(x, y, x + (2 / 3) * (qx - x), y + (2 / 3) * (qy - y), nx + (2 / 3) * (qx - nx), ny + (2 / 3) * (qy - ny), nx, ny)
          ;[ncx, ncy] = [qx, qy]
          break
        }
        case 'A': {
          const [rx, ry, angle] = [p.number(), p.number(), p.number()]
          const [large, sweep] = [p.flag(), p.flag()]
          nx = ox + p.number()
          ny = oy + p.number()
          bounds.add(x, y)
          bounds.arc(x, y, rx, ry, angle, large, sweep, nx, ny)
          break
        }
        case 'Z':
          ;[nx, ny] = [sx, sy]
          break
      }
      ;[x, y, cx, cy] = [nx, ny, ncx, ncy]
      last = upper === 'M' ? (rel ? 'l' : 'L') : command
      if (upper === 'Z' && p.more()) break
    }
  } catch {
    return bounds.empty ? null : bounds.box
  }
  return bounds.empty ? null : bounds.box
}

function length(id: number, name: string): number {
  const value = parseFloat(dom.getAttribute(id, name) ?? '')
  return Number.isFinite(value) ? value : 0
}

function pointsBox(text: string): Box | null {
  const bounds = new Bounds()
  const n = (text.match(new RegExp(NUMBER.source, 'g')) ?? []).map(Number)
  for (let i = 0; i + 1 < n.length; i += 2) bounds.add(n[i]!, n[i + 1]!)
  return bounds.empty ? null : bounds.box
}

export function shapeBox(id: number, transformOf: (id: number) => Affine, measure?: (id: number) => Box | null): Box | null {
  switch (dom.localName(id)) {
    case 'path':
      return pathBox(dom.getAttribute(id, 'd') ?? '')
    case 'rect': {
      const [x, y] = [length(id, 'x'), length(id, 'y')]
      return [x, y, x + length(id, 'width'), y + length(id, 'height')]
    }
    case 'circle': {
      const [cx, cy, r] = [length(id, 'cx'), length(id, 'cy'), length(id, 'r')]
      return [cx - r, cy - r, cx + r, cy + r]
    }
    case 'ellipse': {
      const [cx, cy, rx, ry] = [length(id, 'cx'), length(id, 'cy'), length(id, 'rx'), length(id, 'ry')]
      return [cx - rx, cy - ry, cx + rx, cy + ry]
    }
    case 'line': {
      const bounds = new Bounds()
      bounds.add(length(id, 'x1'), length(id, 'y1'))
      bounds.add(length(id, 'x2'), length(id, 'y2'))
      return bounds.box
    }
    case 'polyline':
    case 'polygon':
      return pointsBox(dom.getAttribute(id, 'points') ?? '')
  }
  if (!CONTAINERS.has(dom.localName(id))) return null
  const bounds = new Bounds()
  for (const child of dom.children(id)) {
    if (dom.nodeType(child) !== NodeType.Element || computedStyleOf(child, '', 'display') === 'none') continue
    const box = shapeBox(child, transformOf, measure) ?? measure?.(child) ?? null
    if (!box) continue
    const own = transformOf(child)
    const m = dom.localName(child) === 'svg' ? times(own, nestedSvg(child)) : own
    for (const [px, py] of [
      [box[0], box[1]],
      [box[2], box[1]],
      [box[2], box[3]],
      [box[0], box[3]],
    ] as const) {
      bounds.add(m.a * px + m.c * py + m.e, m.b * px + m.d * py + m.f)
    }
  }
  return bounds.empty ? null : bounds.box
}
