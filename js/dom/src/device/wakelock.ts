import { defineHandlers, Event, EventTarget, fire } from '../core/event'
import { DOMException } from '../core/exception'
import { getters, KEY, tag } from '../core/host'

export interface WakeLockNative {
  wakeLock?(why: string): Promise<number>
  releaseWakeLock?(id: number): void
}

const sentinels = new WeakMap<WakeLockSentinel, { id: number; released: boolean }>()

export class WakeLockSentinel extends EventTarget {
  readonly #native: WakeLockNative
  readonly #held: Set<WakeLockSentinel>

  constructor(key: symbol, native: WakeLockNative, held: Set<WakeLockSentinel>, id: number) {
    if (key !== KEY) throw new TypeError('Illegal constructor')
    super()
    this.#native = native
    this.#held = held
    sentinels.set(this, { id, released: false })
    held.add(this)
  }

  release(): Promise<void> {
    const state = sentinels.get(this)
    if (state && !state.released) {
      state.released = true
      this.#held.delete(this)
      this.#native.releaseWakeLock?.(state.id)
      fire(this, new Event('release'))
    }
    return Promise.resolve()
  }
}
getters(WakeLockSentinel.prototype, {
  released(this: WakeLockSentinel) {
    return sentinels.get(this)?.released ?? true
  },
  type: 'screen',
})
defineHandlers(WakeLockSentinel.prototype, ['release'])
tag(WakeLockSentinel)

type Visible = { readonly visibilityState: string; addEventListener(type: string, listener: () => void): void }

export class WakeLock {
  readonly #native: WakeLockNative
  readonly #document: () => Visible | undefined
  readonly #held = new Set<WakeLockSentinel>()
  #watching = false

  constructor(key: symbol, native: WakeLockNative, document: () => Visible | undefined) {
    if (key !== KEY) throw new TypeError('Illegal constructor')
    this.#native = native
    this.#document = document
  }

  request(type: string = 'screen'): Promise<WakeLockSentinel> {
    if (String(type) !== 'screen') {
      return Promise.reject(new TypeError(`Failed to execute 'request' on 'WakeLock': The provided value '${String(type)}' is not a valid enum value of type WakeLockType.`))
    }
    const document = this.#document()
    if (document?.visibilityState !== 'visible') return Promise.reject(new DOMException('The requesting page is not visible', 'NotAllowedError'))
    if (!this.#watching) {
      this.#watching = true
      document.addEventListener('visibilitychange', () => {
        if (this.#document()?.visibilityState !== 'visible') for (const sentinel of [...this.#held]) void sentinel.release()
      })
    }
    const acquire = this.#native.wakeLock
    if (!acquire) return Promise.reject(new DOMException('Wake Lock is not available here', 'NotAllowedError'))
    return acquire.call(this.#native, 'a page keeps the screen on').then(
      (id) => new WakeLockSentinel(KEY, this.#native, this.#held, id),
      (error: unknown) => {
        throw new DOMException((error as Error)?.message ?? String(error), 'NotAllowedError')
      },
    )
  }
}
tag(WakeLock)
