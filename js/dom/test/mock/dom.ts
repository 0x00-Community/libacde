import { PropertySymbol, Window } from 'happy-dom'
import type { AnimationEventRecord, AnimationEvents, DomNative, DomStats, Metrics } from '../../src/native.ts'

type HNode = any

export interface Box {
  x: number
  y: number
  width: number
  height: number
}

const NONE = -1
const NS = ['http://www.w3.org/1999/xhtml', 'http://www.w3.org/2000/svg', 'http://www.w3.org/1998/Math/MathML']

export class MockDom implements DomNative {
  readonly window: any
  readonly doc: any
  #next = 0
  #ids = new WeakMap<HNode, number>()
  #nodes = new Map<number, HNode>()
  readonly freed = new Set<number>()
  readonly pinned = new Set<number>()
  #created = new Set<number>()
  readonly calls = new Map<string, number>()
  readonly boxes = new Map<number, Box>()
  readonly scroll = new Map<number, { left: number; top: number }>()
  readonly images = new Map<number, number[]>()
  #scrolled: number[] = []
  readonly animationEvents: AnimationEventRecord[] = []
  #loaded: number[] = []
  readonly canvasCommands = new Map<number, Uint8Array[]>()
  focus = { id: NONE, visible: false }
  selection = [NONE, 0, NONE, 0]
  selectionRanges = new Map<number, [number, number]>()
  view = { width: 1280, height: 800, scale: 1 }
  media: Record<string, string> = {}

  constructor() {
    this.window = new Window({ url: 'http://acde.local/', width: 1280, height: 800, settings: {
      disableJavaScriptEvaluation: true, disableCSSFileLoading: true, disableJavaScriptFileLoading: true, handleDisabledFileLoadingAsSuccess: true,
      navigation: { disableChildFrameNavigation: true, disableChildPageNavigation: true, disableMainFrameNavigation: true },
    } as never })
    this.doc = this.window.document
    for (const name of Object.getOwnPropertyNames(MockDom.prototype)) {
      if (name === 'constructor' || name.startsWith('_')) continue
      const fn = (this as any)[name]
      if (typeof fn !== 'function') continue
      ;(this as any)[name] = (...args: unknown[]) => {
        this.calls.set(name, (this.calls.get(name) ?? 0) + 1)
        return fn.apply(this, args)
      }
    }
  }

  _id(node: HNode | null | undefined): number {
    if (!node) return NONE
    node = this._canon(node)
    let id = this.#ids.get(node)
    if (id === undefined) {
      id = this.#next++
      this.#ids.set(node, id)
      this.#nodes.set(id, node)
    }
    return id
  }

  _canon(node: HNode): HNode {
    return node[PropertySymbol.proxy as unknown as symbol] ?? node
  }

  _made(node: HNode): number {
    const id = this._id(node)
    this.#created.add(id)
    this.pinned.add(id)
    return id
  }

  _node(id: number): HNode {
    if (this.freed.has(id)) throw new Error(`NotFoundError: node ${id} was freed`)
    const node = this.#nodes.get(id)
    if (!node) throw new Error(`NotFoundError: no node ${id}`)
    return node
  }

  _ids(list: Iterable<HNode>): Uint32Array {
    return Uint32Array.from([...list].map((n) => this._id(n)))
  }

  _call<T>(fn: () => T): T {
    try {
      return fn()
    } catch (error: any) {
      const message = String(error?.message ?? error)
      const known = /^[A-Z][A-Za-z]*Error$/.test(error?.name ?? '') && error.name !== 'TypeError' ? error.name : null
      const name = known
        ?? (/hierarchy|parent of the node|child of itself|cannot be inserted|may not be inserted|only one element/i.test(message) ? 'HierarchyRequestError'
          : /not a valid selector|syntax|invalid selector/i.test(message) ? 'SyntaxError'
          : /invalid character|not a valid name|valid XML name/i.test(message) ? 'InvalidCharacterError'
          : /not a child|not found|does not exist/i.test(message) ? 'NotFoundError' : 'Error')
      throw new Error(`${name}: ${message}`)
    }
  }

  _root(node: HNode): HNode {
    let n = node
    while (n.parentNode) n = n.parentNode
    return n
  }

  _subtree(node: HNode, out: HNode[] = []): HNode[] {
    out.push(node)
    if (node.localName === 'template' && node.content) this._subtree(node.content, out)
    for (const child of [...node.childNodes]) this._subtree(child, out)
    return out
  }

  _maybeFree(node: HNode): void {
    const root = this._root(node)
    if (root === this.doc) return
    const all = this._subtree(root)
    if (all.some((n) => this.pinned.has(this.#ids.get(this._canon(n)) ?? NONE))) return
    for (const n of all) {
      const id = this.#ids.get(this._canon(n))
      if (id === undefined) continue
      this.#nodes.delete(id)
      this.#ids.delete(this._canon(n))
      this.freed.add(id)
    }
  }

  document(): number {
    return this._id(this.doc)
  }

  parseDocument(html: string): void {
    const old = [...this.doc.childNodes]
    this.doc.open()
    this.doc.write(html)
    this.doc.close()
    for (const node of old) this._maybeFree(node)
  }

  viewport(): { width: number; height: number; scale: number } {
    return { ...this.view }
  }

  nodeType(id: number): number {
    return this.#docs.has(id) ? 9 : this._node(id).nodeType
  }
  parent(id: number): number {
    return this._id(this._node(id).parentNode)
  }
  firstChild(id: number): number {
    return this._id(this._node(id).firstChild)
  }
  lastChild(id: number): number {
    return this._id(this._node(id).lastChild)
  }
  nextSibling(id: number): number {
    return this._id(this._node(id).nextSibling)
  }
  previousSibling(id: number): number {
    return this._id(this._node(id).previousSibling)
  }
  childNodes(id: number): Uint32Array {
    return this._ids(this._node(id).childNodes)
  }
  children(id: number): Uint32Array {
    const node = this._node(id)
    return this._ids([...node.childNodes].filter((n: HNode) => n.nodeType === 1))
  }
  childCount(id: number): number {
    return this._node(id).childNodes.length
  }
  isConnected(id: number): boolean {
    return this._root(this._node(id)) === this.doc
  }
  ancestors(id: number): Uint32Array {
    const out: HNode[] = []
    for (let n = this._node(id).parentNode; n; n = n.parentNode) out.push(n)
    return this._ids(out)
  }
  describe(id: number): [number, number, boolean, string] {
    const node = this._node(id)
    this.pinned.add(id)
    const doc = this.#docs.get(id)
    if (doc) return [9, 0, false, doc[0]!]
    const element = node.nodeType === 1
    return [node.nodeType, element ? this.namespace(id) : 0, this._root(node) === this.doc, element ? node.localName : '']
  }
  compareDocumentPosition(a: number, b: number): number {
    return this._node(a).compareDocumentPosition(this._node(b))
  }

  createElement(ns: number, localName: string): number {
    return this._call(() => this._made(ns === 0 ? this.doc.createElement(localName) : this.doc.createElementNS(NS[ns], localName)))
  }
  createText(data: string): number {
    return this._made(this.doc.createTextNode(data))
  }
  createComment(data: string): number {
    return this._made(this.doc.createComment(data))
  }
  createFragment(): number {
    return this._made(this.doc.createDocumentFragment())
  }
  createDocumentType(name: string, publicId: string, systemId: string): number {
    return this._made(this.doc.implementation.createDocumentType(name, publicId, systemId))
  }
  documentType(id: number): string[] {
    const node = this._node(id)
    return [node.name, node.publicId, node.systemId]
  }
  clone(id: number, deep: boolean): number {
    return this._made(this._node(id).cloneNode(deep))
  }

  insertBefore(parent: number, child: number, reference: number): void {
    const p = this._node(parent), c = this._node(child)
    const r = reference === NONE ? null : this._node(reference)
    this._call(() => p.insertBefore(c, r))
  }
  remove(child: number): void {
    const node = this._node(child)
    if (!node.parentNode) return
    node.parentNode.removeChild(node)
    this._maybeFree(node)
  }
  replaceChildren(parent: number, ids: Uint32Array): void {
    const p = this._node(parent)
    const old = [...p.childNodes]
    const nodes = [...ids].map((id) => this._node(id))
    this._call(() => p.replaceChildren(...nodes))
    for (const node of old) if (!node.parentNode) this._maybeFree(node)
  }

  localName(id: number): string {
    const node = this._node(id)
    return node.localName
  }
  namespace(id: number): number {
    const at = NS.indexOf(this._node(id).namespaceURI)
    return at < 0 ? 3 : at
  }
  namespaceURI(id: number): string {
    return this._node(id).namespaceURI ?? ''
  }
  prefix(id: number): string {
    return this._node(id).prefix ?? ''
  }
  attributesNS(id: number): string[] {
    const out: string[] = []
    for (const attr of [...this._node(id).attributes]) out.push(attr.namespaceURI ?? '', attr.prefix ?? '', attr.localName, attr.value)
    return out
  }
  getAttributeNS(id: number, namespace: string, localName: string): string | null {
    return this._node(id).getAttributeNS(namespace || null, localName)
  }
  setAttributeNS(id: number, namespace: string, prefix: string, localName: string, value: string): boolean {
    const node = this._node(id)
    const same = node.getAttributeNS(namespace || null, localName) === value
    this._call(() => node.setAttributeNS(namespace || null, prefix ? `${prefix}:${localName}` : localName, value))
    return !same
  }
  removeAttributeNS(id: number, namespace: string, localName: string): boolean {
    const node = this._node(id)
    const had = node.hasAttributeNS(namespace || null, localName)
    node.removeAttributeNS(namespace || null, localName)
    return had
  }
  #docs = new Map<number, string[]>()
  #owners = new Map<number, number>()
  createElementNS(namespace: string, prefix: string, localName: string): number {
    return this._call(() => this._made(this.doc.createElementNS(namespace || null, prefix ? `${prefix}:${localName}` : localName)))
  }
  createCDATASection(data: string): number {
    const xml = new this.window.DOMParser().parseFromString('<x/>', 'application/xml')
    return this._made(this.doc.adoptNode(xml.createCDATASection(data)))
  }
  createProcessingInstruction(target: string, data: string): number {
    return this._made(this.doc.createProcessingInstruction(target, data))
  }
  processingInstructionTarget(id: number): string {
    return this._node(id).target
  }
  createDocument(kind: string, contentType: string, url: string): number {
    const id = this._made(this.doc.createDocumentFragment())
    this.#docs.set(id, [kind, contentType, url])
    return id
  }
  documentInfo(id: number): string[] {
    return this.#docs.get(id) ?? ['html', 'text/html', '']
  }
  parseXml(text: string, contentType: string, url: string): number {
    const parsed = new this.window.DOMParser().parseFromString(text, contentType)
    const id = this.createDocument('document', contentType, url)
    const fragment = this._node(id)
    for (const child of [...parsed.childNodes]) fragment.appendChild(this.doc.adoptNode(child))
    return id
  }
  ownerDocument(id: number): number {
    const root = this._id(this._root(this._node(id)))
    if (this.#docs.has(root)) return root
    return this.#owners.get(id) ?? this.document()
  }
  adopt(id: number, document: number): void {
    if (document === this.document()) this.#owners.delete(id)
    else this.#owners.set(id, document)
  }
  inHtmlDocument(id: number): boolean {
    return this.documentInfo(this.ownerDocument(id))[0] === 'html'
  }
  serializeXml(id: number): string {
    return new this.window.XMLSerializer().serializeToString(this._node(id))
  }
  getElementsByTagNameNS(root: number, namespace: string, localName: string): Uint32Array {
    const all = [...this._node(root).querySelectorAll('*')]
    return this._ids(all.filter((el: HNode) => (namespace === '*' || (el.namespaceURI ?? '') === namespace) && (localName === '*' || el.localName === localName)))
  }
  getAttribute(id: number, name: string): string | null {
    return this._node(id).getAttribute(name)
  }
  setAttribute(id: number, name: string, value: string): boolean {
    const node = this._node(id)
    const same = node.getAttribute(name) === value
    this._call(() => node.setAttribute(name, value))
    if (node.localName === 'img' && (name === 'src' || name === 'srcset')) this._loadImage(id, value)
    return !same || name === 'value' || name === 'style' || (node.localName === 'canvas' && (name === 'width' || name === 'height'))
  }
  removeAttribute(id: number, name: string): boolean {
    const node = this._node(id)
    const had = node.hasAttribute(name)
    node.removeAttribute(name)
    return had
  }
  hasAttribute(id: number, name: string): boolean {
    return this._node(id).hasAttribute(name)
  }
  attributes(id: number): string[] {
    const out: string[] = []
    for (const attr of [...this._node(id).attributes]) out.push(attr.name, attr.value)
    return out
  }
  id(id: number): string {
    return this._node(id).getAttribute('id') ?? ''
  }
  className(id: number): string {
    return this._node(id).getAttribute('class') ?? ''
  }

  data(id: number): string {
    return this._node(id).data
  }
  setData(id: number, data: string): void {
    this._node(id).data = data
  }

  textContent(id: number): string {
    return this._node(id).textContent ?? ''
  }
  setTextContent(id: number, text: string): void {
    const node = this._node(id)
    const old = [...node.childNodes]
    node.textContent = text
    if (node.firstChild) this._id(node.firstChild)
    for (const child of old) this._maybeFree(child)
  }
  innerText(id: number): string {
    return this._node(id).textContent ?? ''
  }
  innerHTML(id: number): string {
    return this._node(id).innerHTML
  }
  outerHTML(id: number): string {
    return this._node(id).outerHTML
  }
  parseFragment(context: number, html: string): number {
    const ctx = this._node(context)
    const template = this.doc.createElement('template')
    const svg = ctx.namespaceURI === NS[1]
    template.innerHTML = svg ? `<svg>${html}</svg>` : html
    const fragment = this.doc.createDocumentFragment()
    const source = svg ? template.content.firstChild : template.content
    for (const child of [...source.childNodes]) fragment.appendChild(child)
    return this._made(fragment)
  }
  templateContent(id: number): number {
    return this._id(this._node(id).content)
  }
  parseInert(html: string, _url: string): number {
    const parsed = new this.window.DOMParser().parseFromString(html, 'text/html')
    const fragment = this.doc.createDocumentFragment()
    for (const child of [...parsed.childNodes]) fragment.appendChild(this.doc.adoptNode(child))
    return this._made(fragment)
  }

  querySelector(root: number, selector: string): number {
    return this._call(() => this._id(this._node(root).querySelector(selector)))
  }
  querySelectorAll(root: number, selector: string): Uint32Array {
    return this._call(() => this._ids(this._node(root).querySelectorAll(selector)))
  }
  matches(id: number, selector: string): boolean {
    return this._call(() => this._node(id).matches(selector))
  }
  closest(id: number, selector: string): number {
    return this._call(() => this._id(this._node(id).closest(selector)))
  }
  getElementById(id: string): number {
    return this._id(this.doc.getElementById(id))
  }
  getElementsByClassName(root: number, classNames: string): Uint32Array {
    const node = this._node(root)
    if (node.getElementsByClassName) return this._ids(node.getElementsByClassName(classNames))
    const selector = classNames.trim().split(/\s+/).filter(Boolean).map((c) => `.${c}`).join('')
    return selector ? this._ids(node.querySelectorAll(selector)) : new Uint32Array(0)
  }
  getElementsByTagName(root: number, name: string): Uint32Array {
    const node = this._node(root)
    return this._ids(node.getElementsByTagName ? node.getElementsByTagName(name) : node.querySelectorAll(name))
  }

  value(id: number): string {
    return this._node(id).value ?? ''
  }
  setValue(id: number, value: string): void {
    this._node(id).value = value
  }
  checked(id: number): boolean {
    return !!this._node(id).checked
  }
  setChecked(id: number, checked: boolean): void {
    this._node(id).checked = checked
  }
  selectedIndex(id: number): number {
    return this._node(id).selectedIndex
  }
  setSelectedIndex(id: number, index: number): void {
    this._node(id).selectedIndex = index
  }
  options(select: number): Uint32Array {
    const node = this._node(select)
    const list: HNode[] = []
    for (const child of node.children ?? []) {
      if (child.localName === 'option') list.push(child)
      else if (child.localName === 'optgroup') for (const inner of child.children) if (inner.localName === 'option') list.push(inner)
    }
    return this._ids(list)
  }
  optionSelected(option: number): boolean {
    return !!this._node(option).selected
  }
  setOptionSelected(option: number, selected: boolean): void {
    this._node(option).selected = selected
  }
  resetSelect(select: number): void {
    const node = this._node(select)
    for (const option of node.options) option.selected = option.hasAttribute('selected')
    if (!node.multiple && node.selectedIndex < 0 && node.options.length) node.selectedIndex = 0
  }
  listBox: (select: number, x: number, y: number) => number = () => -1
  listBoxOptionAt(select: number, x: number, y: number): number {
    return this.listBox(select, x, y)
  }
  picker = { select: NONE, active: -1, page: 8 }
  pickerHit: (x: number, y: number) => number = () => -2
  openPicker(select: number): boolean {
    const node = this._node(select)
    if (node.multiple || Number(node.getAttribute('size') ?? 0) > 1) return false
    this.picker = { select, active: node.selectedIndex, page: this.picker.page }
    return true
  }
  closePicker(): void {
    this.picker = { ...this.picker, select: NONE, active: -1 }
  }
  pickerState(): Float64Array {
    return Float64Array.of(this.picker.select, this.picker.active, this.picker.page)
  }
  pickerOptionAt(x: number, y: number): number {
    return this.picker.select === NONE ? -2 : this.pickerHit(x, y)
  }
  pickerOptionRect(_option: number): Float64Array | null {
    return null
  }
  setPickerActive(option: number): void {
    if (this.picker.select !== NONE) this.picker.active = option
  }
  setSelectionRange(id: number, start: number, end: number): void {
    this._node(id)
    this.selectionRanges.set(id, [start, end])
  }

  setFocus(id: number, visible: boolean): void {
    this.focus = { id, visible }
  }
  setActive(_id: number): void {
  }
  setSelection(anchorNode: number, anchorOffset: number, focusNode: number, focusOffset: number): void {
    this.selection = [anchorNode, anchorOffset, focusNode, focusOffset]
  }

  _hidden(node: HNode): boolean {
    for (let n = node; n && n.nodeType === 1; n = n.parentNode) {
      if (n.hasAttribute('hidden') || /display\s*:\s*none/.test(n.getAttribute('style') ?? '')) return true
    }
    return false
  }

  _box(id: number): Box | null {
    const node = this._node(id)
    if (this._root(node) !== this.doc || this._hidden(node)) return null
    return this.boxes.get(id) ?? { x: 0, y: 0, width: 0, height: 0 }
  }

  rect(id: number): Float64Array {
    const b = this._box(id)
    return b ? Float64Array.of(b.x, b.y, b.width, b.height) : new Float64Array(4)
  }
  clientRects(id: number): Float64Array {
    const b = this._box(id)
    return b ? Float64Array.of(b.x, b.y, b.width, b.height) : new Float64Array(0)
  }
  metrics(id: number): Metrics | null {
    const b = this._box(id)
    if (!b) return null
    const node = this._node(id)
    const s = this.scroll.get(id) ?? { left: 0, top: 0 }
    const parent = node.parentNode && node.parentNode.nodeType === 1 && node.localName !== 'body' ? this._id(node.parentNode) : NONE
    const pb = parent === NONE ? { x: 0, y: 0 } : (this.boxes.get(parent) ?? { x: 0, y: 0 })
    const content = this.boxes.get(Number(node.getAttribute?.('data-content-of') ?? NaN))
    return {
      offsetLeft: b.x - pb.x, offsetTop: b.y - pb.y, offsetWidth: b.width, offsetHeight: b.height, offsetParent: parent,
      clientLeft: 0, clientTop: 0, clientWidth: b.width, clientHeight: b.height,
      scrollLeft: s.left, scrollTop: s.top, scrollWidth: Math.max(b.width, content?.width ?? 0),
      scrollHeight: Math.max(b.height, Number(node.getAttribute?.('data-scroll-height') ?? 0)),
    }
  }
  hitTest(x: number, y: number): number {
    const all = this.hitTestAll(x, y)
    return all.length === 0 ? NONE : all[0]!
  }
  hitTestAll(x: number, y: number): Uint32Array {
    const hits: number[] = []
    for (const el of [...this.doc.querySelectorAll('*')]) {
      const id = this._id(el)
      const b = this.boxes.get(id)
      if (b && !this._hidden(el) && x >= b.x && y >= b.y && x < b.x + b.width && y < b.y + b.height) hits.push(id)
    }
    return Uint32Array.from(hits.reverse())
  }
  computedStyle(id: number, pseudo: string, property: string): string {
    const node = this._node(id)
    const style = this.window.getComputedStyle(node, pseudo || undefined)
    const value = style.getPropertyValue(property)
    if (value) return value
    if (/^(margin|padding)-/.test(property)) return '0px'
    if (property === 'width' || property === 'height') {
      const b = this._box(id)
      return b ? `${property === 'width' ? b.width : b.height}px` : 'auto'
    }
    return value
  }
  computedStyleAll(id: number, pseudo: string): string[] {
    const style = this.window.getComputedStyle(this._node(id), pseudo || undefined)
    const out: string[] = []
    for (let i = 0; i < style.length; i++) out.push(style.item(i), style.getPropertyValue(style.item(i)))
    return out
  }
  readonly supported: [string, string][] = []
  cssSupports(condition: string): boolean {
    this.supported.push(['condition', condition])
    return /display:\s*grid/.test(condition)
  }
  cssSupportsDeclaration(property: string, value: string): boolean {
    this.supported.push([property, value])
    return property === 'display' && value === 'grid'
  }
  caretRect(_id: number, _offset: number): Float64Array {
    return new Float64Array(4)
  }
  caret: ((x: number, y: number) => { node: number; offset: number } | null) | null = null
  caretFromPoint(x: number, y: number): { node: number; offset: number } | null {
    return this.caret ? this.caret(x, y) : null
  }
  textRects(_id: number, _start: number, _end: number): Float64Array {
    return new Float64Array(0)
  }
  parseColor(color: string): Float64Array | null {
    const named: Record<string, number[]> = {
      black: [0, 0, 0], white: [255, 255, 255], red: [255, 0, 0], darkblue: [0, 0, 139], rebeccapurple: [102, 51, 153], transparent: [0, 0, 0, 0],
    }
    const text = color.trim().toLowerCase()
    const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/.exec(text)
    let rgba: number[] | undefined = named[text]
    if (hex) {
      const h = hex[1]!.length === 3 ? [...hex[1]!].map((c) => c + c).join('') : hex[1]!
      rgba = [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)]
    }
    const rgb = /^rgba?\(\s*(\d+)[\s,]+(\d+)[\s,]+(\d+)(?:[\s,/]+([\d.]+))?\s*\)$/.exec(text)
    if (rgb) rgba = [Number(rgb[1]), Number(rgb[2]), Number(rgb[3]), rgb[4] === undefined ? 1 : Number(rgb[4])]
    if (text === 'oklch(0.628 0.2577 29.23)') rgba = [255, 0, 0]
    if (!rgba) return null
    return Float64Array.of(rgba[0]! / 255, rgba[1]! / 255, rgba[2]! / 255, rgba[3] ?? 1)
  }
  sheetRuleCount(id: number): number {
    const node = this._node(id)
    if (node.localName === 'link') return this._root(node) === this.doc && this.sheets.get(id) === 1 ? (this.sheetSources.get(id)?.rules.length ?? 0) : -1
    const sheet = node.sheet
    return this._root(node) === this.doc && sheet ? sheet.cssRules.length : -1
  }
  sheets = new Map<number, number>()
  sheetSources = new Map<number, { url: string; opaque: boolean; serial: number; rules: string[] }>()
  #sheetSerials = 0
  sheetState(id: number): number {
    return this._root(this._node(id)) === this.doc ? (this.sheets.get(id) ?? 3) : 3
  }
  sheetUrl(id: number): string | null {
    return this.sheetRuleCount(id) < 0 ? null : (this.sheetSources.get(id)?.url ?? null)
  }
  sheetOpaque(id: number): boolean {
    return this.sheetRuleCount(id) >= 0 && !!this.sheetSources.get(id)?.opaque
  }
  sheetSerial(id: number): number {
    return this.sheetRuleCount(id) < 0 ? -1 : (this.sheetSources.get(id)?.serial ?? -1)
  }
  _sheet(id: number, state: number, opaque = false, url = new URL(this._node(id).getAttribute('href') ?? '', 'http://acde.local/').href, rules: string[] = []): void {
    this.sheets.set(id, state)
    if (state === 1) this.sheetSources.set(id, { url, opaque, serial: ++this.#sheetSerials, rules })
    if (state === 2) this.sheetSources.delete(id)
    if (state === 1 || state === 2) this.#loaded.push(id)
  }
  sheetRule(id: number, index: number): string {
    if (this._node(id).localName === 'link') return this.sheetSources.get(id)!.rules[index]!
    return this._node(id).sheet.cssRules[index].cssText
  }
  sheetInsertRule(id: number, rule: string, index: number): number {
    if (!/^[^{}]+\{[^{}]*\}$|^@[^{}]+(\{[\s\S]*\})?;?$/.test(rule.trim())) throw new Error(`SyntaxError: cannot parse '${rule}'`)
    return this._call(() => this._node(id).sheet.insertRule(rule, index))
  }
  sheetDeleteRule(id: number, index: number): void {
    this._call(() => this._node(id).sheet.deleteRule(index))
  }
  matchMedia(query: string): boolean {
    const m = /^\s*\(\s*([a-z-]+)\s*:\s*([a-z0-9-]+)\s*\)\s*$/.exec(query)
    if (m && m[1]! in this.media) return this.media[m[1]!] === m[2]
    const width = /\(\s*(min|max)-width\s*:\s*(\d+)px\s*\)/.exec(query)
    if (width) return width[1] === 'min' ? this.view.width >= Number(width[2]) : this.view.width <= Number(width[2])
    return false
  }

  scrollTo(id: number, left: number, top: number, _smooth: boolean): void {
    const node = this._node(id)
    const b = this.boxes.get(id) ?? { width: 0, height: 0 }
    const maxTop = Math.max(0, Number(node.getAttribute?.('data-scroll-height') ?? 0) - b.height)
    const s = { left: Math.max(0, left), top: Math.min(Math.max(0, top), maxTop) }
    const old = this.scroll.get(id)
    if (old && old.left === s.left && old.top === s.top) return
    this.scroll.set(id, s)
    this.#scrolled.push(id)
  }
  takeScrolled(): Uint32Array {
    const out = Uint32Array.from(new Set(this.#scrolled))
    this.#scrolled = []
    return out
  }
  _scrollNatively(id: number, top: number): void {
    this.scroll.set(id, { left: 0, top })
    this.#scrolled.push(id)
  }

  _loadImage(id: number, src: string): void {
    this.images.set(id, [0, 0, 0])
    if (/held/.test(src)) return
    setTimeout(() => {
      if (this.freed.has(id)) return
      this.images.set(id, /missing/.test(src) ? [0, 0, 2] : [16, 8, 1])
      this.#loaded.push(id)
    }, 1)
  }
  _imageDone(id: number, state: number[]): void {
    this.images.set(id, state)
    this.#loaded.push(id)
  }
  imageState(id: number): Float64Array {
    const node = this._node(id)
    if (!node.getAttribute('src')) return Float64Array.of(0, 0, 3)
    return Float64Array.from(this.images.get(id) ?? [0, 0, 0])
  }
  readonly openStates = new Map<number, { modal: boolean; open: boolean }>()
  setOpenState(id: number, modal: boolean, open: boolean): void {
    this.openStates.set(id, { modal, open })
  }

  takeAnimationEvents(): AnimationEvents {
    return { animating: false, events: this.animationEvents.splice(0) }
  }

  takeLoaded(): Uint32Array {
    const out = Uint32Array.from(this.#loaded)
    this.#loaded = []
    return out
  }
  loader: ((id: number, url: string, destination: string, cors: string, served?: boolean, cached?: boolean) => void) | null = null
  loads = new Map<number, Uint8Array | null>()
  #sheetLoads = new Map<number, { link: number; url: string }>()
  setLoader(load: ((id: number, url: string, destination: string, cors: string, served?: boolean, cached?: boolean) => void) | null): void {
    this.loader = load
  }
  opaqueLoads = new Set<number>()
  loaded(id: number, bytes: Uint8Array | null, opaque: boolean): void {
    this.loads.set(id, bytes && bytes.slice())
    if (opaque) this.opaqueLoads.add(id)
    const sheet = this.#sheetLoads.get(id)
    if (sheet) this._sheet(sheet.link, bytes ? 1 : 2, opaque, sheet.url)
  }
  _fetchSheet(link: number, url: string, cors = ''): void {
    const id = 1000 + this.#sheetLoads.size
    this.#sheetLoads.set(id, { link, url })
    this.sheets.set(link, 0)
    this.loader!(id, url, 'style', cors)
  }
  canvas(id: number, commands: Uint8Array): void {
    this._node(id)
    const list = this.canvasCommands.get(id) ?? []
    list.push(commands.slice())
    this.canvasCommands.set(id, list)
  }
  measureText(_id: number, _font: string, text: string): Float64Array {
    return Float64Array.of(text.length * 6, 0, text.length * 6, 8, 2, 9, 3)
  }
  toDataURL(_id: number, _type: string): string {
    return 'data:image/png;base64,iVBORw0KGgo='
  }
  getImageData(_id: number, _x: number, _y: number, width: number, height: number): Uint8Array {
    return new Uint8Array(Math.abs(width * height * 4))
  }
  capture(): null {
    return null
  }

  setPanel(): void {}

  unpin(id: number): void {
    this.pinned.delete(id)
    const node = this.#nodes.get(id)
    if (node) this._maybeFree(node)
  }
  pinnedIn(root: number): Uint32Array {
    const out: number[] = []
    for (const node of this._subtree(this._node(root))) {
      const id = this.#ids.get(this._canon(node))
      if (id !== undefined && this.pinned.has(id)) out.push(id)
    }
    return Uint32Array.from(out)
  }

  commit(): void {
    for (const id of this.#created) {
      const node = this.#nodes.get(id)
      if (node && !this.pinned.has(id)) this._maybeFree(node)
    }
    this.#created.clear()
  }

  stats(): DomStats {
    const none = { count: 0, totalMs: 0, maxMs: 0 }
    return {
      resolve: none, frame: none, frameLocked: none, resolveMs: 0, paintMs: 0, damageMs: 0, layersMs: 0, fragmentsRecorded: 0, fragmentsReused: 0,
      animating: false, nodes: this.#nodes.size, images: 0, imageBytes: 0, mallocBytes: 0,
    }
  }

  node(id: number): HNode {
    return this._node(id)
  }
}
