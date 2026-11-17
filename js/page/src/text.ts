import { hostTime } from '@dopame/acde-dom'
import type { DomNative, WebInternals } from '@dopame/acde-dom'
import { InputMethod, type ImePanelEvent } from './ime'
import { ImePanel } from './panel'
import type { Shell } from './native'

type Field = HTMLInputElement | HTMLTextAreaElement

export interface TextInputState {
  readonly text: string
  readonly cursor: number
  readonly anchor: number
  readonly password: boolean
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
  #inactive = false
  #composing: Element | null = null
  #preedit: HTMLElement | null = null
  #placeholder: { field: Element; value: string } | null = null
  readonly #panel: ImePanel
  readonly #editing: WebInternals['editing']

  constructor(page: WebInternals, dom: DomNative) {
    this.#window = page.window as unknown as Window & typeof globalThis
    this.#document = page.document as unknown as Document
    this.#page = page
    this.#dom = dom
    this.#editing = page.editing
    this.#panel = new ImePanel(this.#document, { select: (index) => this.#ime?.select(index), page: (next) => this.#ime?.page(next) })
  }

  attachIme(ime: InputMethod): void {
    this.#ime = ime
  }

  attach(shell: Shell): void {
    this.#shell = shell
    if (!this.#ime && shell.setImeArea) this.#ime = InputMethod.area((rect) => shell.setImeArea?.(rect.x, rect.y, rect.width, rect.height))
    this.#page.env.clipboard = {
      writeText: async (text: string) => void this.#shell?.setClipboard(String(text)),
      readText: async () => (await this.#shell?.readClipboard()) ?? '',
    }
  }

  async clipboard(target: Element, key: 'c' | 'x' | 'v'): Promise<void> {
    const data = new this.#window.DataTransfer()
    const type = key === 'c' ? 'copy' : key === 'x' ? 'cut' : 'paste'
    if (type === 'paste') data.setData('text/plain', (await this.#shell?.readClipboard()) ?? '')
    const event = new this.#window.ClipboardEvent(type, { bubbles: true, cancelable: true, composed: true, clipboardData: data } as never)
    const handled = !target.dispatchEvent(event as unknown as Event)
    if (type === 'paste') {
      const text = data.getData('text/plain')
      if (!handled && text) this.#editing.insertText(target as never, text, 'insertFromPaste')
      return
    }
    const text = handled ? data.getData('text/plain') : this.#editing.selectedText(target as never)
    if (text) this.#shell?.setClipboard(text)
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
    if (!active) this.#panel.hide()
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
      this.#panel.hide()
      this.#last = null
      return
    }
    const field = this.#field(editable)
    const text = field ? field.value : (this.#selection()?.focusNode?.textContent ?? '')
    const cursor = field ? (field.selectionEnd ?? text.length) : (this.#selection()?.focusOffset ?? 0)
    const anchor = field ? (field.selectionStart ?? cursor) : (this.#selection()?.anchorOffset ?? cursor)
    const password = field?.type === 'password'
    const last = editable === this.#reported ? this.#last : null
    const same = !!last && last.text === text && last.cursor === cursor && last.anchor === anchor && last.password === password
    const now = hostTime.now()
    if (same && now - this.#checked < TextEntry.RECHECK_MS) return
    const rect = this.#caretRect(editable)
    this.#checked = now
    if (same && last.rect.x === rect.x && last.rect.y === rect.y && last.rect.width === rect.width && last.rect.height === rect.height) return
    this.#reported = editable
    this.#last = { text, cursor, anchor, password, rect }
    this.#ime.focus(this.#last)
  }

  panel(event: ImePanelEvent): void {
    const target = this.active()
    if (!this.#editable(target)) return this.#panel.hide()
    this.#panel.show(event, this.#caretRect(target!))
  }

  preedit(text: string): void {
    const target = this.active()
    if (!this.#editable(target)) return
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
      this.#preedit?.remove()
      this.#preedit = null
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
    this.#preedit.setAttribute('style', `position:fixed;left:${rect.x}px;top:${rect.y}px;height:${rect.height}px;line-height:${rect.height}px;z-index:2147483647;text-decoration:underline;background:Canvas;color:CanvasText;pointer-events:none;white-space:pre`)
    if (!this.#preedit.isConnected) this.#document.body.append(this.#preedit)
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
