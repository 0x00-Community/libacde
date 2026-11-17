import type { DomNative, WebInternals } from '@dopame/acde-dom'
import type { DomNode, DomRecord, DomSource } from './dom'
import type { OverlaySource } from './overlay'

type Observer = { observe(target: unknown, options: object): void; disconnect(): void }
type InlineStyle = { readonly length: number; item(i: number): string; getPropertyValue(name: string): string; getPropertyPriority(name: string): string }
type ObserverClass = new (callback: (records: readonly DomRecord[]) => void) => Observer

const LOOKUP = Symbol.for('acde.devtools.node')
const HIGHLIGHT = Symbol.for('acde.devtools.highlight')

export class PageDom {
  static overlay(page: WebInternals, highlight: ((id: number) => void) | undefined): OverlaySource {
    return {
      highlight: (id) => highlight?.(id),
      idOf: (node) => page.idOf(node),
      nodeOf: (id) => page.nodeOf(id),
      expose: (show) => {
        Object.defineProperty(page.window, HIGHLIGHT, { value: show, configurable: true, enumerable: false, writable: false })
      },
    }
  }

  static source(page: WebInternals, dom: DomNative): DomSource {
    return {
      document: () => page.document as unknown as DomNode,
      idOf: (node) => page.idOf(node),
      nodeOf: (id) => (page.nodeOf(id) as DomNode | null | undefined) ?? null,
      observe: (changed) => {
        const Observer = (page.window as unknown as { MutationObserver: ObserverClass }).MutationObserver
        const observer = new Observer(changed)
        observer.observe(page.document, { childList: true, subtree: true, attributes: true, characterData: true })
        return () => observer.disconnect()
      },
      query: (root, selector) => {
        const all = (root as { querySelectorAll?: (selector: string) => ArrayLike<DomNode> }).querySelectorAll
        return all ? Array.from(all.call(root, selector)) : []
      },
      computed: (node) => {
        const all = dom.computedStyleAll(page.idOf(node), '')
        const out: [string, string][] = []
        for (let i = 0; i + 1 < all.length; i += 2) out.push([all[i]!, all[i + 1]!])
        return out
      },
      inline: (node) => {
        const style = (node as { style?: InlineStyle }).style
        if (!style) return []
        const out: [string, string, boolean][] = []
        for (let i = 0; i < style.length; i++) {
          const name = style.item(i)
          out.push([name, style.getPropertyValue(name), style.getPropertyPriority(name) === 'important'])
        }
        return out
      },
      rect: (node) => {
        const [x = 0, y = 0, width = 0, height = 0] = dom.rect(page.idOf(node))
        return [x, y, width, height]
      },
      expose: (lookup) => {
        Object.defineProperty(page.window, LOOKUP, { value: lookup, configurable: true, enumerable: false, writable: false })
      },
    }
  }
}
