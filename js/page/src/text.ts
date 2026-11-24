import { hostTime } from '@acde/dom'
import type { DomNative, WebInternals } from '@acde/dom'
import { InputMethod, type ImePanelEvent } from './ime'
import { ImeLayer, ImePanel } from './panel'
import type { ClipboardBytes, Shell } from './native'

type Field = HTMLInputElement | HTMLTextAreaElement

export interface TextInputState {
  readonly text: string
  readonly cursor: number
  readonly anchor: number
  readonly secret: boolean
  readonly rect: { x: number; y: number; width: number; height: number }
}

export class TextEntry {
  readonly #window: Window & typeof globalThis
  readonly #document: Document
  readonly #page: WebInternals
  readonly #dom: DomNative
  #shell: Shell | null = null
  #ime: InputMethod | null = null
  #reported: Element | null = null
  #last: TextInputState | null = null
  #checked = 0
  static readonly RECHECK_MS = 100
  static readonly SECRET_AUTOCOMPLETE = new Set(['current-password', 'new-password', 'one-time-code'])
  #inactive = false
  #composing: Element | null = null
  #preedit: HTMLElement | null = null
  #placeholder: { field: Element; value: string } | null = null
  readonly #panel: ImePanel
  readonly #layer: ImeLayer
  readonly #editing: WebInternals['editing']

  constructor(page: WebInternals, dom: DomNative) {
    this.#window = page.window as unknown as Window & typeof globalThis
    this.#document = page.document as unknown as Document
    this.#page = page
    this.#dom = dom
    this.#editing = page.editing
    this.#layer = new ImeLayer(this.#document, dom, (node) => page.idOf(node))
    this.#panel = new ImePanel(this.#document, this.#layer, { select: (index) => this.#ime?.select(index), page: (next) => this.#ime?.page(next) })
  }

  attachIme(ime: InputMethod): void {
    this.#ime = ime
  }

  attach(shell: Shell): void {
    this.#shell = shell
    this.#layer.attach(shell)
    if (!this.#ime && shell.setImeArea) this.#ime = InputMethod.area((rect) => shell.setImeArea?.(rect.x, rect.y, rect.width, rect.height))
    this.#page.env.clipboard = {
      writeText: async (text: string) => this.#write([TextEntry.#text(String(text))]),
      readText: async () => TextEntry.#decode(await this.#read(TextEntry.TEXT)),
      types: async () => (await this.#shell?.clipboardTypes()) ?? [],
      read: (type: string) => this.#read(type),
      write: async (entries) => this.#write(entries),
    }
  }

  static readonly TEXT = 'text/plain'
  static readonly PNG = 'image/png'
  static readonly URI_LIST = 'text/uri-list'

  static #text(text: string, type = TextEntry.TEXT): ClipboardBytes {
    return { type, data: new globalThis.TextEncoder().encode(text) }
  }

  static #decode(data: Uint8Array | null): string {
    return data ? new globalThis.TextDecoder().decode(data) : ''
  }

  #write(entries: readonly ClipboardBytes[]): void {
    this.#shell?.setClipboard(entries)
  }

  async #read(type: string): Promise<Uint8Array | null> {
    return (await this.#shell?.readClipboard(type)) ?? null
  }

  async #pasted(data: DataTransfer): Promise<void> {
    const types = (await this.#shell?.clipboardTypes()) ?? []
    for (const type of types.includes(TextEntry.TEXT) ? types : [TextEntry.TEXT, ...types]) {
      const bytes = await this.#read(type)
      if (!bytes) continue
      if (type === TextEntry.PNG) data.items.add(new this.#window.File([bytes as Uint8Array<ArrayBuffer>], 'image.png', { type }) as never)
      else data.setData(type, TextEntry.#decode(bytes))
    }
  }

  async clipboard(target: Element, key: 'c' | 'x' | 'v'): Promise<void> {
    const data = new this.#window.DataTransfer()
    const type = key === 'c' ? 'copy' : key === 'x' ? 'cut' : 'paste'
    if (type !== 'paste' && TextEntry.secret(target)) return
    if (type === 'paste') await this.#pasted(data as never)
    const event = new this.#window.ClipboardEvent(type, { bubbles: true, cancelable: true, composed: true, clipboardData: data } as never)
    const handled = !target.dispatchEvent(event as unknown as Event)
    if (type === 'paste') {
      const text = data.getData('text/plain')
      if (!handled && text) this.#editing.insertText(target as never, text, 'insertFromPaste')
      return
    }
    const text = handled ? data.getData('text/plain') : this.#editing.selectedText(target as never)
    const uris = handled ? data.getData(TextEntry.URI_LIST) : ''
    const entries = [...(text ? [TextEntry.#text(text)] : []), ...(uris ? [TextEntry.#text(uris, TextEntry.URI_LIST)] : [])]
    if (entries.length) this.#write(entries)
    if (type === 'cut' && !handled && text) this.#editing.insertText(target as never, '', 'deleteByCut')
  }

  active(): Element {
    let element = this.#document.activeElement
    while (element?.shadowRoot?.activeElement) element = element.shadowRoot.activeElement
    return element ?? this.#document.body
  }

  set surfaceActive(active: boolean) {
    if (this.#inactive === !active) return
    this.#inactive = !active
    if (!active && this.#reported) this.#ime?.blur()
    if (!active) this.#hideFloat()
    this.#reported = null
    this.#last = null
  }

  report(): void {
    if (!this.#ime || this.#inactive) return
    const target = this.active()
    const editable = this.#editable(target) ? target : null
    if (!editable) {
      if (this.#reported) this.#ime.blur()
      this.#reported = null
      this.#hideFloat()
      this.#last = null
      return
    }
    const field = this.#field(editable)
    const text = field ? field.value : (this.#selection()?.focusNode?.textContent ?? '')
    const cursor = field ? (field.selectionEnd ?? text.length) : (this.#selection()?.focusOffset ?? 0)
    const anchor = field ? (field.selectionStart ?? cursor) : (this.#selection()?.anchorOffset ?? cursor)
    const secret = TextEntry.secret(editable)
    const last = editable === this.#reported ? this.#last : null
    const same = !!last && last.text === text && last.cursor === cursor && last.anchor === anchor && last.secret === secret
    const now = hostTime.now()
    if (same && now - this.#checked < TextEntry.RECHECK_MS) return
    const rect = this.#caretRect(editable)
    this.#checked = now
    if (same && last.rect.x === rect.x && last.rect.y === rect.y && last.rect.width === rect.width && last.rect.height === rect.height) return
    this.#reported = editable
    this.#last = { text, cursor, anchor, secret, rect }
    this.#ime.focus(this.#last)
  }

  panel(event: ImePanelEvent): void {
    const target = this.active()
    if (!this.#editable(target) || TextEntry.secret(target!)) {
      this.#panel.hide()
      return
    }
    this.#panel.show(event, this.#caretRect(target!))
  }

  preedit(text: string): void {
    const target = this.active()
    if (!this.#editable(target) || TextEntry.secret(target)) return
    if (!text && this.#composing !== target) return
    if (this.#composing !== target) {
      this.#composing = target
      this.#fire(target, 'compositionstart', '')
    }
    this.#fire(target, 'compositionupdate', text)
    this.#showPreedit(target, text)
    if (!text) this.#endComposition('')
  }

  commit(text: string): void {
    const target = this.active()
    if (!this.#composing) {
      if (text) this.#editing.insertText(target as never, text, 'insertText')
      return
    }
    if (text) this.#editing.insertText(target as never, text, 'insertCompositionText')
    this.#endComposition(text)
  }

  deleteSurrounding(offset: number, length: number): void {
    this.#editing.deleteSurrounding(this.active() as never, offset, length)
  }

  static secret(target: Element): boolean {
    if (target.localName === 'input' && (target as HTMLInputElement).type === 'password') return true
    if (target.localName !== 'input' && target.localName !== 'textarea') return false
    return (target.getAttribute('autocomplete') ?? '').toLowerCase().split(/\s+/).some((token) => TextEntry.SECRET_AUTOCOMPLETE.has(token))
  }

  #fire(target: Element, type: string, data: string): void {
    target.dispatchEvent(new this.#window.CompositionEvent(type, { bubbles: true, cancelable: type === 'compositionstart', composed: true, data } as never) as unknown as Event)
  }

  #endComposition(data: string): void {
    const target = this.#composing
    this.#composing = null
    this.#showPreedit(null, '')
    if (target) this.#fire(target, 'compositionend', data)
  }

  #showPreedit(target: Element | null, text: string): void {
    if (this.#placeholder && (this.#placeholder.field !== target || !text)) {
      if (this.#placeholder.field.getAttribute('placeholder') === null) this.#placeholder.field.setAttribute('placeholder', this.#placeholder.value)
      this.#placeholder = null
    }
    if (!target || !text) {
      const shown = !!this.#preedit?.isConnected
      this.#preedit?.remove()
      this.#preedit = null
      if (shown) this.#layer.changed()
      return
    }
    const placeholder = this.#field(target) ? target.getAttribute('placeholder') : null
    if (placeholder !== null && !this.#placeholder) {
      this.#placeholder = { field: target, value: placeholder }
      target.removeAttribute('placeholder')
    }
    const rect = this.#caretRect(target)
    this.#preedit ??= this.#document.createElement('span')
    this.#preedit.textContent = text
    const place = `position:${this.#layer.position};left:${rect.x}px;top:${rect.y}px;height:${rect.height}px;line-height:${rect.height}px`
    const look = 'z-index:2147483647;text-decoration:underline;background:Canvas;color:CanvasText;pointer-events:none;white-space:pre'
    this.#preedit.setAttribute('style', `${place};${look}`)
    this.#layer.mount(this.#preedit)
    this.#layer.changed()
  }

  #hideFloat(): void {
    this.#panel.hide()
    this.#showPreedit(null, '')
  }

  #caretRect(target: Element): { x: number; y: number; width: number; height: number } {
    const field = this.#field(target)
    const point = field ? { node: field as Node, offset: field.selectionEnd ?? field.value.length } : this.#point()
    const id = point ? this.#page.idOf(point.node) : -1
    try {
      const caret = point && id >= 0 ? this.#dom.caretRect(id, point.offset) : null
      if (caret && caret[3]! > 0) return { x: caret[0]!, y: caret[1]!, width: caret[2]!, height: caret[3]! }
      const box = this.#page.idOf(target)
      if (box >= 0) {
        const [x = 0, y = 0, , height = 0] = this.#dom.rect(box)
        return { x, y, width: 1, height: Math.min(height, 24) || 20 }
      }
    } catch {
    }
    return { x: 0, y: 0, width: 1, height: 20 }
  }

  #selection(): Selection | null {
    const root = this.active().getRootNode() as Document | (ShadowRoot & { getSelection?: () => Selection | null })
    return (root as { getSelection?: () => Selection | null }).getSelection?.() ?? this.#document.getSelection()
  }

  #point(): { node: Text; offset: number } | null {
    const selection = this.#selection()
    if (!selection?.isCollapsed || selection.focusNode?.nodeType !== 3) return null
    return { node: selection.focusNode as Text, offset: selection.focusOffset }
  }

  #field(target: Element): Field | null {
    return this.#editing.textField(target as never) as unknown as Field | null
  }

  #editable(target: Element): boolean {
    return !!this.#field(target) || this.#editing.editable(target as never)
  }
}
