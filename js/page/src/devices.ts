import { Level, Log } from './log'

type Handler = (value: unknown) => void

export interface DevicesNatives {
  watch(type: string, callback: Handler): number
  unwatch(id: number): void
}

export class DevicesModule {
  static readonly #EVENTS = ['changed', 'added'] as const
  readonly #natives: DevicesNatives
  readonly #handlers = new Map<string, Set<Handler>>(DevicesModule.#EVENTS.map((type) => [type, new Set<Handler>()]))
  readonly #watching = new Map<string, number>()

  private constructor(natives: DevicesNatives) {
    this.#natives = natives
  }

  static wrap(natives: DevicesNatives): object {
    const { watch: _watch, unwatch: _unwatch, ...methods } = natives as DevicesNatives & Record<string, unknown>
    const module = new DevicesModule(natives)
    return { ...methods, on: (type: string, handler: unknown) => module.on(type, handler) }
  }

  on(type: string, handler: unknown): () => boolean {
    const set = this.#handlers.get(type)
    if (!set) throw new TypeError(`acde:devices has no ${String(type)} event`)
    if (typeof handler !== 'function') throw new TypeError('acde:devices on: the handler is not a function')
    const fn = handler as Handler
    set.add(fn)
    if (!this.#watching.has(type)) {
      const deliver = (value: unknown) => {
        if (value == null) return
        for (const each of [...set]) {
          try {
            each(value)
          } catch (error) {
            Log.write(Level.Warn, 'acde_host', `acde:devices ${type}: ${(error as Error)?.message ?? String(error)}`)
          }
        }
      }
      this.#watching.set(type, this.#natives.watch(type, deliver))
    }
    return () => {
      const had = set.delete(fn)
      const id = this.#watching.get(type)
      if (!set.size && id !== undefined) {
        this.#watching.delete(type)
        this.#natives.unwatch(id)
      }
      return had
    }
  }
}
