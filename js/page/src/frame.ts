import type { DoubleClickAction, FrameTheme } from '@dopame/acde-api'
import type { AppManifest } from '@dopame/acde-api/manifest'
import { FrameThemes } from './frametheme.ts'
import type { NativeSurface } from './native'
import { Level, Log } from './log'

interface SchemePage {
  readonly document: { readonly documentElement: Element | null; readonly head: Element | null; querySelector(selector: string): Element | null }
  getComputedStyle(element: Element): { getPropertyValue(name: string): string }
  matchMedia?(query: string): SchemeQuery
  readonly MutationObserver: new (callback: () => void) => SchemeObserver
}

interface SchemeQuery {
  readonly matches: boolean
  addEventListener(type: 'change', listener: () => void): void
}

interface SchemeObserver {
  observe(target: unknown, options: MutationObserverInit): void
  disconnect(): void
}

const DARK = /(^|\s)dark(\s|$)/i
const LIGHT = /(^|\s)light(\s|$)/i
const SCHEME_ATTRIBUTES = ['name', 'content', 'rel', 'href', 'media', 'disabled']

export class Frame {
  static readonly HEIGHT = 32
  static readonly #SIZE: readonly [number, number] = [800, 600]
  static readonly DOUBLE_CLICK: readonly DoubleClickAction[] = ['fill', 'minimize', 'none']

  static height(app: Pick<AppManifest, 'frame'> | null | undefined): number {
    return app && app.frame !== false ? Frame.HEIGHT : 0
  }

  static windowSize(app: Pick<AppManifest, 'frame' | 'size'>): readonly [number, number] {
    const [width, height] = app.size ?? Frame.#SIZE
    return [width, height + Frame.height(app)]
  }

  static desktop(
    surface: Pick<NativeSurface, 'setDoubleClick' | 'setFrameTheme'>,
    warn: (line: string) => void = (line) => Log.write(Level.Warn, 'acde_host', line),
  ): {
    setDoubleClick(action: DoubleClickAction): void
    setFrameTheme(theme: FrameTheme | null): void
  } {
    return {
      setFrameTheme: (theme) => {
        const wire = FrameThemes.parse(theme)
        if (typeof wire === 'string') throw new TypeError(`setFrameTheme: ${wire}`)
        if (wire.ignored.length) warn(`setFrameTheme: ignored ${wire.ignored.join(', ')} (not known to this runtime)`)
        surface.setFrameTheme?.(...wire.colors, wire.style, wire.side, wire.align)
      },
      setDoubleClick: (action) => {
        if (!Frame.DOUBLE_CLICK.includes(action)) throw new TypeError(`setDoubleClick: no action ${String(action)} (${Frame.DOUBLE_CLICK.join(', ')})`)
        surface.setDoubleClick?.(action)
      },
    }
  }

  static scheme(page: SchemePage): string {
    const root = page.document.documentElement
    const own = root ? page.getComputedStyle(root).getPropertyValue('color-scheme').trim() : ''
    if (own && own !== 'normal') return own
    return (page.document.querySelector('meta[name="color-scheme" i]')?.getAttribute('content') ?? '').trim()
  }

  static darkPage(page: SchemePage): boolean {
    return DARK.test(Frame.scheme(page))
  }

  static darkFrame(page: SchemePage, prefersDark: boolean): boolean {
    const scheme = Frame.scheme(page)
    return DARK.test(scheme) && (prefersDark || !LIGHT.test(scheme))
  }

  static followScheme(page: SchemePage, surface: Pick<NativeSurface, 'setPageDark'>, later: (run: () => void) => void): void {
    if (!surface.setPageDark) return
    let shown: boolean | null = null
    let queued = false
    let root: Element | null = null
    const schedule = () => {
      if (queued) return
      queued = true
      later(check)
    }
    const top = new page.MutationObserver(schedule)
    const rootWatch = new page.MutationObserver(schedule)
    const head = new page.MutationObserver(schedule)
    const prefers = page.matchMedia?.('(prefers-color-scheme: dark)') ?? null
    prefers?.addEventListener('change', schedule)
    const check = () => {
      queued = false
      const current = page.document.documentElement
      if (current !== root) {
        rootWatch.disconnect()
        root = current
        if (root) rootWatch.observe(root, { attributes: true, childList: true })
      }
      Frame.watchHead(head, page.document.head)
      const dark = Frame.darkFrame(page, prefers?.matches ?? false)
      if (dark === shown) return
      shown = dark
      surface.setPageDark?.(dark)
    }
    top.observe(page.document, { childList: true })
    check()
  }

  static watchHead(observer: SchemeObserver, head: Element | null): void {
    observer.disconnect()
    if (!head) return
    observer.observe(head, { childList: true })
    for (const child of Array.from(head.children)) {
      const style = child.localName === 'style'
      observer.observe(child, { attributes: true, attributeFilter: SCHEME_ATTRIBUTES, childList: style })
      if (style) for (const text of Array.from(child.childNodes)) observer.observe(text, { characterData: true })
    }
  }
}
