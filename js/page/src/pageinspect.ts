import { hostTime } from '@dopame/acde-dom'
import type { DomNative, WebInternals } from '@dopame/acde-dom'
import type { Host } from './host'

export interface PageNode {
  readonly id: number
  readonly role: string
  readonly name?: string
  readonly value?: string
  readonly states?: readonly string[]
  readonly level?: number
  readonly rect: readonly [number, number, number, number]
  readonly more?: number
  readonly children?: readonly PageNode[]
}

export interface PageTree {
  readonly hidden: number
  readonly truncated: boolean
  readonly nodes: readonly PageNode[]
  readonly width: number
  readonly height: number
}

export interface PageMatch extends PageNode {
  readonly tag: string
  readonly visible: boolean
  readonly attributes?: Readonly<Record<string, string | null>>
}

export interface PageQuery {
  readonly count: number
  readonly nodes: readonly PageMatch[]
}

export type PagePoint = { readonly x: number; readonly y: number } | { readonly error: string }

type Field = HTMLInputElement | HTMLTextAreaElement

export class PageInspect {
  static readonly LIMIT = 600

  constructor(
    readonly page: WebInternals,
    readonly dom: DomNative,
    readonly host: Host,
    readonly offset: readonly [number, number],
  ) {}

  serve(method: string, args: unknown[]): unknown {
    switch (method) {
      case 'tree':
        return this.tree(Number(args[0] ?? -1), Number(args[1] ?? PageInspect.LIMIT), args[2] === true, args[3] === true)
      case 'query':
        return this.query(String(args[0] ?? ''), Number(args[1] ?? -1), Number(args[2] ?? PageInspect.LIMIT), Array.isArray(args[3]) ? args[3].map(String) : [])
      case 'point':
        return this.point(Number(args[0]))
      case 'type':
        return this.type(Number(args[0]), String(args[1] ?? ''), args[2] === true)
      case 'focus':
        return this.focus(Number(args[0]))
      case 'select':
        return this.select(Number(args[0]), Array.isArray(args[1]) ? args[1].map(String) : [])
      case 'settle':
        return this.settle(Number(args[0]) || 200, Number(args[1]) || 3000)
      case 'offset':
        return [this.offset[0], this.offset[1]]
      case 'text':
        return this.host.insertText(String(args[0] ?? ''))
      default:
        throw new Error(`inspect has no ${method}`)
    }
  }

  tree(root: number, limit: number, offscreen = false, invisible = false): PageTree {
    if (!this.dom.semantics) throw new Error('this acde has no semantics')
    const parsed = JSON.parse(this.dom.semantics(root < 0 ? -1 : root, limit, offscreen, invisible)) as Omit<PageTree, 'width' | 'height'>
    const window = this.page.window as unknown as Window
    return { ...parsed, nodes: parsed.nodes.map((n) => this.#moved(n)), width: window.innerWidth, height: window.innerHeight }
  }

  query(selector: string, within: number, limit: number, attributes: readonly string[] = []): PageQuery {
    if (!this.dom.semantics) throw new Error('this acde has no semantics')
    const document = this.page.document as unknown as Document
    const root: ParentNode | null = within >= 0 ? this.#element(within) : document
    if (!root) throw new Error(PageInspect.#gone(within))
    let found: Element[]
    try {
      found = [...root.querySelectorAll(selector)]
    } catch (error) {
      throw new Error(`bad selector ${JSON.stringify(selector)}: ${(error as Error)?.message ?? error}`)
    }
    return {
      count: found.length,
      nodes: found.slice(0, Math.max(1, limit)).map((element) => {
        const match = this.#match(element)
        return attributes.length ? { ...match, attributes: Object.fromEntries(attributes.map((name) => [name, element.getAttribute(name)])) } : match
      }),
    }
  }

  #match(element: Element): PageMatch {
    const id = this.page.idOf(element)
    const tree = JSON.parse(this.dom.semantics!(id, 1, true, true)) as { nodes: PageNode[] }
    const own = tree.nodes.length === 1 && tree.nodes[0]!.id === id ? tree.nodes[0]! : null
    const box = element.getBoundingClientRect()
    const style = (this.page.window as unknown as Window).getComputedStyle(element)
    const visible = box.width > 0 && box.height > 0 && style.visibility !== 'hidden' && style.display !== 'none' && Number(style.opacity || '1') > 0
    const rect: [number, number, number, number] = [Math.round(box.left), Math.round(box.top), Math.round(box.width), Math.round(box.height)]
    const text = (element.getAttribute('aria-label') || element.getAttribute('title') || element.textContent || '').replace(/\s+/g, ' ').trim()
    const base: PageNode = own
      ? (({ children: _children, more: _more, ...kept }) => ({ ...kept, rect }))(own)
      : { id, role: 'generic', ...(text && { name: text.length > 120 ? `${text.slice(0, 120)}…` : text }), rect }
    return { ...this.#moved(base), tag: element.localName, visible }
  }

  #moved(node: PageNode): PageNode {
    const [dx, dy] = this.offset
    if (!dx && !dy) return node
    return {
      ...node,
      rect: [node.rect[0] + dx, node.rect[1] + dy, node.rect[2], node.rect[3]],
      ...(node.children ? { children: node.children.map((c) => this.#moved(c)) } : {}),
    }
  }

  point(id: number): PagePoint {
    const element = this.#element(id)
    if (!element) return { error: PageInspect.#gone(id) }
    const window = this.page.window as unknown as Window
    const row = this.dom.pickerOptionRect(id)
    if (row) return this.#windowPoint(row[0]! + row[2]! / 2, row[1]! + row[3]! / 2)
    let rect = this.#rect(element)
    if (!rect || !element.checkVisibility({ visibilityProperty: true })) return { error: 'the element is not displayed' }
    const inside = (r: DOMRect) => r.right > 0 && r.bottom > 0 && r.left < window.innerWidth && r.top < window.innerHeight
    if (!inside(rect) || !this.#hits(element, rect)) {
      element.scrollIntoView({ block: 'center', inline: 'center' })
      rect = this.#rect(element)
      if (!rect) return { error: 'the element is not displayed' }
    }
    const left = Math.max(rect.left, 0)
    const top = Math.max(rect.top, 0)
    const right = Math.min(rect.right, window.innerWidth)
    const bottom = Math.min(rect.bottom, window.innerHeight)
    if (right <= left || bottom <= top) return { error: 'the element is outside the visible area' }
    const points: [number, number][] = [
      [(left + right) / 2, (top + bottom) / 2],
      [left + (right - left) * 0.25, (top + bottom) / 2],
      [left + (right - left) * 0.75, (top + bottom) / 2],
      [(left + right) / 2, top + (bottom - top) * 0.25],
      [(left + right) / 2, top + (bottom - top) * 0.75],
      [left + 2, top + 2],
    ]
    let covered = -1
    for (const [x, y] of points) {
      const hit = this.dom.hitTest(x, y)
      if (hit < 0) continue
      if (this.#within(hit, element)) return this.#windowPoint(x, y)
      covered = hit
    }
    if (covered < 0) return this.#windowPoint(points[0]![0], points[0]![1])
    return { error: `covered by another element: ${this.#describe(covered)}` }
  }

  #windowPoint(x: number, y: number): PagePoint {
    return { x: Math.round(x + this.offset[0]), y: Math.round(y + this.offset[1]) }
  }

  type(id: number, text: string, replace: boolean): { value: string | null } {
    const element = this.#element(id)
    if (!element) throw new Error(PageInspect.#gone(id))
    const target = this.#editable(element)
    if (!target) throw new Error('cannot type into this element')
    const document = this.page.document as unknown as Document
    if (document.activeElement !== target) target.focus()
    if (replace) {
      if ('select' in target && typeof (target as Field).select === 'function') (target as Field).select()
      else {
        const selection = (this.page.window as unknown as Window).getSelection()
        selection?.selectAllChildren(target)
      }
      if (!text) this.page.editing.insertText(target as never, '', 'deleteContentBackward')
    }
    if (text) this.host.insertText(text)
    const value = 'value' in target ? String((target as Field).value) : (target.textContent ?? '')
    return { value: (target as Field).type === 'password' ? '•••' : value.slice(0, 200) }
  }

  select(id: number, values: readonly string[]): { selected: string[] } {
    const element = this.#element(id)
    if (!element) throw new Error(PageInspect.#gone(id))
    const window = this.page.window as unknown as typeof globalThis
    const select = element instanceof window.HTMLSelectElement ? element : (element.closest('select') ?? element.querySelector('select'))
    if (!select) throw new Error('this element is not a <select> and has none inside')
    if (select.disabled) throw new Error('this <select> is disabled')
    const options = [...select.options]
    const picked = values.map((value) => {
      const option = options.find((o) => o.value === value) ?? options.find((o) => o.label === value || o.text === value)
      const listed = () => options.map((o) => JSON.stringify(o.value === o.text ? o.value : `${o.value} ${o.text}`)).join(', ')
      if (!option) throw new Error(`no option ${JSON.stringify(value)} (there are: ${listed()})`)
      const group = option.parentElement instanceof window.HTMLOptGroupElement && option.parentElement.disabled
      if (option.disabled || group) throw new Error(`option ${JSON.stringify(value)} is disabled`)
      return option
    })
    if (!select.multiple && picked.length > 1) throw new Error(`this <select> takes one option, got ${picked.length}`)
    if (select.multiple) for (const option of options) option.selected = picked.includes(option)
    else select.selectedIndex = picked[0]?.index ?? -1
    select.dispatchEvent(new window.Event('input', { bubbles: true, composed: true }))
    select.dispatchEvent(new window.Event('change', { bubbles: true }))
    return { selected: [...select.selectedOptions].map((o) => (o as HTMLOptionElement).value) }
  }

  focus(id: number): boolean {
    const element = this.#element(id)
    if (!element) throw new Error(PageInspect.#gone(id))
    element.focus()
    return (this.page.document as unknown as Document).activeElement === element
  }

  settle(quiet: number, max: number): Promise<number> {
    const window = this.page.window as unknown as typeof globalThis
    const document = this.page.document as unknown as Document
    const started = hostTime.now()
    let last = started
    const observer = new window.MutationObserver(() => (last = hostTime.now()))
    observer.observe(document, { subtree: true, childList: true, attributes: true, characterData: true })
    return new Promise((resolve) => {
      const check = () => {
        const now = hostTime.now()
        if (now - last >= quiet || now - started >= max) {
          observer.disconnect()
          return resolve(now - started)
        }
        hostTime.setTimeout(check, Math.min(quiet - (now - last), max - (now - started)))
      }
      hostTime.setTimeout(check, quiet)
    })
  }

  #element(id: number): HTMLElement | null {
    let node: Node | null
    try {
      node = this.page.nodeOf(id) as Node | null
    } catch {
      return null
    }
    while (node && node.nodeType !== 1) node = node.parentNode
    return node?.isConnected ? (node as HTMLElement) : null
  }

  static #gone(id: number): string {
    return `element ${id} is gone`
  }

  #rect(element: Element): DOMRect | null {
    const rect = element.getBoundingClientRect()
    return rect.width > 0 && rect.height > 0 ? rect : null
  }

  #hits(element: Element, rect: DOMRect): boolean {
    const hit = this.dom.hitTest(rect.left + rect.width / 2, rect.top + rect.height / 2)
    return hit >= 0 && this.#within(hit, element)
  }

  #within(hit: number, element: Element): boolean {
    const node = this.page.nodeOf(hit) as Node | null
    return !!node && (node === element || element.contains(node))
  }

  #describe(id: number): string {
    const node = this.page.nodeOf(id) as Element | null
    if (!node || node.nodeType !== 1) return `#${id}`
    const label = node.getAttribute('aria-label') ?? (node.textContent ?? '').trim().slice(0, 40)
    return `<${node.localName}${label ? ` "${label}"` : ''}> (${id})`
  }

  #editable(element: HTMLElement): HTMLElement | null {
    const document = this.page.document as unknown as Document
    for (let e: HTMLElement | null = element; e && e !== document.body; e = e.parentElement) {
      if (e instanceof (this.page.window as unknown as typeof globalThis).HTMLTextAreaElement) return e
      if (e instanceof (this.page.window as unknown as typeof globalThis).HTMLInputElement) return e
      if (e.isContentEditable) return e
    }
    const inner = element.querySelector<HTMLElement>('input, textarea, [contenteditable]:not([contenteditable="false"])')
    return inner
  }
}
