import { DOMException, guard } from '../core/exception'
import { BUILD_PATH, EventTarget, type Event } from '../core/event'
import { constants, getters, tag } from '../core/host'
import { NONE, NodeType, Ns } from '../native'
import { legacy, type LegacyShape } from './legacy'
import { characterDataObservers, childListObservers, observing, recordCharacterData, recordChildList, type Observers } from './mutation'
import { adopt, attach, changed, CONNECTED, constructing, detach, documentId, dom, ID, nodeConstructing, treeChanged, treeVersion, version, wrap, type Wrapper, inner } from './realm'
import type { Document } from './document'
import type { Element } from './element'
import { documents, locateNamespace, lookupPrefixOf, namespaceOf, nullable } from './namespaces'

export const ANY_PARENT = -2

export const rangeHooks = {
  active: () => false,
  relevant: (_parent: number, _inside: Node | null) => false,
  removing: (_parent: number, _index: number, _node: Node) => {},
  inserted: (_parent: number, _index: number, _count: number) => {},
  replacing: (_parent: Node) => {},
  replaced: (_node: number, _offset: number, _count: number, _length: number) => {},
  split: (_node: number, _tail: number, _offset: number) => {},
}

export const pathHooks = {
  build: null as unknown as (node: Node, event: Event, path: EventTarget[]) => void,
}

const CHILD_LIST = Symbol('childList')

const SOURCE = Symbol('source')
const CACHE = Symbol('cache')
const CACHE_VERSION = Symbol('cacheVersion')
const TREE = Symbol('tree')
const ITEMS = Symbol('items')

const HANDLER = Symbol('handler')

let listConstructing = false

class LiveList {
  [SOURCE]: (() => Uint32Array) | null = null;
  [CACHE]: Uint32Array | null = null;
  [CACHE_VERSION] = -1;
  [TREE] = false
  declare [HANDLER]: ProxyHandler<LiveList>

  constructor() {
    if (!listConstructing) throw new TypeError('Illegal constructor')
  }
}

export function liveIds(list: LiveList): Uint32Array {
  const now = list[TREE] ? treeVersion : version
  if (list[CACHE_VERSION] !== now || !list[CACHE]) {
    list[CACHE] = list[SOURCE]!()
    list[CACHE_VERSION] = now
  }
  return list[CACHE]
}

export function listShape<T extends LiveList>(cls: { prototype: T }, shape: LegacyShape<T>): void {
  Object.defineProperty(cls.prototype, HANDLER, { value: legacy(shape), configurable: true })
}

export class NodeList extends LiveList {
  [ITEMS]: Node[] | null = null

  get length(): number {
    return this[ITEMS] ? this[ITEMS].length : liveIds(this).length
  }

  item(index: number): Node | null {
    const i = Number(index) >>> 0
    if (this[ITEMS]) return this[ITEMS][i] ?? null
    const ids = liveIds(this)
    return i < ids.length ? wrap<Node>(ids[i]!) : null
  }

  forEach(callback: (node: Node, index: number, list: NodeList) => void, thisArg?: unknown): void {
    const n = this.length
    for (let i = 0; i < n; i++) {
      const node = this.item(i)
      if (node) callback.call(thisArg, node, i, this)
    }
  }

  *entries(): IterableIterator<[number, Node]> {
    for (let i = 0; i < this.length; i++) yield [i, this.item(i)!]
  }

  *keys(): IterableIterator<number> {
    for (let i = 0; i < this.length; i++) yield i
  }

  *values(): IterableIterator<Node> {
    for (let i = 0; i < this.length; i++) yield this.item(i)!
  }

  [Symbol.iterator](): IterableIterator<Node> {
    return this.values()
  }
}
tag(NodeList)
listShape(NodeList, {
  length: (list) => liveIds(list).length,
  item: (list, index) => {
    const ids = liveIds(list)
    return index < ids.length ? wrap<Node>(ids[index]!) : null
  },
})

export function staticNodeList(nodes: readonly Node[]): NodeList {
  listConstructing = true
  try {
    const list = new NodeList()
    list[ITEMS] = nodes as Node[]
    for (let i = 0; i < nodes.length; i++) Object.defineProperty(list, i, { value: nodes[i], enumerable: true, configurable: true })
    return list
  } finally {
    listConstructing = false
  }
}

export function staticNodeListOfIds(ids: Uint32Array): NodeList {
  const nodes: Node[] = new Array(ids.length)
  for (let i = 0; i < ids.length; i++) nodes[i] = wrap<Node>(ids[i]!)!
  return staticNodeList(nodes)
}

export function namedBy(id: number, key: string): boolean {
  return dom.id(id) === key || (dom.getAttribute(id, 'name') === key && dom.namespace(id) === Ns.Html)
}

export function namesOf(ids: Uint32Array): string[] {
  const names: string[] = []
  for (let i = 0; i < ids.length; i++) {
    const id = dom.id(ids[i]!)
    if (id) names.push(id)
    const name = dom.namespace(ids[i]!) === Ns.Html ? dom.getAttribute(ids[i]!, 'name') : null
    if (name) names.push(name)
  }
  return names
}

export class HTMLCollection extends LiveList {
  get length(): number {
    return liveIds(this).length
  }

  item(index: number): Element | null {
    const ids = liveIds(this)
    const i = Number(index) >>> 0
    return i < ids.length ? wrap<Element>(ids[i]!) : null
  }

  namedItem(name: string): Element | null {
    const key = String(name)
    if (!key) return null
    const ids = liveIds(this)
    for (let i = 0; i < ids.length; i++) if (namedBy(ids[i]!, key)) return wrap<Element>(ids[i]!)
    return null
  }

  *[Symbol.iterator](): IterableIterator<Element> {
    for (let i = 0; i < this.length; i++) yield this.item(i)!
  }
}
tag(HTMLCollection)

export function collectionShape<T extends HTMLCollection>(extra: LegacyShape<T> = {}): LegacyShape<T> {
  return {
    length: (list) => liveIds(list).length,
    item: (list, index) => {
      const ids = liveIds(list)
      return index < ids.length ? wrap<Element>(ids[index]!) : null
    },
    named: (list, key) => list.namedItem(key) ?? undefined,
    names: (list) => namesOf(liveIds(list)),
    ...extra,
  }
}
listShape(HTMLCollection, collectionShape())

export function liveCollection<T extends LiveList>(source: () => Uint32Array, cls: new () => T = HTMLCollection as never, tree = false): T {
  listConstructing = true
  try {
    const list = new cls()
    list[SOURCE] = source
    list[TREE] = tree
    return new Proxy(list, list[HANDLER] as ProxyHandler<T>)
  } finally {
    listConstructing = false
  }
}

export function cssString(value: string): string {
  return `"${value.replace(/["\\\n\r\f]/g, (c) => (c === '"' || c === '\\' ? `\\${c}` : `\\${c.charCodeAt(0).toString(16)} `))}"`
}

export function elementById(root: number, id: string): Element | null {
  return wrap<Element>(guard(() => dom.querySelector(root, `[id=${cssString(id)}]`)))
}

function nodeArg(value: unknown, method: string, iface = 'Node'): Node {
  if (!(value instanceof Node)) throw new TypeError(`Failed to execute '${method}' on '${iface}': parameter 1 is not of type 'Node'.`)
  return value
}

function convertNodes(nodes: readonly unknown[]): Node | null {
  if (nodes.length === 0) return null
  if (nodes.length === 1) {
    const only = nodes[0]
    return only instanceof Node ? only : new Text(String(only))
  }
  const fragment = new DocumentFragment()
  for (const item of nodes) insert(fragment, item instanceof Node ? item : new Text(String(item)), null)
  return fragment
}

function indexIn(parent: number, child: number): number {
  return dom.childNodes(parent).indexOf(child)
}

function siblings(id: number): [Node | null, Node | null] {
  return [wrap<Node>(dom.previousSibling(id)), wrap<Node>(dom.nextSibling(id))]
}

export function insert(parent: Node, node: Node, ref: Node | null): void {
  const pid = parent[ID], nid = node[ID], rid = ref ? ref[ID] : NONE
  const fragment = inner(node, 'nodeType') === NodeType.DocumentFragment
  const mo = observing(), ranges = rangeHooks.active()
  let oldParent = NONE
  let oldObservers: Observers | null = null
  let oldPrev: Node | null = null, oldNext: Node | null = null
  if (!fragment && (mo || (ranges && rangeHooks.relevant(ANY_PARENT, node)))) {
    oldParent = dom.parent(nid)
    if (oldParent !== NONE) {
      oldObservers = mo ? childListObservers(oldParent) : null
      if (oldObservers) [oldPrev, oldNext] = siblings(nid)
      if (ranges && rangeHooks.relevant(oldParent, node)) rangeHooks.removing(oldParent, indexIn(oldParent, nid), node)
    }
  }
  const observers = mo ? childListObservers(pid) : null
  const emptied = mo && fragment ? childListObservers(nid) : null
  const shifts = ranges && rangeHooks.relevant(pid, null)
  const children = fragment && (observers || emptied || shifts) ? dom.childNodes(nid) : null
  const previous = observers ? wrap<Node>(rid === NONE ? dom.lastChild(pid) : dom.previousSibling(rid)) : null
  const wasConnected = fragment ? false : node[CONNECTED]
  const connected = parent[CONNECTED]
  const pinned = wasConnected !== connected ? dom.pinnedIn(nid) : null
  guard(() => dom.insertBefore(pid, nid, rid))
  treeChanged()
  if (pinned) {
    if (connected) attach(fragment ? pinned.filter((id) => id !== nid) : pinned)
    else detach(pinned)
  }
  if (shifts) {
    const count = children ? children.length : 1
    if (count > 0) rangeHooks.inserted(pid, indexIn(pid, children ? children[0]! : nid), count)
  }
  if (oldObservers) recordChildList(oldParent, oldObservers, [], [node], oldPrev, oldNext)
  if (observers || emptied) {
    const added: Node[] = []
    if (children) for (const id of children) added.push(wrap<Node>(id)!)
    else added.push(node)
    if (emptied && added.length) recordChildList(nid, emptied, [], added, null, null)
    if (observers && added.length) recordChildList(pid, observers, added, [], previous, ref)
  }
}

export function remove(node: Node): void {
  const nid = node[ID]
  const mo = observing(), ranges = rangeHooks.active() && rangeHooks.relevant(ANY_PARENT, node)
  if (!mo && !ranges) {
    const pinned = node[CONNECTED] ? dom.pinnedIn(nid) : null
    guard(() => dom.remove(nid))
    treeChanged()
    if (pinned) detach(pinned)
    return
  }
  const pid = dom.parent(nid)
  if (pid === NONE) return
  const observers = mo ? childListObservers(pid) : null
  const [prev, next] = observers ? siblings(nid) : [null, null]
  if (ranges && rangeHooks.relevant(pid, node)) rangeHooks.removing(pid, indexIn(pid, nid), node)
  const pinned = node[CONNECTED] ? dom.pinnedIn(nid) : null
  guard(() => dom.remove(nid))
  treeChanged()
  if (pinned) detach(pinned)
  if (observers) recordChildList(pid, observers, [], [node], prev, next)
}

export function replaceAll(parent: Node, incoming: readonly Node[], mutate: () => void): void {
  const pid = parent[ID]
  const observers = observing() ? childListObservers(pid) : null
  const removed: Node[] = []
  if (observers) for (const id of dom.childNodes(pid)) removed.push(wrap<Node>(id)!)
  if (rangeHooks.active() && rangeHooks.relevant(pid, parent)) rangeHooks.replacing(parent)
  const connected = parent[CONNECTED]
  let before: Uint32Array | null = null
  if (connected) before = dom.pinnedIn(pid)
  else for (const node of incoming) if (node[CONNECTED]) detach(dom.pinnedIn(node[ID]))
  const added: Node[] = []
  if (observers) for (const node of incoming) {
    if (dom.nodeType(node[ID]) === NodeType.DocumentFragment) for (const id of dom.childNodes(node[ID])) added.push(wrap<Node>(id)!)
    else added.push(node)
  }
  guard(mutate)
  treeChanged()
  if (connected) {
    const after = dom.pinnedIn(pid)
    const was = new Set(before!)
    const now = new Set(after)
    const gone: number[] = [], came: number[] = []
    for (const id of before!) if (!now.has(id)) gone.push(id)
    for (const id of after) if (!was.has(id)) came.push(id)
    if (gone.length) detach(gone)
    if (came.length) attach(came)
  }
  if (observers) {
    if (added.length === 0 && incoming.length === 0) {
      const first = dom.firstChild(pid)
      if (first !== NONE) added.push(wrap<Node>(first)!)
    }
    if (added.length || removed.length) recordChildList(pid, observers, added, removed, null, null)
  }
}

export function replaceData(node: CharacterData, offset: number, count: number, data: string, whole: string | null = null): void {
  const id = node[ID]
  const old = whole ?? dom.data(id)
  if (offset > old.length) throw new DOMException(`The offset ${offset} is larger than the node's length (${old.length}).`, 'IndexSizeError')
  const end = Math.min(old.length, offset + count)
  const next = old.slice(0, offset) + data + old.slice(end)
  const interest = observing() ? characterDataObservers(id) : null
  dom.setData(id, next)
  changed()
  if (rangeHooks.active()) rangeHooks.replaced(id, offset, end - offset, data.length)
  if (interest) recordCharacterData(node, interest, old)
}

export class Node extends EventTarget implements Wrapper {

  declare [ID]: number
  declare [CONNECTED]: boolean
  [CHILD_LIST]: NodeList | null = null

  constructor() {
    super()
    const outer = adopt(this)
    if (outer !== this) return outer
  }

  get nodeType(): number {
    return 0
  }

  get nodeName(): string {
    return ''
  }

  get nodeValue(): string | null {
    return null
  }
  set nodeValue(_value: string | null) {}

  get textContent(): string | null {
    return dom.textContent(this[ID])
  }
  set textContent(value: string | null) {
    setTextContent(this, value === null || value === undefined ? '' : String(value))
  }

  get baseURI(): string {
    return baseHooks.url()
  }

  get isConnected(): boolean {
    return this[CONNECTED]
  }

  get ownerDocument(): Document | null {
    return wrap<Document>(documents.others ? dom.ownerDocument(this[ID]) : documentId)
  }

  get parentNode(): Node | null {
    return wrap<Node>(dom.parent(this[ID]))
  }

  get parentElement(): Element | null {
    const parent = dom.parent(this[ID])
    return parent !== NONE && dom.nodeType(parent) === NodeType.Element ? wrap<Element>(parent) : null
  }

  get childNodes(): NodeList {
    let list = this[CHILD_LIST]
    if (!list) {
      const id = this[ID]
      list = this[CHILD_LIST] = liveCollection(() => dom.childNodes(id), NodeList, true)
    }
    return list
  }

  get firstChild(): Node | null {
    return wrap<Node>(dom.firstChild(this[ID]))
  }

  get lastChild(): Node | null {
    return wrap<Node>(dom.lastChild(this[ID]))
  }

  get previousSibling(): Node | null {
    return wrap<Node>(dom.previousSibling(this[ID]))
  }

  get nextSibling(): Node | null {
    return wrap<Node>(dom.nextSibling(this[ID]))
  }

  hasChildNodes(): boolean {
    return dom.firstChild(this[ID]) !== NONE
  }

  getRootNode(_options?: { composed?: boolean }): Node {
    const ancestors = dom.ancestors(this[ID])
    return ancestors.length ? wrap<Node>(ancestors[ancestors.length - 1]!)! : this
  }

  contains(other: Node | null): boolean {
    if (other === null || other === undefined) return false
    if (!(other instanceof Node)) throw new TypeError("Failed to execute 'contains' on 'Node': parameter 1 is not of type 'Node'.")
    if (other === this) return true
    return (dom.compareDocumentPosition(this[ID], other[ID]) & 16) !== 0
  }

  compareDocumentPosition(other: Node): number {
    nodeArg(other, 'compareDocumentPosition')
    if (other === this) return 0
    return dom.compareDocumentPosition(this[ID], other[ID])
  }

  isSameNode(other: Node | null): boolean {
    return other === this
  }

  isEqualNode(other: Node | null): boolean {
    if (!other || !(other instanceof Node)) return false
    return equalNodes(this, other)
  }

  cloneNode(deep = false): Node {
    return cloneOf(this, !!deep)
  }

  normalize(): void {
    const walk = (parent: number) => {
      let child = dom.firstChild(parent)
      while (child !== NONE) {
        const next = dom.nextSibling(child)
        if (dom.nodeType(child) === NodeType.Text) {
          let data = dom.data(child)
          if (data === '') {
            remove(wrap<Node>(child)!)
            child = next
            continue
          }
          let sibling = next
          while (sibling !== NONE && dom.nodeType(sibling) === NodeType.Text) {
            const after = dom.nextSibling(sibling)
            data += dom.data(sibling)
            remove(wrap<Node>(sibling)!)
            sibling = after
          }
          if (data !== dom.data(child)) replaceData(wrap<Text>(child)!, 0, Infinity, data)
          child = sibling
          continue
        }
        if (dom.nodeType(child) === NodeType.Element) walk(child)
        child = next
      }
    }
    walk(this[ID])
  }

  appendChild<T extends Node>(node: T): T {
    nodeArg(node, 'appendChild')
    insert(this, node, null)
    return node
  }

  insertBefore<T extends Node>(node: T, child: Node | null): T {
    nodeArg(node, 'insertBefore')
    if (child !== null && child !== undefined && !(child instanceof Node)) throw new TypeError("Failed to execute 'insertBefore' on 'Node': parameter 2 is not of type 'Node'.")
    const ref = child === node ? inner(node, 'nextSibling') : (child ?? null)
    insert(this, node, ref)
    return node
  }

  removeChild<T extends Node>(child: T): T {
    nodeArg(child, 'removeChild')
    if (dom.parent(child[ID]) !== this[ID]) throw new DOMException("Failed to execute 'removeChild' on 'Node': The node to be removed is not a child of this node.", 'NotFoundError')
    remove(child)
    return child
  }

  replaceChild<T extends Node>(node: Node, child: T): T {
    nodeArg(node, 'replaceChild')
    nodeArg(child, 'replaceChild')
    if (dom.parent(child[ID]) !== this[ID]) throw new DOMException("Failed to execute 'replaceChild' on 'Node': The node to be replaced is not a child of this node.", 'NotFoundError')
    if (node === child) return child
    let ref = inner(child, 'nextSibling')
    if (ref === node) ref = inner(node, 'nextSibling')
    remove(child)
    insert(this, node, ref)
    return child
  }

  lookupNamespaceURI(prefix: string | null): string | null {
    return locateNamespace(this[ID], nullable(prefix))
  }

  lookupPrefix(namespace: string | null): string | null {
    return lookupPrefixOf(this[ID], nullable(namespace))
  }

  isDefaultNamespace(namespace: string | null): boolean {
    return locateNamespace(this[ID], null) === nullable(namespace)
  }

  [BUILD_PATH](event: Event, path: EventTarget[]): void {
    pathHooks.build(this, event, path)
  }
}
constants(Node, { ELEMENT_NODE: 1, ATTRIBUTE_NODE: 2, TEXT_NODE: 3, CDATA_SECTION_NODE: 4, ENTITY_REFERENCE_NODE: 5, ENTITY_NODE: 6, PROCESSING_INSTRUCTION_NODE: 7, COMMENT_NODE: 8, DOCUMENT_NODE: 9, DOCUMENT_TYPE_NODE: 10, DOCUMENT_FRAGMENT_NODE: 11, NOTATION_NODE: 12, DOCUMENT_POSITION_DISCONNECTED: 1, DOCUMENT_POSITION_PRECEDING: 2, DOCUMENT_POSITION_FOLLOWING: 4, DOCUMENT_POSITION_CONTAINS: 8, DOCUMENT_POSITION_CONTAINED_BY: 16, DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC: 32 })
tag(Node)

function equalNodes(one: Node, other: Node): boolean {
  if (other === one) return true
  const type = dom.nodeType(one[ID])
  if (type !== dom.nodeType(other[ID])) return false
  switch (type) {
    case NodeType.Element: return equalElements(one[ID], other[ID])
    case NodeType.Text:
    case NodeType.CDataSection:
    case NodeType.Comment: return dom.data(one[ID]) === dom.data(other[ID])
    case NodeType.ProcessingInstruction: return dom.data(one[ID]) === dom.data(other[ID]) && dom.processingInstructionTarget(one[ID]) === dom.processingInstructionTarget(other[ID])
    case NodeType.DocumentType: {
      const a = dom.documentType(one[ID]), b = dom.documentType(other[ID])
      return a[0] === b[0] && a[1] === b[1] && a[2] === b[2]
    }
    default: {
      const a = dom.childNodes(one[ID]), b = dom.childNodes(other[ID])
      return a.length === b.length && a.every((id, i) => equalNodes(wrap<Node>(id)!, wrap<Node>(b[i]!)!))
    }
  }
}

function equalElements(a: number, b: number): boolean {
  if (dom.localName(a) !== dom.localName(b) || namespaceOf(a) !== namespaceOf(b) || dom.prefix(a) !== dom.prefix(b)) return false
  const attrs = (id: number) => {
    const list = dom.attributesNS(id)
    const map = new Map<string, string>()
    for (let i = 0; i < list.length; i += 4) map.set(`${list[i]} ${list[i + 2]}`, list[i + 3]!)
    return map
  }
  const x = attrs(a), y = attrs(b)
  if (x.size !== y.size) return false
  for (const [k, v] of x) if (y.get(k) !== v) return false
  const ca = dom.childNodes(a), cb = dom.childNodes(b)
  return ca.length === cb.length && ca.every((id, i) => equalNodes(wrap<Node>(id)!, wrap<Node>(cb[i]!)!))
}

export const baseHooks = { url: () => '' }

export const clonedHooks: ((clone: Node) => void)[] = []

export function cloneOf(node: Node, deep: boolean): Node {
  const clone = wrap<Node>(guard(() => dom.clone(node[ID], deep)))!
  for (const hook of clonedHooks) hook(clone)
  return clone
}

export function setTextContent(node: Node, text: string): void {
  const type = inner(node, 'nodeType')
  if (type === NodeType.Element || type === NodeType.DocumentFragment) {
    replaceAll(node, [], () => dom.setTextContent(node[ID], text))
  } else if (type === NodeType.Text || type === NodeType.Comment || type === NodeType.CDataSection || type === NodeType.ProcessingInstruction) {
    replaceData(node as CharacterData, 0, Infinity, text)
  }
}

function elementFrom(id: number, forward: boolean): Element | null {
  while (id !== NONE && dom.nodeType(id) !== NodeType.Element) id = forward ? dom.nextSibling(id) : dom.previousSibling(id)
  return wrap<Element>(id)
}

function viable(ref: Node | null, nodes: unknown[], forward: boolean): Node | null {
  while (ref && nodes.includes(ref)) ref = forward ? inner(ref, 'nextSibling') : inner(ref, 'previousSibling')
  return ref
}

export class ParentNodeMixin {
  get children(): HTMLCollection {
    const self = this as unknown as Node & { [CHILDREN]?: HTMLCollection }
    let list = self[CHILDREN]
    if (!list) {
      const id = self[ID]
      list = self[CHILDREN] = liveCollection(() => dom.children(id), HTMLCollection, true)
    }
    return list
  }

  get firstElementChild(): Element | null {
    return elementFrom(dom.firstChild((this as unknown as Node)[ID]), true)
  }

  get lastElementChild(): Element | null {
    return elementFrom(dom.lastChild((this as unknown as Node)[ID]), false)
  }

  get childElementCount(): number {
    return dom.children((this as unknown as Node)[ID]).length
  }

  append(this: Node, ...nodes: unknown[]): void {
    const node = convertNodes(nodes)
    if (node) insert(this, node, null)
  }

  prepend(this: Node, ...nodes: unknown[]): void {
    const node = convertNodes(nodes)
    if (node) insert(this, node, inner(this, 'firstChild'))
  }

  replaceChildren(this: Node, ...nodes: unknown[]): void {
    const node = convertNodes(nodes)
    const incoming = node ? [node] : []
    const id = this[ID]
    replaceAll(this, incoming, () => dom.replaceChildren(id, Uint32Array.from(incoming, (n) => n[ID])))
  }

  querySelector(this: Node, selectors: string): Element | null {
    return wrap<Element>(guard(() => dom.querySelector(this[ID], String(selectors))))
  }

  querySelectorAll(this: Node, selectors: string): NodeList {
    return staticNodeListOfIds(guard(() => dom.querySelectorAll(this[ID], String(selectors))))
  }

  getElementsByTagName(this: Node, name: string): HTMLCollection {
    const id = this[ID], n = String(name)
    return liveCollection(() => dom.getElementsByTagName(id, n), HTMLCollection, true)
  }

  getElementsByTagNameNS(this: Node, namespace: string | null, name: string): HTMLCollection {
    const id = this[ID], ns = namespace === '*' ? '*' : (nullable(namespace) ?? ''), local = String(name)
    return liveCollection(() => dom.getElementsByTagNameNS(id, ns, local), HTMLCollection, true)
  }

  getElementsByClassName(this: Node, names: string): HTMLCollection {
    const id = this[ID], n = String(names)
    return liveCollection(() => dom.getElementsByClassName(id, n))
  }
}

const CHILDREN = Symbol('children')

export class ChildNodeMixin {
  before(this: Node, ...nodes: unknown[]): void {
    const parent = inner(this, 'parentNode')
    if (!parent) return
    const ref = viable(inner(this, 'previousSibling'), nodes, false)
    const node = convertNodes(nodes)
    if (node) insert(parent, node, ref ? inner(ref, 'nextSibling') : inner(parent, 'firstChild'))
  }

  after(this: Node, ...nodes: unknown[]): void {
    const parent = inner(this, 'parentNode')
    if (!parent) return
    const ref = viable(inner(this, 'nextSibling'), nodes, true)
    const node = convertNodes(nodes)
    if (node) insert(parent, node, ref)
  }

  replaceWith(this: Node, ...nodes: unknown[]): void {
    const parent = inner(this, 'parentNode')
    if (!parent) return
    const ref = viable(inner(this, 'nextSibling'), nodes, true)
    const node = convertNodes(nodes)
    if (dom.parent(this[ID]) === parent[ID]) remove(this)
    if (node) insert(parent, node, ref)
  }

  remove(this: Node): void {
    remove(this)
  }

  get previousElementSibling(): Element | null {
    return elementFrom(dom.previousSibling((this as unknown as Node)[ID]), false)
  }

  get nextElementSibling(): Element | null {
    return elementFrom(dom.nextSibling((this as unknown as Node)[ID]), true)
  }
}

export function mixin(target: { prototype: object }, source: { prototype: object }, only?: readonly string[]): void {
  for (const key of Reflect.ownKeys(source.prototype)) {
    if (key === 'constructor' || (only && !only.includes(key as string))) continue
    Object.defineProperty(target.prototype, key, Object.getOwnPropertyDescriptor(source.prototype, key)!)
  }
}

export class CharacterData extends Node {
  declare data: string

  get length(): number {
    return dom.data(this[ID]).length
  }

  substringData(offset: number, count: number): string {
    const data = dom.data(this[ID])
    if (offset > data.length) throw new DOMException(`The offset ${offset} is larger than the node's length (${data.length}).`, 'IndexSizeError')
    return data.substr(offset, count)
  }

  appendData(data: string): void {
    const whole = dom.data(this[ID])
    replaceData(this, whole.length, 0, String(data), whole)
  }

  insertData(offset: number, data: string): void {
    replaceData(this, Number(offset) >>> 0, 0, String(data))
  }

  deleteData(offset: number, count: number): void {
    replaceData(this, Number(offset) >>> 0, Number(count) >>> 0, '')
  }

  replaceData(offset: number, count: number, data: string): void {
    replaceData(this, Number(offset) >>> 0, Number(count) >>> 0, String(data))
  }
}
for (const name of ['data', 'nodeValue', 'textContent']) {
  Object.defineProperty(CharacterData.prototype, name, {
    get(this: CharacterData) {
      return dom.data(this[ID])
    },
    set(this: CharacterData, value: unknown) {
      replaceData(this, 0, Infinity, value === null ? '' : String(value))
    },
    enumerable: true,
    configurable: true,
  })
}
mixin(CharacterData, ChildNodeMixin)
tag(CharacterData)
export interface CharacterData extends ChildNodeMixin {}

export class Text extends CharacterData {
  constructor(data = '') {
    if (!nodeConstructing()) constructing(dom.createText(String(data)))
    super()
  }

  override get nodeType(): number {
    return NodeType.Text
  }

  override get nodeName(): string {
    return '#text'
  }

  get wholeText(): string {
    let first: Node = this
    while (dom.previousSibling(first[ID]) !== NONE && dom.nodeType(dom.previousSibling(first[ID])) === NodeType.Text) first = wrap<Node>(dom.previousSibling(first[ID]))!
    let text = ''
    for (let n: Node | null = first; n && inner(n, 'nodeType') === NodeType.Text; n = inner(n, 'nextSibling')) text += (n as Text).data
    return text
  }

  splitText(offset: number): Text {
    const data = this.data
    if (offset > data.length) throw new DOMException(`The offset ${offset} is larger than the Text node's length.`, 'IndexSizeError')
    const tail = new Text(data.slice(offset))
    const parent = this.parentNode
    if (parent) {
      insert(parent, tail, this.nextSibling)
      if (rangeHooks.active()) rangeHooks.split(this[ID], tail[ID], offset)
    }
    replaceData(this, offset, data.length - offset, '')
    return tail
  }
}
getters(Text.prototype, { assignedSlot: null })
tag(Text)

export class CDATASection extends Text {
  override get nodeType(): number {
    return NodeType.CDataSection
  }

  override get nodeName(): string {
    return '#cdata-section'
  }
}
tag(CDATASection)

export class Comment extends CharacterData {
  constructor(data = '') {
    if (!nodeConstructing()) constructing(dom.createComment(String(data)))
    super()
  }

  override get nodeType(): number {
    return NodeType.Comment
  }

  override get nodeName(): string {
    return '#comment'
  }
}
tag(Comment)

export class ProcessingInstruction extends CharacterData {
  override get nodeType(): number {
    return NodeType.ProcessingInstruction
  }

  override get nodeName(): string {
    return this.target
  }

  get target(): string {
    return dom.processingInstructionTarget(this[ID])
  }
}
tag(ProcessingInstruction)

export class DocumentType extends Node {
  override get nodeType(): number {
    return NodeType.DocumentType
  }
  override get nodeName(): string {
    return this.name
  }
  get name(): string {
    return dom.documentType(this[ID])[0] ?? ''
  }
  get publicId(): string {
    return dom.documentType(this[ID])[1] ?? ''
  }
  get systemId(): string {
    return dom.documentType(this[ID])[2] ?? ''
  }
}
mixin(DocumentType, ChildNodeMixin, ['before', 'after', 'replaceWith', 'remove'])
tag(DocumentType)
export interface DocumentType extends ChildNodeMixin {}

export class DocumentFragment extends Node {
  constructor() {
    if (!nodeConstructing()) constructing(dom.createFragment())
    super()
  }

  override get nodeType(): number {
    return NodeType.DocumentFragment
  }

  override get nodeName(): string {
    return '#document-fragment'
  }

  getElementById(id: string): Element | null {
    return elementById(this[ID], String(id))
  }
}
mixin(DocumentFragment, ParentNodeMixin)
tag(DocumentFragment)
export interface DocumentFragment extends ParentNodeMixin {}
