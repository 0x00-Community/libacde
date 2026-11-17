import { DOMException, guard } from '../core/exception'
import { getHandler, setHandler, type EventTarget } from '../core/event'
import { G, getters, tag } from '../core/host'
import { NONE, NodeType, Ns } from '../native'
import { loads } from './env'
import { DOMRect, rectList, type DOMRectList } from './geometry'
import { computedStyleOf, metricGetters, metricsOf, rectOf } from './layout'
import { attributeObservers, observing, recordAttribute } from './mutation'
import { ChildNodeMixin, insert, mixin, Node, ParentNodeMixin, remove, replaceAll, Text, type DocumentFragment } from './node'
import { bodyId, changed, CONNECTED as CONNECTED_KEY, documentId, dom, ID, moved, wrap, inner } from './realm'
import { string } from './reflect'
import { documents, inHtmlDocument, namespaceOf, nullable, validAttributeLocalName, validateAndExtract } from './namespaces'

export const LOCAL = Symbol('localName')
export const NS = Symbol('namespace')
export const ATTRIBUTE_CHANGED = Symbol('attributeChanged')
const CLASS_LIST = Symbol('classList')
const DATASET = Symbol('dataset')
const ATTRIBUTE_MAP = Symbol('attributes')
const PREFIX = Symbol('prefix')
const NAMESPACE_URI = Symbol('namespaceURI')

export const pendingElement = { local: null as string | null, ns: Ns.Html as Ns }

export const flags = {
  inlineHandlers: false,
  formAttributes: false,
}

export const HANDLER_TYPES = [
  'abort', 'animationcancel', 'animationend', 'animationiteration', 'animationstart', 'auxclick', 'beforeinput', 'beforematch', 'beforetoggle',
  'blur', 'cancel', 'canplay', 'canplaythrough', 'change', 'click', 'close', 'contextlost', 'contextmenu', 'contextrestored', 'copy',
  'cuechange', 'cut', 'dblclick', 'drag', 'dragend', 'dragenter', 'dragleave', 'dragover', 'dragstart', 'drop', 'durationchange', 'emptied',
  'ended', 'error', 'focus', 'focusin', 'focusout', 'formdata', 'gotpointercapture', 'input', 'invalid', 'keydown', 'keypress', 'keyup', 'load',
  'loadeddata', 'loadedmetadata', 'loadstart', 'lostpointercapture', 'mousedown', 'mouseenter', 'mouseleave', 'mousemove', 'mouseout',
  'mouseover', 'mouseup', 'paste', 'pause', 'play', 'playing', 'pointercancel', 'pointerdown', 'pointerenter', 'pointerleave',
  'pointermove', 'pointerout', 'pointerover', 'pointerrawupdate', 'pointerup', 'progress', 'ratechange', 'reset', 'resize', 'scroll',
  'scrollend', 'securitypolicyviolation', 'seeked', 'seeking', 'select', 'selectionchange', 'selectstart', 'slotchange', 'stalled',
  'submit', 'suspend', 'timeupdate', 'toggle', 'transitioncancel', 'transitionend', 'transitionrun', 'transitionstart', 'volumechange',
  'waiting', 'webkitanimationend', 'webkitanimationiteration', 'webkitanimationstart', 'webkittransitionend', 'wheel',
] as const
const HANDLER_SET = new Set<string>(HANDLER_TYPES)
export const BODY_WINDOW_TYPES: ReadonlySet<string> = new Set([
  'blur', 'error', 'focus', 'load', 'resize', 'scroll', 'afterprint', 'beforeprint', 'beforeunload', 'hashchange', 'languagechange',
  'message', 'messageerror', 'offline', 'online', 'pagehide', 'pageshow', 'popstate', 'rejectionhandled', 'storage', 'unhandledrejection',
  'unload',
])
const windowDecided = new Set<string>()
const HANDLER_ATTRIBUTE = /\son[a-z]+\s*=/i
const FORM_ATTRIBUTE = /\sform\s*=/i
const LINK_ELEMENT = /<link[\s/>]/i

export function noteMarkup(html: string): void {
  if (!flags.inlineHandlers && HANDLER_ATTRIBUTE.test(html)) flags.inlineHandlers = true
  if (!flags.formAttributes && FORM_ATTRIBUTE.test(html)) flags.formAttributes = true
  if (LINK_ELEMENT.test(html)) loads.expect()
}

export const customAttributes = {
  before: null as null | ((element: Element, name: string) => string | null | undefined),
  after: null as null | ((element: Element, name: string, old: string | null, value: string | null) => void),
}

export function setAttribute(element: Element, name: string, value: string | null): void {
  const id = element[ID]
  const key = element[NS] === Ns.Html && inHtmlDocument(id) ? name.toLowerCase() : name
  const interest = observing() ? attributeObservers(id, key) : null
  let old: string | null = null
  if (interest && (interest.oldValue || value === null)) {
    old = dom.getAttribute(id, key)
    if (value === null && old === null) return
  }
  const custom = customAttributes.before?.(element, key)
  if (value === null ? dom.removeAttribute(id, key) : guard(() => dom.setAttribute(id, key, value))) changed()
  element[ATTRIBUTE_CHANGED](key, value)
  if (interest) recordAttribute(element, interest, key, old)
  if (custom !== undefined) customAttributes.after!(element, key, custom, value)
}

function setAttributeNamespaced(element: Element, namespace: string | null, prefix: string | null, local: string, value: string | null): void {
  if (namespace === null) {
    if (value !== null || dom.getAttributeNS(element[ID], '', local) !== null) setAttributeExact(element, local, prefix, value)
    return
  }
  const id = element[ID]
  const interest = observing() ? attributeObservers(id, local) : null
  const old = interest ? dom.getAttributeNS(id, namespace, local) : null
  if (value === null && interest && old === null) return
  const done = value === null ? dom.removeAttributeNS(id, namespace, local) : guard(() => dom.setAttributeNS(id, namespace, prefix ?? '', local, value))
  if (done) changed()
  if (interest) recordAttribute(element, interest, local, old, namespace)
}

function setAttributeExact(element: Element, local: string, prefix: string | null, value: string | null): void {
  const id = element[ID]
  const interest = observing() ? attributeObservers(id, local) : null
  const old = interest || value === null ? dom.getAttributeNS(id, '', local) : null
  if (value === null && old === null) return
  const custom = customAttributes.before?.(element, local)
  const done = value === null ? dom.removeAttributeNS(id, '', local) : guard(() => dom.setAttributeNS(id, '', prefix ?? '', local, value))
  if (done) changed()
  element[ATTRIBUTE_CHANGED](local, value)
  if (interest) recordAttribute(element, interest, local, old)
  if (custom !== undefined) customAttributes.after!(element, local, custom, value)
}

function attributeKey(element: Element, name: string): string {
  return element[NS] === Ns.Html && inHtmlDocument(element[ID]) ? name.toLowerCase() : name
}

export class Element extends Node {
  [LOCAL]: string;
  [NS]: Ns;
  [PREFIX]: string | null | undefined = undefined;
  [NAMESPACE_URI]: string | null | undefined = undefined;
  [CLASS_LIST]: DOMTokenList | null = null;
  [DATASET]: object | null = null;
  [ATTRIBUTE_MAP]: NamedNodeMap | null = null

  constructor() {
    super()
    const local = pendingElement.local
    pendingElement.local = null
    this[LOCAL] = local ?? dom.localName(this[ID])
    this[NS] = local !== null ? pendingElement.ns : dom.namespace(this[ID])
  }

  override get nodeType(): number {
    return NodeType.Element
  }

  override get nodeName(): string {
    return inner(this, 'tagName')
  }

  get localName(): string {
    return this[LOCAL]
  }

  get tagName(): string {
    const prefix = inner(this, 'prefix')
    const name = prefix === null ? this[LOCAL] : `${prefix}:${this[LOCAL]}`
    return this[NS] === Ns.Html && inHtmlDocument(this[ID]) ? name.toUpperCase() : name
  }

  get namespaceURI(): string | null {
    if (this[NAMESPACE_URI] === undefined) this[NAMESPACE_URI] = namespaceOf(this[ID])
    return this[NAMESPACE_URI]
  }

  get prefix(): string | null {
    if (this[PREFIX] === undefined) this[PREFIX] = nullable(dom.prefix(this[ID]))
    return this[PREFIX]
  }

  [ATTRIBUTE_CHANGED](name: string, value: string | null): void {
    if (name.length > 2 && name.charCodeAt(0) === 111 && name.charCodeAt(1) === 110) {
      const type = name.slice(2)
      if (reflectsWindow(this, type)) {
        flags.inlineHandlers = true
        setWindowHandler(type, value)
      } else if (HANDLER_SET.has(type)) {
        flags.inlineHandlers = true
        handlerSet(this, type)
        setHandler(this, type, value)
      }
    } else if (name === 'form') flags.formAttributes = true
  }

  get id(): string {
    return dom.id(this[ID])
  }
  set id(value: string) {
    setAttribute(this, 'id', String(value))
  }

  get className(): string {
    return dom.className(this[ID])
  }
  set className(value: string) {
    setAttribute(this, 'class', String(value))
  }

  get classList(): DOMTokenList {
    return (this[CLASS_LIST] ??= new DOMTokenList(this, 'class'))
  }
  set classList(value: string) {
    setAttribute(this, 'class', String(value))
  }

  get attributes(): NamedNodeMap {
    const map = (this[ATTRIBUTE_MAP] ??= new NamedNodeMap(this))
    refreshAttributes(map)
    return map
  }

  getAttribute(name: string): string | null {
    return dom.getAttribute(this[ID], String(name))
  }

  getAttributeNS(namespace: string | null, name: string): string | null {
    return dom.getAttributeNS(this[ID], nullable(namespace) ?? '', String(name))
  }

  setAttribute(name: string, value: string): void {
    setAttribute(this, String(name), String(value))
  }

  setAttributeNS(namespace: string | null, name: string, value: string): void {
    const { namespace: ns, prefix, local } = validateAndExtract(namespace, String(name), 'attribute', "setAttributeNS' on 'Element")
    setAttributeNamespaced(this, ns, prefix, local, String(value))
  }

  removeAttribute(name: string): void {
    setAttribute(this, String(name), null)
  }

  removeAttributeNS(namespace: string | null, name: string): void {
    setAttributeNamespaced(this, nullable(namespace), null, String(name), null)
  }

  hasAttribute(name: string): boolean {
    return dom.hasAttribute(this[ID], String(name))
  }

  hasAttributeNS(namespace: string | null, name: string): boolean {
    return dom.getAttributeNS(this[ID], nullable(namespace) ?? '', String(name)) !== null
  }

  hasAttributes(): boolean {
    return dom.attributes(this[ID]).length > 0
  }

  getAttributeNames(): string[] {
    return attributeNames(this[ID])
  }

  toggleAttribute(name: string, force?: boolean): boolean {
    const n = String(name)
    const has = dom.hasAttribute(this[ID], n)
    if (has && force !== true) {
      setAttribute(this, n, null)
      return false
    }
    if (!has && force !== false) {
      setAttribute(this, n, '')
      return true
    }
    return has
  }

  getAttributeNode(name: string): Attr | null {
    const key = attributeKey(this, String(name))
    const list = dom.attributesNS(this[ID])
    for (let i = 0; i < list.length; i += 4) {
      const qualified = list[i + 1] ? `${list[i + 1]}:${list[i + 2]}` : list[i + 2]!
      if (qualified === key) return attrFor(this, list[i]!, list[i + 1]!, list[i + 2]!)
    }
    return null
  }

  getAttributeNodeNS(namespace: string | null, name: string): Attr | null {
    const ns = nullable(namespace) ?? '', local = String(name)
    const list = dom.attributesNS(this[ID])
    for (let i = 0; i < list.length; i += 4) {
      if (list[i] === ns && list[i + 2] === local) return attrFor(this, list[i]!, list[i + 1]!, local)
    }
    return null
  }

  setAttributeNode(attr: Attr): Attr | null {
    if (!(attr instanceof Attr)) throw new TypeError("Failed to execute 'setAttributeNode' on 'Element': parameter 1 is not of type 'Attr'.")
    const owner = attr.ownerElement
    if (owner !== null && owner !== this) throw new DOMException("Failed to execute 'setAttributeNode' on 'Element': The node provided is an attribute node that is already an attribute of another Element; attribute nodes must be explicitly cloned.", 'InUseAttributeError')
    const old = inner(this, 'getAttributeNodeNS').call(this, attr.namespaceURI, attr.localName)
    if (old === attr) return attr
    const value = attr.value
    if (old) detachAttr(old)
    setAttributeNamespaced(this, attr.namespaceURI, attr.prefix, attr.localName, value)
    ownAttr(this, attr)
    return old
  }

  setAttributeNodeNS(attr: Attr): Attr | null {
    return inner(this, 'setAttributeNode').call(this, attr)
  }

  removeAttributeNode(attr: Attr): Attr {
    if (!(attr instanceof Attr) || attr.ownerElement !== this) throw new DOMException("Failed to execute 'removeAttributeNode' on 'Element': The node provided is owned by another element.", 'NotFoundError')
    detachAttr(attr)
    setAttributeNamespaced(this, attr.namespaceURI, null, attr.localName, null)
    return attr
  }

  get innerHTML(): string {
    return dom.innerHTML(this[ID])
  }
  set innerHTML(html: string) {
    setInnerHTML(this, html === null ? '' : String(html))
  }

  get outerHTML(): string {
    return dom.outerHTML(this[ID])
  }
  set outerHTML(html: string) {
    const parent = inner(this, 'parentNode')
    if (!parent) return
    if (inner(parent, 'nodeType') === NodeType.Document) throw new DOMException("Failed to set the 'outerHTML' property on 'Element': This element's parent is of type '#document', which is not an element node.", 'NoModificationAllowedError')
    const context = inner(parent, 'nodeType') === NodeType.Element ? parent[ID] : bodyId() === NONE ? this[ID] : bodyId()
    const markup = html === null ? '' : String(html)
    noteMarkup(markup)
    const fragment = wrap<DocumentFragment>(guard(() => dom.parseFragment(context, markup)))!
    insert(parent, fragment, this)
    remove(this)
    parsed(parent)
  }

  insertAdjacentHTML(position: string, html: string): void {
    const where = String(position).toLowerCase()
    const outside = where === 'beforebegin' || where === 'afterend'
    if (!outside && where !== 'afterbegin' && where !== 'beforeend') throw new DOMException(`Failed to execute 'insertAdjacentHTML' on 'Element': The value provided ('${position}') is not one of 'beforeBegin', 'afterBegin', 'beforeEnd', or 'afterEnd'.`, 'SyntaxError')
    const parent = inner(this, 'parentNode')
    if (outside && (!parent || inner(parent, 'nodeType') === NodeType.Document)) throw new DOMException("Failed to execute 'insertAdjacentHTML' on 'Element': The element has no parent.", 'NoModificationAllowedError')
    const markup = String(html)
    noteMarkup(markup)
    const context = outside ? parent![ID] : this[ID]
    const fragment = wrap<DocumentFragment>(guard(() => dom.parseFragment(context, markup)))!
    adjacent(this, where, fragment)
    parsed(outside ? parent! : this)
  }

  insertAdjacentElement(position: string, element: Element): Element | null {
    return adjacent(this, String(position).toLowerCase(), element) ? element : null
  }

  insertAdjacentText(position: string, text: string): void {
    adjacent(this, String(position).toLowerCase(), new Text(String(text)))
  }

  matches(selectors: string): boolean {
    return guard(() => dom.matches(this[ID], String(selectors)))
  }

  webkitMatchesSelector(selectors: string): boolean {
    return inner(this, 'matches').call(this, selectors)
  }

  closest(selectors: string): Element | null {
    return wrap<Element>(guard(() => dom.closest(this[ID], String(selectors))))
  }

  getBoundingClientRect(): DOMRect {
    const r = rectOf(this[ID])
    return new DOMRect(r[0], r[1], r[2], r[3])
  }

  getClientRects(): DOMRectList {
    return rectList(dom.clientRects(this[ID]))
  }

  get scrollTop(): number {
    return metricsOf(this[ID])?.scrollTop ?? 0
  }
  set scrollTop(value: number) {
    scrollElement(this, null, Number(value), false)
  }

  get scrollLeft(): number {
    return metricsOf(this[ID])?.scrollLeft ?? 0
  }
  set scrollLeft(value: number) {
    scrollElement(this, Number(value), null, false)
  }

  scrollTo(x?: number | ScrollToOptions, y?: number): void {
    scrollElement(this, ...scrollArgs(x, y))
  }

  scroll(x?: number | ScrollToOptions, y?: number): void {
    inner(this, 'scrollTo').call(this, x, y)
  }

  scrollBy(x?: number | ScrollToOptions, y?: number): void {
    const m = metricsOf(this[ID])
    if (!m) return
    const [dx, dy, smooth] = scrollArgs(x, y, true)
    scrollElement(this, m.scrollLeft + dx!, m.scrollTop + dy!, smooth)
  }

  scrollIntoView(arg?: boolean | ScrollIntoViewOptions): void {
    const options: ScrollIntoViewOptions = typeof arg === 'object' && arg !== null ? arg : { block: arg === false ? 'end' : 'start', inline: 'nearest' }
    scrollIntoView(this, options.block ?? 'start', options.inline ?? 'nearest', options.behavior === 'smooth')
  }

  scrollIntoViewIfNeeded(center = true): void {
    scrollIntoView(this, center ? 'center' : 'nearest', 'nearest', false)
  }

  checkVisibility(options?: { checkOpacity?: boolean; checkVisibilityCSS?: boolean; visibilityProperty?: boolean; opacityProperty?: boolean }): boolean {
    if (metricsOf(this[ID]) === null && this[LOCAL] !== 'body' && this[LOCAL] !== 'html') return false
    if ((options?.checkVisibilityCSS || options?.visibilityProperty) && computedStyleOf(this[ID], '', 'visibility') !== 'visible') return false
    if ((options?.checkOpacity || options?.opacityProperty) && parseFloat(computedStyleOf(this[ID], '', 'opacity')) === 0) return false
    return true
  }

  requestFullscreen(): Promise<void> {
    return Promise.reject(new DOMException('Fullscreen is not supported', 'NotSupportedError'))
  }

  requestPointerLock(): Promise<void> {
    return Promise.reject(new DOMException('Pointer lock is not supported', 'NotSupportedError'))
  }
}
getters(Element.prototype, { assignedSlot: null })
string(Element.prototype, 'slot')
mixin(Element, ParentNodeMixin)
mixin(Element, ChildNodeMixin)
metricGetters(Element.prototype, { clientTop: 'clientTop', clientLeft: 'clientLeft', clientWidth: 'clientWidth', clientHeight: 'clientHeight', scrollWidth: 'scrollWidth', scrollHeight: 'scrollHeight' })
tag(Element)
export interface Element extends ParentNodeMixin, ChildNodeMixin {}

export interface ScrollToOptions {
  left?: number
  top?: number
  behavior?: string
}

interface ScrollIntoViewOptions {
  block?: string
  inline?: string
  behavior?: string
}

function attributeNames(id: number): string[] {
  const list = dom.attributes(id)
  const names: string[] = []
  for (let i = 0; i < list.length; i += 2) names.push(list[i]!)
  return names
}

export const parsedHooks: ((container: Node) => void)[] = []
export function parsed(container: Node): void {
  for (const hook of parsedHooks) hook(container)
}

function setInnerHTML(element: Element, html: string): void {
  noteMarkup(html)
  const id = element[ID]
  const fragment = guard(() => dom.parseFragment(id, html))
  const stand = { [ID]: fragment, [CONNECTED_KEY]: false } as unknown as Node
  replaceAll(element, [stand], () => dom.replaceChildren(id, Uint32Array.of(fragment)))
  parsed(element)
}

function adjacent(element: Element, where: string, node: Node): boolean {
  const inside = where === 'afterbegin' || where === 'beforeend'
  if (!inside && where !== 'beforebegin' && where !== 'afterend') throw new DOMException(`The value provided ('${where}') is not one of 'beforeBegin', 'afterBegin', 'beforeEnd', or 'afterEnd'.`, 'SyntaxError')
  const parent = inside ? element : inner(element, 'parentNode')
  if (!parent) return false
  insert(parent, node, where === 'beforebegin' ? element : where === 'afterbegin' ? inner(element, 'firstChild') : where === 'afterend' ? inner(element, 'nextSibling') : null)
  return true
}

export function scrollArgs(x?: number | ScrollToOptions, y?: number, by = false): [number | null, number | null, boolean] {
  if (typeof x === 'object' && x !== null) return [x.left ?? (by ? 0 : null), x.top ?? (by ? 0 : null), x.behavior === 'smooth']
  return [Number(x ?? 0), Number(y ?? 0), false]
}

export function scrollElement(element: Element, left: number | null, top: number | null, smooth: boolean): void {
  const id = element[ID]
  const m = metricsOf(id)
  if (!m) return
  const x = left === null || !Number.isFinite(left) ? m.scrollLeft : left
  const y = top === null || !Number.isFinite(top) ? m.scrollTop : top
  if (x === m.scrollLeft && y === m.scrollTop) return
  dom.scrollTo(id, x, y, smooth)
  moved()
}

function scrollIntoView(element: Element, block: string, inline: string, smooth: boolean): void {
  const ancestors = dom.ancestors(element[ID])
  let target = rectOf(element[ID])
  for (let i = 0; i < ancestors.length; i++) {
    const id = ancestors[i]!
    if (dom.nodeType(id) !== NodeType.Element) continue
    const m = metricsOf(id)
    if (!m) continue
    const isRoot = dom.localName(id) === 'html'
    const scrollsY = m.scrollHeight > m.clientHeight, scrollsX = m.scrollWidth > m.clientWidth
    if (!scrollsY && !scrollsX) continue
    if (!isRoot) {
      const overflow = computedStyleOf(id, '', 'overflow-y') + ' ' + computedStyleOf(id, '', 'overflow-x')
      if (!/auto|scroll|hidden/.test(overflow)) continue
    }
    const box = isRoot ? Float64Array.of(0, 0, m.clientWidth, m.clientHeight) : rectOf(id)
    const top = box[1]! + m.clientTop, left = box[0]! + m.clientLeft
    const height = m.clientHeight, width = m.clientWidth
    const dy = delta(block, target[1]! - top, target[3]!, height)
    const dx = delta(inline, target[0]! - left, target[2]!, width)
    if (dy || dx) {
      dom.scrollTo(id, m.scrollLeft + dx, m.scrollTop + dy, smooth)
      moved()
      target = rectOf(element[ID])
    }
  }
}

function delta(where: string, offset: number, size: number, view: number): number {
  switch (where) {
    case 'start': return offset
    case 'end': return offset + size - view
    case 'center': return offset + size / 2 - view / 2
    default:
      if (offset < 0 && offset + size < view) return size > view ? offset + size - view : offset
      if (offset + size > view && offset > 0) return size > view ? offset : offset + size - view
      return 0
  }
}

const ATTRS = Symbol('attrs')

function attrKey(namespace: string, local: string): string {
  return `${namespace} ${local}`
}

function attrCache(element: Element): Map<string, Attr> {
  const holder = element as unknown as { [ATTRS]?: Map<string, Attr> }
  return (holder[ATTRS] ??= new Map())
}

function attrFor(element: Element, namespace: string, prefix: string, local: string): Attr {
  const cache = attrCache(element)
  const key = attrKey(namespace, local)
  let attr = cache.get(key)
  if (!attr || attr.ownerElement !== element) {
    attr = new Attr(element, nullable(namespace), nullable(prefix), local)
    cache.set(key, attr)
  }
  return attr
}

let ownAttr: (element: Element, attr: Attr) => void
let detachAttr: (attr: Attr) => void

export class Attr {
  #element: Element | null
  readonly #namespace: string | null
  readonly #prefix: string | null
  readonly #local: string
  #value: string

  constructor(element: Element | null, namespace: string | null, prefix: string | null, local: string, value = '') {
    this.#element = element
    this.#namespace = namespace
    this.#prefix = prefix
    this.#local = local
    this.#value = value
  }

  static {
    ownAttr = (element, attr) => {
      attr.#element = element
      attrCache(element).set(attrKey(attr.#namespace ?? '', attr.#local), attr)
    }
    detachAttr = (attr) => {
      const element = attr.#element
      if (!element) return
      attr.#value = attr.value
      attr.#element = null
      attrCache(element).delete(attrKey(attr.#namespace ?? '', attr.#local))
    }
  }

  get nodeName(): string {
    return this.name
  }
  get name(): string {
    return this.#prefix === null ? this.#local : `${this.#prefix}:${this.#local}`
  }
  get localName(): string {
    return this.#local
  }
  get prefix(): string | null {
    return this.#prefix
  }
  get namespaceURI(): string | null {
    return this.#namespace
  }
  get value(): string {
    if (!this.#element) return this.#value
    return dom.getAttributeNS(this.#element[ID], this.#namespace ?? '', this.#local) ?? this.#value
  }
  set value(value: string) {
    this.#value = String(value)
    if (this.#element) setAttributeNamespaced(this.#element, this.#namespace, this.#prefix, this.#local, this.#value)
  }
  get nodeValue(): string {
    return this.value
  }
  set nodeValue(value: string) {
    this.value = value === null ? '' : value
  }
  get textContent(): string {
    return this.value
  }
  set textContent(value: string) {
    this.value = value === null ? '' : value
  }
  get ownerElement(): Element | null {
    return this.#element
  }
  get ownerDocument(): unknown {
    return this.#element ? this.#element.ownerDocument : attrDocument.get(this) ?? null
  }
}
getters(Attr.prototype, { nodeType: 2, specified: true })
tag(Attr)

export const attrDocument = new WeakMap<Attr, unknown>()

export function createAttr(namespace: string | null, prefix: string | null, local: string): Attr {
  if (namespace === null && prefix === null && !validAttributeLocalName(local)) throw new DOMException(`Failed to execute 'createAttribute' on 'Document': The localName provided ('${local}') contains an invalid character.`, 'InvalidCharacterError')
  return new Attr(null, namespace, prefix, local)
}

let refreshAttributes: (map: NamedNodeMap) => void

export class NamedNodeMap {
  readonly #element: Element
  #length = 0

  constructor(element: Element) {
    this.#element = element
  }

  static {
    refreshAttributes = (map) => {
      const list = dom.attributesNS(map.#element[ID])
      const length = list.length / 4
      for (let i = length; i < map.#length; i++) delete (map as unknown as Record<number, Attr>)[i]
      map.#length = length
      for (let i = 0; i < length; i++) {
        const attr = attrFor(map.#element, list[i * 4]!, list[i * 4 + 1]!, list[i * 4 + 2]!)
        Object.defineProperty(map, i, { value: attr, enumerable: true, configurable: true })
      }
    }
  }

  get length(): number {
    return this.#length
  }

  item(index: number): Attr | null {
    return (this as unknown as Record<number, Attr>)[index] ?? null
  }

  getNamedItem(name: string): Attr | null {
    return inner(this.#element, 'getAttributeNode').call(this.#element, name)
  }

  getNamedItemNS(namespace: string | null, name: string): Attr | null {
    return inner(this.#element, 'getAttributeNodeNS').call(this.#element, namespace, name)
  }

  setNamedItem(attr: Attr): Attr | null {
    return inner(this.#element, 'setAttributeNode').call(this.#element, attr)
  }

  setNamedItemNS(attr: Attr): Attr | null {
    return inner(this.#element, 'setAttributeNode').call(this.#element, attr)
  }

  removeNamedItem(name: string): Attr {
    const attr = inner(this.#element, 'getAttributeNode').call(this.#element, name)
    if (!attr) throw new DOMException(`Failed to execute 'removeNamedItem' on 'NamedNodeMap': No item with name '${name}' was found.`, 'NotFoundError')
    return inner(this.#element, 'removeAttributeNode').call(this.#element, attr)
  }

  removeNamedItemNS(namespace: string | null, name: string): Attr {
    const attr = this.getNamedItemNS(namespace, name)
    if (!attr) throw new DOMException(`Failed to execute 'removeNamedItemNS' on 'NamedNodeMap': No item with name '${name}' was found.`, 'NotFoundError')
    return inner(this.#element, 'removeAttributeNode').call(this.#element, attr)
  }

  *[Symbol.iterator](): IterableIterator<Attr> {
    for (let i = 0; i < this.#length; i++) yield this.item(i)!
  }
}
tag(NamedNodeMap)

const WHITESPACE = /[\t\n\f\r ]+/

export class DOMTokenList {
  readonly #element: Element
  readonly #attribute: string
  #text: string | null = null
  #tokens: string[] = []
  readonly #supported: readonly string[] | null

  constructor(element: Element, attribute: string, supported: readonly string[] | null = null) {
    this.#element = element
    this.#attribute = attribute
    this.#supported = supported
  }

  #current(): string {
    const id = this.#element[ID]
    return this.#attribute === 'class' ? dom.className(id) : (dom.getAttribute(id, this.#attribute) ?? '')
  }

  #read(): string[] {
    const text = this.#current()
    if (text !== this.#text) {
      this.#text = text
      const tokens: string[] = []
      for (const token of text.split(WHITESPACE)) if (token && !tokens.includes(token)) tokens.push(token)
      this.#tokens = tokens
    }
    return this.#tokens
  }

  #write(tokens: string[]): void {
    if (tokens.length === 0 && !dom.hasAttribute(this.#element[ID], this.#attribute)) return
    setAttribute(this.#element, this.#attribute, tokens.join(' '))
  }

  static #check(token: string, method: string): string {
    const t = String(token)
    if (t === '') throw new DOMException(`Failed to execute '${method}' on 'DOMTokenList': The token provided must not be empty.`, 'SyntaxError')
    if (/[\t\n\f\r ]/.test(t)) throw new DOMException(`Failed to execute '${method}' on 'DOMTokenList': The token provided ('${t}') contains HTML space characters, which are not valid in tokens.`, 'InvalidCharacterError')
    return t
  }

  get length(): number {
    return this.#read().length
  }

  get value(): string {
    return this.#current()
  }
  set value(value: string) {
    setAttribute(this.#element, this.#attribute, String(value))
  }

  item(index: number): string | null {
    return this.#read()[index] ?? null
  }

  contains(token: string): boolean {
    return this.#read().includes(String(token))
  }

  add(...tokens: string[]): void {
    const list = this.#read().slice()
    for (const token of tokens) {
      const t = DOMTokenList.#check(token, 'add')
      if (!list.includes(t)) list.push(t)
    }
    this.#write(list)
  }

  remove(...tokens: string[]): void {
    const checked = tokens.map((t) => DOMTokenList.#check(t, 'remove'))
    this.#write(this.#read().filter((t) => !checked.includes(t)))
  }

  toggle(token: string, force?: boolean): boolean {
    const t = DOMTokenList.#check(token, 'toggle')
    const list = this.#read()
    const has = list.includes(t)
    if (has) {
      if (force === true) return true
      this.#write(list.filter((x) => x !== t))
      return false
    }
    if (force === false) return false
    this.#write([...list, t])
    return true
  }

  replace(token: string, replacement: string): boolean {
    const t = DOMTokenList.#check(token, 'replace'), r = DOMTokenList.#check(replacement, 'replace')
    const list = this.#read()
    const at = list.indexOf(t)
    if (at < 0) return false
    const next = list.slice()
    if (next.includes(r)) next.splice(at, 1)
    else next[at] = r
    this.#write(next)
    return true
  }

  supports(token: string): boolean {
    if (!this.#supported) throw new TypeError(`Failed to execute 'supports' on 'DOMTokenList': DOMTokenList has no supported tokens.`)
    return this.#supported.includes(String(token).toLowerCase())
  }

  forEach(callback: (value: string, index: number, list: DOMTokenList) => void, thisArg?: unknown): void {
    this.#read().slice().forEach((v, i) => callback.call(thisArg, v, i, this))
  }

  keys(): IterableIterator<number> {
    return this.#read().slice().keys()
  }

  values(): IterableIterator<string> {
    return this.#read().slice().values()
  }

  entries(): IterableIterator<[number, string]> {
    return this.#read().slice().entries()
  }

  [Symbol.iterator](): IterableIterator<string> {
    return this.values()
  }

  toString(): string {
    return this.value
  }
}
tag(DOMTokenList)

export function tokenList(element: Element, key: symbol, attribute: string, supported: readonly string[] | null = null): DOMTokenList {
  const holder = element as unknown as Record<symbol, DOMTokenList | undefined>
  return (holder[key] ??= new DOMTokenList(element, attribute, supported))
}

export class DOMStringMap {}
tag(DOMStringMap)

function dataName(key: string): string | null {
  if (/-[a-z]/.test(key)) return null
  return `data-${key.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`
}

function dataKey(attribute: string): string | null {
  if (!attribute.startsWith('data-')) return null
  return attribute.slice(5).replace(/-([a-z])/g, (_, c: string) => c.toUpperCase())
}

export function dataset(element: Element): DOMStringMap {
  const holder = element as unknown as { [DATASET]: object | null }
  if (holder[DATASET]) return holder[DATASET] as DOMStringMap
  const map = new Proxy(new DOMStringMap(), {
    get(target, key) {
      if (typeof key !== 'string') return Reflect.get(target, key)
      const name = dataName(key)
      return name === null ? undefined : (dom.getAttribute(element[ID], name) ?? undefined)
    },
    set(target, key, value) {
      if (typeof key !== 'string') return Reflect.set(target, key, value)
      const name = dataName(key)
      if (name === null) throw new DOMException(`Failed to set a named property on 'DOMStringMap': '${key}' is not a valid property name.`, 'SyntaxError')
      setAttribute(element, name, String(value))
      return true
    },
    deleteProperty(_target, key) {
      if (typeof key !== 'string') return true
      const name = dataName(key)
      if (name !== null) setAttribute(element, name, null)
      return true
    },
    has(target, key) {
      if (typeof key !== 'string') return Reflect.has(target, key)
      const name = dataName(key)
      return name !== null && dom.hasAttribute(element[ID], name)
    },
    ownKeys() {
      const list = dom.attributes(element[ID])
      const keys: string[] = []
      for (let i = 0; i < list.length; i += 2) {
        const key = dataKey(list[i]!)
        if (key !== null) keys.push(key)
      }
      return keys
    },
    getOwnPropertyDescriptor(_target, key) {
      if (typeof key !== 'string') return undefined
      const name = dataName(key)
      const value = name === null ? null : dom.getAttribute(element[ID], name)
      return value === null ? undefined : { value, writable: true, enumerable: true, configurable: true }
    },
  })
  holder[DATASET] = map
  return map
}

const SEEN = Symbol('handlersSeen')

export function reflectsWindow(element: Element, type: string): boolean {
  if (!BODY_WINDOW_TYPES.has(type) || element[NS] !== Ns.Html || element[LOCAL] !== 'body') return false
  return (documents.others ? dom.ownerDocument(element[ID]) : documentId) === documentId
}

export function setWindowHandler(type: string, value: unknown): void {
  windowDecided.add(type)
  setHandler(G as unknown as EventTarget, type, value)
}

export function windowHandlerOf(type: string): void {
  if (!flags.inlineHandlers || windowDecided.has(type) || !BODY_WINDOW_TYPES.has(type)) return
  const body = bodyId()
  if (body === NONE || dom.localName(body) !== 'body') return
  const source = dom.getAttribute(body, `on${type}`)
  if (source === null) return
  windowDecided.add(type)
  if (getHandler(G as unknown as EventTarget, type) === null) setHandler(G as unknown as EventTarget, type, source)
}

export function windowHandlerSet(type: string): void {
  windowDecided.add(type)
}

export function handlerOf(element: Element, type: string): unknown {
  if (reflectsWindow(element, type)) return null
  if (!flags.inlineHandlers) return getHandler(element, type)
  const holder = element as unknown as { [SEEN]?: Set<string> }
  const seen = (holder[SEEN] ??= new Set())
  if (!seen.has(type)) {
    seen.add(type)
    const source = dom.getAttribute(element[ID], `on${type}`)
    if (source !== null && getHandler(element, type) === null) setHandler(element, type, source)
  }
  return getHandler(element, type)
}

export function handlerSet(element: Element, type: string): void {
  ;((element as unknown as { [SEEN]?: Set<string> })[SEEN] ??= new Set()).add(type)
}

