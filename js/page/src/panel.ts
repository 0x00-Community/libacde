import type { ImePanelEvent } from './ime'
import { PageSource } from './page'

export interface ImeFloatShell {
  readonly imeFloat?: boolean
  setImeFloat?(x: number, y: number, width: number, height: number): void
}

export interface ImeFloatDom {
  setImeFloat?(id: number, x: number, y: number, width: number, height: number): void
}

type Box = readonly [number, number, number, number]

export class ImeLayer {
  static readonly #ROOT = 'position:fixed;inset:0;z-index:2147483647;pointer-events:none;overflow:visible'
  readonly #document: Document
  readonly #dom: ImeFloatDom
  readonly #idOf: (node: Node) => number
  #shell: ImeFloatShell | null = null
  #root: HTMLElement | null = null
  #sent: Box | null = null

  constructor(document: Document, dom: ImeFloatDom, idOf: (node: Node) => number) {
    this.#document = document
    this.#dom = dom
    this.#idOf = idOf
  }

  attach(shell: ImeFloatShell): void {
    this.#shell = shell
  }

  get floating(): boolean {
    return this.#shell?.imeFloat === true && !!this.#shell.setImeFloat && !!this.#dom.setImeFloat
  }

  get position(): 'absolute' | 'fixed' {
    return this.floating ? 'absolute' : 'fixed'
  }

  mount(element: HTMLElement): void {
    const parent = this.floating ? this.#rooted() : this.#document.body
    if (element.parentNode !== parent) parent.append(element)
  }

  changed(): void {
    if (!this.floating || !this.#root) return
    const box = this.#box()
    const same = box === null ? this.#sent === null : !!this.#sent && box.every((v, i) => v === this.#sent![i])
    if (same) return
    this.#sent = box
    const rect = box ?? [0, 0, 0, 0]
    this.#dom.setImeFloat!(this.#idOf(this.#root), ...rect)
    this.#shell!.setImeFloat!(...rect)
  }

  #rooted(): HTMLElement {
    this.#root ??= this.#document.createElement('div')
    this.#root.setAttribute('data-acde-ime-float', '')
    this.#root.setAttribute('style', ImeLayer.#ROOT)
    if (!this.#root.isConnected) this.#document.body.append(this.#root)
    return this.#root
  }

  #box(): Box | null {
    const boxes = [...this.#root!.children].map((child) => child.getBoundingClientRect()).filter((r) => r.width > 0 && r.height > 0)
    if (!boxes.length) return null
    const left = Math.floor(Math.min(...boxes.map((r) => r.left)))
    const top = Math.floor(Math.min(...boxes.map((r) => r.top)))
    const right = Math.ceil(Math.max(...boxes.map((r) => r.right)))
    const bottom = Math.ceil(Math.max(...boxes.map((r) => r.bottom)))
    return [left, top, right - left, bottom - top]
  }
}

interface ImePanelActions {
  select(index: number): void
  page(next: boolean): void
}

export class ImePanel {
  static readonly #STYLE = [
    'z-index:2147483647', 'display:flex', 'flex-direction:column', 'gap:4px', 'padding:6px 8px',
    'border-radius:12px', 'background:var(--acde-ime-bg, rgba(255, 255, 255, .94))', 'color:var(--acde-ime-fg, #1D1D1F)',
    'box-shadow:0 0 0 .5px rgba(0, 0, 0, .12), 0 10px 30px -8px rgba(40, 20, 70, .35)', 'font:15px/1.4 "Noto Sans CJK SC", sans-serif',
    'white-space:nowrap', 'pointer-events:auto', 'user-select:none',
  ].join(';')
  readonly #document: Document
  readonly #actions: ImePanelActions
  readonly #layer: ImeLayer
  #element: HTMLElement | null = null

  constructor(document: Document, layer: ImeLayer, actions: ImePanelActions) {
    this.#document = document
    this.#layer = layer
    this.#actions = actions
  }

  get shown(): boolean {
    return !!this.#element?.isConnected
  }

  show(event: ImePanelEvent, caret: { x: number; y: number; width: number; height: number }): void {
    if (!event.preedit && !event.auxUp && !event.auxDown && event.candidates.length === 0) return this.hide()
    const panel = (this.#element ??= this.#create())
    const doc = this.#document
    const element = (tag: string, className: string, style: string | null, text?: string) => PageSource.element(doc, tag, className, style, text)
    const line = (cls: string, text: string) => element('div', cls, 'font-size:13px;color:var(--acde-ime-dim, #6E6977)', text)
    const parts: HTMLElement[] = []
    const top = [event.auxUp, event.preedit].filter(Boolean).join(' ')
    if (top) parts.push(line('ime-aux', top))
    if (event.candidates.length) {
      const vertical = event.layout === 'vertical'
      const row = element('div', 'ime-list', `display:flex;flex-direction:${vertical ? 'column' : 'row'};gap:2px;align-items:${vertical ? 'stretch' : 'center'}`)
      event.candidates.forEach((candidate, index) => {
        const on = index === event.highlight
        const item = element('div', on ? 'ime-cand on' : 'ime-cand', `display:flex;gap:4px;align-items:baseline;padding:2px 8px;border-radius:7px;cursor:default;${on ? 'background:var(--acde-ime-accent, #8B5CF6);color:#fff' : ''}`)
        item.dataset['index'] = String(index)
        const label = element('span', 'ime-label', `font-size:12px;${on ? 'opacity:.8' : 'color:var(--acde-ime-dim, #6E6977)'}`, candidate.label.replace(/[.．、]\s*$/, ''))
        item.append(label, element('span', 'ime-text', null, candidate.text))
        row.append(item)
      })
      if (event.hasPrev || event.hasNext) {
        for (const [next, glyph, can] of [[false, '‹', event.hasPrev], [true, '›', event.hasNext]] as const) {
          const arrow = element('div', 'ime-page', `padding:0 6px;font-size:17px;${can ? '' : 'opacity:.3'}`, glyph)
          arrow.dataset['page'] = next ? 'next' : 'prev'
          row.append(arrow)
        }
      }
      parts.push(row)
    }
    if (event.auxDown) parts.push(line('ime-aux down', event.auxDown))
    panel.replaceChildren(...parts)
    const view = doc.defaultView
    const height = (view?.innerHeight ?? 0) || 800
    const below = caret.y + caret.height + 6
    const flips = !this.#layer.floating && below + 72 > height
    const style = flips ? `left:${caret.x - 8}px;bottom:${height - caret.y + 6}px` : `left:${caret.x - 8}px;top:${below}px`
    panel.setAttribute('style', `position:${this.#layer.position};${ImePanel.#STYLE};${style}`)
    this.#layer.mount(panel)
    this.#layer.changed()
  }

  hide(): void {
    if (!this.#element?.isConnected) return
    this.#element.remove()
    this.#layer.changed()
  }

  #create(): HTMLElement {
    const panel = this.#document.createElement('div')
    panel.setAttribute('data-acde-ime-panel', '')
    panel.addEventListener('pointerdown', (event) => {
      event.stopPropagation()
      const target = (event.target as Element | null)?.closest?.('[data-index], [data-page]') as HTMLElement | null
      if (!target) return
      if (target.dataset['index'] !== undefined) this.#actions.select(Number(target.dataset['index']))
      else this.#actions.page(target.dataset['page'] === 'next')
    })
    panel.addEventListener('mousedown', (event) => event.preventDefault())
    return panel
  }
}
