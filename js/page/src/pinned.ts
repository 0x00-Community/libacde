import { hostTime } from '@acde/dom'
import type { PanelReserve, PinAnchor, PinClosed, PinnedPanel, PinOptions, WindowEdge } from '@acde/api'
import type { NativeSurface, PanelEnd } from './native'
import { anchorOf, type Anchor, type AnchorWire } from './pinanchor'
import type { Rpc } from './rpc'

type PanelSurface = Pick<NativeSurface, 'openPanel' | 'placePanel' | 'closePanel' | 'reservePanel'>

interface PanelDom {
  setPanel(id: number, x: number, y: number, width: number, height: number): void
}

interface PinnedPage {
  readonly window: {
    innerWidth: number
    innerHeight: number
    devicePixelRatio?: number
    ResizeObserver: typeof ResizeObserver
    MutationObserver: typeof MutationObserver
    addEventListener(type: string, fn: () => void): void
    requestAnimationFrame(fn: () => void): number
    getComputedStyle(element: Element): CSSStyleDeclaration
    DOMException?: typeof DOMException
  }
  idOf(node: unknown): number
}

type Box = readonly [number, number, number, number]

interface Panel {
  readonly element: Element
  readonly id: number
  readonly margin: number
  readonly exclusive: boolean
  readonly userOnly: boolean
  readonly popup: boolean
  readonly parent: number
  popped: boolean
  reserve: PanelReserve | null
  reserved: string
  anchor: Anchor | null
  sent: string
  readonly closed: Promise<PinClosed>
  readonly end: (why: PinClosed) => void
  window: number | null
  opening: boolean
  rect: Box | null
  input: Box | null
  extent: Box | null
  view: string
  stops: (() => void)[]
}

export class Pinned {
  static readonly #EDGES: Record<WindowEdge, number> = { top: 1, bottom: 2, left: 4, right: 8 }
  static readonly #SETTLED = ['animationend', 'transitionend'] as const
  static readonly #SETTLE_MS = 700
  readonly #rpc: Rpc
  readonly #surface: PanelSurface
  readonly #dom: PanelDom
  readonly #page: PinnedPage
  readonly #panels = new Map<Element, Panel>()
  readonly #handles = new WeakMap<PinnedPanel, Panel>()
  readonly #due = new Set<Panel>()
  #framed = false

  readonly #anchors: boolean

  constructor(rpc: Rpc, surface: PanelSurface, dom: PanelDom, page: PinnedPage, anchors = false) {
    this.#anchors = anchors
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
    const exclusive = Pinned.#exclusive(options.keyboard)
    const userOnly = Pinned.#userOnly(options.userOnly)
    const known = this.#panels.get(element)
    if (known) return this.#handle(known)
    const reserve = options.reserve && Pinned.#EDGES[options.reserve.edge] ? options.reserve : null
    const anchor = options.anchor === undefined ? null : this.#anchor(options.anchor)
    if (anchor && reserve) throw new TypeError('pin: anchor and reserve do not go together (an anchored panel reserves no edge)')
    if (anchor && exclusive) throw new TypeError("pin: anchor and keyboard 'exclusive' do not go together (a menu takes the keyboard with requestGrab)")
    const popup = Pinned.#popup(options.popup, exclusive, reserve)
    const parent = this.#parent(options.parent)
    const margin = Math.max(0, options.margin ?? 0)
    const id = this.#page.idOf(element)
    let end: (why: PinClosed) => void = () => {}
    const closed = new Promise<PinClosed>((resolve) => {
      end = resolve
    })
    const panel: Panel = {
      element, id, margin, exclusive, userOnly, popup, parent, popped: false, reserve, reserved: '', anchor, sent: '', closed, end,
      window: null, opening: false, rect: null, input: null, extent: null, view: '', stops: [],
    }
    this.#panels.set(element, panel)
    const { ResizeObserver, MutationObserver } = this.#page.window
    const resized = new ResizeObserver(() => this.#place(panel))
    resized.observe(element)
    let settle: number | null = null
    let settleAt = 0
    const settled = () => {
      const left = settleAt - hostTime.now()
      if (left > 0) {
        settle = hostTime.setTimeout(settled, left)
        return
      }
      settle = null
      this.#place(panel)
    }
    const changed = new MutationObserver(() => {
      this.#schedule(panel)
      settleAt = hostTime.now() + Pinned.#SETTLE_MS
      if (settle === null) settle = hostTime.setTimeout(settled, Pinned.#SETTLE_MS)
    })
    for (let at: Element | null = element; at; at = at.parentElement) changed.observe(at, { attributes: true, attributeFilter: ['style', 'class', 'hidden'] })
    panel.stops.push(() => resized.disconnect(), () => changed.disconnect(), () => settle !== null && hostTime.clearTimeout(settle), () => this.#due.delete(panel))
    const ended = (event: Event) => {
      if (event.target === element) this.#place(panel)
    }
    for (const type of Pinned.#SETTLED) element.addEventListener(type, ended)
    panel.stops.push(() => {
      for (const type of Pinned.#SETTLED) element.removeEventListener(type, ended)
    })
    this.#place(panel)
    return this.#handle(panel)
  }

  #anchor(given: PinAnchor): Anchor {
    if (!this.#anchors) {
      // globalThis.DOMException, not the bare name: a free `DOMException` in the bundle makes bun rename the DOM
      // layer's class to DOMException2, and --keep-names does not keep the name, so the page's DOMException.name breaks.
      const Exception = this.#page.window.DOMException ?? globalThis.DOMException
      throw new Exception('pin: this window system places no anchored panels', 'NotSupportedError')
    }
    return anchorOf(given)
  }

  ended(id: number, why: PanelEnd): void {
    const panel = [...this.#panels.values()].find((p) => p.id === id)
    if (panel) this.#unpin(panel, why)
  }

  static #exclusive(keyboard: unknown): boolean {
    if (keyboard === undefined || keyboard === 'on-demand') return false
    if (keyboard === 'exclusive') return true
    throw new TypeError(`pin: keyboard is 'exclusive' or 'on-demand', not ${String(keyboard)}`)
  }

  static #popup(popup: unknown, exclusive: boolean, reserve: PanelReserve | null): boolean {
    if (popup === undefined || popup === false) return false
    if (popup !== true) throw new TypeError(`pin: popup is true or false, not ${String(popup)}`)
    if (exclusive) throw new TypeError("pin: popup and keyboard 'exclusive' do not go together (a popup takes the keyboard when it opens)")
    if (reserve) throw new TypeError('pin: popup and reserve do not go together (a popup reserves no edge)')
    return true
  }

  #parent(parent: PinnedPanel | undefined): number {
    if (parent === undefined) return 0
    const panel = this.#handles.get(parent)
    if (!panel || this.#panels.get(panel.element) !== panel) throw new TypeError('pin: parent is a panel pinned here and not closed')
    return panel.id
  }

  static #userOnly(userOnly: unknown): boolean {
    if (userOnly === undefined || typeof userOnly === 'boolean') return userOnly === true
    throw new TypeError(`pin: userOnly is true or false, not ${String(userOnly)}`)
  }

  update(): void {
    for (const panel of this.#panels.values()) this.#place(panel)
  }

  #schedule(panel: Panel): void {
    this.#due.add(panel)
    if (this.#framed) return
    this.#framed = true
    this.#page.window.requestAnimationFrame(() => {
      this.#framed = false
      const due = [...this.#due]
      this.#due.clear()
      for (const each of due) this.#place(each)
    })
  }

  extend(id: number, [x, y, width, height]: readonly [number, number, number, number]): void {
    const panel = [...this.#panels.values()].find((p) => p.id === id)
    if (!panel) return
    const e = this.#extent(panel)
    const right = Math.max(x + width, e ? e[0] + e[2] : -Infinity), bottom = Math.max(y + height, e ? e[1] + e[3] : -Infinity)
    const left = Math.min(x, e?.[0] ?? Infinity), top = Math.min(y, e?.[1] ?? Infinity)
    panel.extent = [left, top, right - left, bottom - top]
    this.#place(panel)
  }

  #view(): string {
    const { innerWidth, innerHeight, devicePixelRatio } = this.#page.window
    return `${innerWidth}x${innerHeight}@${devicePixelRatio ?? 1}`
  }

  #extent(panel: Panel): Box | null {
    const view = this.#view()
    if (panel.view !== view) {
      panel.extent = null
      panel.view = view
    }
    return panel.extent
  }

  #handle(panel: Panel): PinnedPanel {
    const handle: PinnedPanel = {
      update: () => this.#place(panel),
      unpin: () => this.#unpin(panel, 'unpinned'),
      reserve: (reserve) => this.#rereserve(panel, reserve),
      anchor: (next) => this.#reanchor(panel, next),
      closed: panel.closed,
      get popup() {
        return panel.popped
      },
    }
    this.#handles.set(handle, panel)
    return handle
  }

  #reanchor(panel: Panel, next: PinAnchor): void {
    if (!panel.anchor) throw new TypeError('anchor: this panel was pinned without an anchor')
    const anchor = this.#anchor(next)
    if (this.#panels.get(panel.element) !== panel) return
    panel.anchor = anchor
    panel.sent = ''
    panel.input = null
    this.#place(panel)
  }

  #wire(panel: Panel, visible: Box): AnchorWire | null {
    return panel.anchor ? { ...panel.anchor, visible: [...visible] as unknown as AnchorWire['visible'] } : null
  }

  #sendAnchor(panel: Panel, visible: Box): void {
    const wire = this.#wire(panel, visible)
    if (!wire || panel.window === null) return
    const key = JSON.stringify(wire)
    if (key === panel.sent) return
    panel.sent = key
    void this.#rpc.call('panel', 'anchor', [panel.window, wire]).catch(() => undefined)
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

  #unpin(panel: Panel, why: PinClosed): void {
    if (this.#panels.get(panel.element) !== panel) return
    this.#panels.delete(panel.element)
    for (const stop of panel.stops) stop()
    this.#dom.setPanel(panel.id, 0, 0, 0, 0)
    panel.end(why)
    for (const child of [...this.#panels.values()].filter((p) => p.parent === panel.id)) this.#unpin(child, 'parent-closed')
    if (panel.window === null) return
    this.#surface.closePanel(panel.id)
    void this.#rpc.call('panel', 'close', [panel.window]).catch(() => undefined)
  }

  #measure(panel: Panel): { rect: Box; input: Box } | null {
    const box = panel.element.getBoundingClientRect()
    if (!panel.element.isConnected || box.width <= 0 || box.height <= 0) return null
    const { innerWidth, innerHeight } = this.#page.window
    const e = this.#extent(panel)
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
    const reshaped = moved || !same(input, panel.input)
    panel.rect = rect
    if (panel.window === null) return this.#open(panel, rect, input)
    if (moved) {
      this.#dom.setPanel(panel.id, ...rect)
      void this.#rpc.call('panel', 'place', [panel.window, rect]).catch(() => undefined)
    }
    if (reshaped) {
      panel.input = input
      this.#surface.placePanel(panel.id, ...rect, ...input)
      this.#sendAnchor(panel, input)
    }
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
    this.#dom.setPanel(panel.id, ...rect)
    const wire = this.#wire(panel, input)
    if (wire) panel.sent = JSON.stringify(wire)
    this.#rpc.call('panel', 'open', wire ? [rect, wire] : [rect]).then(
      (granted) => {
        panel.opening = false
        const { window, token } = granted as { window: number; token: bigint }
        if (this.#panels.get(panel.element) !== panel) return void this.#rpc.call('panel', 'close', [window]).catch(() => undefined)
        panel.window = window
        const popped = this.#surface.openPanel(panel.id, token, ...rect, panel.exclusive, panel.userOnly, panel.popup, panel.parent)
        panel.popped = panel.popup && popped === true
        this.#dom.setPanel(panel.id, ...rect)
        this.#surface.placePanel(panel.id, ...rect, ...input)
        panel.input = input
        this.#place(panel)
      },
      () => {
        panel.opening = false
        if (this.#panels.get(panel.element) !== panel) return
        if (panel.anchor) return this.#unpin(panel, 'refused')
        this.#dom.setPanel(panel.id, 0, 0, 0, 0)
      },
    )
  }
}
