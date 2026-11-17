import { DOMException } from '../core/exception'
import { illegal, KEY, tag } from '../core/host'
import { fetch, Request, Response } from '../core/net'
import type { CacheEntryNative, CacheNative, CacheQueryNative } from '../native'

interface QueryOptions {
  ignoreSearch?: unknown
  ignoreMethod?: unknown
  ignoreVary?: unknown
  cacheName?: unknown
}

interface ResponseMeta {
  status: number
  statusText: string
  type: string
  url: string
  redirected: boolean
}

function unknown(error: unknown): DOMException {
  const message = String((error as Error)?.message ?? error)
  return new DOMException(message, 'UnknownError')
}

function requestOf(input: unknown): Request {
  if (input instanceof Request) return input
  return new Request(`${input as string}`, undefined)
}

function query(request: Request, options: QueryOptions | undefined): CacheQueryNative {
  return {
    url: request.url,
    method: request.method,
    headers: [...request.headers],
    ignoreSearch: !!options?.ignoreSearch,
    ignoreMethod: !!options?.ignoreMethod,
    ignoreVary: !!options?.ignoreVary,
  }
}

function varyStar(response: Response): boolean {
  return (response.headers.get('vary') ?? '').split(',').some((field) => field.trim() === '*')
}

function httpScheme(url: string): boolean {
  return url.startsWith('http:') || url.startsWith('https:')
}

function responseOf(entry: CacheEntryNative): Response {
  const meta = JSON.parse(entry.response) as ResponseMeta
  const nullBody = meta.type === 'opaque' || [101, 103, 204, 205, 304].includes(meta.status)
  const status = meta.status >= 200 && meta.status <= 599 ? meta.status : 200
  const response = new Response(nullBody ? null : entry.body, { status, statusText: meta.statusText, headers: entry.responseHeaders })
  if (status !== meta.status) Object.defineProperty(response, 'status', { value: meta.status })
  response.url = meta.url
  Object.defineProperty(response, 'type', { value: meta.type })
  if (meta.redirected) Object.defineProperty(response, 'redirected', { value: true })
  return response
}

function requestFromEntry(entry: CacheEntryNative): Request {
  return new Request(entry.url, { method: entry.method, headers: entry.requestHeaders })
}

async function entryOf(request: Request, response: Response): Promise<CacheEntryNative> {
  const meta: ResponseMeta = { status: response.status, statusText: response.statusText, type: response.type, url: response.url, redirected: response.redirected }
  const body = new Uint8Array(await response.arrayBuffer())
  return { url: request.url, method: request.method, requestHeaders: [...request.headers], response: JSON.stringify(meta), responseHeaders: [...response.headers], body }
}

export class Cache {
  readonly #native: CacheNative
  readonly #id: number

  constructor(...key: unknown[]) {
    illegal(key[0])
    this.#native = key[1] as CacheNative
    this.#id = key[2] as number
  }

  async #query(request: unknown, options: QueryOptions | undefined, first: boolean): Promise<CacheEntryNative[]> {
    let wanted: CacheQueryNative | null = null
    if (request !== undefined) {
      const r = requestOf(request)
      if (r.method !== 'GET' && !options?.ignoreMethod) return []
      wanted = query(r, options)
    }
    try {
      return await this.#native.query(this.#id, wanted, first)
    } catch (error) {
      throw unknown(error)
    }
  }

  async match(request: unknown, options?: QueryOptions): Promise<Response | undefined> {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'match' on 'Cache': 1 argument required, but only 0 present.")
    const [entry] = await this.#query(request, options, true)
    return entry ? responseOf(entry) : undefined
  }

  async matchAll(request?: unknown, options?: QueryOptions): Promise<readonly Response[]> {
    return Object.freeze((await this.#query(request, options, false)).map(responseOf))
  }

  async keys(request?: unknown, options?: QueryOptions): Promise<readonly Request[]> {
    return Object.freeze((await this.#query(request, options, false)).map(requestFromEntry))
  }

  async add(request: unknown): Promise<void> {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'add' on 'Cache': 1 argument required, but only 0 present.")
    return this.addAll([request])
  }

  async addAll(requests: unknown): Promise<void> {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'addAll' on 'Cache': 1 argument required, but only 0 present.")
    const list = Array.from(requests as Iterable<unknown>, (r) => requestOf(r))
    for (const r of list) {
      if (!httpScheme(r.url) || r.method !== 'GET') throw new TypeError("Failed to execute 'addAll' on 'Cache': Add/AddAll only supports the GET request method and http or https URLs.")
    }
    const responses = await Promise.all(list.map((r) => fetch(r)))
    for (const response of responses) {
      if (response.type === 'error' || !response.ok || response.status === 206) throw new TypeError("Failed to execute 'addAll' on 'Cache': Request failed")
      if (varyStar(response)) throw new TypeError("Failed to execute 'addAll' on 'Cache': Vary header contains *")
    }
    const entries = await Promise.all(list.map((r, i) => entryOf(r, responses[i]!)))
    for (let i = 0; i < entries.length; i++) {
      for (let j = 0; j < i; j++) {
        if (sameRequest(entries[i]!, entries[j]!)) throw new DOMException("Failed to execute 'addAll' on 'Cache': duplicate requests", 'InvalidStateError')
      }
    }
    await this.#batch(entries.map((entry) => ({ type: 'put' as const, entry })))
  }

  async put(request: unknown, response: unknown): Promise<void> {
    if (arguments.length < 2) throw new TypeError(`Failed to execute 'put' on 'Cache': 2 arguments required, but only ${arguments.length} present.`)
    const r = requestOf(request)
    if (r.method !== 'GET') throw new TypeError("Failed to execute 'put' on 'Cache': Request method '" + r.method + "' is unsupported")
    if (!httpScheme(r.url)) throw new TypeError("Failed to execute 'put' on 'Cache': Request scheme '" + r.url.split(':')[0] + "' is unsupported")
    if (!(response instanceof Response)) throw new TypeError("Failed to execute 'put' on 'Cache': parameter 2 is not of type 'Response'.")
    if (response.status === 206) throw new TypeError("Failed to execute 'put' on 'Cache': Partial response (status code 206) is unsupported")
    if (varyStar(response)) throw new TypeError("Failed to execute 'put' on 'Cache': Vary header contains *")
    if (response.bodyUsed || response.body?.locked) throw new TypeError("Failed to execute 'put' on 'Cache': Response body is already used")
    await this.#batch([{ type: 'put', entry: await entryOf(r, response) }])
  }

  async delete(request: unknown, options?: QueryOptions): Promise<boolean> {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'delete' on 'Cache': 1 argument required, but only 0 present.")
    const r = requestOf(request)
    if (r.method !== 'GET' && !options?.ignoreMethod) return false
    const [deleted] = await this.#batch([{ type: 'delete', query: query(r, options) }])
    return !!deleted
  }

  async #batch(operations: ({ type: 'put'; entry: CacheEntryNative } | { type: 'delete'; query: CacheQueryNative })[]): Promise<boolean[]> {
    try {
      return await this.#native.batch(this.#id, operations)
    } catch (error) {
      throw unknown(error)
    }
  }
}
tag(Cache)

function headerOf(list: readonly [string, string][], name: string): string | null {
  const values = list.filter(([n]) => n.toLowerCase() === name.toLowerCase()).map(([, v]) => v)
  return values.length ? values.join(', ') : null
}

function sameRequest(query: CacheEntryNative, stored: CacheEntryNative): boolean {
  const bare = (url: string) => url.split('#')[0]
  if (query.method !== 'GET' || bare(query.url) !== bare(stored.url)) return false
  const vary = headerOf(stored.responseHeaders, 'vary')
  if (vary === null) return true
  for (const field of vary.split(',').map((f) => f.trim()).filter(Boolean)) {
    if (field === '*' || headerOf(stored.requestHeaders, field) !== headerOf(query.requestHeaders, field)) return false
  }
  return true
}

export class CacheStorage {
  readonly #native: CacheNative

  constructor(...key: unknown[]) {
    illegal(key[0])
    this.#native = key[1] as CacheNative
  }

  async match(request: unknown, options?: QueryOptions): Promise<Response | undefined> {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'match' on 'CacheStorage': 1 argument required, but only 0 present.")
    if (options?.cacheName !== undefined) {
      const name = `${options.cacheName as string}`
      if (!(await this.has(name))) return undefined
      return (await this.open(name)).match(request, options)
    }
    const r = requestOf(request)
    if (r.method !== 'GET' && !options?.ignoreMethod) return undefined
    try {
      const [entry] = await this.#native.query(null, query(r, options), true)
      return entry ? responseOf(entry) : undefined
    } catch (error) {
      throw unknown(error)
    }
  }

  async has(cacheName: unknown): Promise<boolean> {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'has' on 'CacheStorage': 1 argument required, but only 0 present.")
    try {
      return await this.#native.has(`${cacheName as string}`)
    } catch (error) {
      throw unknown(error)
    }
  }

  async open(cacheName: unknown): Promise<Cache> {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'open' on 'CacheStorage': 1 argument required, but only 0 present.")
    try {
      return new Cache(KEY, this.#native, await this.#native.open(`${cacheName as string}`))
    } catch (error) {
      throw unknown(error)
    }
  }

  async delete(cacheName: unknown): Promise<boolean> {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'delete' on 'CacheStorage': 1 argument required, but only 0 present.")
    try {
      return await this.#native.delete(`${cacheName as string}`)
    } catch (error) {
      throw unknown(error)
    }
  }

  async keys(): Promise<string[]> {
    try {
      return await this.#native.names()
    } catch (error) {
      throw unknown(error)
    }
  }
}
tag(CacheStorage)

export function installCaches(native: CacheNative | undefined): Record<string, unknown> {
  return native ? { Cache, CacheStorage, caches: new CacheStorage(KEY, native) } : { Cache, CacheStorage }
}
