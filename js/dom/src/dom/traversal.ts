import { DOMException } from '../core/exception'
import { tag } from '../core/host'
import { NONE } from '../native'
import type { Node } from './node'
import { dom, ID, wrap } from './realm'

export const NodeFilter = {
  FILTER_ACCEPT: 1,
  FILTER_REJECT: 2,
  FILTER_SKIP: 3,
  SHOW_ALL: 0xffffffff,
  SHOW_ELEMENT: 0x1,
  SHOW_ATTRIBUTE: 0x2,
  SHOW_TEXT: 0x4,
  SHOW_CDATA_SECTION: 0x8,
  SHOW_ENTITY_REFERENCE: 0x10,
  SHOW_ENTITY: 0x20,
  SHOW_PROCESSING_INSTRUCTION: 0x40,
  SHOW_COMMENT: 0x80,
  SHOW_DOCUMENT: 0x100,
  SHOW_DOCUMENT_TYPE: 0x200,
  SHOW_DOCUMENT_FRAGMENT: 0x400,
  SHOW_NOTATION: 0x800,
} as const

type Filter = ((node: Node) => number) | { acceptNode(node: Node): number } | null

class Traversal {
  readonly #root: Node
  readonly #show: number
  readonly #filter: Filter
  #active = false

  constructor(root: Node, whatToShow: number, filter: Filter) {
    this.#root = root
    this.#show = whatToShow >>> 0
    this.#filter = filter ?? null
  }

  get root(): Node {
    return this.#root
  }
  get whatToShow(): number {
    return this.#show
  }
  get filter(): Filter {
    return this.#filter
  }

  protected accept(id: number): number {
    const bit = 1 << (dom.nodeType(id) - 1)
    if (!(this.#show & bit)) return NodeFilter.FILTER_SKIP
    const filter = this.#filter
    if (!filter) return NodeFilter.FILTER_ACCEPT
    if (this.#active) throw new DOMException('The filter is already active.', 'InvalidStateError')
    this.#active = true
    try {
      const node = wrap<Node>(id)!
      const result = typeof filter === 'function' ? filter(node) : filter.acceptNode(node)
      return Number(result)
    } finally {
      this.#active = false
    }
  }
}

export class TreeWalker extends Traversal {
  #current: number

  constructor(root: Node, whatToShow: number, filter: Filter) {
    super(root, whatToShow, filter)
    this.#current = root[ID]
  }

  get currentNode(): Node {
    return wrap<Node>(this.#current)!
  }
  set currentNode(node: Node) {
    this.#current = node[ID]
  }

  #rootId(): number {
    return this.root[ID]
  }

  #move(node: number): Node {
    this.#current = node
    return wrap<Node>(node)!
  }

  parentNode(): Node | null {
    let node = this.#current
    while (node !== NONE && node !== this.#rootId()) {
      node = dom.parent(node)
      if (node !== NONE && this.accept(node) === NodeFilter.FILTER_ACCEPT) return this.#move(node)
    }
    return null
  }

  #children(first: boolean): Node | null {
    let node = first ? dom.firstChild(this.#current) : dom.lastChild(this.#current)
    while (node !== NONE) {
      const result = this.accept(node)
      if (result === NodeFilter.FILTER_ACCEPT) return this.#move(node)
      if (result === NodeFilter.FILTER_SKIP) {
        const child = first ? dom.firstChild(node) : dom.lastChild(node)
        if (child !== NONE) {
          node = child
          continue
        }
      }
      while (node !== NONE) {
        const sibling = first ? dom.nextSibling(node) : dom.previousSibling(node)
        if (sibling !== NONE) {
          node = sibling
          break
        }
        const parent = dom.parent(node)
        if (parent === NONE || parent === this.#rootId() || parent === this.#current) return null
        node = parent
      }
    }
    return null
  }

  firstChild(): Node | null {
    return this.#children(true)
  }

  lastChild(): Node | null {
    return this.#children(false)
  }

  #siblings(next: boolean): Node | null {
    let node = this.#current
    if (node === this.#rootId()) return null
    for (;;) {
      let sibling = next ? dom.nextSibling(node) : dom.previousSibling(node)
      while (sibling !== NONE) {
        node = sibling
        const result = this.accept(node)
        if (result === NodeFilter.FILTER_ACCEPT) return this.#move(node)
        sibling = next ? dom.firstChild(node) : dom.lastChild(node)
        if (result === NodeFilter.FILTER_REJECT || sibling === NONE) sibling = next ? dom.nextSibling(node) : dom.previousSibling(node)
      }
      node = dom.parent(node)
      if (node === NONE || node === this.#rootId()) return null
      if (this.accept(node) === NodeFilter.FILTER_ACCEPT) return null
    }
  }

  nextSibling(): Node | null {
    return this.#siblings(true)
  }

  previousSibling(): Node | null {
    return this.#siblings(false)
  }

  previousNode(): Node | null {
    let node = this.#current
    while (node !== this.#rootId()) {
      let sibling = dom.previousSibling(node)
      while (sibling !== NONE) {
        node = sibling
        let result = this.accept(node)
        while (result !== NodeFilter.FILTER_REJECT && dom.lastChild(node) !== NONE) {
          node = dom.lastChild(node)
          result = this.accept(node)
        }
        if (result === NodeFilter.FILTER_ACCEPT) return this.#move(node)
        sibling = dom.previousSibling(node)
      }
      if (node === this.#rootId()) return null
      const parent = dom.parent(node)
      if (parent === NONE) return null
      node = parent
      if (this.accept(node) === NodeFilter.FILTER_ACCEPT) return this.#move(node)
    }
    return null
  }

  nextNode(): Node | null {
    let node = this.#current
    let result: number = NodeFilter.FILTER_ACCEPT
    for (;;) {
      while (result !== NodeFilter.FILTER_REJECT) {
        const child = dom.firstChild(node)
        if (child === NONE) break
        node = child
        result = this.accept(node)
        if (result === NodeFilter.FILTER_ACCEPT) return this.#move(node)
      }
      let sibling = NONE
      let temp = node
      while (temp !== NONE) {
        if (temp === this.#rootId()) return null
        sibling = dom.nextSibling(temp)
        if (sibling !== NONE) {
          node = sibling
          break
        }
        temp = dom.parent(temp)
      }
      if (temp === NONE) return null
      result = this.accept(node)
      if (result === NodeFilter.FILTER_ACCEPT) return this.#move(node)
    }
  }
}
tag(TreeWalker)

export class NodeIterator extends Traversal {
  #reference: number
  #before = true

  constructor(root: Node, whatToShow: number, filter: Filter) {
    super(root, whatToShow, filter)
    this.#reference = root[ID]
  }

  get referenceNode(): Node {
    return wrap<Node>(this.#reference)!
  }
  get pointerBeforeReferenceNode(): boolean {
    return this.#before
  }

  #following(id: number): number {
    const child = dom.firstChild(id)
    if (child !== NONE) return child
    for (let node = id; node !== NONE && node !== this.root[ID]; node = dom.parent(node)) {
      const sibling = dom.nextSibling(node)
      if (sibling !== NONE) return sibling
    }
    return NONE
  }

  #preceding(id: number): number {
    if (id === this.root[ID]) return NONE
    let node = dom.previousSibling(id)
    if (node === NONE) return dom.parent(id)
    while (dom.lastChild(node) !== NONE) node = dom.lastChild(node)
    return node
  }

  #step(forward: boolean): Node | null {
    let node = this.#reference
    let before = this.#before
    for (;;) {
      if (before !== forward) {
        node = forward ? this.#following(node) : this.#preceding(node)
        if (node === NONE) return null
      } else before = !forward
      if (this.accept(node) === NodeFilter.FILTER_ACCEPT) {
        this.#reference = node
        this.#before = !forward
        return wrap<Node>(node)
      }
    }
  }

  nextNode(): Node | null {
    return this.#step(true)
  }

  previousNode(): Node | null {
    return this.#step(false)
  }

  detach(): void {}
}
tag(NodeIterator)
