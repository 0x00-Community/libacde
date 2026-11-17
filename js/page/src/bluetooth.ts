import { Level, Log } from './log'

type Handler = (value: unknown) => void

export interface BluetoothNatives {
  watch(callback: Handler): void
  pairing(callback: Handler | null): void
}

export class BluetoothModule {
  readonly #natives: BluetoothNatives
  readonly #changed = new Set<Handler>()
  readonly #asks = new Set<Handler>()
  #watching = false

  private constructor(natives: BluetoothNatives) {
    this.#natives = natives
  }

  static wrap(natives: BluetoothNatives): object {
    const { watch: _watch, pairing: _pairing, ...methods } = natives as BluetoothNatives & Record<string, unknown>
    const module = new BluetoothModule(natives)
    return { ...methods, on: (type: string, handler: unknown) => module.on(type, handler) }
  }

  static #each(type: string, handlers: Set<Handler>, value: unknown): void {
    for (const handler of [...handlers]) {
      try {
        handler(value)
      } catch (error) {
        Log.write(Level.Warn, 'acde_host', `acde:bluetooth ${type} handler: ${(error as Error)?.message ?? String(error)}`)
      }
    }
  }

  on(type: string, handler: unknown): () => boolean {
    if (type !== 'changed' && type !== 'pairing') throw new TypeError(`acde:bluetooth has no ${String(type)} event`)
    if (typeof handler !== 'function') throw new TypeError('acde:bluetooth on: the handler is not a function')
    const fn = handler as Handler
    if (type === 'changed') {
      if (!this.#watching) {
        this.#watching = true
        this.#natives.watch((state) => BluetoothModule.#each(type, this.#changed, state))
      }
      this.#changed.add(fn)
      return () => this.#changed.delete(fn)
    }
    if (!this.#asks.size) this.#natives.pairing((request) => BluetoothModule.#each(type, this.#asks, request))
    this.#asks.add(fn)
    return () => {
      const had = this.#asks.delete(fn)
      if (had && !this.#asks.size) this.#natives.pairing(null)
      return had
    }
  }
}
