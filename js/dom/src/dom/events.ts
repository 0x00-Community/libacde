import { File } from '../core/blob'
import { serializable } from '../core/clone'
import { eventPath, hooks, type Event, Event as BaseEvent, type EventInit, type EventTarget } from '../core/event'
import { constants, G, listClass, queueTask, tag } from '../core/host'
import { NodeType } from '../native'
import { Element, flags, handlerOf, handlerSet, windowHandlerOf, windowHandlerSet } from './element'
import { metricsOf, rectOf, rootScroll } from './layout'
import { indexKey } from './legacy'
import { pathHooks, type Node } from './node'
import type { FileSystemHandle } from './picker'
import { documentId, dom, existing, ID, wrap, type Wrapper, inner } from './realm'

export const SHADOW_ROOT = Symbol('shadowRoot')
export const ACTIVATION = Symbol('activation')

export function activation(run: () => void): { run(): void; cancel(): void } {
  return { run, cancel: () => {} }
}

type Activatable = { [ACTIVATION]?: (event: Event) => { run(): void; cancel(): void } | null }

function isClick(event: Event): boolean {
  return event.type === 'click' && event instanceof MouseEvent
}

pathHooks.build = (node: Node, event: Event, path: EventTarget[]) => {
  const type = event.type
  const full = isClick(event)
  const inline = flags.inlineHandlers
  if (inline && inner(node, 'nodeType') === NodeType.Element) handlerOf(node as unknown as Element, type)
  const ancestors = dom.ancestors(node[ID])
  let previous: Wrapper = node
  const last = ancestors.length - 1
  for (let i = 0; i < ancestors.length; i++) {
    const id = ancestors[i]!
    let w = existing(id) as (Wrapper & { [SHADOW_ROOT]?: EventTarget }) | undefined
    if (!w) {
      if (full) w = wrap(id)!
      else if (inline && (i < last || dom.nodeType(id) === NodeType.Element) && dom.hasAttribute(id, `on${type}`)) w = wrap(id)!
      else continue
    }
    if (inline && inner((w as unknown as Node), 'nodeType') === NodeType.Element) handlerOf(w as unknown as Element, type)
    const shadow = w[SHADOW_ROOT]
    if (shadow && previous !== w) path.push(shadow)
    path.push(w as unknown as EventTarget)
    previous = w
  }
  const root = ancestors.length ? ancestors[ancestors.length - 1]! : node[ID]
  if (root === documentId && type !== 'load') path.push(G as unknown as EventTarget)
  if (!full) {
    eventPath(event, () => {
      const list: EventTarget[] = [node]
      for (let i = 0; i < ancestors.length; i++) {
        const w = wrap(ancestors[i]!)! as Wrapper & { [SHADOW_ROOT]?: EventTarget }
        if (w[SHADOW_ROOT]) list.push(w[SHADOW_ROOT])
        list.push(w as unknown as EventTarget)
      }
      if (root === documentId && type !== 'load') list.push(G as unknown as EventTarget)
      return list
    })
  }
}

hooks.activation = (event, path) => {
  if (!(event instanceof MouseEvent)) return null
  for (const target of path) {
    const steps = (target as Activatable)[ACTIVATION]?.(event)
    if (steps) return steps
  }
  return null
}

hooks.compile = (target, _type, source) => {
  try {
    return new Function('event', source).bind(target) as (event: Event) => unknown
  } catch {
    return null
  }
}

hooks.window = windowHandlerOf

hooks.inline = (target, type, set) => {
  if (target === (G as unknown as EventTarget)) {
    if (set) windowHandlerSet(type)
    else windowHandlerOf(type)
    return
  }
  if (!(target instanceof Element)) return
  if (set) handlerSet(target, type)
  else handlerOf(target, type)
}

hooks.passiveDefault = (target, type) => {
  if (type !== 'wheel' && type !== 'mousewheel' && type !== 'touchstart' && type !== 'touchmove') return false
  if (target === (G as unknown as EventTarget)) return true
  const id = (target as unknown as Wrapper)[ID]
  if (id === documentId) return true
  if (id === undefined) return false
  const local = dom.nodeType(id) === NodeType.Element ? dom.localName(id) : ''
  if (local !== 'html' && local !== 'body') return false
  const parent = dom.parent(id)
  if (parent === documentId) return true
  return parent >= 0 && dom.nodeType(parent) === NodeType.Element && dom.localName(parent) === 'html' && dom.parent(parent) === documentId
}

const INIT = Symbol('init')

type Init = Record<string, unknown>

function initFields(cls: { prototype: object }, fields: Init): void {
  for (const [name, fallback] of Object.entries(fields)) {
    const coerce = typeof fallback === 'number' ? Number : typeof fallback === 'boolean' ? Boolean : typeof fallback === 'string' ? String : (v: unknown) => v
    Object.defineProperty(cls.prototype, name, {
      get(this: UIEvent) {
        const value = this[INIT][name]
        return value === undefined || value === null ? fallback : coerce(value)
      },
      enumerable: true,
      configurable: true,
    })
  }
}

export class UIEvent extends BaseEvent {
  readonly [INIT]: Init

  constructor(type: string, init?: (EventInit & Init) | null) {
    super(type, init)
    this[INIT] = init ?? {}
  }

  initUIEvent(type: string, bubbles?: boolean, cancelable?: boolean): void {
    this.initEvent(type, bubbles, cancelable)
  }
}
initFields(UIEvent, { view: null, detail: 0, which: 0 })
tag(UIEvent)

export class FocusEvent extends UIEvent {}
initFields(FocusEvent, { relatedTarget: null })
tag(FocusEvent)

const MODIFIERS: Record<string, string> = {
  Control: 'ctrlKey',
  Shift: 'shiftKey',
  Alt: 'altKey',
  Meta: 'metaKey',
  AltGraph: 'modifierAltGraph',
  CapsLock: 'modifierCapsLock',
}

function modifier(init: Init, key: string): boolean {
  return Object.hasOwn(MODIFIERS, key) && !!init[MODIFIERS[key]!]
}

export class MouseEvent extends UIEvent {
  declare readonly clientX: number
  declare readonly clientY: number
  declare readonly button: number
  declare readonly relatedTarget: EventTarget | null

  get x(): number {
    return this.clientX
  }
  get y(): number {
    return this.clientY
  }
  get pageX(): number {
    return this.clientX + rootScroll(0)
  }
  get pageY(): number {
    return this.clientY + rootScroll(1)
  }
  get offsetX(): number {
    return this.clientX - targetOrigin(this.target, 0)
  }
  get offsetY(): number {
    return this.clientY - targetOrigin(this.target, 1)
  }
  get layerX(): number {
    return this.offsetX
  }
  get layerY(): number {
    return this.offsetY
  }
  get fromElement(): EventTarget | null {
    return this.type === 'mouseover' ? this.relatedTarget : this.type === 'mouseout' ? this.target : null
  }
  get toElement(): EventTarget | null {
    return this.type === 'mouseout' ? this.relatedTarget : this.type === 'mouseover' ? this.target : null
  }
  get which(): number {
    return this.button + 1
  }
  getModifierState(key: string): boolean {
    return modifier(this[INIT], key)
  }
  initMouseEvent(
    type: string,
    bubbles?: boolean,
    cancelable?: boolean,
    _view?: unknown,
    detail?: number,
    screenX?: number,
    screenY?: number,
    clientX?: number,
    clientY?: number,
    ctrlKey?: boolean,
    altKey?: boolean,
    shiftKey?: boolean,
    metaKey?: boolean,
    button?: number,
    relatedTarget?: EventTarget | null,
  ): void {
    this.initEvent(type, bubbles, cancelable)
    Object.assign(this[INIT], { detail, screenX, screenY, clientX, clientY, ctrlKey, altKey, shiftKey, metaKey, button, relatedTarget })
  }
}
initFields(MouseEvent, {
  screenX: 0,
  screenY: 0,
  clientX: 0,
  clientY: 0,
  movementX: 0,
  movementY: 0,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  metaKey: false,
  button: 0,
  buttons: 0,
  relatedTarget: null,
})
tag(MouseEvent)

function targetOrigin(target: EventTarget | null, axis: 0 | 1): number {
  const id = (target as unknown as Wrapper | null)?.[ID]
  if (id === undefined || dom.nodeType(id) !== NodeType.Element) return 0
  const r = rectOf(id)
  const m = metricsOf(id)
  return r[axis]! + (m ? (axis === 0 ? m.clientLeft : m.clientTop) : 0)
}

export class PointerEvent extends MouseEvent {
  getCoalescedEvents(): PointerEvent[] {
    return [this]
  }
  getPredictedEvents(): PointerEvent[] {
    return []
  }
}
initFields(PointerEvent, {
  pointerId: 0,
  width: 1,
  height: 1,
  pressure: 0,
  tangentialPressure: 0,
  tiltX: 0,
  tiltY: 0,
  twist: 0,
  pointerType: '',
  isPrimary: false,
})
tag(PointerEvent)

export class WheelEvent extends MouseEvent {
  declare readonly deltaX: number
  declare readonly deltaY: number
  get wheelDeltaX(): number {
    return Math.round(-this.deltaX * 1.2)
  }
  get wheelDeltaY(): number {
    return Math.round(-this.deltaY * 1.2)
  }
  get wheelDelta(): number {
    return this.wheelDeltaY
  }
}
initFields(WheelEvent, { deltaX: 0, deltaY: 0, deltaZ: 0, deltaMode: 0 })
constants(WheelEvent, { DOM_DELTA_PIXEL: 0, DOM_DELTA_LINE: 1, DOM_DELTA_PAGE: 2 })
tag(WheelEvent)

export class KeyboardEvent extends UIEvent {
  get which(): number {
    const k = this[INIT]
    return Number(k.which ?? k.keyCode ?? k.charCode ?? 0)
  }
  getModifierState(key: string): boolean {
    return modifier(this[INIT], key)
  }
  initKeyboardEvent(
    type: string,
    bubbles?: boolean,
    cancelable?: boolean,
    _view?: unknown,
    key?: string,
    location?: number,
    ctrlKey?: boolean,
    altKey?: boolean,
    shiftKey?: boolean,
    metaKey?: boolean,
  ): void {
    this.initEvent(type, bubbles, cancelable)
    Object.assign(this[INIT], { key, location, ctrlKey, altKey, shiftKey, metaKey })
  }
}
initFields(KeyboardEvent, {
  key: '',
  code: '',
  location: 0,
  repeat: false,
  isComposing: false,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  metaKey: false,
  charCode: 0,
  keyCode: 0,
})
constants(KeyboardEvent, { DOM_KEY_LOCATION_STANDARD: 0, DOM_KEY_LOCATION_LEFT: 1, DOM_KEY_LOCATION_RIGHT: 2, DOM_KEY_LOCATION_NUMPAD: 3 })
tag(KeyboardEvent)

export class CompositionEvent extends UIEvent {}
initFields(CompositionEvent, { data: '' })
tag(CompositionEvent)

export class InputEvent extends UIEvent {
  getTargetRanges(): unknown[] {
    return [...((this[INIT].targetRanges as unknown[] | undefined) ?? [])]
  }
}
initFields(InputEvent, { data: null, inputType: '', isComposing: false, dataTransfer: null })
tag(InputEvent)

export class DragEvent extends MouseEvent {}
initFields(DragEvent, { dataTransfer: null })
tag(DragEvent)

export class ClipboardEvent extends BaseEvent {
  readonly #data: DataTransfer | null
  constructor(type: string, init?: EventInit & { clipboardData?: DataTransfer | null } | null) {
    super(type, init)
    this.#data = init?.clipboardData ?? null
  }
  get clipboardData(): DataTransfer | null {
    return this.#data
  }
}
tag(ClipboardEvent)

type SimpleEvent<T> = new (type: string, init?: EventInit & Partial<T>) => BaseEvent & Readonly<T>

function simpleEvent<T extends Record<string, unknown>>(name: string, defaults: T): SimpleEvent<T> {
  const cls = class extends BaseEvent {
    constructor(type: string, init?: EventInit & Partial<T>) {
      super(type, init)
      for (const key of Object.keys(defaults)) {
        const value = init?.[key as keyof typeof init] ?? defaults[key]
        Object.defineProperty(this, key, { value, enumerable: true })
      }
    }
  }
  Object.defineProperty(cls, 'name', { value: name })
  tag(cls, name)
  return cls as never
}

export const HashChangeEvent = simpleEvent('HashChangeEvent', { oldURL: '', newURL: '' })
export const PopStateEvent = simpleEvent('PopStateEvent', { state: null as unknown, hasUAVisualTransition: false })
export const PageTransitionEvent = simpleEvent('PageTransitionEvent', { persisted: false })
export const StorageEvent = simpleEvent('StorageEvent', {
  key: null as string | null,
  oldValue: null as string | null,
  newValue: null as string | null,
  url: '',
  storageArea: null as unknown,
})
export const TransitionEvent = simpleEvent('TransitionEvent', { propertyName: '', elapsedTime: 0, pseudoElement: '' })
export const AnimationEvent = simpleEvent('AnimationEvent', { animationName: '', elapsedTime: 0, pseudoElement: '' })
export const SubmitEvent = simpleEvent('SubmitEvent', { submitter: null as unknown })
export const ToggleEvent = simpleEvent('ToggleEvent', { oldState: '', newState: '' })
export const FormDataEvent = simpleEvent('FormDataEvent', { formData: null as unknown })
export class BeforeUnloadEvent extends BaseEvent {}
{
  const values = new WeakMap<object, string>()
  Object.defineProperty(BeforeUnloadEvent.prototype, 'returnValue', {
    get(this: BaseEvent) {
      return values.get(this) ?? ''
    },
    set(this: BaseEvent, value: unknown) {
      const text = String(value ?? '')
      values.set(this, text)
      if (text) this.preventDefault()
    },
    enumerable: true,
    configurable: true,
  })
}
tag(BeforeUnloadEvent)
export const MediaQueryListEvent = simpleEvent('MediaQueryListEvent', { media: '', matches: false })
export const SecurityPolicyViolationEvent = simpleEvent('SecurityPolicyViolationEvent', {})

export const FileList = listClass<File>('FileList')
serializable(FileList, 'FileList', (list) => [...list], (files) => new FileList([...files] as File[]))
export type FileList = InstanceType<typeof FileList>

export class DataTransferItem {
  readonly #value: string | File | null
  readonly #handle: FileSystemHandle | null

  constructor(
    readonly kind: 'string' | 'file',
    readonly type: string,
    value: string | File | null,
    handle: FileSystemHandle | null = null,
  ) {
    this.#value = value
    this.#handle = handle
  }
  getAsString(callback: ((data: string) => void) | null): void {
    if (this.kind !== 'string' || typeof callback !== 'function') return
    const value = this.#value as string
    queueTask(() => callback(value))
  }
  getAsFile(): File | null {
    return this.kind === 'file' ? (this.#value as File | null) : null
  }
  getAsFileSystemHandle(): Promise<FileSystemHandle | null> {
    return Promise.resolve(this.kind === 'file' ? this.#handle : null)
  }
  webkitGetAsEntry(): null {
    return null
  }
}
tag(DataTransferItem)

const itemOwners = new WeakMap<object, DataTransfer>()

function itemOwner(list: object): DataTransfer {
  const owner = itemOwners.get(list)
  if (!owner) throw new TypeError('Illegal invocation')
  return owner
}

const itemIndex: ProxyHandler<DataTransferItemList> = {
  get(target, key) {
    const i = indexKey(key)
    return i >= 0 ? transferItems(itemOwner(target))[i] : Reflect.get(target, key)
  },
  has(target, key) {
    const i = indexKey(key)
    return i >= 0 ? i < target.length : Reflect.has(target, key)
  },
  ownKeys(target) {
    return [...Array.from({ length: target.length }, (_, i) => String(i)), ...Reflect.ownKeys(target)]
  },
  getOwnPropertyDescriptor(target, key) {
    const i = indexKey(key)
    if (i < 0) return Reflect.getOwnPropertyDescriptor(target, key)
    const value = transferItems(itemOwner(target))[i]
    return value ? { value, enumerable: true, configurable: true, writable: false } : undefined
  },
  set(target, key, value) {
    return indexKey(key) >= 0 ? true : Reflect.set(target, key, value)
  },
  defineProperty(target, key, descriptor) {
    return indexKey(key) >= 0 ? false : Reflect.defineProperty(target, key, descriptor)
  },
}

export class DataTransferItemList {
  constructor(owner: DataTransfer) {
    itemOwners.set(this, owner)
  }
  get length(): number {
    return transferItems(itemOwner(this)).length
  }
  add(data: string | File, type?: string): DataTransferItem | null {
    const owner = itemOwner(this)
    if (typeof data === 'string') {
      owner.setData(String(type), data)
      return transferItems(owner).find((item) => item.kind === 'string' && item.type === String(type).toLowerCase()) ?? null
    }
    addTransferFile(owner, data)
    return transferItems(owner).at(-1) ?? null
  }
  remove(index: number): void {
    removeTransferItem(itemOwner(this), index)
  }
  clear(): void {
    clearTransferItems(itemOwner(this))
  }
  [Symbol.iterator](): IterableIterator<DataTransferItem> {
    return transferItems(itemOwner(this))[Symbol.iterator]()
  }
}
tag(DataTransferItemList)

function itemList(owner: DataTransfer): DataTransferItemList {
  const list = new Proxy(new DataTransferItemList(owner), itemIndex)
  itemOwners.set(list, owner)
  return list
}

let transferItems: (transfer: DataTransfer) => DataTransferItem[]
let addTransferFile: (transfer: DataTransfer, file: File) => void
let removeTransferItem: (transfer: DataTransfer, index: number) => void
let clearTransferItems: (transfer: DataTransfer) => void
let addTransferEntry: (transfer: DataTransfer, file: File | null, handle: FileSystemHandle) => void

export function transferEntry(transfer: DataTransfer, file: File | null, handle: FileSystemHandle): void {
  addTransferEntry(transfer, file, handle)
}

export class DataTransfer {
  #data = new Map<string, string>()
  #files: File[] = []
  #handles = new WeakMap<File, FileSystemHandle>()
  #entries: FileSystemHandle[] = []
  #items: DataTransferItemList | null = null
  #itemCache = new Map<unknown, DataTransferItem>()
  dropEffect = 'none'
  effectAllowed = 'uninitialized'

  get types(): readonly string[] {
    const types = [...this.#data.keys()]
    if (this.#files.length || this.#entries.length) types.push('Files')
    return Object.freeze(types)
  }

  get files(): FileList {
    return new FileList(this.#files.slice())
  }

  get items(): DataTransferItemList {
    return (this.#items ??= itemList(this))
  }

  static {
    transferItems = (transfer) => {
      const before = transfer.#itemCache
      const cache = new Map<unknown, DataTransferItem>()
      const item = (key: unknown, make: () => DataTransferItem) => {
        const found = before.get(key) ?? make()
        cache.set(key, found)
        return found
      }
      const items: DataTransferItem[] = []
      for (const [type, value] of transfer.#data) items.push(item(JSON.stringify([type, value]), () => new DataTransferItem('string', type, value)))
      for (const file of transfer.#files) items.push(item(file, () => new DataTransferItem('file', file.type, file, transfer.#handles.get(file) ?? null)))
      for (const handle of transfer.#entries) items.push(item(handle, () => new DataTransferItem('file', '', null, handle)))
      transfer.#itemCache = cache
      return items
    }
    addTransferFile = (transfer, file) => {
      transfer.#files.push(file)
    }
    removeTransferItem = (transfer, index) => {
      const keys = [...transfer.#data.keys()]
      const files = transfer.#files.length
      if (index < keys.length) transfer.#data.delete(keys[index]!)
      else if (index < keys.length + files) transfer.#files.splice(index - keys.length, 1)
      else transfer.#entries.splice(index - keys.length - files, 1)
    }
    clearTransferItems = (transfer) => {
      transfer.#data.clear()
      transfer.#files.length = 0
      transfer.#entries.length = 0
    }
    addTransferEntry = (transfer, file, handle) => {
      if (!file) return void transfer.#entries.push(handle)
      transfer.#files.push(file)
      transfer.#handles.set(file, handle)
    }
  }

  static #normalize(format: string): string {
    const f = String(format).toLowerCase()
    return f === 'text' ? 'text/plain' : f === 'url' ? 'text/uri-list' : f
  }

  setData(format: string, data: string): void {
    this.#data.set(DataTransfer.#normalize(format), String(data))
  }

  getData(format: string): string {
    const key = DataTransfer.#normalize(format)
    const value = this.#data.get(key) ?? ''
    return String(format).toLowerCase() === 'url' ? (value.split(/\r?\n/).find((line) => line && !line.startsWith('#')) ?? '') : value
  }

  clearData(format?: string): void {
    if (format === undefined) this.#data.clear()
    else this.#data.delete(DataTransfer.#normalize(format))
  }

  setDragImage(_element: unknown, _x: number, _y: number): void {}
}
tag(DataTransfer)
