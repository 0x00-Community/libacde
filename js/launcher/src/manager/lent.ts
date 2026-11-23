import { Level, Log } from '../../../page/src/log'
import { CALLBACK, withCallbacks, type ManagerEvent } from './manager'

export const CALLS = 'acde_host::calls'
export const RELEASED_KEPT = 1024

export type Violation = 'stale-load' | 'unknown-callback'

export interface LentOptions {
  readonly send: (frame: object) => void
  readonly report?: ((violation: Violation, text: string) => void) | null
}

export class LentCallbacks {
  readonly #send: (frame: object) => void
  readonly #report: ((violation: Violation, text: string) => void) | null
  readonly #pageOf = new Map<number, number>()
  readonly #wideOf = new Map<number, number>()
  readonly #released = new Map<number, number>()
  #next = 0
  #load = 1

  constructor({ send, report = null }: LentOptions) {
    this.#send = send
    this.#report = report
  }

  get load(): number {
    return this.#load
  }

  async call(service: string, method: string, args: readonly unknown[], send: (args: unknown[]) => Promise<unknown>): Promise<unknown> {
    const lent: number[] = []
    const out = withCallbacks(args, (fn) => {
      const wide = this.#wide(fn)
      lent.push(wide)
      return { [CALLBACK]: wide }
    }) as unknown[]
    if (!Log.enabled(CALLS)) return await send(out)
    const started = Log.clock()
    const note = (ok: boolean) => {
      const ms = (Log.clock() - started).toFixed(3)
      Log.write(Level.Debug, CALLS, `event=call load=${this.#load} service=${service} method=${method} lent=[${lent.join(',')}] ok=${ok} ms=${ms}`)
    }
    try {
      const value = await send(out)
      note(true)
      return value
    } catch (error) {
      note(false)
      throw error
    }
  }

  invoke(event: ManagerEvent): { rpc: 'invoke'; fn: number; args: unknown } | null {
    const wide = event['fn']
    const fn = typeof wide === 'number' ? this.#pageOf.get(wide) : undefined
    if (fn === undefined) {
      this.#stale(wide)
      return null
    }
    if (Log.enabled(CALLS)) Log.write(Level.Debug, CALLS, `event=invoke load=${this.#load} fn=${String(wide)} page=${fn}`)
    return { rpc: 'invoke', fn, args: event['args'] ?? [] }
  }

  releaseAll(): void {
    for (const wide of this.#pageOf.keys()) {
      this.#send({ release: { fn: wide } })
      this.#released.set(wide, this.#load)
    }
    for (const old of [...this.#released.keys()].slice(0, Math.max(0, this.#released.size - RELEASED_KEPT))) this.#released.delete(old)
    this.#pageOf.clear()
    this.#wideOf.clear()
    this.#load++
  }

  #stale(wide: unknown): void {
    const load = typeof wide === 'number' ? this.#released.get(wide) : undefined
    const violation: Violation = load === undefined ? 'unknown-callback' : 'stale-load'
    const what = `acde: a service called back callback ${String(wide)}`
    const text = load === undefined
      ? `${what}, which this page never lent or released long ago (${violation})`
      : `${what} of load ${load} after the page reloaded and released it; this is load ${this.#load} (${violation})`
    const line = `event=stale-invoke code=${violation} fn=${String(wide)} lent-by=${load ?? 'none'} load=${this.#load}`
    Log.write(this.#report ? Level.Warn : Level.Debug, CALLS, line)
    this.#report?.(violation, text)
  }

  #wide(fn: number): number {
    const known = this.#wideOf.get(fn)
    if (known !== undefined) return known
    const wide = ++this.#next
    this.#wideOf.set(fn, wide)
    this.#pageOf.set(wide, fn)
    return wide
  }
}
