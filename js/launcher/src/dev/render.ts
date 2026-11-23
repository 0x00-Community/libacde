import type { InspectWindow } from '@acde/api'
import type { PageRender, RenderLayer } from '../../../page/src/dev/render'

export type RenderAnswerLayer = Omit<RenderLayer, 'element'> & { readonly element?: string }

export interface RenderAnswer extends Omit<PageRender, 'layers'> {
  readonly web: string
  readonly window: string | null
  readonly layers: readonly RenderAnswerLayer[]
}

export function renderAnswer(web: string, window: InspectWindow | undefined, page: PageRender): RenderAnswer {
  const [ox, oy] = window?.rect ?? [0, 0]
  const layers = page.layers.map(({ element, ...layer }): RenderAnswerLayer => {
    const [x, y, width, height] = layer.rect
    return {
      ...layer,
      rect: layer.panel === undefined ? [x + ox, y + oy, width, height] : layer.rect,
      ...(element !== undefined && window && { element: `${window.ref}.${element}` }),
    }
  })
  return { web, window: window?.ref ?? null, raster: page.raster, composedBy: page.composedBy, layers }
}
