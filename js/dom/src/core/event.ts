import { DOMException } from './exception'
import { constants, G, host, report, tag } from './host'
import { now as clockNow } from './clock'

const enum Phase {
  None = 0,
  Capturing = 1,
  AtTarget = 2,
  Bubbling = 3,
}

interface Listener {
  readonly callback: unknown
  readonly capture: boolean
  readonly once: boolean
  readonly passive: boolean
  removed: boolean
  readonly handler: HandlerEntry | null
}

interface HandlerEntry {
  value: unknown
  listener: Listener | null
}

export const LISTENERS = Symbol('listeners')
const HANDLERS = Symbol('handlers')
export const BUILD_PATH = Symbol('buildPath')

type Targetish = EventTarget & { [LISTENERS]?: Map<string, Listener[]> | null; [HANDLERS]?: Map<string, HandlerEntry> | null; [BUILD_PATH]?: (event: Event, path: EventTarget[]) => void }

export const hooks = {
  activation: null as null | ((event: Event, path: readonly EventTarget[]) => { run(): void; cancel(): void } | null),
  compile: null as null | ((target: EventTarget, type: string, source: string) => ((event: Event) => unknown) | null),
  passiveDefault: null as null | ((target: EventTarget, type: string) => boolean),
  inline: null as null | ((target: EventTarget, type: string, set: boolean) => void),
  checkpoint: null as null | (() => void),
  listen: null as null | ((type: string) => void),
  window: null as null | ((type: string) => void),
}

let thrown = 0
export function listenerThrows(): number {
  return thrown
}

let depth = 0
let checkpoints = false

let currentEvent: Event | undefined
export function windowEvent(): Event | undefined {
  return currentEvent
}

export interface EventInit {
  bubbles?: boolean
  cancelable?: boolean
  composed?: boolean
}

let eventTrusted: (event: Event, trusted: boolean) => void
export let eventPath: (event: Event, full: (() => EventTarget[]) | null) => void
let dispatch: (target: EventTarget, event: Event) => boolean
let invoke: (target: EventTarget, event: Event, capture: boolean) => void

export class Event {
  #type: string
  #bubbles: boolean
  #cancelable: boolean
  #composed: boolean
  #target: EventTarget | null = null
  #current: EventTarget | null = null
  #phase: Phase = Phase.None
  #stop = false
  #stopNow = false
  #canceled = false
  #passive = false
  #dispatching = false
  #initialized = true
  #trusted = false
  #timeStamp: number
  #path: EventTarget[] | null = null
  #full: (() => EventTarget[]) | null = null

  constructor(type: string, init?: EventInit | null) {
    if (arguments.length === 0) throw new TypeError("Failed to construct 'Event': 1 argument required, but only 0 present.")
    this.#type = String(type)
    this.#bubbles = !!init?.bubbles
    this.#cancelable = !!init?.cancelable
    this.#composed = !!init?.composed
    this.#timeStamp = clockNow()
  }

  static {
    eventTrusted = (event, trusted) => {
      event.#trusted = trusted
    }
    eventPath = (event, full) => {
      event.#full = full
    }
    dispatch = (target, event) => {
      if (event.#dispatching || !event.#initialized) throw new DOMException('The event is already being dispatched.', 'InvalidStateError')
      event.#dispatching = true
      depth++
      event.#target = target
      event.#canceled = event.#canceled && event.#cancelable
      const path: EventTarget[] = [target]
      ;(target as Targetish)[BUILD_PATH]?.(event, path)
      if (hooks.window && path[path.length - 1] === (G as unknown as EventTarget)) hooks.window(event.#type)
      event.#path = path
      const activation = event.#type === 'click' && hooks.activation ? hooks.activation(event, path) : null
      const previous = currentEvent
      try {
        for (let i = path.length - 1; i >= 0 && !event.#stop; i--) {
          event.#phase = i === 0 ? Phase.AtTarget : Phase.Capturing
          invoke(path[i]!, event, true)
        }
        for (let i = 0; i < path.length && !event.#stop; i++) {
          if (i > 0 && !event.#bubbles) break
          event.#phase = i === 0 ? Phase.AtTarget : Phase.Bubbling
          invoke(path[i]!, event, false)
        }
      } finally {
        depth--
        currentEvent = previous
        event.#phase = Phase.None
        event.#current = null
        event.#dispatching = false
        event.#stop = false
        event.#stopNow = false
        event.#path = null
        event.#full = null
      }
      if (activation) {
        if (event.#canceled) activation.cancel()
        else activation.run()
      }
      return !event.#canceled
    }
    invoke = (target, event, capture) => {
      const map = (target as Targetish)[LISTENERS]
      const list = map?.get(event.#type)
      if (!list || list.length === 0) return
      event.#current = target
      for (let i = 0; i < list.length; i++) {
        const listener = list[i]!
        if (listener.removed) continue
        if (event.#phase === Phase.Capturing && !listener.capture) continue
        if (event.#phase === Phase.Bubbling && listener.capture) continue
        if (event.#phase === Phase.AtTarget && listener.capture !== capture) continue
        if (listener.once) removeListener(target as Targetish, event.#type, listener)
        event.#passive = listener.passive
        currentEvent = event
        try {
          call(target, listener, event)
        } catch (error) {
          thrown++
          report(error)
        }
        if (checkpoints && depth === 1) {
          host.runMicrotasks?.()
          hooks.checkpoint?.()
        }
        event.#passive = false
        event.#current = target
        if (event.#stopNow) break
      }
    }
  }

  get type(): string {
    return this.#type
  }
  get target(): EventTarget | null {
    return this.#target
  }
  get srcElement(): EventTarget | null {
    return this.#target
  }
  get currentTarget(): EventTarget | null {
    return this.#current
  }
  get eventPhase(): number {
    return this.#phase
  }
  get bubbles(): boolean {
    return this.#bubbles
  }
  get cancelable(): boolean {
    return this.#cancelable
  }
  get composed(): boolean {
    return this.#composed
  }
  get defaultPrevented(): boolean {
    return this.#canceled
  }
  get isTrusted(): boolean {
    return this.#trusted
  }
  get timeStamp(): number {
    return this.#timeStamp
  }
  get returnValue(): boolean {
    return !this.#canceled
  }
  set returnValue(value: boolean) {
    if (!value) this.preventDefault()
  }
  get cancelBubble(): boolean {
    return this.#stop
  }
  set cancelBubble(value: boolean) {
    if (value) this.#stop = true
  }

  composedPath(): EventTarget[] {
    if (!this.#dispatching) return []
    if (this.#full) return this.#full()
    return this.#path ? this.#path.slice() : []
  }

  stopPropagation(): void {
    this.#stop = true
  }

  stopImmediatePropagation(): void {
    this.#stop = true
    this.#stopNow = true
  }

  preventDefault(): void {
    if (this.#cancelable && !this.#passive) this.#canceled = true
  }

  initEvent(type: string, bubbles = false, cancelable = false): void {
    if (this.#dispatching) return
    this.#initialized = true
    this.#stop = false
    this.#stopNow = false
    this.#canceled = false
    this.#trusted = false
    this.#target = null
    this.#type = String(type)
    this.#bubbles = !!bubbles
    this.#cancelable = !!cancelable
  }
}
constants(Event, { NONE: 0, CAPTURING_PHASE: 1, AT_TARGET: 2, BUBBLING_PHASE: 3 })
tag(Event)

function call(target: EventTarget, listener: Listener, event: Event): void {
  if (listener.handler) {
    let fn = listener.handler.value
    if (typeof fn === 'string') fn = listener.handler.value = hooks.compile?.(target, event.type, fn) ?? null
    if (typeof fn !== 'function') return
    if (event.type === 'error' && event instanceof ErrorEvent && target === (G as unknown as EventTarget)) {
      if (fn.call(target, event.message, event.filename, event.lineno, event.colno, event.error) === true) event.preventDefault()
      return
    }
    const result = fn.call(target, event)
    if (result === false) event.preventDefault()
    else if (event.type === 'beforeunload' && result !== undefined && result !== null) {
      event.preventDefault()
      if (typeof (event as { returnValue?: unknown }).returnValue === 'string' && !(event as { returnValue?: unknown }).returnValue) (event as { returnValue?: unknown }).returnValue = String(result)
    }
    return
  }
  const callback = listener.callback
  if (typeof callback === 'function') callback.call(target, event)
  else if (callback !== null && typeof callback === 'object') {
    const handle = (callback as { handleEvent?: unknown }).handleEvent
    if (typeof handle !== 'function') throw new TypeError("The provided callback's 'handleEvent' property is not callable.")
    handle.call(callback, event)
  }
}

function removeListener(target: Targetish, type: string, listener: Listener): void {
  listener.removed = true
  const list = target[LISTENERS]?.get(type)
  if (!list) return
  const at = list.indexOf(listener)
  if (at >= 0) {
    const next = list.slice()
    next.splice(at, 1)
    if (next.length) target[LISTENERS]!.set(type, next)
    else target[LISTENERS]!.delete(type)
  }
}

function addListener(target: Targetish, type: string, listener: Listener): void {
  let map = target[LISTENERS]
  if (!map) map = target[LISTENERS] = new Map()
  const list = map.get(type)
  map.set(type, list ? [...list, listener] : [listener])
  hooks.listen?.(type)
}

function flatten(options: unknown): { capture: boolean; once: boolean; passive: boolean | null; signal: unknown } {
  if (typeof options === 'boolean') return { capture: options, once: false, passive: null, signal: null }
  if (options === null || typeof options !== 'object') return { capture: false, once: false, passive: null, signal: null }
  const o = options as { capture?: unknown; once?: unknown; passive?: unknown; signal?: unknown }
  return { capture: !!o.capture, once: !!o.once, passive: o.passive === undefined ? null : !!o.passive, signal: o.signal ?? null }
}

export class EventTarget {
  [LISTENERS]: Map<string, Listener[]> | null = null

  addEventListener(type: string, callback: unknown, options?: unknown): void {
    const target = (this ?? G) as Targetish
    if (callback === null || callback === undefined) return
    const { capture, once, passive, signal } = flatten(options)
    const s = signal as { aborted?: boolean; addEventListener?: (type: string, fn: () => void, o?: unknown) => void } | null
    if (s?.aborted) return
    type = String(type)
    const list = target[LISTENERS]?.get(type)
    if (list) for (const l of list) if (l.callback === callback && l.capture === capture && !l.handler) return
    const listener: Listener = { callback, capture, once, passive: passive ?? hooks.passiveDefault?.(target, type) ?? false, removed: false, handler: null }
    addListener(target, type, listener)
    s?.addEventListener?.('abort', () => removeListener(target, type, listener), { once: true })
  }

  removeEventListener(type: string, callback: unknown, options?: unknown): void {
    const target = (this ?? G) as Targetish
    const capture = typeof options === 'boolean' ? options : !!(options as { capture?: unknown } | null)?.capture
    const list = target[LISTENERS]?.get(String(type))
    if (!list) return
    for (const l of list) {
      if (l.callback === callback && l.capture === capture && !l.handler) return removeListener(target, String(type), l)
    }
  }

  dispatchEvent(event: Event): boolean {
    const target = (this ?? G) as EventTarget
    if (!(event instanceof Event)) throw new TypeError("Failed to execute 'dispatchEvent' on 'EventTarget': parameter 1 is not of type 'Event'.")
    eventTrusted(event, false)
    return dispatch(target, event)
  }
}
tag(EventTarget)

export function setHandler(target: EventTarget, type: string, value: unknown): void {
  const t = target as Targetish
  let handlers = t[HANDLERS]
  const entry = handlers?.get(type)
  const empty = value === null || value === undefined || (typeof value !== 'function' && typeof value !== 'string' && typeof value !== 'object')
  if (empty) {
    if (entry) {
      if (entry.listener) removeListener(t, type, entry.listener)
      handlers!.delete(type)
    }
    return
  }
  if (entry) {
    entry.value = value
    return
  }
  if (!handlers) handlers = t[HANDLERS] = new Map()
  const fresh: HandlerEntry = { value, listener: null }
  fresh.listener = { callback: null, capture: false, once: false, passive: false, removed: false, handler: fresh }
  handlers.set(type, fresh)
  addListener(t, type, fresh.listener)
}

export function getHandler(target: EventTarget, type: string): unknown {
  const entry = (target as Targetish)[HANDLERS]?.get(type)
  if (!entry) return null
  if (typeof entry.value === 'string') entry.value = hooks.compile?.(target, type, entry.value) ?? null
  return typeof entry.value === 'function' || (entry.value && typeof entry.value === 'object') ? entry.value : null
}

export function defineHandlers(proto: object, types: readonly string[]): void {
  for (const type of types) {
    Object.defineProperty(proto, `on${type}`, {
      get(this: EventTarget) {
        hooks.inline?.(this ?? G, type, false)
        return getHandler(this ?? G, type)
      },
      set(this: EventTarget, value: unknown) {
        hooks.inline?.(this ?? G, type, true)
        setHandler(this ?? G, type, typeof value === 'function' || (value !== null && typeof value === 'object') ? value : null)
      },
      enumerable: true,
      configurable: true,
    })
  }
}

interface CustomEventInit<T = unknown> extends EventInit {
  detail?: T
}

export class CustomEvent<T = unknown> extends Event {
  #detail: T | null

  constructor(type: string, init?: CustomEventInit<T> | null) {
    super(type, init)
    this.#detail = init?.detail ?? null
  }

  get detail(): T | null {
    return this.#detail
  }

  initCustomEvent(type: string, bubbles = false, cancelable = false, detail: T | null = null): void {
    this.initEvent(type, bubbles, cancelable)
    this.#detail = detail
  }
}
tag(CustomEvent)

export class ErrorEvent extends Event {
  readonly message: string
  readonly filename: string
  readonly lineno: number
  readonly colno: number
  readonly error: unknown

  constructor(type: string, init?: EventInit & { message?: string; filename?: string; lineno?: number; colno?: number; error?: unknown } | null) {
    super(type, init)
    this.message = String(init?.message ?? '')
    this.filename = String(init?.filename ?? '')
    this.lineno = Number(init?.lineno ?? 0)
    this.colno = Number(init?.colno ?? 0)
    this.error = init?.error
  }
}
tag(ErrorEvent)

export class PromiseRejectionEvent extends Event {
  readonly promise: Promise<unknown>
  readonly reason: unknown

  constructor(type: string, init: EventInit & { promise: Promise<unknown>; reason?: unknown }) {
    super(type, init)
    this.promise = init?.promise
    this.reason = init?.reason
  }
}
tag(PromiseRejectionEvent)

export class MessageEvent<T = unknown> extends Event {
  readonly data: T
  readonly origin: string
  readonly lastEventId: string
  readonly source: unknown
  readonly ports: readonly unknown[]

  constructor(type: string, init?: EventInit & { data?: T; origin?: string; lastEventId?: string; source?: unknown; ports?: unknown[] } | null) {
    super(type, init)
    this.data = init?.data as T
    this.origin = String(init?.origin ?? '')
    this.lastEventId = String(init?.lastEventId ?? '')
    this.source = init?.source ?? null
    this.ports = Object.freeze([...(init?.ports ?? [])])
  }
}
tag(MessageEvent)

export class ProgressEvent extends Event {
  readonly lengthComputable: boolean
  readonly loaded: number
  readonly total: number

  constructor(type: string, init?: EventInit & { lengthComputable?: boolean; loaded?: number; total?: number } | null) {
    super(type, init)
    this.lengthComputable = !!init?.lengthComputable
    this.loaded = Number(init?.loaded ?? 0)
    this.total = Number(init?.total ?? 0)
  }
}
tag(ProgressEvent)

export class CloseEvent extends Event {
  readonly wasClean: boolean
  readonly code: number
  readonly reason: string

  constructor(type: string, init?: EventInit & { wasClean?: boolean; code?: number; reason?: string } | null) {
    super(type, init)
    this.wasClean = !!init?.wasClean
    this.code = Number(init?.code ?? 0)
    this.reason = String(init?.reason ?? '')
  }
}
tag(CloseEvent)

export function fire(target: EventTarget, event: Event): boolean {
  eventTrusted(event, true)
  return dispatch(target, event)
}

export function fireFromTask(target: EventTarget, event: Event): boolean {
  const outer = checkpoints
  checkpoints = depth === 0
  try {
    return fire(target, event)
  } finally {
    checkpoints = outer
  }
}
