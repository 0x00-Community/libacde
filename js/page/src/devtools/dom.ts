import { DevToolsError, type DevToolsDomain, type DevToolsParams, type DevToolsSession } from './index'

export interface DomNode {
  readonly nodeType: number
  readonly nodeName: string
  readonly localName?: string | null
  readonly nodeValue: string | null
  readonly childNodes: ArrayLike<DomNode>
  readonly parentNode: DomNode | null
  readonly previousSibling: DomNode | null
  readonly attributes?: ArrayLike<{ readonly name: string; readonly value: string }> | null
  readonly publicId?: string
  readonly systemId?: string
  readonly documentURI?: string
  readonly baseURI?: string
}

export interface DomRecord {
  readonly type: string
  readonly target: DomNode
  readonly addedNodes: ArrayLike<DomNode>
  readonly removedNodes: ArrayLike<DomNode>
  readonly attributeName: string | null
}

export interface DomSource {
  document(): DomNode
  idOf(node: DomNode): number
  nodeOf(id: number): DomNode | null
  observe(changed: (records: readonly DomRecord[]) => void): () => void
  expose(lookup: (id: number) => DomNode): void
  computed(node: DomNode): [string, string][]
  inline(node: DomNode): [string, string, boolean][]
  rect(node: DomNode): readonly [number, number, number, number] | null
  query(root: DomNode, selector: string): DomNode[]
}

export type Quad = [number, number, number, number, number, number, number, number]

type Emit = (session: number, method: string, params: DevToolsParams) => void

const TEXT = 3
const ELEMENT = 1
const DOCUMENT_TYPE = 10
const DOCUMENT = 9
const SEARCH_LIMIT = 1000
const READ_ONLY = 'the Elements panel shows the page read only; change the page from its code or the console'
const EDITS = [
  'setAttributeValue',
  'setAttributesAsText',
  'removeAttribute',
  'setNodeName',
  'setNodeValue',
  'setOuterHTML',
  'removeNode',
  'moveTo',
  'copyTo',
  'undo',
  'redo',
  'markUndoableState',
  'setFileInputFiles',
]

export class DomDomain {
  readonly #source: DomSource
  readonly #emit: Emit
  readonly #sessions = new Set<number>()
  readonly #expanded = new Map<number, Set<number>>()
  readonly #searches = new Map<number, Map<string, DomNode[]>>()
  #searchSeq = 0
  #stop: (() => void) | null = null

  constructor(source: DomSource, emit: Emit) {
    this.#source = source
    this.#emit = emit
    source.expose((id) => {
      const node = source.nodeOf(id)
      if (!node) throw new Error(`no node ${id} in this page`)
      return node
    })
  }

  static create(source: DomSource, emit: Emit): DevToolsDomain {
    const domain = new DomDomain(source, emit)
    const methods: Record<string, (params: DevToolsParams, session: DevToolsSession) => unknown> = {
      enable: (_, session) => domain.#enable(session),
      disable: (_, session) => domain.#disable(session),
      getDocument: (params, session) => domain.#document(params, session),
      requestChildNodes: (params, session) => domain.#children(params, session.id),
      describeNode: (params, session) => ({ node: domain.#describe(domain.#node(params), DomDomain.#depth(params, 0), domain.#known(session.id)) }),
      resolveNode: () => {
        throw new DevToolsError(-32000, 'resolveNode is answered by the runtime')
      },
      getBoxModel: (params) => ({ model: DomDomain.box(source, domain.#node(params)) }),
      pushNodesByBackendIdsToFrontend: (params) => ({ backendNodeIds: params['backendNodeIds'] ?? [] }),
      querySelector: (params, session) => {
        const found = domain.#query(params)[0]
        return { nodeId: found ? domain.#reveal(found, session.id) : 0 }
      },
      querySelectorAll: (params, session) => ({ nodeIds: domain.#query(params).map((node) => domain.#reveal(node, session.id)) }),
      performSearch: (params, session) => domain.#search(params, session),
      getSearchResults: (params, session) => domain.#results(params, session),
      discardSearchResults: (params, session) => void domain.#searches.get(session.id)?.delete(String(params['searchId'])),
    }
    for (const name of EDITS) {
      methods[name] = () => {
        throw new DevToolsError(-32000, READ_ONLY)
      }
    }
    return { name: 'DOM', methods, closed: (session) => domain.#closed(session) }
  }

  static node(source: DomSource, params: DevToolsParams): DomNode {
    const id = Number(params['nodeId'] ?? params['backendNodeId'])
    const node = Number.isInteger(id) ? source.nodeOf(id) : null
    if (!node) throw new DevToolsError(-32000, 'Could not find node with given id')
    return node
  }

  static box(source: DomSource, node: DomNode): Record<string, unknown> {
    const rect = node.nodeType === ELEMENT ? source.rect(node) : null
    const values = new Map(source.computed(node))
    if (!rect || values.get('display') === 'none') throw new DevToolsError(-32000, 'Could not compute box model.')
    const px = (name: string) => Number.parseFloat(values.get(name) ?? '') || 0
    const sides = (prefix: string, suffix = '') => ['top', 'right', 'bottom', 'left'].map((side) => px(`${prefix}-${side}${suffix}`))
    const [x, y, width, height] = rect
    const border = [x, y, x + width, y + height]
    const inset = ([l, t, r, b]: number[], [top, right, bottom, left]: number[], sign: number) => [
      l! + sign * left!,
      t! + sign * top!,
      r! - sign * right!,
      b! - sign * bottom!,
    ]
    const padding = inset(border, sides('border', '-width'), 1)
    const content = inset(padding, sides('padding'), 1)
    const margin = inset(border, sides('margin'), -1)
    const quad = ([l, t, r, b]: number[]): Quad => [l!, t!, r!, t!, r!, b!, l!, b!]
    return { content: quad(content), padding: quad(padding), border: quad(border), margin: quad(margin), width, height }
  }

  #query(params: DevToolsParams): DomNode[] {
    const root = this.#node(params)
    const selector = String(params['selector'] ?? '')
    try {
      return this.#source.query(root, selector)
    } catch {
      throw new DevToolsError(-32000, `'${selector}' is not a valid selector.`)
    }
  }

  #known(session: number): Set<number> {
    let known = this.#expanded.get(session)
    if (!known) this.#expanded.set(session, (known = new Set()))
    return known
  }

  #closed(session: number): void {
    this.#searches.delete(session)
    this.#expanded.delete(session)
    this.#sessions.delete(session)
    if (this.#sessions.size === 0) {
      this.#stop?.()
      this.#stop = null
    }
  }

  #reveal(node: DomNode, session: number): number {
    const known = this.#known(session)
    const path: DomNode[] = []
    for (let parent = node.parentNode; parent; parent = parent.parentNode) path.unshift(parent)
    for (const parent of path) {
      const id = this.#source.idOf(parent)
      if (known.has(id)) continue
      known.add(id)
      this.#emit(session, 'setChildNodes', { parentId: id, nodes: this.#shown(parent).map((child) => this.#describe(child, 0, known)) })
    }
    return this.#source.idOf(node)
  }

  #search(params: DevToolsParams, session: DevToolsSession): unknown {
    const query = String(params['query'] ?? '').trim()
    const found = query ? DomDomain.matches(this.#source, query).slice(0, SEARCH_LIMIT) : []
    const searchId = `${session.id}.${++this.#searchSeq}`
    let searches = this.#searches.get(session.id)
    if (!searches) this.#searches.set(session.id, (searches = new Map()))
    searches.set(searchId, found)
    return { searchId, resultCount: found.length }
  }

  #results(params: DevToolsParams, session: DevToolsSession): unknown {
    const found = this.#searches.get(session.id)?.get(String(params['searchId']))
    if (!found) throw new DevToolsError(-32000, 'No search session with given id found')
    const from = Number(params['fromIndex'])
    const to = Number(params['toIndex'])
    if (!(Number.isInteger(from) && Number.isInteger(to) && from >= 0 && from <= to && to <= found.length)) {
      throw new DevToolsError(-32000, 'Invalid search result range')
    }
    return { nodeIds: found.slice(from, to).map((node) => this.#reveal(node, session.id)) }
  }

  static matches(source: DomSource, query: string): DomNode[] {
    const exact = query.length > 1 && query.startsWith('"') && query.endsWith('"')
    const text = (exact ? query.slice(1, -1) : query).toLowerCase()
    const open = !exact && query.startsWith('<')
    const close = !exact && query.length > 1 && query.endsWith('>')
    const tag = query.slice(open ? 1 : 0, close ? -1 : undefined).toLowerCase()
    const hit = (value: string) => (exact ? value.toLowerCase() === text : value.toLowerCase().includes(text))
    const named = (node: DomNode) => {
      const name = (node.localName ?? node.nodeName).toLowerCase()
      if (open || close) return tag !== '' && (open && close ? name === tag : open ? name.startsWith(tag) : name.endsWith(tag))
      return hit(name)
    }
    const found = new Set<DomNode>()
    const walk = (node: DomNode) => {
      if (node.nodeType === ELEMENT) {
        const attributes = node.attributes ? Array.from(node.attributes) : []
        if (named(node) || (!open && !close && attributes.some((a) => hit(a.name) || hit(a.value)))) found.add(node)
      } else if (node.nodeType === TEXT && !open && !close && (node.nodeValue ?? '').trim() && hit(node.nodeValue ?? '')) {
        found.add(node)
      }
      for (let i = 0; i < node.childNodes.length; i++) walk(node.childNodes[i]!)
    }
    const document = source.document()
    walk(document)
    let selected: DomNode[] = []
    try {
      selected = source.query(document, query)
    } catch {}
    if (!selected.length) return [...found]
    const order = new Map<DomNode, number>()
    let index = 0
    const number = (node: DomNode) => {
      order.set(node, index++)
      for (let i = 0; i < node.childNodes.length; i++) number(node.childNodes[i]!)
    }
    number(document)
    for (const node of selected) found.add(node)
    return [...found].sort((a, b) => (order.get(a) ?? 0) - (order.get(b) ?? 0))
  }

  #enable(session: DevToolsSession): void {
    session.enabled.add('DOM')
    this.#sessions.add(session.id)
    this.#stop ??= this.#source.observe((records) => this.#changed(records))
  }

  #disable(session: DevToolsSession): void {
    session.enabled.delete('DOM')
    this.#sessions.delete(session.id)
    this.#expanded.delete(session.id)
    if (this.#sessions.size === 0) {
      this.#stop?.()
      this.#stop = null
    }
  }

  #document(params: DevToolsParams, session: DevToolsSession): unknown {
    if (!this.#sessions.has(session.id)) this.#enable(session)
    const known = new Set<number>()
    this.#expanded.set(session.id, known)
    return { root: this.#describe(this.#source.document(), DomDomain.#depth(params, 2), known) }
  }

  #children(params: DevToolsParams, session: number): void {
    const parent = this.#node(params)
    const id = this.#source.idOf(parent)
    const known = this.#known(session)
    known.add(id)
    const depth = DomDomain.#depth(params, 1)
    this.#emit(session, 'setChildNodes', { parentId: id, nodes: this.#shown(parent).map((child) => this.#describe(child, depth - 1, known)) })
  }

  #node(params: DevToolsParams): DomNode {
    return DomDomain.node(this.#source, params)
  }

  static #depth(params: DevToolsParams, fallback: number): number {
    const depth = params['depth']
    if (depth === -1) return Number.POSITIVE_INFINITY
    return typeof depth === 'number' && depth >= 0 ? depth : fallback
  }

  #shown(node: DomNode): DomNode[] {
    const out: DomNode[] = []
    for (let i = 0; i < node.childNodes.length; i++) {
      const child = node.childNodes[i]!
      if (child.nodeType === TEXT && !(child.nodeValue ?? '').trim()) continue
      out.push(child)
    }
    return out
  }

  #describe(node: DomNode, depth: number, known: Set<number>): Record<string, unknown> {
    const id = this.#source.idOf(node)
    const shown = this.#shown(node)
    const out: Record<string, unknown> = {
      nodeId: id,
      backendNodeId: id,
      nodeType: node.nodeType,
      nodeName: node.nodeName,
      localName: node.nodeType === ELEMENT ? (node.localName ?? '') : '',
      nodeValue: node.nodeValue ?? '',
      childNodeCount: shown.length,
    }
    if (node.parentNode) out['parentId'] = this.#source.idOf(node.parentNode)
    if (node.attributes) out['attributes'] = Array.from(node.attributes).flatMap((a) => [a.name, a.value])
    if (node.nodeType === DOCUMENT) Object.assign(out, { documentURL: node.documentURI ?? '', baseURL: node.baseURI ?? '', xmlVersion: '' })
    if (node.nodeType === DOCUMENT_TYPE) Object.assign(out, { publicId: node.publicId ?? '', systemId: node.systemId ?? '' })
    if (depth > 0 && shown.length) {
      known.add(id)
      out['children'] = shown.map((child) => this.#describe(child, depth - 1, known))
    }
    return out
  }

  #previous(node: DomNode): number {
    let previous = node.previousSibling
    while (previous && previous.nodeType === TEXT && !(previous.nodeValue ?? '').trim()) previous = previous.previousSibling
    return previous ? this.#source.idOf(previous) : 0
  }

  #changed(records: readonly DomRecord[]): void {
    for (const session of this.#sessions) {
      const known = this.#expanded.get(session)
      if (known) for (const record of records) this.#tell(session, known, record)
    }
  }

  #forget(known: Set<number>, node: DomNode): void {
    if (!known.delete(this.#source.idOf(node))) return
    for (let i = 0; i < node.childNodes.length; i++) this.#forget(known, node.childNodes[i]!)
  }

  #tell(session: number, known: Set<number>, record: DomRecord): void {
    const target = this.#source.idOf(record.target)
    const parent = record.target.parentNode
    const seen = record.target.nodeType === DOCUMENT || (parent !== null && known.has(this.#source.idOf(parent)))
    if (record.type === 'attributes' && record.attributeName) {
      if (!seen) return
      const value = record.target.attributes ? Array.from(record.target.attributes).find((a) => a.name === record.attributeName) : undefined
      if (value) this.#emit(session, 'attributeModified', { nodeId: target, name: record.attributeName, value: value.value })
      else this.#emit(session, 'attributeRemoved', { nodeId: target, name: record.attributeName })
      return
    }
    if (record.type === 'characterData') {
      if (seen) this.#emit(session, 'characterDataModified', { nodeId: target, characterData: record.target.nodeValue ?? '' })
      return
    }
    if (record.type !== 'childList') return
    if (!known.has(target)) {
      if (seen) this.#emit(session, 'childNodeCountUpdated', { nodeId: target, childNodeCount: this.#shown(record.target).length })
      return
    }
    for (let i = 0; i < record.removedNodes.length; i++) {
      const node = record.removedNodes[i]!
      if (node.nodeType === TEXT && !(node.nodeValue ?? '').trim()) continue
      this.#forget(known, node)
      this.#emit(session, 'childNodeRemoved', { parentNodeId: target, nodeId: this.#source.idOf(node) })
    }
    for (let i = 0; i < record.addedNodes.length; i++) {
      const node = record.addedNodes[i]!
      if (node.parentNode !== record.target || (node.nodeType === TEXT && !(node.nodeValue ?? '').trim())) continue
      this.#emit(session, 'childNodeInserted', { parentNodeId: target, previousNodeId: this.#previous(node), node: this.#describe(node, 0, known) })
    }
  }
}
