import { guarded, host, tag } from '../core/host'
import { now as clockNow } from '../core/clock'
import type { Metrics } from '../native'
import type { Element } from './element'
import { frames } from './env'
import { DOMRectReadOnly } from './geometry'
import { computedStyleOf, metricsOf, rectOf } from './layout'
import { dom, documentId, ID, inner } from './realm'

let gatherResize: (observer: ResizeObserver) => void
let gatherIntersection: (observer: IntersectionObserver, time: number) => void

interface ResizeObserverSize {
  readonly inlineSize: number
  readonly blockSize: number
}

export class ResizeObserverEntry {
  constructor(
    readonly target: Element,
    readonly contentRect: DOMRectReadOnly,
    readonly borderBoxSize: readonly ResizeObserverSize[],
    readonly contentBoxSize: readonly ResizeObserverSize[],
    readonly devicePixelContentBoxSize: readonly ResizeObserverSize[],
  ) {}
}
tag(ResizeObserverEntry)

interface Observation {
  box: string
  last: [number, number] | null
  key: string
  sizes: { content: [number, number]; border: [number, number]; padding: [number, number] } | null
}

const resizeObservers = new Set<ResizeObserver>()

export class ResizeObserver {
  readonly #callback: (entries: ResizeObserverEntry[], observer: ResizeObserver) => void
  readonly #targets = new Map<Element, Observation>()

  constructor(callback: (entries: ResizeObserverEntry[], observer: ResizeObserver) => void) {
    if (typeof callback !== 'function') throw new TypeError("Failed to construct 'ResizeObserver': The callback provided as parameter 1 is not a function.")
    this.#callback = callback
  }

  static {
    gatherResize = (observer) => {
      const entries: ResizeObserverEntry[] = []
      const scale = dom.viewport().scale
      for (const [target, observation] of observer.#targets) {
        const sizes = measure(target, observation)
        const size = observation.box === 'border-box' ? sizes.border : sizes.content
        if (observation.last && observation.last[0] === size[0] && observation.last[1] === size[1]) continue
        observation.last = size
        const content = { inlineSize: sizes.content[0], blockSize: sizes.content[1] }
        entries.push(new ResizeObserverEntry(target, new DOMRectReadOnly(sizes.padding[0], sizes.padding[1], sizes.content[0], sizes.content[1]), [{ inlineSize: sizes.border[0], blockSize: sizes.border[1] }], [content], [{ inlineSize: Math.round(sizes.content[0] * scale), blockSize: Math.round(sizes.content[1] * scale) }]))
      }
      if (entries.length === 0) return
      guarded(() => observer.#callback.call(observer, entries, observer))
    }
  }

  observe(target: Element, options?: { box?: string }): void {
    this.#targets.set(target, { box: options?.box ?? 'content-box', last: null, key: '', sizes: null })
    resizeObservers.add(this)
    frames.request()
  }

  unobserve(target: Element): void {
    this.#targets.delete(target)
    if (this.#targets.size === 0) resizeObservers.delete(this)
  }

  disconnect(): void {
    this.#targets.clear()
    resizeObservers.delete(this)
  }
}
tag(ResizeObserver)

function measure(target: Element, observation: Observation): NonNullable<Observation['sizes']> {
  const m: Metrics | null = inner(target, 'isConnected') ? metricsOf(target[ID]) : null
  const key = m ? `${m.offsetWidth},${m.offsetHeight},${m.clientWidth},${m.clientHeight}` : ''
  if (observation.sizes && key === observation.key) return observation.sizes
  observation.key = key
  if (!m) return (observation.sizes = { content: [0, 0], border: [0, 0], padding: [0, 0] })
  const id = target[ID]
  const px = (name: string) => parseFloat(computedStyleOf(id, '', name)) || 0
  const left = px('padding-left'), right = px('padding-right'), top = px('padding-top'), bottom = px('padding-bottom')
  const clientW = m.clientWidth || m.offsetWidth, clientH = m.clientHeight || m.offsetHeight
  return (observation.sizes = {
    content: [Math.max(0, clientW - left - right), Math.max(0, clientH - top - bottom)],
    border: [m.offsetWidth, m.offsetHeight],
    padding: [left, top],
  })
}

export class IntersectionObserverEntry {
  constructor(
    readonly time: number,
    readonly rootBounds: DOMRectReadOnly | null,
    readonly boundingClientRect: DOMRectReadOnly,
    readonly intersectionRect: DOMRectReadOnly,
    readonly isIntersecting: boolean,
    readonly intersectionRatio: number,
    readonly target: Element,
  ) {}
}
tag(IntersectionObserverEntry)

const intersectionObservers = new Set<IntersectionObserver>()

type Side = [number, string]
type Margin = [Side, Side, Side, Side]

function parseMargin(text: string): Margin {
  const parts = text.trim().split(/\s+/).filter(Boolean)
  const values = parts.map((p): Side => {
    const m = /^(-?[\d.]+)(px|%)?$/.exec(p)
    if (!m) throw new SyntaxError(`Failed to construct 'IntersectionObserver': rootMargin must be specified in pixels or percent.`)
    return [parseFloat(m[1]!), m[2] ?? 'px']
  })
  const [t = [0, 'px'] as Side, r = t, b = t, l = r] = values
  return [t, r, b, l]
}

export class IntersectionObserver {
  readonly #callback: (entries: IntersectionObserverEntry[], observer: IntersectionObserver) => void
  readonly #root: Element | null
  readonly #margin: Margin
  readonly #thresholds: number[]
  readonly #targets = new Map<Element, { index: number; intersecting: boolean } | null>()

  constructor(callback: (entries: IntersectionObserverEntry[], observer: IntersectionObserver) => void, options?: { root?: Element | null; rootMargin?: string; threshold?: number | number[] }) {
    if (typeof callback !== 'function') throw new TypeError("Failed to construct 'IntersectionObserver': The callback provided as parameter 1 is not a function.")
    this.#callback = callback
    const root = options?.root ?? null
    this.#root = root && (root as unknown as { [ID]: number })[ID] === documentId ? null : root
    this.#margin = parseMargin(options?.rootMargin ?? '0px')
    const threshold = options?.threshold ?? 0
    this.#thresholds = (Array.isArray(threshold) ? threshold : [threshold]).map(Number).sort((a, b) => a - b)
    if (this.#thresholds.some((t) => !(t >= 0 && t <= 1))) throw new RangeError("Failed to construct 'IntersectionObserver': Threshold values must be numbers between 0 and 1")
  }

  get root(): Element | null {
    return this.#root
  }
  get rootMargin(): string {
    return this.#margin.map(([v, u]) => `${v}${u}`).join(' ')
  }
  get thresholds(): readonly number[] {
    return this.#thresholds.slice()
  }

  static {
    gatherIntersection = (observer, time) => {
      const entries: IntersectionObserverEntry[] = []
      const root = observer.#rootBounds()
      for (const [target, previous] of observer.#targets) {
        const rendered = inner(target, 'isConnected') && metricsOf(target[ID]) !== null
        const r = rendered ? rectOf(target[ID]) : Float64Array.of(0, 0, 0, 0)
        const box = new DOMRectReadOnly(r[0], r[1], r[2], r[3])
        let inter = new DOMRectReadOnly(0, 0, 0, 0)
        let intersecting = false
        if (rendered && root && (!observer.#root || inner(observer.#root, 'contains').call(observer.#root, target))) {
          const x0 = Math.max(r[0]!, root.x), y0 = Math.max(r[1]!, root.y)
          const x1 = Math.min(r[0]! + r[2]!, root.x + root.width), y1 = Math.min(r[1]! + r[3]!, root.y + root.height)
          if (x1 >= x0 && y1 >= y0) {
            intersecting = true
            inter = new DOMRectReadOnly(x0, y0, x1 - x0, y1 - y0)
          }
        }
        const area = r[2]! * r[3]!
        const ratio = !intersecting ? 0 : area > 0 ? (inter.width * inter.height) / area : 1
        let index = 0
        for (const t of observer.#thresholds) if (ratio >= t && (t > 0 || intersecting)) index++
        if (previous && previous.index === index && previous.intersecting === intersecting) continue
        observer.#targets.set(target, { index, intersecting })
        entries.push(new IntersectionObserverEntry(time, root, box, inter, intersecting, ratio, target))
      }
      if (entries.length === 0) return
      guarded(() => observer.#callback.call(observer, entries, observer))
    }
  }

  #rootBounds(): DOMRectReadOnly | null {
    let x: number, y: number, w: number, h: number
    if (this.#root) {
      if (!inner(this.#root, 'isConnected')) return null
      const m = metricsOf(this.#root[ID])
      if (!m) return null
      const r = rectOf(this.#root[ID])
      x = r[0]! + m.clientLeft
      y = r[1]! + m.clientTop
      w = m.clientWidth
      h = m.clientHeight
    } else {
      const v = dom.viewport()
      x = 0
      y = 0
      w = v.width
      h = v.height
    }
    const [t, rr, b, l] = this.#margin
    const amount = ([v, u]: Side, size: number) => (u === '%' ? (v / 100) * size : v)
    const top = amount(t, h), right = amount(rr, w), bottom = amount(b, h), left = amount(l, w)
    return new DOMRectReadOnly(x - left, y - top, w + left + right, h + top + bottom)
  }

  observe(target: Element): void {
    if (!this.#targets.has(target)) this.#targets.set(target, null)
    intersectionObservers.add(this)
    frames.request()
  }

  unobserve(target: Element): void {
    this.#targets.delete(target)
    if (this.#targets.size === 0) intersectionObservers.delete(this)
  }

  disconnect(): void {
    this.#targets.clear()
    intersectionObservers.delete(this)
  }

  takeRecords(): IntersectionObserverEntry[] {
    return []
  }
}
tag(IntersectionObserver)

export function watching(): boolean {
  return resizeObservers.size > 0 || intersectionObservers.size > 0
}

export function deliverObservations(): void {
  for (const observer of [...resizeObservers]) gatherResize(observer)
  if (intersectionObservers.size) {
    const time = clockNow()
    for (const observer of [...intersectionObservers]) gatherIntersection(observer, time)
  }
}
