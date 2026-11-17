import { Level, Log } from './log'

type Handler = (state: unknown) => void

export interface WatchNatives {
  watch(callback: Handler): void
}

export class Watched {
  readonly #name: string
  readonly #watch: WatchNatives['watch']
  readonly #handlers: Map<string, Set<Handler>>
  readonly #typed: boolean
  #watching = false

  private constructor(name: string, watch: WatchNatives['watch'], events: readonly string[], typed: boolean) {
    this.#name = name
    this.#watch = watch
    this.#handlers = new Map(events.map((type) => [type, new Set<Handler>()]))
    this.#typed = typed
  }

  static module(name: string, natives: WatchNatives): object {
    return Watched.#wrap(name, natives, ['changed'], false)
  }

  static typed(name: string, natives: WatchNatives, events: readonly string[]): object {
    return Watched.#wrap(name, natives, events, true)
  }

  static #wrap(name: string, natives: WatchNatives, events: readonly string[], typed: boolean): object {
    const { watch, ...methods } = natives as WatchNatives & Record<string, unknown>
    const watched = new Watched(name, watch, events, typed)
    return { ...methods, on: (type: string, handler: unknown) => watched.on(type, handler) }
  }

  #deliver(type: string, value: unknown): void {
    for (const each of [...(this.#handlers.get(type) ?? [])]) {
      try {
        each(value)
      } catch (error) {
        Log.write(Level.Warn, 'acde_host', `acde:${this.#name} ${type}: ${(error as Error)?.message ?? String(error)}`)
      }
    }
  }

  #event(event: unknown): void {
    if (!this.#typed) return this.#deliver('changed', event)
    const { type, state } = event as { type: string; state?: unknown }
    this.#deliver(type, type === 'changed' ? state : event)
  }

  on(type: string, handler: unknown): () => boolean {
    const set = this.#handlers.get(type)
    if (!set) throw new TypeError(`acde:${this.#name} has no ${String(type)} event`)
    if (typeof handler !== 'function') throw new TypeError(`acde:${this.#name} on: the handler is not a function`)
    const fn = handler as Handler
    set.add(fn)
    if (!this.#watching) {
      this.#watching = true
      this.#watch((event) => this.#event(event))
    }
    return () => set.delete(fn)
  }
}
