import { DOMException, guard } from '../core/exception'
import { Event, fire } from '../core/event'
import { constants, host, tag } from '../core/host'
import { NONE, NodeType } from '../native'
import { DOMRect, DOMRectList } from './geometry'
import { rectOf } from './layout'
import { ANY_PARENT, CharacterData, DocumentFragment, insert, Node, rangeHooks, remove, type Text } from './node'
import { noteMarkup } from './element'
import { bodyId, CONNECTED, documentId, dom, ID, rootElementId, wrap, inner } from './realm'

const enum Position {
  Before = -1,
  Equal = 0,
  After = 1,
}

const PRECEDING = 2
const CONTAINS = 8
const CONTAINED_BY = 16

function nodeLength(node: Node): number {
  const type = inner(node, 'nodeType')
  if (type === NodeType.Text || type === NodeType.Comment) return dom.data(node[ID]).length
  if (type === NodeType.DocumentType) return 0
  return dom.childCount(node[ID])
}

function indexOf(node: Node): number {
  const parent = dom.parent(node[ID])
  return parent === NONE ? 0 : dom.childNodes(parent).indexOf(node[ID])
}

function root(id: number): number {
  const ancestors = dom.ancestors(id)
  return ancestors.length ? ancestors[ancestors.length - 1]! : id
}

function contains(a: number, b: number): boolean {
  return a === b || (dom.compareDocumentPosition(a, b) & CONTAINED_BY) !== 0
}

function commonAncestor(a: number, b: number): number {
  if (a === b) return a
  const above = new Set(dom.ancestors(b))
  above.add(b)
  if (above.has(a)) return a
  const up = dom.ancestors(a)
  for (let i = 0; i < up.length; i++) if (above.has(up[i]!)) return up[i]!
  return NONE
}

function childToward(ancestor: number, id: number): number {
  const up = dom.ancestors(id)
  const at = up.indexOf(ancestor)
  return at <= 0 ? id : up[at - 1]!
}

function compare(a: number, ao: number, b: number, bo: number): Position {
  if (a === b) return ao === bo ? Position.Equal : ao < bo ? Position.Before : Position.After
  const position = dom.compareDocumentPosition(a, b)
  if (position & PRECEDING) return -ordered(b, bo, a, (position & CONTAINS) !== 0) as Position
  return ordered(a, ao, b, (position & CONTAINED_BY) !== 0)
}

function ordered(a: number, ao: number, b: number, ancestor: boolean): Position {
  if (ancestor && dom.childNodes(a).indexOf(childToward(a, b)) < ao) return Position.After
  return Position.Before
}

const live = new Set<WeakRef<Range>>()
let selectionRange: Range | null = null
let unlisted = false
const listed = new WeakSet<Range>()

function unlistedRange(): Range {
  unlisted = true
  try {
    return new Range()
  } finally {
    unlisted = false
  }
}

function listRange(range: Range): void {
  if (listed.has(range)) return
  listed.add(range)
  live.add(new WeakRef(range))
}

function someLive(test: (range: Range) => boolean): boolean {
  if (selectionRange && test(selectionRange)) return true
  for (const ref of live) {
    const range = ref.deref()
    if (!range) live.delete(ref)
    else if (range !== selectionRange && test(range)) return true
  }
  return false
}

function moveBoundaries(move: (container: Node, offset: number) => [Node, number] | null): void {
  someLive((range) => {
    const start = move(range.startContainer, range.startOffset)
    const end = move(range.endContainer, range.endOffset)
    if (start) setBoundary(range, true, start[0], start[1])
    if (end) setBoundary(range, false, end[0], end[1])
    if (start || end) changed(range)
    return false
  })
}

rangeHooks.active = () => live.size > 0 || selectionRange !== null

function boundaryRelevant(container: Node, parent: number, inside: Node | null): boolean {
  if (parent === ANY_PARENT ? !isCharacterData(container) && (!inside || container[CONNECTED] === inside[CONNECTED]) : container[ID] === parent) return true
  return inside !== null && (container === inside || (container[CONNECTED] === inside[CONNECTED] && contains(inside[ID], container[ID])))
}

rangeHooks.relevant = (parent, inside) =>
  someLive((range) => {
    const start = range.startContainer, end = range.endContainer
    return boundaryRelevant(start, parent, inside) || (end !== start && boundaryRelevant(end, parent, inside))
  })

rangeHooks.removing = (parent, index, node) =>
  moveBoundaries((container, offset) => {
    const id = container[ID]
    if (id === parent) return offset > index ? [container, offset - 1] : null
    if (container === node || (container[CONNECTED] === node[CONNECTED] && contains(node[ID], id))) return [wrap<Node>(parent)!, index]
    return null
  })

rangeHooks.inserted = (parent, index, count) => moveBoundaries((container, offset) => (container[ID] === parent && offset > index ? [container, offset + count] : null))

rangeHooks.replacing = (parent) =>
  moveBoundaries((container, offset) => {
    if (container === parent ? offset === 0 : !(container[CONNECTED] === parent[CONNECTED] && contains(parent[ID], container[ID]))) return null
    return [parent, 0]
  })

rangeHooks.replaced = (node, offset, count, length) =>
  moveBoundaries((container, o) => {
    if (container[ID] !== node) return null
    if (o > offset && o <= offset + count) return [container, offset]
    if (o > offset + count) return [container, o + length - count]
    return null
  })

rangeHooks.split = (node, tail, offset) => moveBoundaries((container, o) => (container[ID] === node && o > offset ? [wrap<Node>(tail)!, o - offset] : null))

function checkPoint(node: Node, offset: number, method: string, iface = 'Range'): void {
  if (!(node instanceof Node)) throw new TypeError(`Failed to execute '${method}' on '${iface}': parameter 1 is not of type 'Node'.`)
  if (inner(node, 'nodeType') === NodeType.DocumentType) throw new DOMException(`Failed to execute '${method}' on '${iface}': The node provided is of type 'DocumentType'.`, 'InvalidNodeTypeError')
  const length = nodeLength(node)
  if (offset > length) throw new DOMException(`Failed to execute '${method}' on '${iface}': The offset ${offset} is larger than the node's length (${length}).`, 'IndexSizeError')
}

const START_NODE = Symbol('startContainer')
const START_OFFSET = Symbol('startOffset')
const END_NODE = Symbol('endContainer')
const END_OFFSET = Symbol('endOffset')

export class AbstractRange {
  protected [START_NODE]: Node
  protected [START_OFFSET]: number
  protected [END_NODE]: Node
  protected [END_OFFSET]: number

  constructor(sc: Node, so: number, ec: Node, eo: number) {
    this[START_NODE] = sc
    this[START_OFFSET] = so
    this[END_NODE] = ec
    this[END_OFFSET] = eo
  }

  get startContainer(): Node {
    return this[START_NODE]
  }
  get startOffset(): number {
    return this[START_OFFSET]
  }
  get endContainer(): Node {
    return this[END_NODE]
  }
  get endOffset(): number {
    return this[END_OFFSET]
  }
  get collapsed(): boolean {
    return this[START_NODE] === this[END_NODE] && this[START_OFFSET] === this[END_OFFSET]
  }
}
tag(AbstractRange)

export class StaticRange extends AbstractRange {
  constructor(init: { startContainer: Node; startOffset: number; endContainer: Node; endOffset: number }) {
    if (!init || !(init.startContainer instanceof Node) || !(init.endContainer instanceof Node)) throw new TypeError("Failed to construct 'StaticRange': required members missing.")
    super(init.startContainer, init.startOffset >>> 0, init.endContainer, init.endOffset >>> 0)
  }
}
tag(StaticRange)

let selectionChanged: () => void = () => {}
let setBoundary: (range: Range, start: boolean, node: Node, offset: number) => void

function changed(range: Range): void {
  if (range === selectionRange) selectionChanged()
}

export class Range extends AbstractRange {
  constructor() {
    const document = wrap<Node>(documentId)!
    super(document, 0, document, 0)
    if (!unlisted) listRange(this)
  }

  static {
    setBoundary = (range, start, node, offset) => {
      if (start) {
        range[START_NODE] = node
        range[START_OFFSET] = offset
      } else {
        range[END_NODE] = node
        range[END_OFFSET] = offset
      }
    }
  }

  get commonAncestorContainer(): Node {
    const common = commonAncestor(this[START_NODE][ID], this[END_NODE][ID])
    return common === NONE ? this[START_NODE] : wrap<Node>(common)!
  }

  setStart(node: Node, offset: number): void {
    const o = Number(offset) >>> 0
    checkPoint(node, o, 'setStart')
    if (root(node[ID]) !== root(this[END_NODE][ID]) || compare(node[ID], o, this[END_NODE][ID], this[END_OFFSET]) === Position.After) setBoundary(this, false, node, o)
    setBoundary(this, true, node, o)
    changed(this)
  }

  setEnd(node: Node, offset: number): void {
    const o = Number(offset) >>> 0
    checkPoint(node, o, 'setEnd')
    if (root(node[ID]) !== root(this[START_NODE][ID]) || compare(node[ID], o, this[START_NODE][ID], this[START_OFFSET]) === Position.Before) setBoundary(this, true, node, o)
    setBoundary(this, false, node, o)
    changed(this)
  }

  #parentOf(node: Node, method: string): Node {
    const parent = inner(node, 'parentNode')
    if (!parent) throw new DOMException(`Failed to execute '${method}' on 'Range': the given Node has no parent.`, 'InvalidNodeTypeError')
    return parent
  }

  setStartBefore(node: Node): void {
    this.setStart(this.#parentOf(node, 'setStartBefore'), indexOf(node))
  }
  setStartAfter(node: Node): void {
    this.setStart(this.#parentOf(node, 'setStartAfter'), indexOf(node) + 1)
  }
  setEndBefore(node: Node): void {
    this.setEnd(this.#parentOf(node, 'setEndBefore'), indexOf(node))
  }
  setEndAfter(node: Node): void {
    this.setEnd(this.#parentOf(node, 'setEndAfter'), indexOf(node) + 1)
  }

  collapse(toStart = false): void {
    if (toStart) setBoundary(this, false, this[START_NODE], this[START_OFFSET])
    else setBoundary(this, true, this[END_NODE], this[END_OFFSET])
    changed(this)
  }

  selectNode(node: Node): void {
    const parent = this.#parentOf(node, 'selectNode')
    const index = indexOf(node)
    setBoundary(this, true, parent, index)
    setBoundary(this, false, parent, index + 1)
    changed(this)
  }

  selectNodeContents(node: Node): void {
    if (inner(node, 'nodeType') === NodeType.DocumentType) throw new DOMException("Failed to execute 'selectNodeContents' on 'Range': The node provided is of type 'DocumentType'.", 'InvalidNodeTypeError')
    setBoundary(this, true, node, 0)
    setBoundary(this, false, node, nodeLength(node))
    changed(this)
  }

  compareBoundaryPoints(how: number, source: Range): number {
    if (how < 0 || how > 3) throw new DOMException("Failed to execute 'compareBoundaryPoints' on 'Range': The comparison method provided must be one of 'START_TO_START', 'START_TO_END', 'END_TO_END', or 'END_TO_START'.", 'NotSupportedError')
    if (root(this[START_NODE][ID]) !== root(source.startContainer[ID])) throw new DOMException("Failed to execute 'compareBoundaryPoints' on 'Range': The source range is in a different document than this range.", 'WrongDocumentError')
    const [a, ao] = how === 0 || how === 3 ? [this[START_NODE], this[START_OFFSET]] : [this[END_NODE], this[END_OFFSET]]
    const [b, bo] = how === 0 || how === 1 ? [source.startContainer, source.startOffset] : [source.endContainer, source.endOffset]
    return compare(a[ID], ao, b[ID], bo)
  }

  comparePoint(node: Node, offset: number): number {
    if (root(node[ID]) !== root(this[START_NODE][ID])) throw new DOMException("Failed to execute 'comparePoint' on 'Range': The node provided and the Range are not in the same tree.", 'WrongDocumentError')
    const o = Number(offset) >>> 0
    checkPoint(node, o, 'comparePoint')
    if (compare(node[ID], o, this[START_NODE][ID], this[START_OFFSET]) === Position.Before) return -1
    if (compare(node[ID], o, this[END_NODE][ID], this[END_OFFSET]) === Position.After) return 1
    return 0
  }

  isPointInRange(node: Node, offset: number): boolean {
    if (root(node[ID]) !== root(this[START_NODE][ID])) return false
    if (offset > nodeLength(node)) throw new DOMException(`Failed to execute 'isPointInRange' on 'Range': The offset ${offset} is larger than the node's length.`, 'IndexSizeError')
    return compare(node[ID], offset, this[START_NODE][ID], this[START_OFFSET]) !== Position.Before && compare(node[ID], offset, this[END_NODE][ID], this[END_OFFSET]) !== Position.After
  }

  intersectsNode(node: Node): boolean {
    const id = node[ID]
    if (root(id) !== root(this[START_NODE][ID])) return false
    const parent = dom.parent(id)
    if (parent === NONE) return true
    const index = dom.childNodes(parent).indexOf(id)
    return compare(parent, index, this[END_NODE][ID], this[END_OFFSET]) === Position.Before && compare(parent, index + 1, this[START_NODE][ID], this[START_OFFSET]) === Position.After
  }

  cloneRange(): Range {
    const range = new Range()
    setBoundary(range, true, this[START_NODE], this[START_OFFSET])
    setBoundary(range, false, this[END_NODE], this[END_OFFSET])
    return range
  }

  detach(): void {}

  override toString(): string {
    const sc = this[START_NODE], ec = this[END_NODE]
    if (sc === ec && isText(sc)) return dom.data(sc[ID]).slice(this[START_OFFSET], this[END_OFFSET])
    let out = ''
    if (isText(sc)) out += dom.data(sc[ID]).slice(this[START_OFFSET])
    for (const id of this.#contained()) {
      const type = dom.nodeType(id)
      if (type === NodeType.Text) out += dom.data(id)
      else if (type === NodeType.Element || type === NodeType.DocumentFragment) out += dom.textContent(id)
    }
    if (isText(ec)) out += dom.data(ec[ID]).slice(0, this[END_OFFSET])
    return out
  }

  #contained(): number[] {
    const sc = this[START_NODE][ID], ec = this[END_NODE][ID], so = this[START_OFFSET], eo = this[END_OFFSET]
    if (sc === ec) return isCharacterData(this[START_NODE]) ? [] : Array.from(dom.childNodes(sc).subarray(so, eo))
    const starts = [sc, ...dom.ancestors(sc)], ends = [ec, ...dom.ancestors(ec)]
    const endAt = new Map<number, number>()
    for (let i = 0; i < ends.length; i++) endAt.set(ends[i]!, i)
    let si = 0
    while (si < starts.length && !endAt.has(starts[si]!)) si++
    if (si === starts.length) return []
    const ei = endAt.get(starts[si]!)!
    const out: number[] = []
    const push = (ids: Uint32Array) => {
      for (let i = 0; i < ids.length; i++) out.push(ids[i]!)
    }
    if (si > 0) {
      if (!isCharacterData(this[START_NODE])) push(dom.childNodes(sc).subarray(so))
      for (let k = 1; k < si; k++) {
        const ids = dom.childNodes(starts[k]!)
        push(ids.subarray(ids.indexOf(starts[k - 1]!) + 1))
      }
    }
    const ids = dom.childNodes(starts[si]!)
    const lo = si === 0 ? so : ids.indexOf(starts[si - 1]!) + 1
    const hi = ei === 0 ? eo : ids.indexOf(ends[ei - 1]!)
    if (hi > lo) push(ids.subarray(lo, hi))
    if (ei > 0) {
      for (let k = ei - 1; k >= 1; k--) {
        const ids = dom.childNodes(ends[k]!)
        push(ids.subarray(0, ids.indexOf(ends[k - 1]!)))
      }
      if (!isCharacterData(this[END_NODE])) push(dom.childNodes(ec).subarray(0, eo))
    }
    return out
  }

  #collapsePoint(): [Node, number] {
    const sc = this[START_NODE][ID], ec = this[END_NODE][ID]
    const common = commonAncestor(sc, ec)
    if (common === sc || common === NONE) return [this[START_NODE], this[START_OFFSET]]
    const child = childToward(common, sc)
    return [wrap<Node>(common)!, dom.childNodes(common).indexOf(child) + 1]
  }

  deleteContents(): void {
    if (this.collapsed) return
    const sc = this[START_NODE], so = this[START_OFFSET], ec = this[END_NODE], eo = this[END_OFFSET]
    if (sc === ec && isCharacterData(sc)) {
      ;(sc as CharacterData).replaceData(so, eo - so, '')
      return
    }
    const contained = this.#contained()
    const [newNode, newOffset] = this.#collapsePoint()
    if (isCharacterData(sc)) (sc as CharacterData).replaceData(so, nodeLength(sc) - so, '')
    for (const id of contained) remove(wrap<Node>(id)!)
    if (isCharacterData(ec)) (ec as CharacterData).replaceData(0, eo, '')
    this[START_NODE] = this[END_NODE] = newNode
    this[START_OFFSET] = this[END_OFFSET] = newOffset
    changed(this)
  }

  extractContents(): DocumentFragment {
    return this.#extract(true)
  }

  cloneContents(): DocumentFragment {
    return this.#extract(false)
  }

  #extract(move: boolean): DocumentFragment {
    const fragment = new DocumentFragment()
    if (this.collapsed) return fragment
    const sc = this[START_NODE], so = this[START_OFFSET], ec = this[END_NODE], eo = this[END_OFFSET]
    const partial = (node: Node, a: Node, ao: number, b: Node, bo: number) => {
      const clone = inner(node, 'cloneNode').call(node, false)
      fragment.appendChild(clone)
      if (isCharacterData(node)) {
        ;(clone as CharacterData).data = dom.data(node[ID]).slice(ao, bo)
        if (move) (node as CharacterData).replaceData(ao, bo - ao, '')
        return
      }
      const sub = unlistedRange()
      setBoundary(sub, true, a, ao)
      setBoundary(sub, false, b, bo)
      inner(clone, 'appendChild').call(clone, sub.#extract(move))
    }
    if (sc === ec && isCharacterData(sc)) {
      partial(sc, sc, so, ec, eo)
      return fragment
    }
    const common = commonAncestor(sc[ID], ec[ID])
    const children = dom.childNodes(common)
    const firstId = sc[ID] === common ? NONE : childToward(common, sc[ID])
    const lastId = ec[ID] === common ? NONE : childToward(common, ec[ID])
    const firstAt = firstId === NONE ? -1 : children.indexOf(firstId)
    const lo = firstId === NONE ? so : firstAt + 1
    const hi = lastId === NONE ? eo : children.indexOf(lastId)
    const contained = hi > lo ? Array.from(children.subarray(lo, hi), (id) => wrap<Node>(id)!) : []
    const firstPartial = firstId === NONE ? null : wrap<Node>(firstId)!
    const lastPartial = lastId === NONE ? null : wrap<Node>(lastId)!
    let newNode: Node = sc, newOffset = so
    if (move && firstId !== NONE) {
      newNode = wrap<Node>(common)!
      newOffset = firstAt + 1
    }
    if (firstPartial) partial(firstPartial, sc, so, firstPartial, nodeLength(firstPartial))
    for (const node of contained) fragment.appendChild(move ? node : inner(node, 'cloneNode').call(node, true))
    if (lastPartial) partial(lastPartial, lastPartial, 0, ec, eo)
    if (move) {
      this[START_NODE] = this[END_NODE] = newNode
      this[START_OFFSET] = this[END_OFFSET] = newOffset
      changed(this)
    }
    return fragment
  }

  insertNode(node: Node): void {
    const sc = this[START_NODE]
    if (inner(sc, 'nodeType') === NodeType.Comment || (isText(sc) && !inner(sc, 'parentNode')) || sc === node) throw new DOMException("Failed to execute 'insertNode' on 'Range': The range start is in a node that cannot be split.", 'HierarchyRequestError')
    let ref: Node | null
    let parent: Node
    if (isText(sc)) {
      ref = (sc as Text).splitText(this[START_OFFSET])
      parent = inner(ref, 'parentNode')!
    } else {
      ref = wrap<Node>(dom.childNodes(sc[ID])[this[START_OFFSET]] ?? NONE)
      parent = sc
    }
    if (ref === node) ref = inner(node, 'nextSibling')
    if (inner(node, 'parentNode')) remove(node)
    const index = ref ? indexOf(ref) : nodeLength(parent)
    const added = inner(node, 'nodeType') === NodeType.DocumentFragment ? nodeLength(node) : 1
    insert(parent, node, ref)
    if (this.collapsed) setBoundary(this, false, parent, index + added)
    changed(this)
  }

  surroundContents(parent: Node): void {
    const type = inner(parent, 'nodeType')
    if (type === NodeType.Document || type === NodeType.DocumentType || type === NodeType.DocumentFragment) throw new DOMException("Failed to execute 'surroundContents' on 'Range': The node provided is of an invalid type.", 'InvalidNodeTypeError')
    const fragment = this.extractContents()
    for (let first = inner(parent, 'firstChild'); first; first = inner(parent, 'firstChild')) remove(first)
    this.insertNode(parent)
    inner(parent, 'appendChild').call(parent, fragment)
    this.selectNode(parent)
  }

  createContextualFragment(html: string): DocumentFragment {
    const markup = String(html)
    noteMarkup(markup)
    let context = this[START_NODE]
    if (inner(context, 'nodeType') !== NodeType.Element) context = inner(context, 'parentNode') ?? wrap<Node>(bodyId()) ?? wrap<Node>(rootElementId())!
    return wrap<DocumentFragment>(guard(() => dom.parseFragment(context[ID], markup)))!
  }

  #rects(): Float64Array[] {
    const sc = this[START_NODE], ec = this[END_NODE]
    const rects: Float64Array[] = []
    const push = (list: Float64Array) => {
      for (let i = 0; i + 3 < list.length; i += 4) rects.push(list.subarray(i, i + 4))
    }
    if (isText(sc) && sc === ec) {
      push(dom.textRects(sc[ID], this[START_OFFSET], this[END_OFFSET]))
      if (rects.length === 0 && this.collapsed) push(dom.caretRect(sc[ID], this[START_OFFSET]))
      return rects
    }
    if (isText(sc)) push(dom.textRects(sc[ID], this[START_OFFSET], nodeLength(sc)))
    const visit = (id: number) => {
      const type = dom.nodeType(id)
      if (type === NodeType.Text) push(dom.textRects(id, 0, dom.data(id).length))
      else if (type === NodeType.Element) {
        rects.push(rectOf(id))
        for (let child = dom.firstChild(id); child !== NONE; child = dom.nextSibling(child)) visit(child)
      }
    }
    for (const id of this.#contained()) visit(id)
    if (isText(ec)) push(dom.textRects(ec[ID], 0, this[END_OFFSET]))
    if (rects.length === 0 && inner(sc, 'nodeType') === NodeType.Element) {
      const child = dom.childNodes(sc[ID])[this[START_OFFSET]]
      rects.push(child !== undefined && dom.nodeType(child) === NodeType.Element ? rectOf(child) : rectOf(sc[ID]))
    }
    return rects
  }

  getClientRects(): DOMRectList {
    return new DOMRectList(this.#rects().map((r) => new DOMRect(r[0], r[1], r[2], r[3])))
  }

  getBoundingClientRect(): DOMRect {
    const rects = this.#rects().filter((r) => r[2]! > 0 || r[3]! > 0)
    if (rects.length === 0) {
      const all = this.#rects()
      return all[0] ? new DOMRect(all[0][0], all[0][1], all[0][2], all[0][3]) : new DOMRect()
    }
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity
    for (const r of rects) {
      x0 = Math.min(x0, r[0]!)
      y0 = Math.min(y0, r[1]!)
      x1 = Math.max(x1, r[0]! + r[2]!)
      y1 = Math.max(y1, r[1]! + r[3]!)
    }
    return new DOMRect(x0, y0, x1 - x0, y1 - y0)
  }
}
constants(Range, { START_TO_START: 0, START_TO_END: 1, END_TO_END: 2, END_TO_START: 3 })
tag(Range)

function isText(node: Node): boolean {
  return inner(node, 'nodeType') === NodeType.Text
}

function isCharacterData(node: Node): boolean {
  const type = inner(node, 'nodeType')
  return type === NodeType.Text || type === NodeType.Comment
}

let changeQueued = false

export class Selection {
  #range: Range | null = null
  #backward = false
  #exposed = false
  #painted = ''

  constructor() {
    selectionChanged = () => this.#changed()
  }

  #changed(): void {
    const r = this.#range
    const key = r ? `${this.anchorNode![ID]}:${this.anchorOffset}:${this.focusNode![ID]}:${this.focusOffset}` : ''
    if (key !== this.#painted) {
      this.#painted = key
      if (r) dom.setSelection(this.anchorNode![ID], this.anchorOffset, this.focusNode![ID], this.focusOffset)
      else dom.setSelection(NONE, 0, NONE, 0)
    }
    if (!changeQueued) {
      changeQueued = true
      host.setTimer(0, () => {
        changeQueued = false
        fire(wrap<Node>(documentId)!, new Event('selectionchange'))
      })
    }
  }

  #set(range: Range | null, backward = false): void {
    const old = this.#range
    if (old && old !== range && this.#exposed) listRange(old)
    this.#exposed = false
    this.#range = range
    selectionRange = range
    this.#backward = backward && !!range && !range.collapsed
    this.#changed()
  }

  get anchorNode(): Node | null {
    const r = this.#range
    return r ? (this.#backward ? r.endContainer : r.startContainer) : null
  }
  get anchorOffset(): number {
    const r = this.#range
    return r ? (this.#backward ? r.endOffset : r.startOffset) : 0
  }
  get focusNode(): Node | null {
    const r = this.#range
    return r ? (this.#backward ? r.startContainer : r.endContainer) : null
  }
  get focusOffset(): number {
    const r = this.#range
    return r ? (this.#backward ? r.startOffset : r.endOffset) : 0
  }
  get baseNode(): Node | null {
    return this.anchorNode
  }
  get baseOffset(): number {
    return this.anchorOffset
  }
  get extentNode(): Node | null {
    return this.focusNode
  }
  get extentOffset(): number {
    return this.focusOffset
  }
  get isCollapsed(): boolean {
    return !this.#range || this.#range.collapsed
  }
  get rangeCount(): number {
    return this.#range ? 1 : 0
  }
  get type(): string {
    return !this.#range ? 'None' : this.#range.collapsed ? 'Caret' : 'Range'
  }
  get direction(): string {
    return !this.#range || this.#range.collapsed ? 'none' : this.#backward ? 'backward' : 'forward'
  }

  getRangeAt(index: number): Range {
    if (index !== 0 || !this.#range) throw new DOMException(`Failed to execute 'getRangeAt' on 'Selection': ${index} is not a valid index.`, 'IndexSizeError')
    this.#exposed = true
    return this.#range
  }

  addRange(range: Range): void {
    if (!(range instanceof Range)) throw new TypeError("Failed to execute 'addRange' on 'Selection': parameter 1 is not of type 'Range'.")
    if (this.#range) return
    if (!inner(range.startContainer, 'isConnected')) return
    this.#set(range)
  }

  removeRange(range: Range): void {
    if (range !== this.#range) throw new DOMException("Failed to execute 'removeRange' on 'Selection': The given range isn't in document.", 'NotFoundError')
    this.#set(null)
  }

  removeAllRanges(): void {
    if (this.#range) this.#set(null)
  }

  empty(): void {
    this.removeAllRanges()
  }

  collapse(node: Node | null, offset = 0): void {
    if (node === null) return this.removeAllRanges()
    const o = Number(offset) >>> 0
    checkPoint(node, o, 'collapse', 'Selection')
    const range = unlistedRange()
    setBoundary(range, true, node, o)
    setBoundary(range, false, node, o)
    this.#set(range)
  }

  setPosition(node: Node | null, offset = 0): void {
    this.collapse(node, offset)
  }

  collapseToStart(): void {
    if (!this.#range) throw new DOMException("Failed to execute 'collapseToStart' on 'Selection': there is no selection.", 'InvalidStateError')
    this.collapse(this.#range.startContainer, this.#range.startOffset)
  }

  collapseToEnd(): void {
    if (!this.#range) throw new DOMException("Failed to execute 'collapseToEnd' on 'Selection': there is no selection.", 'InvalidStateError')
    this.collapse(this.#range.endContainer, this.#range.endOffset)
  }

  extend(node: Node, offset = 0): void {
    if (!this.#range) throw new DOMException("Failed to execute 'extend' on 'Selection': This Selection object doesn't have any Ranges.", 'InvalidStateError')
    const anchor = this.anchorNode!, anchorOffset = this.anchorOffset
    this.setBaseAndExtent(anchor, anchorOffset, node, offset)
  }

  setBaseAndExtent(anchor: Node, anchorOffset: number, focus: Node, focusOffset: number): void {
    const ao = Number(anchorOffset) >>> 0, fo = Number(focusOffset) >>> 0
    checkPoint(anchor, ao, 'setBaseAndExtent', 'Selection')
    checkPoint(focus, fo, 'setBaseAndExtent', 'Selection')
    const range = unlistedRange()
    const same = anchor === focus || root(anchor[ID]) === root(focus[ID])
    const backward = same && compare(focus[ID], fo, anchor[ID], ao) === Position.Before
    setBoundary(range, true, !same ? focus : backward ? focus : anchor, !same ? fo : backward ? fo : ao)
    setBoundary(range, false, backward ? anchor : focus, backward ? ao : fo)
    this.#set(range, backward)
  }

  selectAllChildren(node: Node): void {
    const range = unlistedRange()
    range.selectNodeContents(node)
    this.#set(range)
  }

  deleteFromDocument(): void {
    this.#range?.deleteContents()
  }

  containsNode(node: Node, allowPartial = false): boolean {
    const r = this.#range
    if (!r || root(node[ID]) !== root(r.startContainer[ID])) return false
    const parent = inner(node, 'parentNode')
    if (!parent) return false
    if (allowPartial) return r.intersectsNode(node)
    const index = indexOf(node)
    return compare(parent[ID], index, r.startContainer[ID], r.startOffset) !== Position.Before && compare(parent[ID], index + 1, r.endContainer[ID], r.endOffset) !== Position.After
  }

  modify(_alter?: string, _direction?: string, _granularity?: string): void {}

  getComposedRanges(): StaticRange[] {
    const r = this.#range
    return r ? [new StaticRange({ startContainer: r.startContainer, startOffset: r.startOffset, endContainer: r.endContainer, endOffset: r.endOffset })] : []
  }

  toString(): string {
    return this.#range?.toString() ?? ''
  }
}
tag(Selection)

let selection: Selection | null = null
export function getSelection(): Selection {
  return (selection ??= new Selection())
}

export class CaretPosition {
  constructor(
    readonly offsetNode: Node,
    readonly offset: number,
  ) {}
  getClientRect(): DOMRect | null {
    const r = isText(this.offsetNode) ? dom.caretRect(this.offsetNode[ID], this.offset) : rectOf(this.offsetNode[ID])
    return new DOMRect(r[0], r[1], r[2], r[3])
  }
}
tag(CaretPosition)
