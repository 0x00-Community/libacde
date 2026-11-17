import { DevToolsError, type DevToolsDomain, type DevToolsParams, type DevToolsSession } from './index'

export interface OverlaySource {
  highlight(id: number): void
  idOf(node: unknown): number
  nodeOf(id: number): unknown
  expose(highlight: (node: unknown) => boolean): void
}

const NONE = -1
const QUIET = [
  'setInspectMode',
  'setShowViewportSizeOnResize',
  'setPausedInDebuggerMessage',
  'setShowGridOverlays',
  'setShowFlexOverlays',
  'setShowScrollSnapOverlays',
  'setShowContainerQueryOverlays',
  'setShowIsolatedElements',
  'setShowPaintRects',
  'setShowLayoutShiftRegions',
  'setShowScrollBottleneckRects',
  'setShowWebVitals',
  'setShowAdHighlights',
  'setShowDebugBorders',
  'setShowFPSCounter',
  'setShowHinge',
  'setShowWindowControlsOverlay',
]

export class OverlayDomain {
  readonly #source: OverlaySource
  #shown: number | null = null
  #by: number | null = null

  constructor(source: OverlaySource) {
    this.#source = source
    source.expose((node) => {
      let id = NONE
      try {
        id = node && typeof node === 'object' ? source.idOf(node) : NONE
      } catch {}
      if (!Number.isInteger(id) || id < 0 || source.nodeOf(id) !== node) return false
      this.#show(id, null)
      return true
    })
  }

  static create(source: OverlaySource): DevToolsDomain {
    const overlay = new OverlayDomain(source)
    const methods: Record<string, (params: DevToolsParams, session: DevToolsSession) => unknown> = {
      disable: (_, session) => {
        session.enabled.delete('Overlay')
        overlay.#hide(session.id)
      },
      highlightNode: (params, session) => overlay.#highlight(params, session),
      hideHighlight: (_, session) => overlay.#hide(session.id),
    }
    for (const name of QUIET) methods[name] = () => ({})
    return { name: 'Overlay', methods, closed: (session) => overlay.#hide(session) }
  }

  #highlight(params: DevToolsParams, session: DevToolsSession): void {
    const id = Number(params['nodeId'] ?? params['backendNodeId'])
    if (!Number.isInteger(id) || id < 0 || !this.#source.nodeOf(id)) throw new DevToolsError(-32000, 'Could not find node with given id')
    this.#show(id, session.id)
  }

  #hide(session: number): void {
    if (this.#shown === null || (this.#by !== null && this.#by !== session)) return
    this.#source.highlight(NONE)
    this.#shown = null
    this.#by = null
  }

  #show(id: number, session: number | null): void {
    this.#by = session
    if (this.#shown === id) return
    this.#source.highlight(id)
    this.#shown = id
  }
}
