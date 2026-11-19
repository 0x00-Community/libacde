export type SystemErrorCode = 'bad_request' | 'forbidden' | 'unsupported' | 'service_not_running' | 'failed'

const STATUS: Readonly<Record<SystemErrorCode, number>> = {
  bad_request: 400,
  forbidden: 403,
  unsupported: 501,
  service_not_running: 503,
  failed: 500,
}

const CODES = new Set(Object.keys(STATUS))

const FORBIDDEN = /not given to|not in the capabilities/i

const PATTERNS: readonly [RegExp, SystemErrorCode][] = [
  [/no compositor|not supported|no xkeyboard-config|no input method framework|no .* here\b/i, 'unsupported'],
  [/ServiceUnknown|NameHasNoOwner|is not running|isn't running|system bus|session bus|not available/i, 'service_not_running'],
]

const NOT_A_REASON = new Set(['GenericFailure'])

type Callable = (...args: unknown[]) => unknown

export const SYSTEM_ERROR = 'AcdeError'

interface Unpacked {
  readonly message: string
  readonly code: string | null
}

function unpack(error: unknown): Unpacked {
  const message = error instanceof Error ? error.message : String(error)
  const given = (error as { code?: unknown } | null)?.code
  const own = typeof given === 'string' ? given : null
  if (!message.startsWith('{')) return { message, code: own }
  try {
    const inner = JSON.parse(message) as { code?: unknown; message?: unknown }
    if (typeof inner.code === 'string' && typeof inner.message === 'string') return { message: inner.message, code: inner.code }
  } catch {
    return { message, code: own }
  }
  return { message, code: own }
}

export function systemErrorCode(error: unknown): SystemErrorCode {
  const { message, code } = unpack(error)
  if (code !== null && CODES.has(code)) return code as SystemErrorCode
  if (FORBIDDEN.test(message)) return 'forbidden'
  const matched = PATTERNS.find(([pattern]) => pattern.test(message))?.[1]
  if (matched) return matched
  if (error instanceof TypeError || (error as { name?: unknown } | null)?.name === 'TypeError') return 'bad_request'
  return 'failed'
}

export function systemError(module: string, error: unknown): Error {
  if ((error as { name?: unknown } | null)?.name === SYSTEM_ERROR) return error as Error
  const code = systemErrorCode(error)
  const unpacked = unpack(error)
  const reason = unpacked.code !== null && !CODES.has(unpacked.code) && !NOT_A_REASON.has(unpacked.code) ? { reason: unpacked.code } : {}
  const status = (error as { status?: unknown } | null)?.status
  const made = new Error(unpacked.message, { cause: error })
  made.name = SYSTEM_ERROR
  return Object.assign(made, { status: typeof status === 'number' ? status : STATUS[code], code, module, ...reason })
}

export function checkedModule<T extends object>(module: string, value: T): T {
  const wrapped: Record<string, unknown> = {}
  for (const [key, member] of Object.entries(value)) {
    if (typeof member !== 'function') {
      wrapped[key] = member
      continue
    }
    wrapped[key] = (...args: unknown[]) => {
      let result: unknown
      try {
        result = (member as Callable).apply(value, args)
      } catch (error) {
        throw systemError(module, error)
      }
      if (result instanceof Promise) return result.catch((error: unknown) => Promise.reject(systemError(module, error)))
      return result
    }
  }
  return wrapped as T
}

type Handler = (change: unknown) => void

export interface SystemSource {
  storage(): Promise<unknown>
  info(): Promise<unknown>
  time(): Promise<unknown>
}

export interface SystemSnapshot {
  readonly storage: unknown
  readonly hostname: string
  readonly timezone: string
  readonly ntp: boolean
}

export class SystemWatch {
  static readonly EVERY_MS = 30_000
  readonly #source: SystemSource
  readonly #every: number
  readonly #handlers = new Set<Handler>()
  #timer: ReturnType<typeof setInterval> | null = null
  #last = ''

  constructor(source: SystemSource, every = SystemWatch.EVERY_MS) {
    this.#source = source
    this.#every = every
  }

  on(type: string, handler: unknown): () => boolean {
    if (type !== 'changed') throw new TypeError(`acde:system has no ${String(type)} event (changed)`)
    if (typeof handler !== 'function') throw new TypeError('acde:system on: the handler is not a function')
    const fn = handler as Handler
    this.#handlers.add(fn)
    if (!this.#timer) this.#start()
    return () => {
      const removed = this.#handlers.delete(fn)
      if (!this.#handlers.size) this.#stop()
      return removed
    }
  }

  async after<T>(change: Promise<T>): Promise<T> {
    const done = await change
    if (this.#handlers.size) await this.check(false)
    return done
  }

  #start(): void {
    void this.check(true)
    this.#timer = setInterval(() => void this.check(false), this.#every)
  }

  #stop(): void {
    if (this.#timer) clearInterval(this.#timer)
    this.#timer = null
    this.#last = ''
  }

  async #read(): Promise<SystemSnapshot> {
    const [storage, info, time] = await Promise.all([this.#source.storage(), this.#source.info(), this.#source.time()])
    const { hostname } = (info ?? {}) as { hostname?: unknown }
    const { timezone, ntp } = (time ?? {}) as { timezone?: unknown; ntp?: unknown }
    return { storage, hostname: String(hostname ?? ''), timezone: String(timezone ?? ''), ntp: ntp === true }
  }

  async check(first: boolean): Promise<void> {
    let snapshot: SystemSnapshot
    try {
      snapshot = await this.#read()
    } catch {
      return
    }
    const seen = JSON.stringify(snapshot)
    if (seen === this.#last) return
    this.#last = seen
    if (first) return
    for (const handler of [...this.#handlers]) handler(snapshot)
  }
}
