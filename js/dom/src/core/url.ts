import { host, tag } from './host'

type Parts = NonNullable<ReturnType<typeof host.parseUrl>>

const SPECIAL = new Set(['http:', 'https:', 'ws:', 'wss:', 'ftp:', 'file:'])

export const objectUrls = new Map<string, unknown>()
export const objectUrlOrigin = { value: 'null' }

function formEncode(text: string): string {
  return encodeURIComponent(text).replace(/%20/g, '+').replace(/[!'()~]/g, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase()}`)
}

function formDecode(text: string): string {
  const plus = text.replace(/\+/g, ' ')
  try {
    return decodeURIComponent(plus)
  } catch {
    return plus.replace(/(%[0-9A-Fa-f]{2})+/g, (run) => {
      try {
        return decodeURIComponent(run)
      } catch {
        return run
      }
    })
  }
}

let linking = false
let linkParams: (params: URLSearchParams, url: URL) => void
let reparse: (params: URLSearchParams, query: string) => void
let setSearch: (url: URL, search: string) => void

export class URLSearchParams {
  #list: [string, string][] = []
  #url: URL | null = null

  constructor(init?: string | URLSearchParams | Record<string, string> | Iterable<readonly [string, string]> | null) {
    if (init === undefined || init === null) return
    if (typeof init === 'object') {
      if (init instanceof URLSearchParams) this.#list = init.#list.map(([k, v]) => [k, v])
      else if (typeof (init as Iterable<unknown>)[Symbol.iterator] === 'function') {
        for (const pair of init as Iterable<readonly [string, string]>) {
          const items = [...(pair as Iterable<string>)]
          if (items.length !== 2) throw new TypeError("Failed to construct 'URLSearchParams': Sequence initializer must only contain pair elements")
          this.#list.push([String(items[0]), String(items[1])])
        }
      } else for (const key of Object.keys(init)) this.#list.push([key, String((init as Record<string, string>)[key])])
    } else this.#parse(String(init))
  }

  static {
    linkParams = (params, url) => {
      params.#url = url
    }
    reparse = (params, query) => {
      params.#list = []
      params.#parse(query)
    }
  }

  #parse(text: string): void {
    const query = text.startsWith('?') ? text.slice(1) : text
    for (const part of query.split('&')) {
      if (!part) continue
      const eq = part.indexOf('=')
      this.#list.push(eq < 0 ? [formDecode(part), ''] : [formDecode(part.slice(0, eq)), formDecode(part.slice(eq + 1))])
    }
  }

  #update(): void {
    if (!this.#url || linking) return
    linking = true
    try {
      const text = this.toString()
      setSearch(this.#url, text ? `?${text}` : '')
    } finally {
      linking = false
    }
  }

  get size(): number {
    return this.#list.length
  }

  append(name: string, value: string): void {
    this.#list.push([String(name), String(value)])
    this.#update()
  }

  delete(name: string, value?: string): void {
    const n = String(name)
    this.#list = this.#list.filter(([k, v]) => k !== n || (value !== undefined && v !== String(value)))
    this.#update()
  }

  get(name: string): string | null {
    const n = String(name)
    return this.#list.find(([k]) => k === n)?.[1] ?? null
  }

  getAll(name: string): string[] {
    const n = String(name)
    return this.#list.filter(([k]) => k === n).map(([, v]) => v)
  }

  has(name: string, value?: string): boolean {
    const n = String(name)
    return this.#list.some(([k, v]) => k === n && (value === undefined || v === String(value)))
  }

  set(name: string, value: string): void {
    const n = String(name), v = String(value)
    const at = this.#list.findIndex(([k]) => k === n)
    if (at < 0) this.#list.push([n, v])
    else {
      this.#list[at]![1] = v
      this.#list = this.#list.filter(([k], i) => k !== n || i === at)
    }
    this.#update()
  }

  sort(): void {
    this.#list.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0))
    this.#update()
  }

  forEach(callback: (value: string, key: string, parent: URLSearchParams) => void, thisArg?: unknown): void {
    for (const [k, v] of this.#list.slice()) callback.call(thisArg, v, k, this)
  }

  *keys(): IterableIterator<string> {
    for (const [k] of this.#list) yield k
  }

  *values(): IterableIterator<string> {
    for (const [, v] of this.#list) yield v
  }

  *entries(): IterableIterator<[string, string]> {
    for (const [k, v] of this.#list) yield [k, v]
  }

  [Symbol.iterator](): IterableIterator<[string, string]> {
    return this.entries()
  }

  toString(): string {
    return this.#list.map(([k, v]) => `${formEncode(k)}=${formEncode(v)}`).join('&')
  }
}
tag(URLSearchParams)

export class URL {
  #parts: Parts
  #params: URLSearchParams | null = null

  constructor(url: string | URL, base?: string | URL) {
    const parts = host.parseUrl(String(url), base === undefined ? null : String(base))
    if (!parts) throw new TypeError(`Failed to construct 'URL': Invalid URL`)
    this.#parts = parts
  }

  static {
    setSearch = (url, search) => {
      url.#rebuild({ search })
    }
  }

  static canParse(url: string, base?: string): boolean {
    return host.parseUrl(String(url), base === undefined ? null : String(base)) !== null
  }

  static parse(url: string, base?: string): URL | null {
    try {
      return new URL(url, base)
    } catch {
      return null
    }
  }

  static createObjectURL(object: unknown): string {
    const url = `blob:${objectUrlOrigin.value}/${host.randomUUID()}`
    objectUrls.set(url, object)
    return url
  }

  static revokeObjectURL(url: string): void {
    objectUrls.delete(String(url))
  }

  #rebuild(change: Partial<Parts>): void {
    const p = { ...this.#parts, ...change }
    const authority = this.#parts.href.startsWith(`${this.#parts.protocol}//`) || SPECIAL.has(p.protocol)
    const userinfo = p.username || p.password ? `${p.username}${p.password ? `:${p.password}` : ''}@` : ''
    const hostPart = change.hostname !== undefined && change.port === undefined ? `${p.hostname}${this.#parts.port ? `:${this.#parts.port}` : ''}` : change.host !== undefined ? p.host : `${p.hostname}${p.port ? `:${p.port}` : ''}`
    const search = p.search && !p.search.startsWith('?') ? `?${p.search}` : p.search === '?' ? '' : p.search
    const hash = p.hash && !p.hash.startsWith('#') ? `#${p.hash}` : p.hash === '#' ? '' : p.hash
    let path = p.pathname
    if (authority && path && !path.startsWith('/')) path = `/${path}`
    const href = authority ? `${p.protocol}//${userinfo}${hostPart}${path}${search}${hash}` : `${p.protocol}${path}${search}${hash}`
    const parsed = host.parseUrl(href, null)
    if (!parsed) return
    this.#parts = parsed
    if (this.#params && change.search !== undefined && !linking) reparse(this.#params, parsed.search)
  }

  get href(): string {
    return this.#parts.href
  }
  set href(value: string) {
    const parsed = host.parseUrl(String(value), null)
    if (!parsed) throw new TypeError(`Failed to set the 'href' property on 'URL': Invalid URL`)
    this.#parts = parsed
    if (this.#params) reparse(this.#params, parsed.search)
  }
  get origin(): string {
    return this.#parts.origin
  }
  get protocol(): string {
    return this.#parts.protocol
  }
  set protocol(value: string) {
    const scheme = String(value).replace(/:.*$/, '')
    if (!/^[A-Za-z][A-Za-z0-9+.-]*$/.test(scheme)) return
    const parsed = host.parseUrl(`${scheme}:${this.#parts.href.slice(this.#parts.protocol.length)}`, null)
    if (parsed && SPECIAL.has(parsed.protocol) === SPECIAL.has(this.#parts.protocol)) this.#parts = parsed
  }
  get username(): string {
    return this.#parts.username
  }
  set username(value: string) {
    this.#rebuild({ username: encodeURIComponent(String(value)) })
  }
  get password(): string {
    return this.#parts.password
  }
  set password(value: string) {
    this.#rebuild({ password: encodeURIComponent(String(value)) })
  }
  get host(): string {
    return this.#parts.host
  }
  set host(value: string) {
    this.#rebuild({ host: String(value) })
  }
  get hostname(): string {
    return this.#parts.hostname
  }
  set hostname(value: string) {
    this.#rebuild({ hostname: String(value) })
  }
  get port(): string {
    return this.#parts.port
  }
  set port(value: string) {
    const port = String(value).replace(/[^\d].*$/, '')
    this.#rebuild({ port })
  }
  get pathname(): string {
    return this.#parts.pathname
  }
  set pathname(value: string) {
    this.#rebuild({ pathname: String(value) })
  }
  get search(): string {
    return this.#parts.search
  }
  set search(value: string) {
    const v = String(value)
    this.#rebuild({ search: v === '' ? '' : v.startsWith('?') ? v : `?${v}` })
  }
  get searchParams(): URLSearchParams {
    if (!this.#params) {
      this.#params = new URLSearchParams(this.#parts.search)
      linkParams(this.#params, this)
    }
    return this.#params
  }
  get hash(): string {
    return this.#parts.hash
  }
  set hash(value: string) {
    const v = String(value)
    this.#rebuild({ hash: v === '' ? '' : v.startsWith('#') ? v : `#${v}` })
  }

  toString(): string {
    return this.#parts.href
  }

  toJSON(): string {
    return this.#parts.href
  }
}
tag(URL)
