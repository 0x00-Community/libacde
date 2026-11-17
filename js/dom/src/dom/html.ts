import { DOMException } from '../core/exception'
import { defineHandlers, Event, EventTarget, fire, getHandler } from '../core/event'
import { G, getters, host, queueTask, tag } from '../core/host'
import { URL } from '../core/url'
import { NONE, NodeType, Ns } from '../native'
import { ACTIVATION, activation, MouseEvent, ToggleEvent } from './events'
import { customClass, customName } from './custom'
import { ATTRIBUTE_CHANGED, BODY_WINDOW_TYPES, dataset, Element, flags, HANDLER_TYPES, LOCAL, NS, pendingElement, reflectsWindow, setAttribute, setWindowHandler, tokenList, windowHandlerOf, type DOMStringMap, type DOMTokenList } from './element'
import { loads, resolveUrl } from './env'
import { focus, focusable, setFocus } from './focus'
import { metricGetters, metricsOf } from './layout'
import { CDATASection, CharacterData, Comment, DocumentFragment, DocumentType, type HTMLCollection, liveCollection, mixin, Node, ProcessingInstruction, replaceAll, setTextContent, Text } from './node'
import { bool, long, nullable, string, url, urlParts } from './reflect'
import { classes, construct, constructing, dom, existing, ID, lifecycle, nodeConstructing, wrap, inner } from './realm'
import { CSSStyleSheet, inlineStyle, type CSSStyleDeclaration } from './style'
import { followLink } from './window'
import { hidePopover, setModal, showPopover, togglePopover } from './popover'

const htmlClasses = new Map<string, new () => Element>()

export function register(cls: { prototype: object; name: string } & (new () => Element), ...locals: string[]): void {
  for (const local of locals) htmlClasses.set(local, cls)
  tag(cls)
}

export const customHooks = { upgrading: null as HTMLElement | null }

export class HTMLOrSVGElementMixin {
  get style(): CSSStyleDeclaration {
    return inlineStyle(this as unknown as Element)
  }
  set style(value: string) {
    setAttribute(this as unknown as Element, 'style', String(value))
  }

  get dataset(): DOMStringMap {
    return dataset(this as unknown as Element)
  }

  declare nonce: string
  declare autofocus: boolean

  get tabIndex(): number {
    const self = this as unknown as Element
    const raw = dom.getAttribute(self[ID], 'tabindex')
    const n = raw === null ? NaN : parseInt(raw, 10)
    if (!Number.isNaN(n)) return n
    return defaultFocusable(self) ? 0 : -1
  }
  set tabIndex(value: number) {
    setAttribute(this as unknown as Element, 'tabindex', String(Math.trunc(Number(value)) || 0))
  }

  focus(this: Element, options?: { preventScroll?: boolean; focusVisible?: boolean }): void {
    if (!inner(this, 'isConnected') || !focusable(this[ID])) return
    setFocus(this, options?.focusVisible)
  }

  blur(this: Element): void {
    if (focus.id === this[ID]) setFocus(null)
  }
}
string(HTMLOrSVGElementMixin.prototype, 'nonce')
bool(HTMLOrSVGElementMixin.prototype, 'autofocus')

function defaultFocusable(element: Element): boolean {
  const local = element[LOCAL]
  if (element[NS] !== Ns.Html) return local === 'a' && dom.hasAttribute(element[ID], 'href')
  if (local === 'a' || local === 'area') return dom.hasAttribute(element[ID], 'href')
  return ['button', 'input', 'select', 'textarea', 'iframe', 'summary', 'object', 'embed'].includes(local) || dom.closest(element[ID], '[contenteditable]:not([contenteditable="false" i])') === element[ID]
}

export class HTMLElement extends Element {
  constructor() {
    const upgrade = customHooks.upgrading
    if (upgrade) {
      customHooks.upgrading = null
      return upgrade
    }
    if (!nodeConstructing()) {
      const name = customName(new.target)
      if (!name) throw new TypeError('Illegal constructor')
      pendingElement.local = name
      pendingElement.ns = Ns.Html
      constructing(dom.createElement(Ns.Html, name))
    }
    super()
  }

  get hidden(): boolean | string {
    const value = dom.getAttribute(this[ID], 'hidden')
    if (value === null) return false
    return value.toLowerCase() === 'until-found' ? 'until-found' : true
  }
  set hidden(value: boolean | string) {
    if (value === 'until-found') setAttribute(this, 'hidden', 'until-found')
    else if (!!value !== dom.hasAttribute(this[ID], 'hidden')) setAttribute(this, 'hidden', value ? '' : null)
  }

  get draggable(): boolean {
    const value = dom.getAttribute(this[ID], 'draggable')?.toLowerCase()
    if (value === 'true') return true
    if (value === 'false') return false
    return this[LOCAL] === 'img' || (this[LOCAL] === 'a' && dom.hasAttribute(this[ID], 'href'))
  }
  set draggable(value: boolean) {
    setAttribute(this, 'draggable', value ? 'true' : 'false')
  }

  get spellcheck(): boolean {
    for (let id = this[ID]; id !== NONE; id = dom.parent(id)) {
      if (dom.nodeType(id) !== NodeType.Element) break
      const value = dom.getAttribute(id, 'spellcheck')?.toLowerCase()
      if (value === 'true' || value === '') return true
      if (value === 'false') return false
    }
    return true
  }
  set spellcheck(value: boolean) {
    setAttribute(this, 'spellcheck', value ? 'true' : 'false')
  }

  get translate(): boolean {
    const id = dom.closest(this[ID], '[translate]')
    return id === NONE || dom.getAttribute(id, 'translate')?.toLowerCase() !== 'no'
  }
  set translate(value: boolean) {
    setAttribute(this, 'translate', value ? 'yes' : 'no')
  }

  get contentEditable(): string {
    const value = dom.getAttribute(this[ID], 'contenteditable')
    if (value === null) return 'inherit'
    const lower = value.toLowerCase()
    return lower === '' || lower === 'true' ? 'true' : lower === 'false' ? 'false' : lower === 'plaintext-only' ? 'plaintext-only' : 'inherit'
  }
  set contentEditable(value: string) {
    const lower = String(value).toLowerCase()
    if (lower === 'inherit') setAttribute(this, 'contenteditable', null)
    else if (lower === 'true' || lower === 'false' || lower === 'plaintext-only') setAttribute(this, 'contenteditable', lower)
    else throw new DOMException(`Failed to set the 'contentEditable' property on 'HTMLElement': The value provided ('${value}') is not one of 'true', 'false', 'plaintext-only', or 'inherit'.`, 'SyntaxError')
  }

  get isContentEditable(): boolean {
    return editable(this[ID])
  }

  get innerText(): string {
    return dom.innerText(this[ID])
  }
  set innerText(value: string) {
    setInnerText(this, value === null || value === undefined ? '' : String(value))
  }

  get outerText(): string {
    return inner(this, 'innerText')
  }
  set outerText(value: string) {
    const parent = inner(this, 'parentNode')
    if (!parent) throw new DOMException("Failed to set the 'outerText' property on 'HTMLElement': The element has no parent.", 'NoModificationAllowedError')
    const text = new Text(String(value))
    inner(this, 'replaceWith').call(this, text)
  }

  get offsetParent(): Element | null {
    const m = metricsOf(this[ID])
    return m ? wrap<Element>(m.offsetParent) : null
  }

  click(): void {
    if (clicking.has(this) || (dom.hasAttribute(this[ID], 'disabled') && /^(button|input|select|textarea|fieldset|option|optgroup)$/.test(this[LOCAL]))) return
    clicking.add(this)
    try {
      EventTarget.prototype.dispatchEvent.call(this, new MouseEvent('click', { bubbles: true, cancelable: true, composed: true, view: G }))
    } finally {
      clicking.delete(this)
    }
  }

  attachInternals(): never {
    throw new DOMException('ElementInternals is not supported', 'NotSupportedError')
  }

  showPopover(): void {
    showPopover(this as never)
  }
  hidePopover(): void {
    hidePopover(this as never)
  }
  togglePopover(force?: boolean | { force?: boolean }): boolean {
    return togglePopover(this as never, force)
  }
}
mixin(HTMLElement, HTMLOrSVGElementMixin)
metricGetters(HTMLElement.prototype, { offsetTop: 'offsetTop', offsetLeft: 'offsetLeft', offsetWidth: 'offsetWidth', offsetHeight: 'offsetHeight' })
string(HTMLElement.prototype, 'title')
string(HTMLElement.prototype, 'lang')
string(HTMLElement.prototype, 'dir')
string(HTMLElement.prototype, 'accessKey', 'accesskey')
string(HTMLElement.prototype, 'autocapitalize')
string(HTMLElement.prototype, 'inputMode', 'inputmode')
string(HTMLElement.prototype, 'enterKeyHint', 'enterkeyhint')
nullable(HTMLElement.prototype, 'popover')
bool(HTMLElement.prototype, 'inert')
export interface HTMLElement extends HTMLOrSVGElementMixin {}

function editable(element: number): boolean {
  const id = dom.closest(element, '[contenteditable]')
  if (id === NONE) return false
  const value = dom.getAttribute(id, 'contenteditable')!.toLowerCase()
  if (value === 'false') return false
  if (value === '' || value === 'true' || value === 'plaintext-only') return true
  const parent = dom.parent(id)
  return parent !== NONE && dom.nodeType(parent) === NodeType.Element ? editable(parent) : false
}

const clicking = new WeakSet<Element>()

defineHandlers(HTMLElement.prototype, HANDLER_TYPES)

function setInnerText(element: Element, text: string): void {
  const parts = text.replace(/\r\n?/g, '\n').split('\n')
  if (parts.length === 1) {
    setTextContent(element, text)
    return
  }
  const fragment = new DocumentFragment()
  parts.forEach((part, i) => {
    if (i > 0) fragment.appendChild(createHtml('br'))
    if (part) fragment.appendChild(new Text(part))
  })
  const id = element[ID]
  replaceAll(element, [fragment], () => dom.replaceChildren(id, Uint32Array.of(fragment[ID])))
}

function simple(name: string, locals: string, setup?: (proto: object) => void): typeof HTMLElement {
  const cls = { [name]: class extends HTMLElement {} }[name]!
  if (setup) setup(cls.prototype)
  register(cls, ...locals.split(' ').filter(Boolean))
  return cls
}

export const HTMLUnknownElement = simple('HTMLUnknownElement', '')
export const HTMLDivElement = simple('HTMLDivElement', 'div', (p) => string(p, 'align'))
export const HTMLSpanElement = simple('HTMLSpanElement', 'span')
export const HTMLParagraphElement = simple('HTMLParagraphElement', 'p', (p) => string(p, 'align'))
export const HTMLHeadingElement = simple('HTMLHeadingElement', 'h1 h2 h3 h4 h5 h6', (p) => string(p, 'align'))
export const HTMLPreElement = simple('HTMLPreElement', 'pre listing xmp', (p) => long(p, 'width', 0))
export const HTMLQuoteElement = simple('HTMLQuoteElement', 'blockquote q', (p) => url(p, 'cite'))
export const HTMLBRElement = simple('HTMLBRElement', 'br', (p) => string(p, 'clear'))
export const HTMLHRElement = simple('HTMLHRElement', 'hr', (p) => {
  string(p, 'align')
  string(p, 'color')
  string(p, 'size')
  string(p, 'width')
  bool(p, 'noShade', 'noshade')
})
export const HTMLUListElement = simple('HTMLUListElement', 'ul', (p) => {
  bool(p, 'compact')
  string(p, 'type')
})
export const HTMLOListElement = simple('HTMLOListElement', 'ol', (p) => {
  bool(p, 'reversed')
  long(p, 'start', 1)
  string(p, 'type')
})
export const HTMLLIElement = simple('HTMLLIElement', 'li', (p) => {
  long(p, 'value', 0)
  string(p, 'type')
})
export const HTMLDListElement = simple('HTMLDListElement', 'dl', (p) => bool(p, 'compact'))
export const HTMLModElement = simple('HTMLModElement', 'ins del', (p) => {
  url(p, 'cite')
  string(p, 'dateTime', 'datetime')
})
export const HTMLTimeElement = simple('HTMLTimeElement', 'time', (p) => string(p, 'dateTime', 'datetime'))
export const HTMLDataElement = simple('HTMLDataElement', 'data', (p) => string(p, 'value'))
export const HTMLMenuElement = simple('HTMLMenuElement', 'menu', (p) => bool(p, 'compact'))
export const HTMLHeadElement = simple('HTMLHeadElement', 'head')
export const HTMLHtmlElement = simple('HTMLHtmlElement', 'html', (p) => string(p, 'version'))
export const HTMLBodyElement = simple('HTMLBodyElement', 'body', (p) => {
  for (const name of ['text', 'link', 'vLink', 'aLink', 'bgColor', 'background']) string(p, name)
  for (const type of BODY_WINDOW_TYPES) {
    Object.defineProperty(p, `on${type}`, {
      get(this: Element) {
        if (!reflectsWindow(this, type)) return null
        windowHandlerOf(type)
        return getHandler(G as unknown as EventTarget, type)
      },
      set(this: Element, value: unknown) {
        if (!reflectsWindow(this, type)) return
        setWindowHandler(type, typeof value === 'function' || (value !== null && typeof value === 'object') ? value : null)
      },
      enumerable: true,
      configurable: true,
    })
  }
})
export const HTMLMetaElement = simple('HTMLMetaElement', 'meta', (p) => {
  string(p, 'name')
  string(p, 'content')
  string(p, 'httpEquiv', 'http-equiv')
  string(p, 'media')
  string(p, 'scheme')
})
export const HTMLBaseElement = simple('HTMLBaseElement', 'base', (p) => {
  url(p, 'href')
  string(p, 'target')
})
export const HTMLTitleElement = simple('HTMLTitleElement', 'title', textAlias)
export const HTMLMapElement = simple('HTMLMapElement', 'map', (p) => string(p, 'name'))
export const HTMLEmbedElement = simple('HTMLEmbedElement', 'embed', (p) => {
  url(p, 'src')
  string(p, 'type')
  string(p, 'width')
  string(p, 'height')
})
export const HTMLObjectElement = simple('HTMLObjectElement', 'object', (p) => {
  url(p, 'data')
  string(p, 'type')
  string(p, 'name')
  string(p, 'width')
  string(p, 'height')
  Object.defineProperty(p, 'contentDocument', { get: () => null, enumerable: true, configurable: true })
  Object.defineProperty(p, 'contentWindow', { get: () => null, enumerable: true, configurable: true })
})
export const HTMLParamElement = simple('HTMLParamElement', 'param', (p) => {
  string(p, 'name')
  string(p, 'value')
})
export const HTMLPictureElement = simple('HTMLPictureElement', 'picture')
export const HTMLSourceElement = simple('HTMLSourceElement', 'source', (p) => {
  url(p, 'src')
  string(p, 'srcset')
  string(p, 'sizes')
  string(p, 'type')
  string(p, 'media')
  long(p, 'width', 0)
  long(p, 'height', 0)
})
export const HTMLTrackElement = simple('HTMLTrackElement', 'track', (p) => {
  url(p, 'src')
  string(p, 'kind')
  string(p, 'srclang')
  string(p, 'label')
  bool(p, 'default')
})
export const HTMLTableCaptionElement = simple('HTMLTableCaptionElement', 'caption', (p) => string(p, 'align'))
export const HTMLTableColElement = simple('HTMLTableColElement', 'col colgroup', (p) => long(p, 'span', 1, 'span', 1))
export const HTMLTableCellElement = simple('HTMLTableCellElement', 'td th', (p) => {
  long(p, 'colSpan', 1, 'colspan', 1)
  long(p, 'rowSpan', 1, 'rowspan', 0)
  string(p, 'headers')
  string(p, 'scope')
  string(p, 'abbr')
  Object.defineProperty(p, 'cellIndex', {
    get(this: Element) {
      const parent = dom.parent(this[ID])
      if (parent === NONE) return -1
      const cells = dom.children(parent)
      let index = 0
      for (let i = 0; i < cells.length; i++) {
        if (cells[i] === this[ID]) return index
        const local = dom.localName(cells[i]!)
        if (local === 'td' || local === 'th') index++
      }
      return -1
    },
    enumerable: true,
    configurable: true,
  })
})
export const HTMLSlotElement = simple('HTMLSlotElement', 'slot', (p) => {
  string(p, 'name')
  Object.assign(p, { assignedNodes: () => [], assignedElements: () => [], assign: () => {} })
})
export const HTMLDataListElement = simple('HTMLDataListElement', 'datalist', (p) => {
  Object.defineProperty(p, 'options', {
    get(this: Element) {
      return this.getElementsByTagName('option')
    },
    enumerable: true,
    configurable: true,
  })
})

const TABLE_ROWS = ':scope > tr, :scope > thead > tr, :scope > tbody > tr, :scope > tfoot > tr'
const ROWS = Symbol('rows')
const BODIES = Symbol('bodies')
const CELLS = Symbol('cells')

function within(element: Element, key: symbol, selector: string): HTMLCollection {
  const holder = element as unknown as Record<symbol, HTMLCollection | undefined>
  const id = element[ID]
  return (holder[key] ??= liveCollection<HTMLCollection>(() => dom.querySelectorAll(id, selector)))
}

export class HTMLTableElement extends HTMLElement {
  get caption(): Element | null {
    return this.querySelector(':scope > caption')
  }
  get tHead(): Element | null {
    return this.querySelector(':scope > thead')
  }
  get tFoot(): Element | null {
    return this.querySelector(':scope > tfoot')
  }
  get tBodies(): HTMLCollection {
    return within(this, BODIES, ':scope > tbody')
  }
  get rows(): HTMLCollection {
    return within(this, ROWS, TABLE_ROWS)
  }
  createTBody(): Element {
    const body = createHtml('tbody')
    const bodies = this.tBodies
    const last = bodies.item(bodies.length - 1)
    this.insertBefore(body, last ? last.nextSibling : null)
    return body
  }
  createTHead(): Element {
    return this.tHead ?? this.insertBefore(createHtml('thead'), this.firstChild)
  }
  createTFoot(): Element {
    return this.tFoot ?? this.appendChild(createHtml('tfoot'))
  }
  createCaption(): Element {
    return this.caption ?? this.insertBefore(createHtml('caption'), this.firstChild)
  }
  deleteCaption(): void {
    this.caption?.remove()
  }
  deleteTHead(): void {
    this.tHead?.remove()
  }
  deleteTFoot(): void {
    this.tFoot?.remove()
  }
  insertRow(index = -1): Element {
    const rows = this.rows
    const row = createHtml('tr')
    if (index === -1 || index === rows.length) {
      const body = this.querySelector(':scope > tbody:last-of-type') ?? (rows.length ? rows.item(rows.length - 1)!.parentNode : null) ?? this.createTBody()
      if (body === this || !body) this.appendChild(row)
      else body.appendChild(row)
    } else {
      const ref = rows.item(index)
      if (!ref) throw new DOMException(`The index provided (${index}) is outside the range of rows.`, 'IndexSizeError')
      ref.parentNode!.insertBefore(row, ref)
    }
    return row
  }
  deleteRow(index: number): void {
    const rows = this.rows
    const row = rows.item(index === -1 ? rows.length - 1 : index)
    if (!row) throw new DOMException(`The index provided (${index}) is outside the range of rows.`, 'IndexSizeError')
    ;(row as Element).remove()
  }
}
register(HTMLTableElement, 'table')

export class HTMLTableSectionElement extends HTMLElement {
  get rows(): HTMLCollection {
    return within(this, ROWS, ':scope > tr')
  }
  insertRow(index = -1): Element {
    const row = createHtml('tr')
    const rows = this.rows
    this.insertBefore(row, index === -1 ? null : rows.item(index))
    return row
  }
  deleteRow(index: number): void {
    const rows = this.rows
    ;(rows.item(index === -1 ? rows.length - 1 : index) as Element | null)?.remove()
  }
}
register(HTMLTableSectionElement, 'thead', 'tbody', 'tfoot')

export class HTMLTableRowElement extends HTMLElement {
  get cells(): HTMLCollection {
    return within(this, CELLS, ':scope > td, :scope > th')
  }
  get rowIndex(): number {
    const table = dom.closest(this[ID], 'table')
    return table === NONE ? -1 : dom.querySelectorAll(table, TABLE_ROWS).indexOf(this[ID])
  }
  get sectionRowIndex(): number {
    const parent = dom.parent(this[ID])
    return parent === NONE ? -1 : dom.children(parent).indexOf(this[ID])
  }
  insertCell(index = -1): Element {
    const cell = createHtml('td')
    const cells = this.cells
    this.insertBefore(cell, index === -1 ? null : cells.item(index))
    return cell
  }
  deleteCell(index: number): void {
    const cells = this.cells
    ;(cells.item(index === -1 ? cells.length - 1 : index) as Element | null)?.remove()
  }
}
register(HTMLTableRowElement, 'tr')

export class HTMLDetailsElement extends HTMLElement {
  override [ATTRIBUTE_CHANGED](name: string, value: string | null): void {
    super[ATTRIBUTE_CHANGED](name, value)
    if (name !== 'open') return
    const open = value !== null
    queueTask(() => fire(this, new ToggleEvent('toggle', { oldState: open ? 'closed' : 'open', newState: open ? 'open' : 'closed' })))
  }
}
bool(HTMLDetailsElement.prototype, 'open')
string(HTMLDetailsElement.prototype, 'name')
register(HTMLDetailsElement, 'details')

function summaryActivation(this: HTMLElement): { run(): void; cancel(): void } | null {
  const parent = dom.parent(this[ID])
  if (parent === NONE || dom.localName(parent) !== 'details') return null
  const first = dom.querySelector(parent, ':scope > summary')
  if (first !== this[ID]) return null
  return activation(() => {
    const details = wrap<HTMLDetailsElement>(parent)! as unknown as { open: boolean }
    details.open = !details.open
  })
}

export class HTMLDialogElement extends HTMLElement {
  returnValue = ''

  show(): void {
    if (dom.hasAttribute(this[ID], 'open')) return
    setAttribute(this, 'open', '')
  }

  showModal(): void {
    if (!this.isConnected) throw new DOMException("Failed to execute 'showModal' on 'HTMLDialogElement': The element is not in a Document.", 'InvalidStateError')
    if (dom.hasAttribute(this[ID], 'open')) return
    setAttribute(this, 'open', '')
    setModal(this as never, true)
    const first = this.querySelector('[autofocus]') ?? this.querySelector('button, input, select, textarea, a[href], [tabindex]')
    if (first) inner(first as HTMLElement, 'focus').call(first as HTMLElement)
  }

  close(returnValue?: string): void {
    if (!dom.hasAttribute(this[ID], 'open')) return
    setAttribute(this, 'open', null)
    setModal(this as never, false)
    if (returnValue !== undefined) this.returnValue = String(returnValue)
    queueTask(() => fire(this, new Event('close')))
  }

  requestClose(returnValue?: string): void {
    if (fire(this, new Event('cancel', { cancelable: true }))) this.close(returnValue)
  }
}
bool(HTMLDialogElement.prototype, 'open')
register(HTMLDialogElement, 'dialog')

export class HTMLTemplateElement extends HTMLElement {
  get content(): DocumentFragment {
    return wrap<DocumentFragment>(dom.templateContent(this[ID]))!
  }

  override get innerHTML(): string {
    return dom.innerHTML(this[ID])
  }
  override set innerHTML(html: string) {
    const content = this.content
    const markup = html === null ? '' : String(html)
    const parsed = dom.parseFragment(this[ID], markup)
    const id = content[ID]
    replaceAll(content, [wrap<Node>(parsed)!], () => dom.replaceChildren(id, Uint32Array.of(parsed)))
  }

  get shadowRootMode(): string {
    return dom.getAttribute(this[ID], 'shadowrootmode') ?? ''
  }
}
register(HTMLTemplateElement, 'template')

export class HTMLScriptElement extends HTMLElement {
  declare text: string
  static supports(type: string): boolean {
    return type === 'classic' || type === 'module'
  }
}
textAlias(HTMLScriptElement.prototype)
url(HTMLScriptElement.prototype, 'src')
string(HTMLScriptElement.prototype, 'type')
bool(HTMLScriptElement.prototype, 'noModule', 'nomodule')
bool(HTMLScriptElement.prototype, 'async')
bool(HTMLScriptElement.prototype, 'defer')
nullable(HTMLScriptElement.prototype, 'crossOrigin', 'crossorigin')
string(HTMLScriptElement.prototype, 'integrity')
string(HTMLScriptElement.prototype, 'charset')
string(HTMLScriptElement.prototype, 'referrerPolicy', 'referrerpolicy')
register(HTMLScriptElement, 'script')

export class HTMLStyleElement extends HTMLElement {
  #sheet: unknown = null
  get sheet(): unknown {
    if (!this.isConnected) return null
    if (!this.#sheet) this.#sheet = new CSSStyleSheet(undefined, this)
    return this.#sheet
  }
}
string(HTMLStyleElement.prototype, 'media')
string(HTMLStyleElement.prototype, 'type')
bool(HTMLStyleElement.prototype, 'disabled')
register(HTMLStyleElement, 'style')

const REL_LIST = Symbol('relList')

const enum SheetState {
  Loaded = 1,
  Failed = 2,
}

export class HTMLLinkElement extends HTMLElement {
  #sheet: CSSStyleSheet | null = null
  #serial = -1

  get relList(): DOMTokenList {
    return tokenList(this, REL_LIST, 'rel', ['stylesheet', 'preload', 'modulepreload', 'icon', 'alternate', 'prefetch', 'preconnect', 'dns-prefetch', 'manifest', 'next'])
  }

  get sheet(): CSSStyleSheet | null {
    const serial = dom.sheetSerial(this[ID])
    if (serial < 0) return (this.#sheet = null)
    if (!this.#sheet || serial !== this.#serial) {
      this.#sheet = new CSSStyleSheet(undefined, this)
      this.#serial = serial
    }
    return this.#sheet
  }

  override [ATTRIBUTE_CHANGED](name: string, value: string | null): void {
    super[ATTRIBUTE_CHANGED](name, value)
    if ((name === 'href' || name === 'rel' || name === 'crossorigin') && inner(this, 'isConnected')) loads.expect()
  }
}

export function deliverSheet(id: number): void {
  const state = dom.sheetState(id)
  if (state !== SheetState.Loaded && state !== SheetState.Failed) return
  let link = existing(id) as HTMLLinkElement | undefined
  if (!link && flags.inlineHandlers && (dom.hasAttribute(id, 'onload') || dom.hasAttribute(id, 'onerror'))) link = wrap<HTMLLinkElement>(id)!
  if (!link) return
  fire(link, new Event(state === SheetState.Loaded ? 'load' : 'error'))
}

lifecycle.connected.push((node) => {
  if (node instanceof HTMLLinkElement || (node instanceof HTMLElement && node[LOCAL] === 'img')) loads.expect()
})
url(HTMLLinkElement.prototype, 'href')
string(HTMLLinkElement.prototype, 'rel')
string(HTMLLinkElement.prototype, 'as')
string(HTMLLinkElement.prototype, 'media')
string(HTMLLinkElement.prototype, 'type')
string(HTMLLinkElement.prototype, 'hreflang')
string(HTMLLinkElement.prototype, 'sizes')
string(HTMLLinkElement.prototype, 'integrity')
nullable(HTMLLinkElement.prototype, 'crossOrigin', 'crossorigin')
bool(HTMLLinkElement.prototype, 'disabled')
register(HTMLLinkElement, 'link')

export class HTMLAnchorElement extends HTMLElement {
  declare readonly relList: DOMTokenList
  declare text: string

  override toString(): string {
    return (this as unknown as { href: string }).href
  }

  [ACTIVATION](): { run(): void; cancel(): void } | null {
    const href = dom.getAttribute(this[ID], 'href')
    if (href === null) return null
    return activation(() => followLink(resolveUrl(href), dom.getAttribute(this[ID], 'target') ?? '', dom.getAttribute(this[ID], 'download')))
  }
}
hyperlink(HTMLAnchorElement.prototype)
textAlias(HTMLAnchorElement.prototype)
for (const name of ['target', 'rel', 'hreflang', 'type', 'ping', 'download', 'name', 'coords', 'shape', 'charset', 'rev']) string(HTMLAnchorElement.prototype, name)
string(HTMLAnchorElement.prototype, 'referrerPolicy', 'referrerpolicy')
register(HTMLAnchorElement, 'a')

export class HTMLAreaElement extends HTMLElement {
  declare readonly relList: DOMTokenList
  [ACTIVATION](): { run(): void; cancel(): void } | null {
    return HTMLAnchorElement.prototype[ACTIVATION].call(this as unknown as HTMLAnchorElement)
  }
}
hyperlink(HTMLAreaElement.prototype)
for (const name of ['target', 'rel', 'download', 'alt', 'coords', 'shape', 'ping']) string(HTMLAreaElement.prototype, name)
register(HTMLAreaElement, 'area')

function textAlias(proto: object): void {
  Object.defineProperty(proto, 'text', {
    get(this: Element) {
      return dom.textContent(this[ID])
    },
    set(this: Element, value: string) {
      setTextContent(this, String(value))
    },
    enumerable: true,
    configurable: true,
  })
}

function hyperlink(proto: object): void {
  const parsed = (el: Element): URL | null => {
    const raw = dom.getAttribute(el[ID], 'href')
    if (raw === null) return null
    try {
      return new URL(raw, resolveUrl('.'))
    } catch {
      return null
    }
  }
  url(proto, 'href')
  Object.defineProperty(proto, 'relList', {
    get(this: Element) {
      return tokenList(this, REL_LIST, 'rel', ['noreferrer', 'noopener', 'opener'])
    },
    enumerable: true,
    configurable: true,
  })
  urlParts(proto, ['origin', 'protocol', 'username', 'password', 'host', 'hostname', 'port', 'pathname', 'search', 'hash'], parsed, (el: Element, u) => setAttribute(el, 'href', u.href))
}

register(HTMLElement, ...['abbr', 'acronym', 'address', 'article', 'aside', 'b', 'basefont', 'bdi', 'bdo', 'big', 'center', 'cite', 'code', 'dd', 'dfn', 'dt', 'em', 'figcaption', 'figure', 'footer', 'header', 'hgroup', 'i', 'kbd', 'main', 'mark', 'nav', 'nobr', 'noembed', 'noframes', 'noscript', 'plaintext', 'rb', 'rp', 'rt', 'rtc', 'ruby', 's', 'samp', 'search', 'section', 'small', 'strike', 'strong', 'sub', 'summary', 'sup', 'tt', 'u', 'var', 'wbr'])
Object.defineProperty(HTMLElement.prototype, ACTIVATION, {
  value(this: HTMLElement) {
    return this[LOCAL] === 'summary' ? summaryActivation.call(this) : null
  },
  writable: true,
  configurable: true,
})

export const svgClasses = new Map<string, new () => Element>()
export const svgFallback = { cls: null as unknown as new () => Element }

function classOf(local: string): new () => Element {
  const known = htmlClasses.get(local)
  if (known) return known
  const custom = local.includes('-') ? customClass(local) : null
  if (custom) return custom
  return local.includes('-') ? HTMLElement : HTMLUnknownElement
}

classes.of = (type, ns, local) => {
  switch (type) {
    case NodeType.Element: {
      pendingElement.local = local
      pendingElement.ns = ns
      if (ns === Ns.Html) return classOf(local)
      if (ns === Ns.Svg) return svgClasses.get(local) ?? svgFallback.cls
      return Element
    }
    case NodeType.Text: return Text as never
    case NodeType.CDataSection: return CDATASection as never
    case NodeType.ProcessingInstruction: return ProcessingInstruction as never
    case NodeType.Comment: return Comment as never
    case NodeType.DocumentFragment: return DocumentFragment as never
    case NodeType.DocumentType: return DocumentType as never
    case NodeType.Document: return (local === 'xml' ? documentClass.xml : local === 'html' ? documentClass.html : documentClass.cls) as never
    default: return CharacterData as never
  }
}

export const documentClass = { cls: null as unknown as new () => Node, html: null as unknown as new () => Node, xml: null as unknown as new () => Node }

export function createHtml(local: string): HTMLElement {
  const id = dom.createElement(Ns.Html, local)
  pendingElement.local = local
  pendingElement.ns = Ns.Html
  return construct(classOf(local), id) as HTMLElement
}
