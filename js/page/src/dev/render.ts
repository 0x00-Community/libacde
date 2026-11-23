import type { DomNative } from '@acde/dom'

export type PageRaster =
  | { readonly path: 'gpu'; readonly adapter: string; readonly driver: string; readonly backend: string; readonly software: boolean }
  | { readonly path: 'cpu'; readonly canvases: string | null }
  | { readonly path: 'lost'; readonly adapter: string }
  | { readonly path: 'none' }
  | { readonly path: 'pending' }

interface BackdropFact {
  readonly panel: number | null
  readonly rect: readonly [number, number, number, number]
  readonly glass: boolean
  readonly refracts: boolean
  readonly group: string | null
  readonly pieces: number
  readonly leftOut: boolean
}

export type ComposedBy = 'host-window' | 'compositor'

export type RenderDegraded = 'no-gpu' | 'cpu-cannot-refract' | 'group-full'

export interface RenderLayer {
  readonly panel?: number
  readonly rect: readonly [number, number, number, number]
  readonly kind: 'glass' | 'frost'
  readonly group?: string
  readonly pieces?: number
  readonly element?: number
  readonly drawn: string
  readonly degraded?: RenderDegraded
}

export interface PageRender {
  readonly raster: PageRaster & { readonly degraded?: RenderDegraded }
  readonly composedBy: ComposedBy
  readonly layers: readonly RenderLayer[]
}

export class PageRenderReport {
  constructor(
    readonly dom: DomNative,
    readonly offset: readonly [number, number],
    readonly hosted: boolean,
  ) {}

  report(): PageRender {
    if (!this.dom.renderReport) throw new Error('this acde reports no render path')
    const { raster, backdrops } = JSON.parse(this.dom.renderReport()) as { raster: PageRaster; backdrops: BackdropFact[] }
    const composedBy: ComposedBy = this.hosted ? 'host-window' : 'compositor'
    const gpu = raster.path === 'gpu'
    return {
      raster: gpu ? raster : { ...raster, degraded: 'no-gpu' },
      composedBy,
      layers: backdrops.map((fact) => this.#layer(fact, raster, composedBy)),
    }
  }

  #layer(fact: BackdropFact, raster: PageRaster, composedBy: ComposedBy): RenderLayer {
    const [x, y, width, height] = fact.rect
    const element = fact.panel === null ? this.#element(x + width / 2, y + height / 2) : null
    const drawn = PageRenderReport.drawn(fact, raster, composedBy)
    const degraded = fact.leftOut ? 'group-full' : drawn === 'cpu-glass-frosted' ? 'cpu-cannot-refract' : null
    return {
      ...(fact.panel !== null && { panel: fact.panel }),
      rect: fact.panel === null ? [x + this.offset[0], y + this.offset[1], width, height] : fact.rect,
      kind: fact.glass ? 'glass' : 'frost',
      ...(fact.group !== null && { group: fact.group, pieces: fact.pieces }),
      ...(element !== null && { element }),
      drawn,
      ...(degraded !== null && { degraded }),
    }
  }

  static drawn(fact: Pick<BackdropFact, 'glass' | 'refracts'>, raster: PageRaster, composedBy: ComposedBy): string {
    if (composedBy === 'compositor') return 'compositor'
    if (!fact.glass) return raster.path === 'gpu' || raster.path === 'cpu' ? 'frost' : 'none'
    if (raster.path === 'gpu') return 'gpu-glass'
    if (raster.path === 'cpu') return fact.refracts ? 'cpu-glass-frosted' : 'cpu-glass'
    return 'none'
  }

  #element(x: number, y: number): number | null {
    for (const id of this.dom.hitTestAll(x, y)) {
      const filter = this.dom.computedStyle(id, '', 'backdrop-filter')
      if (filter && filter !== 'none') return id
    }
    return null
  }
}
