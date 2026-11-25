import type { InspectNode, InspectWindow } from '@acde/api'
import type { PageHit, PageMatch } from '../../../../page/src/pageinspect'
import { Outline, type Line } from '../outline'
import type { InspectHost, ShellWindow } from './types'

export type HitSurface = 'page' | 'decoration' | 'panel' | 'native' | 'none'

export interface HitElement {
  readonly ref: string
  readonly line: string
  readonly tag: string
}

export interface HitWindow {
  readonly ref: string
  readonly app: string | null
  readonly title: string | null
}

export interface HitAnswer {
  readonly x: number
  readonly y: number
  readonly window: HitWindow | null
  readonly surface: HitSurface
  readonly local?: readonly [number, number]
  readonly element?: HitElement & { readonly chain: readonly HitElement[]; readonly through: readonly HitElement[] }
  readonly gone?: true
  readonly note?: string
}

type Convert = (window: InspectWindow, node: PageMatch) => InspectNode

type Top = { readonly z: number; readonly window: InspectWindow } | { readonly z: number; readonly panel: ShellWindow }

export class Hit {
  static readonly DESKTOP: HitWindow = { ref: 'w0', app: null, title: 'desktop' }

  constructor(
    readonly host: InspectHost,
    readonly windows: readonly InspectWindow[],
    readonly convert: Convert,
  ) {}

  async at(x: number, y: number): Promise<HitAnswer> {
    const top = this.#top(x, y)
    if (!top) return { x, y, window: null, surface: 'none' }
    if ('panel' in top) return { x, y, window: Hit.DESKTOP, surface: 'panel', local: [x - top.panel.x, y - top.panel.y] }
    const window = top.window
    const local = [x - window.rect[0], y - window.rect[1]] as const
    const base = { x, y, window: Hit.#named(window), local }
    if (window.kind === 'native') return { ...base, surface: 'native' }
    const rpc = this.host.web(window.id)
    if (!rpc) return { ...base, surface: 'page', note: this.host.elsewhere?.(window.id) ? 'its page runs in another process' : 'the page is not running' }
    let answer: PageHit
    try {
      answer = (await rpc.call('inspect', 'hit', [...local])) as PageHit
    } catch {
      return { ...base, surface: 'page', gone: true }
    }
    if ('decoration' in answer) return { ...base, surface: 'decoration' }
    if ('outside' in answer || !answer.target) return { ...base, surface: 'page' }
    const element = (node: PageMatch) => this.#element(window, node)
    return { ...base, surface: 'page', element: { ...element(answer.target), chain: answer.chain.map(element), through: answer.through.map(element) } }
  }

  #top(x: number, y: number): Top | null {
    const inside = ([left, top, width, height]: readonly number[]) => x >= left! && y >= top! && x < left! + width! && y < top! + height!
    const windows: Top[] = this.windows.filter((w) => !w.hidden && inside(w.rect)).map((window) => ({ z: window.z, window }))
    const panels: Top[] = this.host.shell
      .windows()
      .filter((row) => !row.hidden && this.host.panel(row.window ?? row.id) && inside([row.x, row.y, row.width, row.height]))
      .map((panel) => ({ z: panel.z, panel }))
    return [...windows, ...panels].reduce<Top | null>((best, at) => (!best || at.z > best.z ? at : best), null)
  }

  #element(window: InspectWindow, node: PageMatch): HitElement {
    const out: Line[] = []
    Outline.lines([this.convert(window, node)], 0, out)
    const [ref, line] = [`${window.ref}.${node.id}`, out[0]?.[1] ?? '']
    return { ref, line: line.replace(/^- /, ''), tag: node.tag }
  }

  static #named(window: InspectWindow): HitWindow {
    return { ref: window.ref, app: window.app, title: window.title }
  }
}
