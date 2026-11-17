import { Level, Log } from './log'

type RpcMessage =
  | { rpc: 'call'; id: number; service: string; method: string; args: unknown[]; refs?: true }
  | { rpc: 'result'; id: number; value?: unknown; error?: RemoteError; refs?: true }
  | { rpc: 'invoke'; fn: number; args: unknown[]; refs?: true }
  | { rpc: 'release'; fn: number }

interface RemoteError {
  readonly name: string
  readonly message: string
  readonly fields: Record<string, unknown>
}

const FN = '__acdeFn'
const PERF = 'perf::rpc'

type RpcServer = (service: string, method: string, args: unknown[]) => unknown

export class Rpc {
  readonly #post: (message: RpcMessage) => void
  #next = 1
  readonly #pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void; timed?: [string, number] }>()
  readonly #functions = new Map<number, (...args: unknown[]) => unknown>()
  readonly #held = new FinalizationRegistry<number>((fn) => this.#post({ rpc: 'release', fn }))
  #server: RpcServer | null = null

  constructor(post: (message: unknown) => void) {
    this.#post = post
  }

  receive(message: unknown): boolean {
    if (typeof message !== 'object' || message === null || !('rpc' in message)) return false
    void this.#receive(message as RpcMessage)
    return true
  }

  static calls<const N extends string>(rpc: () => Rpc, service: string, ...names: N[]): Record<N, (...args: unknown[]) => Promise<unknown>> {
    return Object.fromEntries(names.map((name) => [name, (...args: unknown[]) => rpc().call(service, name, args)])) as Record<N, (...args: unknown[]) => Promise<unknown>>
  }

  serve(server: RpcServer): void {
    this.#server = server
  }

  call(service: string, method: string, args: readonly unknown[] = []): Promise<unknown> {
    const id = this.#next++
    const timed: [string, number] | undefined = Log.enabled(PERF) ? [`${service}.${method}`, Log.clock()] : undefined
    return new Promise((resolve, reject) => {
      this.#pending.set(id, { resolve, reject, timed })
      const out = this.#out(args) as unknown[]
      this.#post({ rpc: 'call', id, service, method, args: out, ...(out !== args ? { refs: true as const } : {}) })
    })
  }

  fail(reason: string): void {
    for (const { reject } of this.#pending.values()) reject(new Error(reason))
    this.#pending.clear()
    this.#functions.clear()
  }

  async #receive(message: RpcMessage): Promise<void> {
    if (message.rpc === 'release') {
      this.#functions.delete(message.fn)
      return
    }
    const incoming = <T>(value: T): T => (message.refs ? (this.#in(value) as T) : value)
    if (message.rpc === 'result') {
      const waiting = this.#pending.get(message.id)
      if (!waiting) return
      this.#pending.delete(message.id)
      if (waiting.timed) Log.write(Level.Debug, PERF, `call ${waiting.timed[0]} error=${!!message.error} ms=${(Log.clock() - waiting.timed[1]).toFixed(3)}`)
      if (message.error) waiting.reject(Rpc.#error(message.error))
      else waiting.resolve(incoming(message.value))
      return
    }
    if (message.rpc === 'invoke') {
      try {
        await this.#functions.get(message.fn)?.(...incoming(message.args))
      } catch (error) {
        Log.write(Level.Warn, 'acde_host', `callback failed: ${(error as Error).message}`)
      }
      return
    }
    const started = Log.enabled(PERF) ? Log.clock() : -1
    try {
      if (!this.#server) throw new Error('acde: nothing serves calls here')
      const value = await this.#server(message.service, message.method, incoming(message.args))
      if (started >= 0) Log.write(Level.Debug, PERF, `serve ${message.service}.${message.method} ms=${(Log.clock() - started).toFixed(3)}`)
      const out = this.#out(value)
      this.#post({ rpc: 'result', id: message.id, value: out, ...(out !== value ? { refs: true as const } : {}) })
    } catch (error) {
      this.#post({ rpc: 'result', id: message.id, error: Rpc.#remote(error) })
    }
  }

  #out(value: unknown): unknown {
    if (typeof value === 'function') {
      const fn = this.#next++
      this.#functions.set(fn, value as (...args: unknown[]) => unknown)
      return { [FN]: fn }
    }
    return Rpc.#map(value, (item) => this.#out(item))
  }

  #in(value: unknown): unknown {
    if (Rpc.#plain(value) && typeof value[FN] === 'number') {
      const fn = value[FN]
      const call = (...args: unknown[]) => {
        const out = this.#out(args) as unknown[]
        this.#post({ rpc: 'invoke', fn, args: out, ...(out !== args ? { refs: true as const } : {}) })
      }
      this.#held.register(call, fn)
      return call
    }
    return Rpc.#map(value, (item) => this.#in(item))
  }

  static #map(value: unknown, convert: (item: unknown) => unknown): unknown {
    if (Array.isArray(value)) {
      let copy: unknown[] | null = null
      for (let i = 0; i < value.length; i++) {
        const item: unknown = value[i]
        const converted = convert(item)
        if (copy) copy.push(converted)
        else if (!Object.is(converted, item)) copy = [...value.slice(0, i), converted]
      }
      return copy ?? value
    }
    if (!Rpc.#plain(value)) return value
    let copy: Record<string, unknown> | null = null
    for (const key of Object.keys(value)) {
      const item = value[key]
      const converted = convert(item)
      if (!copy && Object.is(converted, item)) continue
      copy ??= { ...value }
      Object.defineProperty(copy, key, { value: converted, writable: true, enumerable: true, configurable: true })
    }
    return copy ?? value
  }

  static #plain(value: unknown): value is Record<string, unknown> {
    if (typeof value !== 'object' || value === null) return false
    const proto = Object.getPrototypeOf(value)
    return proto === Object.prototype || proto === null
  }

  static #remote(error: unknown): RemoteError {
    if (!(error instanceof Error)) return { name: 'Error', message: String(error), fields: {} }
    const fields: Record<string, unknown> = {}
    for (const [key, value] of Object.entries(error)) if (value === null || ['string', 'number', 'boolean'].includes(typeof value)) fields[key] = value
    return { name: error.name, message: error.message, fields }
  }

  static #error(remote: RemoteError): Error {
    const error = new Error(remote.message)
    error.name = remote.name
    return Object.assign(error, remote.fields)
  }
}
