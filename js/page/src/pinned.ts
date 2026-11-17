import { hostTime } from '@dopame/acde-dom'
import type { PanelReserve, PinnedPanel, PinOptions, WindowEdge } from '@dopame/acde-api'
import type { NativeSurface } from './native'
import type { Rpc } from './rpc'

type PanelSurface = Pick<NativeSurface, 'openPanel' | 'placePanel' | 'closePanel' | 'reservePanel'>

interface PanelDom {
  setPanel(id: number, x: number, y: number, width: number, height: number): void
}

interface PinnedPage {
  readonly window: {
    innerWidth: number
    innerHeight: number
    ResizeObserver: typeof ResizeObserver
    MutationObserver: typeof MutationObserver
    addEventListener(type: string, fn: () => void): void
    requestAnimationFrame(fn: () => void): number
    getComputedStyle(element: Element): CSSStyleDeclaration
  }
  idOf(node: unknown): number
}

type Box = readonly [number, number, number, number]

interface Panel {
  readonly element: Element
  readonly id: number
  readonly margin: number
  reserve: PanelReserve | null
  reserved: string
  window: number | null
  opening: boolean
  rect: Box | null
  extent: Box | null
  stops: (() => void)[]
}

export class Pinned {
  static readonly #EDGES: Record<WindowEdge, number> = { top: 1, bottom: 2, left: 4, right: 8 }
  readonly #rpc: Rpc
  readonly #surface: PanelSurface
  readonly #dom: PanelDom
  readonly #page: PinnedPage
  readonly #panels = new Map<Element, Panel>()

  constructor(rpc: Rpc, surface: PanelSurface, dom: PanelDom, page: PinnedPage) {
    this.#rpc = rpc
    this.#surface = surface
    this.#dom = dom
    this.#page = page
    page.window.addEventListener('resize', () => {
      this.update()
      page.window.requestAnimationFrame(() => this.update())
      hostTime.setTimeout(() => this.update(), 700)
    })
  }

  pin(element: Element, options: PinOptions = {}): PinnedPanel {
    const known = this.#panels.get(element)
    if (known) return this.#handle(known)
    const reserve = options.reserve && Pinned.#EDGES[options.reserve.edge] ? options.reserve : null
    const margin = Math.max(0, options.margin ?? 0)
    const id = this.#page.idOf(element)
    const panel: Panel = { element, id, margin, reserve, reserved: '', window: null, opening: false, rect: null, extent: null, stops: [] }
    this.#panels.set(element, panel)
    const { ResizeObserver, MutationObserver } = this.#page.window
    const resized = new ResizeObserver(() => this.#place(panel))
    resized.observe(element)
    let settle: number | null = null
    const changed = new MutationObserver(() => {
      this.#place(panel)
      if (settle !== null) hostTime.clearTimeout(settle)
      settle = hostTime.setTimeout(() => {
        settle = null
        this.#place(panel)
      }, 700)
    })
    for (let at: Element | null = element; at; at = at.parentElement) changed.observe(at, { attributes: true, attributeFilter: ['style', 'class', 'hidden'] })
    panel.stops.push(() => resized.disconnect(), () => changed.disconnect(), () => settle !== null && hostTime.clearTimeout(settle))
    this.#place(panel)
    return this.#handle(panel)
  }

  update(): void {
    for (const panel of this.#panels.values()) this.#place(panel)
  }

  extend(id: number, [x, y, width, height]: readonly [number, number, number, number]): void {
    const panel = [...this.#panels.values()].find((p) => p.id === id)
    if (!panel) return
    const e = panel.extent
    const right = Math.max(x + width, e ? e[0] + e[2] : -Infinity), bottom = Math.max(y + height, e ? e[1] + e[3] : -Infinity)
    const left = Math.min(x, e?.[0] ?? Infinity), top = Math.min(y, e?.[1] ?? Infinity)
    panel.extent = [left, top, right - left, bottom - top]
    this.#place(panel)
  }

  #handle(panel: Panel): PinnedPanel {
    return { update: () => this.#place(panel), unpin: () => this.#unpin(panel), reserve: (reserve) => this.#rereserve(panel, reserve) }
  }

  #rereserve(panel: Panel, reserve: PanelReserve | null): void {
    if (reserve !== null && (typeof reserve !== 'object' || !Pinned.#EDGES[reserve.edge])) {
      throw new TypeError(`reserve: edge is one of ${Object.keys(Pinned.#EDGES).join(', ')}`)
    }
    if (reserve?.thickness !== undefined && !(Number.isFinite(reserve.thickness) && reserve.thickness >= 0)) {
      throw new TypeError('reserve: thickness is a number of pixels, 0 or more')
    }
    if (this.#panels.get(panel.element) !== panel) return
    panel.reserve = reserve
    if (panel.rect) this.#reserve(panel, panel.rect)  }

  #unpin(panel: Panel): void {
    if (this.#panels.get(panel.element) !== panel) return
    this.#panels.delete(panel.element)
    for (const stop of panel.stops) stop()
    this.#dom.setPanel(panel.id, 0, 0, 0, 0)
    if (panel.window === null) return
    this.#surface.closePanel(panel.id)
    void this.#rpc.call('panel', 'close', [panel.window]).catch(() => undefined)
  }

  #measure(panel: Panel): { rect: Box; input: Box } | null {
    const box = panel.element.getBoundingClientRect()
    if (!panel.element.isConnected || box.width <= 0 || box.height <= 0) return null
    const { innerWidth, innerHeight } = this.#page.window
    const e = panel.extent
    const x = Math.max(0, Math.floor(Math.min(box.left - panel.margin, e?.[0] ?? Infinity)))
    const y = Math.max(0, Math.floor(Math.min(box.top - panel.margin, e?.[1] ?? Infinity)))
    const right = Math.min(innerWidth, Math.ceil(Math.max(box.right + panel.margin, e ? e[0] + e[2] : -Infinity)))
    const bottom = Math.min(innerHeight, Math.ceil(Math.max(box.bottom + panel.margin, e ? e[1] + e[3] : -Infinity)))
    if (right <= x || bottom <= y) {
      const [left, top] = [Math.floor(box.left - panel.margin), Math.floor(box.top - panel.margin)]
      return { rect: [left, top, Math.ceil(box.right + panel.margin) - left, Math.ceil(box.bottom + panel.margin) - top], input: [0, 0, 0, 0] }
    }
    const style = this.#page.window.getComputedStyle(panel.element)
    const inert = style.pointerEvents === 'none' || style.visibility === 'hidden' || Number(style.opacity) === 0
    const input: Box = inert ? [0, 0, 0, 0] : [Math.floor(box.left) - x, Math.floor(box.top) - y, Math.ceil(box.width), Math.ceil(box.height)]
    return { rect: [x, y, right - x, bottom - y], input }
  }

  #place(panel: Panel): void {
    if (this.#panels.get(panel.element) !== panel || panel.opening) return
    const measured = this.#measure(panel)
    const same = (a: Box | null, b: Box | null) => !!a && !!b && a.every((v, i) => v === b[i])
    if (!measured) {
      if (!panel.element.isConnected || panel.element.getBoundingClientRect().width <= 0) panel.extent = null
      if (panel.rect === null) return
      panel.rect = null
      if (panel.window !== null) void this.#rpc.call('panel', 'place', [panel.window, [0, 0, 0, 0]]).catch(() => undefined)
      return
    }
    const { rect, input } = measured
    const moved = !same(rect, panel.rect)
    panel.rect = rect
    if (panel.window === null) return this.#open(panel, rect, input)
    if (moved) {
      this.#dom.setPanel(panel.id, ...rect)
      void this.#rpc.call('panel', 'place', [panel.window, rect]).catch(() => undefined)
    }
    this.#surface.placePanel(panel.id, ...rect, ...input)
    this.#reserve(panel, rect)
  }

  #reserve(panel: Panel, [x, y, width, height]: Box): void {
    if (panel.window === null) return
    const { innerWidth, innerHeight } = this.#page.window
    const sides = { top: y + height, bottom: innerHeight - y, left: x + width, right: innerWidth - x }
    const edge = panel.reserve ? Pinned.#EDGES[panel.reserve.edge] : 0
    const thickness = panel.reserve ? Math.max(0, Math.round(panel.reserve.thickness ?? sides[panel.reserve.edge])) : 0
    const key = `${edge}:${thickness}`
    if (key === panel.reserved || (panel.reserved === '' && edge === 0)) return
    panel.reserved = key
    this.#surface.reservePanel?.(panel.id, edge, thickness)
  }

  #open(panel: Panel, rect: Box, input: Box): void {
    panel.opening = true
    this.#rpc.call('panel', 'open', [rect]).then(
      (granted) => {
        panel.opening = false
        const { window, token } = granted as { window: number; token: bigint }
        if (this.#panels.get(panel.element) !== panel) return void this.#rpc.call('panel', 'close', [window]).catch(() => undefined)
        panel.window = window
        this.#surface.openPanel(panel.id, token, ...rect)
        this.#dom.setPanel(panel.id, ...rect)
        this.#surface.placePanel(panel.id, ...rect, ...input)
        this.#place(panel)
      },
      () => {
        panel.opening = false
      },
    )
  }
}
