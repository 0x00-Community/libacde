import type { AcdeVoice, BackendConnection, ServiceBackend, ServiceFactory, ServiceInstance, ServiceProcess, SpawnOptions } from '@acde/api'
import { sys } from '../../../page/src/sys/natives'
import { pathToFileURL } from '../../../page/src/sys/url'
import { process, stopGroup } from '../../../page/src/sys/process'
import type { BackendLink, ManagerEvent, ManagerLink, ServiceShape, Welcome } from './manager'

const FN = '__acdeFn'
const STOP_MS = 3000
const HOLD_MS = 1 << 30

export type Hold = () => () => void

export const holdTheLoop: Hold = () => {
  const timer = setInterval(() => {}, HOLD_MS)
  return () => clearInterval(timer)
}
const FUNCTION_KEYS = ['length', 'name', 'prototype', 'caller', 'arguments']

export interface IncomingCall {
  readonly id: number
  readonly service: string
  readonly method: string
  readonly args: readonly unknown[]
}

export function incomingCall(event: ManagerEvent): IncomingCall | null {
  const frame = event as unknown as Partial<IncomingCall> & { type?: string }
  if (typeof frame.id !== 'number' || typeof frame.service !== 'string' || (frame.type !== undefined && frame.type !== 'call')) return null
  return { id: frame.id, service: frame.service, method: String(frame.method ?? ''), args: Array.isArray(frame.args) ? frame.args : [] }
}

export function released(event: ManagerEvent): number | null {
  const release = (event as { release?: { fn?: unknown } }).release
  const fn = event.type === 'release' ? event['fn'] : release?.fn
  return typeof fn === 'number' ? fn : null
}

export function backendUp(event: ManagerEvent): boolean | undefined {
  return event.type === 'backend' ? event['up'] === true : undefined
}

export class BackendState {
  readonly #watchers = new Set<(state: { readonly up: boolean }) => void>()
  readonly #connect: (() => Promise<BackendLink>) | undefined
  readonly #log: (line: string) => void
  #up: boolean
  readonly api: ServiceBackend

  constructor(readonly role: string, up: boolean, connect: (() => Promise<BackendLink>) | undefined, log: (line: string) => void) {
    this.#up = up
    this.#connect = connect
    this.#log = log
    const state = this
    this.api = Object.freeze({
      role,
      get up() {
        return state.#up
      },
      connect: () => state.connect(),
      onChange: (handler: (state: { readonly up: boolean }) => void) => state.onChange(handler),
    })
  }

  get up(): boolean {
    return this.#up
  }

  connect(): Promise<BackendConnection> {
    if (!this.#connect) return Promise.reject(Object.assign(new Error('this acde cannot connect to the desktop backend'), { status: 501, code: 'unsupported' }))
    return this.#connect()
  }

  onChange(handler: (state: { readonly up: boolean }) => void): () => boolean {
    this.#watchers.add(handler)
    return () => this.#watchers.delete(handler)
  }

  take(up: boolean): void {
    if (up === this.#up) return
    this.#up = up
    for (const watcher of [...this.#watchers]) {
      try {
        watcher({ up })
      } catch (error) {
        this.#log(`backend.onChange: ${(error as Error).message}`)
      }
    }
  }
}

function unwatched(): () => boolean {
  let on = true
  return () => {
    const was = on
    on = false
    return was
  }
}

function members(target: unknown): { methods: Set<string>; values: Record<string, unknown> } {
  const methods = new Set<string>()
  const values: Record<string, unknown> = {}
  const read = (key: string): unknown => {
    try {
      return (target as Record<string, unknown>)[key]
    } catch {
      return undefined
    }
  }
  for (let o = target as object | null; o && o !== Object.prototype && o !== Function.prototype; o = Object.getPrototypeOf(o)) {
    const own = Object.getOwnPropertyNames(o).filter((key) => key !== 'constructor' && !methods.has(key) && !(key in values))
    for (const key of own.filter((key) => !(o === target && typeof target === 'function' && FUNCTION_KEYS.includes(key)))) {
      const value = read(key)
      if (typeof value === 'function') methods.add(key)
      else if (value === null || ['string', 'number', 'boolean'].includes(typeof value)) values[key] = value
    }
  }
  return { methods, values }
}

function synced(target: unknown, methods: Set<string>, sync: readonly string[]): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const method of sync.filter((name) => methods.has(name))) {
    try {
      out[method] = (target as Record<string, () => unknown>)[method]!()
    } catch {
      out[method] = null
    }
  }
  return out
}

export function shapeOf(target: unknown, sync: readonly string[]): ServiceShape {
  const { methods, values } = members(target)
  return { methods: [...methods], values, sync: synced(target, methods, sync), callable: typeof target === 'function' }
}

export function withCallbacks(value: unknown, call: (fn: number) => (...args: unknown[]) => void): unknown {
  if (Array.isArray(value)) return value.map((item) => withCallbacks(item, call))
  if (typeof value !== 'object' || value === null || Object.getPrototypeOf(value) !== Object.prototype) return value
  const marked = (value as Record<string, unknown>)[FN]
  if (typeof marked === 'number') return call(marked)
  return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, withCallbacks(item, call)]))
}

function failure(error: unknown): { status: number; code: string; message: string } {
  const e = error as { status?: unknown; code?: unknown; message?: unknown }
  const status = typeof e?.status === 'number' ? e.status : 500
  return { status, code: typeof e?.code === 'string' ? e.code : status === 500 ? 'internal' : 'error', message: String(e?.message ?? error) }
}

export class ServiceHost {
  readonly #processes = new Set<number>()
  readonly #released = new Set<number>()
  readonly backend: BackendState | null
  #instance: ServiceInstance | null = null
  #wake: (() => void) | null = null
  #ended: number | null = null

  constructor(
    readonly manager: ManagerLink,
    readonly welcome: Welcome,
    readonly native: Readonly<Record<string, unknown>>,
    readonly hold: Hold = holdTheLoop,
  ) {
    if (!manager.send) throw new Error('this native.Manager cannot answer as a service (no send)')
    const given = welcome.backend
    this.backend = given ? new BackendState(given.role, given.up === true, manager.connectBackend?.bind(manager), (line) => this.#log(line)) : null
    manager.onEvents(() => this.#wake?.())
  }

  get name(): string {
    return this.welcome.name ?? 'service'
  }

  #log(line: string): void {
    process.stderr.write(`[${this.name}] ${line}\n`)
  }

  #spawn(argv: readonly string[], options: SpawnOptions = {}): ServiceProcess {
    const native = sys()
    if (!native.spawn || !native.write || !native.endInput) throw new Error('spawn: this acde cannot start processes')
    const processes = this.#processes
    const pid = native.spawn(argv, {
      cwd: options.cwd,
      env: { ...options.env },
      prefix: `[${this.name}] `,
      stdin: true,
      onStdout: options.onLine ? (line) => options.onLine!(line) : undefined,
    }, (code, signal) => {
      processes.delete(pid)
      options.onExit?.(code, signal)
    })
    processes.add(pid)
    return { pid, write: (text) => native.write!(pid, text), end: () => native.endInput!(pid), kill: (signal = 'SIGTERM') => native.kill(-pid, signal) }
  }

  #import(specifier: string): Promise<object> {
    const name = String(specifier).replace(/^acde:/, '')
    if (!(this.welcome.modules ?? []).includes(name)) {
      return Promise.reject(new TypeError(`acde:${name} is not among the shell modules of service ${JSON.stringify(this.name)} in the desktop manifest`))
    }
    if (name === 'voice') return this.#voice().then((voice) => ({ voice, default: voice }))
    const value = new Proxy({}, { get: (_, method) => (...args: unknown[]) => this.manager.call(name, String(method), args) })
    return Promise.resolve({ [name]: value, default: value })
  }

  #voiceOnce: Promise<AcdeVoice> | null = null

  #voice(): Promise<AcdeVoice> {
    this.#voiceOnce ??= (async () => {
      const Voice = this.native['Voice'] as (new () => AcdeVoice) | undefined
      if (typeof Voice !== 'function') throw new TypeError('acde:voice: this acde has no speech recognition')
      const voice = new Voice()
      return { listen: (onEvent) => voice.listen(onEvent), stop: (id) => voice.stop(id) }
    })()
    return this.#voiceOnce
  }

  async start(): Promise<void> {
    const { module: path, dir, stateDir } = this.welcome
    if (!path) throw new Error('the session manager sent no service module in its welcome')
    const loaded = (await import(pathToFileURL(path).href)) as { default?: ServiceFactory }
    if (typeof loaded.default !== 'function') throw new Error(`${path} has no default export function`)
    const context = {
      name: this.name,
      package: dir,
      state: stateDir ?? '',
      env: process.env,
      log: (line: string) => this.#log(line),
      spawn: (argv: readonly string[], options?: SpawnOptions) => this.#spawn(argv, options),
      import: ((specifier: string) => this.#import(specifier)) as never,
      ...(this.backend ? { backend: this.backend.api } : {}),
      backendToken: () => null,
      onBackendToken: unwatched,
    }
    this.#instance = await loaded.default(context as never)
    this.ready()
  }

  ready(): void {
    const instance = this.#instance
    if (!instance) return
    const sync = (instance.sync ?? []).map((name) => name.split('.') as [string, string])
    const provides = Object.fromEntries(
      Object.entries(instance.api ?? {}).map(([ns, target]) => [ns, shapeOf(target, sync.filter(([owner]) => owner === ns).map(([, method]) => method))]),
    )
    this.manager.send!({ ready: { provides } })
  }

  async #answer(call: IncomingCall): Promise<void> {
    try {
      const target = this.#instance?.api?.[call.service] as Record<string, (...a: unknown[]) => unknown> | ((...a: unknown[]) => unknown) | undefined
      if (!target) throw Object.assign(new Error(`acde.${call.service} is not provided by ${this.name}`), { status: 404, code: 'not_found' })
      const args = withCallbacks(call.args, (fn) => (...given: unknown[]) => {
        if (!this.#released.has(fn)) this.manager.send!({ invoke: { fn, args: given } })
      }) as unknown[]
      let value: unknown
      if (call.method === '' && typeof target === 'function') value = await target(...args)
      else {
        const fn = (target as Record<string, (...a: unknown[]) => unknown>)[call.method]
        if (typeof fn !== 'function') throw Object.assign(new Error(`acde.${call.service} has no ${call.method}`), { status: 404, code: 'not_found' })
        value = await fn.apply(target, args)
      }
      this.manager.send!({ id: call.id, ok: value === undefined ? null : value })
    } catch (error) {
      this.manager.send!({ id: call.id, error: failure(error) })
    }
  }

  async stop(): Promise<void> {
    try {
      await this.#instance?.close?.()
    } catch (error) {
      this.#log(`close: ${(error as Error).message}`)
    }
    for (const pid of [...this.#processes]) stopGroup(pid, () => this.#processes.has(pid), STOP_MS)
  }

  async run(): Promise<number> {
    const release = this.hold()
    try {
      return await this.#serveUntilClosed()
    } finally {
      release()
    }
  }

  async #serveUntilClosed(): Promise<number> {
    while (this.#ended === null) {
      for (const event of this.manager.pump()) {
        if (event.type === 'managerclosed') this.#ended = 0
        const call = incomingCall(event)
        if (call) void this.#answer(call)
        const fn = released(event)
        if (fn !== null) this.#released.add(fn)
        const up = backendUp(event)
        if (up !== undefined) this.backend?.take(up)
      }
      if (this.#ended !== null) break
      await new Promise<void>((wake) => {
        this.#wake = wake
      })
    }
    await this.stop()
    return this.#ended
  }
}
