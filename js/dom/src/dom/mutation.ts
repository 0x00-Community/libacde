import { guarded, host, tag } from '../core/host'
import { staticNodeList, type Node, type NodeList } from './node'
import { dom, existing, ID, wrap, type Wrapper } from './realm'

interface MutationObserverInit {
  childList?: boolean
  attributes?: boolean
  characterData?: boolean
  subtree?: boolean
  attributeOldValue?: boolean
  characterDataOldValue?: boolean
  attributeFilter?: string[]
}

export interface Options {
  childList: boolean
  attributes: boolean
  characterData: boolean
  subtree: boolean
  attributeOldValue: boolean
  characterDataOldValue: boolean
  attributeFilter: Set<string> | null
}

interface Registration {
  readonly observer: MutationObserver
  options: Options
}

const REGISTRATIONS = Symbol('registrations')
type Observed = Wrapper & { [REGISTRATIONS]?: Registration[] }

let registered = 0
let subtrees = 0
export function observing(): boolean {
  return registered > 0
}

export type Observers = Map<MutationObserver, Options>

const pending = new Set<MutationObserver>()
let queued = false

export class MutationRecord {
  constructor(
    readonly type: 'childList' | 'attributes' | 'characterData',
    readonly target: object,
    readonly addedNodes: NodeList,
    readonly removedNodes: NodeList,
    readonly previousSibling: object | null,
    readonly nextSibling: object | null,
    readonly attributeName: string | null,
    readonly attributeNamespace: string | null,
    readonly oldValue: string | null,
  ) {}
}
tag(MutationRecord)

let enqueue: (observer: MutationObserver, record: MutationRecord) => void
let deliverOne: (observer: MutationObserver) => void

export class MutationObserver {
  readonly #callback: (records: MutationRecord[], observer: MutationObserver) => void
  #records: MutationRecord[] = []
  #nodes: Observed[] = []

  constructor(callback: (records: MutationRecord[], observer: MutationObserver) => void) {
    if (typeof callback !== 'function') throw new TypeError("Failed to construct 'MutationObserver': parameter 1 is not of type 'Function'.")
    this.#callback = callback
  }

  static {
    enqueue = (observer, record) => {
      observer.#records.push(record)
      pending.add(observer)
      if (!queued) {
        queued = true
        host.queueMicrotask(deliver)
      }
    }
    deliverOne = (observer) => {
      const records = observer.#records
      if (records.length === 0) return
      observer.#records = []
      guarded(() => observer.#callback.call(observer, records, observer))
    }
  }

  observe(target: object, init: MutationObserverInit = {}): void {
    const node = target as Observed
    if (!node || typeof node !== 'object' || !(ID in node)) throw new TypeError("Failed to execute 'observe' on 'MutationObserver': parameter 1 is not of type 'Node'.")
    let attributes = init.attributes
    let characterData = init.characterData
    if (attributes === undefined && (init.attributeOldValue !== undefined || init.attributeFilter !== undefined)) attributes = true
    if (characterData === undefined && init.characterDataOldValue !== undefined) characterData = true
    if (!init.childList && !attributes && !characterData) throw new TypeError("Failed to execute 'observe' on 'MutationObserver': The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.")
    if (init.attributeOldValue && !attributes) throw new TypeError("Failed to execute 'observe' on 'MutationObserver': The options object may only set 'attributeOldValue' to true when 'attributes' is true or not present.")
    if (init.attributeFilter && !attributes) throw new TypeError("Failed to execute 'observe' on 'MutationObserver': The options object may only set 'attributeFilter' when 'attributes' is true or not present.")
    if (init.characterDataOldValue && !characterData) throw new TypeError("Failed to execute 'observe' on 'MutationObserver': The options object may only set 'characterDataOldValue' to true when 'characterData' is true or not present.")
    const options: Options = {
      childList: !!init.childList,
      attributes: !!attributes,
      characterData: !!characterData,
      subtree: !!init.subtree,
      attributeOldValue: !!init.attributeOldValue,
      characterDataOldValue: !!init.characterDataOldValue,
      attributeFilter: init.attributeFilter ? new Set([...init.attributeFilter].map(String)) : null,
    }
    const list = (node[REGISTRATIONS] ??= [])
    const existingReg = list.find((r) => r.observer === this)
    if (existingReg) {
      subtrees += Number(options.subtree) - Number(existingReg.options.subtree)
      existingReg.options = options
    } else {
      list.push({ observer: this, options })
      this.#nodes.push(node)
      registered++
      if (options.subtree) subtrees++
    }
  }

  disconnect(): void {
    for (const node of this.#nodes) {
      const list = node[REGISTRATIONS]
      if (!list) continue
      const at = list.findIndex((r) => r.observer === this)
      if (at >= 0) {
        if (list[at]!.options.subtree) subtrees--
        list.splice(at, 1)
        registered--
      }
    }
    this.#nodes = []
    this.#records = []
    pending.delete(this)
  }

  takeRecords(): MutationRecord[] {
    const records = this.#records
    this.#records = []
    return records
  }
}
tag(MutationObserver)

function deliver(): void {
  queued = false
  const observers = [...pending]
  pending.clear()
  for (const observer of observers) deliverOne(observer)
}

function interested(target: number, accept: (options: Options) => boolean): Observers | null {
  let found: Observers | null = null
  const visit = (id: number, self: boolean) => {
    const node = existing(id) as Observed | undefined
    const list = node?.[REGISTRATIONS]
    if (!list || list.length === 0) return
    for (const r of list) {
      if (!self && !r.options.subtree) continue
      if (!accept(r.options)) continue
      found ??= new Map()
      if (!found.has(r.observer)) found.set(r.observer, r.options)
    }
  }
  visit(target, true)
  if (subtrees === 0) return found
  const ancestors = dom.ancestors(target)
  for (let i = 0; i < ancestors.length; i++) visit(ancestors[i]!, false)
  return found
}

export function childListObservers(parent: number): Observers | null {
  return interested(parent, (o) => o.childList)
}

export function recordChildList(parent: number, observers: Observers, added: readonly Node[], removed: readonly Node[], previous: object | null, next: object | null): void {
  const target = wrap<object>(parent)!
  const addedList = staticNodeList(added)
  const removedList = staticNodeList(removed)
  for (const observer of observers.keys()) enqueue(observer, new MutationRecord('childList', target, addedList, removedList, previous, next, null, null, null))
}

export function attributeObservers(element: number, name: string): { observers: Observers; oldValue: boolean } | null {
  const observers = interested(element, (o) => o.attributes && (!o.attributeFilter || o.attributeFilter.has(name)))
  if (!observers) return null
  for (const options of observers.values()) if (options.attributeOldValue) return { observers, oldValue: true }
  return { observers, oldValue: false }
}

export function recordAttribute(target: object, interest: { observers: Observers }, name: string, oldValue: string | null, namespace: string | null = null): void {
  const empty = staticNodeList([])
  for (const [observer, options] of interest.observers) enqueue(observer, new MutationRecord('attributes', target, empty, empty, null, null, name, namespace, options.attributeOldValue ? oldValue : null))
}

export function characterDataObservers(node: number): Observers | null {
  return interested(node, (o) => o.characterData)
}

export function recordCharacterData(target: object, observers: Observers, oldValue: string | null): void {
  const empty = staticNodeList([])
  for (const [observer, options] of observers) enqueue(observer, new MutationRecord('characterData', target, empty, empty, null, null, null, null, options.characterDataOldValue ? oldValue : null))
}
