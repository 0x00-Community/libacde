export type SystemErrorCode = 'bad_request' | 'forbidden' | 'unsupported' | 'service_not_running' | 'failed'

const STATUS: Readonly<Record<SystemErrorCode, number>> = {
  bad_request: 400,
  forbidden: 403,
  unsupported: 501,
  service_not_running: 503,
  failed: 500,
}

const CODES = new Set(Object.keys(STATUS))

const PATTERNS: readonly [RegExp, SystemErrorCode][] = [
  [/not given to this web|not in the capabilities/i, 'forbidden'],
  [/no compositor|not supported|no xkeyboard-config|no input method framework|no .* here\b/i, 'unsupported'],
  [/ServiceUnknown|NameHasNoOwner|is not running|isn't running|system bus|session bus|not available/i, 'service_not_running'],
]

type Callable = (...args: unknown[]) => unknown

export const SYSTEM_ERROR = 'AcdeError'

export function systemErrorCode(error: unknown): SystemErrorCode {
  const given = (error as { code?: unknown } | null)?.code
  if (typeof given === 'string' && CODES.has(given)) return given as SystemErrorCode
  if (error instanceof TypeError || (error as { name?: unknown } | null)?.name === 'TypeError') return 'bad_request'
  const message = error instanceof Error ? error.message : String(error)
  return PATTERNS.find(([pattern]) => pattern.test(message))?.[1] ?? 'failed'
}

export function systemError(module: string, error: unknown): Error {
  if ((error as { name?: unknown } | null)?.name === SYSTEM_ERROR) return error as Error
  const code = systemErrorCode(error)
  const message = error instanceof Error ? error.message : String(error)
  const status = (error as { status?: unknown } | null)?.status
  const made = new Error(message, { cause: error })
  made.name = SYSTEM_ERROR
  return Object.assign(made, { status: typeof status === 'number' ? status : STATUS[code], code, module })
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

export class StorageWatch {
  static readonly EVERY_MS = 30_000
  readonly #read: () => Promise<unknown>
  readonly #every: number
  readonly #handlers = new Set<Handler>()
  #timer: ReturnType<typeof setInterval> | null = null
  #last = ''

  constructor(read: () => Promise<unknown>, every = StorageWatch.EVERY_MS) {
    this.#read = read
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

  #start(): void {
    void this.check(true)
    this.#timer = setInterval(() => void this.check(false), this.#every)
  }

  #stop(): void {
    if (this.#timer) clearInterval(this.#timer)
    this.#timer = null
    this.#last = ''
  }

  async check(first: boolean): Promise<void> {
    let storage: unknown
    try {
      storage = await this.#read()
    } catch {
      return
    }
    const seen = JSON.stringify(storage)
    if (seen === this.#last) return
    this.#last = seen
    if (first) return
    for (const handler of [...this.#handlers]) handler({ storage })
  }
}
