import { NONE, type DomNative, type NodeType, type Ns } from '../native'

export let dom: DomNative = null as unknown as DomNative
export let documentId = 0

export const ID = Symbol('id')
export const CONNECTED = Symbol('connected')

export interface Wrapper {
  [ID]: number;
  [CONNECTED]: boolean
}

type WrapperClass = new () => Wrapper

const strong = new Map<number, Wrapper>()
const weak = new Map<number, WeakRef<Wrapper>>()
const collected = new FinalizationRegistry<number>((id) => {
  const ref = weak.get(id)
  if (ref?.deref()) return
  if (ref) weak.delete(id)
  if (!strong.has(id)) dom.unpin(id)
})

export let version = 0
export let treeVersion = 0
export function changed(): void {
  version++
}
export function treeChanged(): void {
  version++
  treeVersion++
}

export let layoutEpoch = 0
export function moved(): void {
  layoutEpoch++
}

let creating = NONE
let creatingConnected = false
let shaping: ProxyHandler<Wrapper> | null = null

export function exotic<T extends object>(handler: ProxyHandler<T>): void {
  if (creating !== NONE) shaping = handler as ProxyHandler<Wrapper>
}

export function adopt<T extends Wrapper>(wrapper: T): T {
  const id = creating
  if (id === NONE) throw new TypeError('Illegal constructor')
  creating = NONE
  wrapper[ID] = id
  wrapper[CONNECTED] = creatingConnected
  let outer = wrapper
  if (shaping) {
    outer = new Proxy(wrapper, shaping as ProxyHandler<T>)
    shaping = null
  }
  if (creatingConnected) strong.set(id, outer)
  else weak.set(id, new WeakRef(outer))
  collected.register(outer, id)
  return outer
}

export function nodeConstructing(): boolean {
  return creating !== NONE
}

export function constructing(id: number, connected = false): void {
  creating = id
  creatingConnected = connected
  shaping = null
}

export function construct<T>(cls: new () => T, id: number, connected = false): T {
  constructing(id, connected)
  try {
    return new cls()
  } finally {
    shaping = null
    if (creating === id) {
      creating = NONE
      if (!existing(id)) dom.unpin(id)
    }
  }
}

export const classes = {
  of: null as unknown as (type: NodeType, ns: Ns, local: string) => WrapperClass,
}

export function existing(id: number): Wrapper | undefined {
  const node = strong.get(id)
  if (node) return node
  return weak.get(id)?.deref()
}

export function wrap<T = Wrapper>(id: number): T | null {
  if (id < 0) return null
  const node = existing(id)
  if (node) return node as T
  const [type, ns, connected, local] = dom.describe(id)
  return construct(classes.of(type, ns, local), id, connected) as T
}

export function idOf(node: unknown): number {
  return node !== null && typeof node === 'object' && ID in node ? (node as Wrapper)[ID] : NONE
}

export const lifecycle = {
  connected: [] as ((node: Wrapper) => void)[],
  disconnected: [] as ((node: Wrapper) => void)[],
}

export function attach(ids: Uint32Array | readonly number[]): void {
  const hooks = lifecycle.connected
  for (let i = 0; i < ids.length; i++) {
    const id = ids[i]!
    let node = strong.get(id)
    if (!node) {
      const ref = weak.get(id)
      node = ref?.deref()
      if (!node) continue
      weak.delete(id)
      strong.set(id, node)
    }
    if (node[CONNECTED]) continue
    node[CONNECTED] = true
    for (const hook of hooks) hook(node)
  }
}

export function detach(ids: Uint32Array | readonly number[]): void {
  const hooks = lifecycle.disconnected
  for (let i = 0; i < ids.length; i++) {
    const id = ids[i]!
    const node = strong.get(id)
    if (!node) continue
    strong.delete(id)
    weak.set(id, new WeakRef(node))
    if (!node[CONNECTED]) continue
    node[CONNECTED] = false
    for (const hook of hooks) hook(node)
  }
}

let rootsVersion = -1
let root = NONE
let head = NONE
let body = NONE

export function roots(): void {
  if (rootsVersion === treeVersion) return
  rootsVersion = treeVersion
  root = documentElementOf(documentId)
  head = root === NONE ? NONE : childNamed(root, 'head')
  body = root === NONE ? NONE : childNamed(root, 'body', 'frameset')
}

export function documentElementOf(id: number): number {
  const children = dom.children(id)
  return children.length ? children[0]! : NONE
}

export function childNamed(parent: number, name: string, other = name): number {
  const children = dom.children(parent)
  for (let i = 0; i < children.length; i++) {
    const local = dom.localName(children[i]!)
    if (local === name || local === other) return children[i]!
  }
  return NONE
}

export function rootElementId(): number {
  roots()
  return root
}

export function headId(): number {
  roots()
  return head
}

export function bodyId(): number {
  roots()
  return body
}

const STATE_CALLS = ['setFocus', 'setActive', 'setValue', 'setChecked', 'setSelectedIndex'] as const

export function bindDom(d: DomNative): void {
  const state: Partial<DomNative> = {}
  for (const name of STATE_CALLS) {
    const call = d[name] as (...args: unknown[]) => void
    ;(state as Record<string, unknown>)[name] = (...args: unknown[]) => {
      call.apply(d, args)
      layoutEpoch++
    }
  }
  dom = { ...d, ...state }
  documentId = d.document()
}

export function wrapperStats(): { strong: number; weak: number } {
  return { strong: strong.size, weak: weak.size }
}

export function strongIds(): number[] {
  return [...strong.keys()]
}

export function inner<T extends object, K extends keyof T>(node: T, key: K): T[K] {
  return Reflect.get(Object.getPrototypeOf(node) as object, key, node) as T[K]
}
