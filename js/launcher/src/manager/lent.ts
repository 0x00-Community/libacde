import { CALLBACK, withCallbacks, type ManagerEvent } from './manager'

export class LentCallbacks {
  readonly #send: (frame: object) => void
  readonly #pageOf = new Map<number, number>()
  readonly #wideOf = new Map<number, number>()
  #next = 0

  constructor(send: (frame: object) => void) {
    this.#send = send
  }

  lend(args: readonly unknown[]): unknown[] {
    return withCallbacks(args, (fn) => ({ [CALLBACK]: this.#wide(fn) })) as unknown[]
  }

  invoke(event: ManagerEvent): { rpc: 'invoke'; fn: number; args: unknown } | null {
    const fn = typeof event['fn'] === 'number' ? this.#pageOf.get(event['fn']) : undefined
    return fn === undefined ? null : { rpc: 'invoke', fn, args: event['args'] ?? [] }
  }

  releaseAll(): void {
    for (const wide of this.#pageOf.keys()) this.#send({ release: { fn: wide } })
    this.#pageOf.clear()
    this.#wideOf.clear()
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
