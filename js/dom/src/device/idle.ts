import { defineHandlers, Event, EventTarget, fire } from '../core/event'
import { DOMException } from '../core/exception'
import { getters, tag } from '../core/host'

export interface IdleStatus {
  readonly idle: boolean
  readonly idleForMs: number | null
  readonly locked: boolean
}

export interface IdleNative {
  idle?(): Promise<IdleStatus>
  watchIdle?(callback: (status: IdleStatus) => void): void
  idlePermission?(): 'granted' | 'denied' | 'prompt'
  requestIdlePermission?(): Promise<boolean>
}

type UserState = 'active' | 'idle'
type ScreenState = 'locked' | 'unlocked'

const MIN_THRESHOLD = 60_000

class IdleSource {
  readonly #native: IdleNative
  readonly #listeners = new Set<(status: IdleStatus, at: number) => void>()
  #last: { status: IdleStatus; at: number } | null = null
  #watching = false

  constructor(native: IdleNative) {
    this.#native = native
  }

  async current(): Promise<{ status: IdleStatus; at: number }> {
    if (!this.#last) this.#last = { status: await this.#native.idle!(), at: performance.now() }
    return this.#last
  }

  listen(listener: (status: IdleStatus, at: number) => void): () => void {
    if (!this.#watching) {
      this.#watching = true
      this.#native.watchIdle!((status) => {
        const at = performance.now()
        this.#last = { status, at }
        for (const each of [...this.#listeners]) each(status, at)
      })
    }
    this.#listeners.add(listener)
    return () => this.#listeners.delete(listener)
  }
}

const sources = new WeakMap<IdleNative, IdleSource>()
const states = new WeakMap<IdleDetector, { user: UserState | null; screen: ScreenState | null }>()
let native: IdleNative | null = null

export function idlePermission(): 'granted' | 'denied' | 'prompt' {
  return native?.idle ? (native.idlePermission?.() ?? 'denied') : 'denied'
}

export class IdleDetector extends EventTarget {
  #stop: (() => void) | null = null
  #timer: ReturnType<typeof setTimeout> | null = null

  constructor() {
    super()
    states.set(this, { user: null, screen: null })
  }

  static install(given: IdleNative): void {
    native = given
  }

  static async requestPermission(): Promise<'granted' | 'denied'> {
    const gate = native
    if (!gate?.requestIdlePermission) return 'denied'
    return (await gate.requestIdlePermission()) ? 'granted' : 'denied'
  }

  async start(options: { threshold?: number; signal?: AbortSignal } = {}): Promise<void> {
    const threshold = Number(options.threshold ?? MIN_THRESHOLD)
    if (!Number.isFinite(threshold) || threshold < MIN_THRESHOLD) throw new TypeError(`Failed to execute 'start' on 'IdleDetector': Minimum threshold is 1 minute.`)
    const signal = options.signal
    if (signal?.aborted) throw signal.reason ?? new DOMException('The operation was aborted.', 'AbortError')
    const gate = native
    if (!gate?.idle || gate.idlePermission?.() !== 'granted') throw new DOMException('Idle detection permission denied', 'NotAllowedError')
    if (this.#stop) throw new DOMException('Idle detector is already started.', 'InvalidStateError')
    let source = sources.get(gate)
    if (!source) sources.set(gate, (source = new IdleSource(gate)))
    const { status, at } = await source.current()
    if (signal?.aborted) throw signal.reason ?? new DOMException('The operation was aborted.', 'AbortError')
    this.#apply(status, at, threshold, false)
    this.#stop = source.listen((next, when) => this.#apply(next, when, threshold, true))
    signal?.addEventListener('abort', () => this.#halt(), { once: true })
  }

  #halt(): void {
    this.#stop?.()
    this.#stop = null
    if (this.#timer !== null) clearTimeout(this.#timer)
    this.#timer = null
  }

  #apply(status: IdleStatus, at: number, threshold: number, notify: boolean): void {
    if (this.#timer !== null) clearTimeout(this.#timer)
    this.#timer = null
    const idleFor = status.idle ? (status.idleForMs ?? 0) + (performance.now() - at) : 0
    const user: UserState = status.idle && idleFor >= threshold ? 'idle' : 'active'
    if (status.idle && user === 'active') this.#timer = setTimeout(() => this.#apply(status, at, threshold, true), threshold - idleFor)
    const screen: ScreenState = status.locked ? 'locked' : 'unlocked'
    const state = states.get(this)!
    const changed = state.user !== user || state.screen !== screen
    state.user = user
    state.screen = screen
    if (changed && notify) fire(this, new Event('change'))
  }
}
getters(IdleDetector.prototype, {
  userState(this: IdleDetector) {
    return states.get(this)?.user ?? null
  },
  screenState(this: IdleDetector) {
    return states.get(this)?.screen ?? null
  },
})
defineHandlers(IdleDetector.prototype, ['change'])
tag(IdleDetector)
