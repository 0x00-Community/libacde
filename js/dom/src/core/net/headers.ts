import { tag } from '../host'

export type HeadersInit = Headers | Record<string, string> | Iterable<readonly [string, string]>

export class Headers {
  #map = new Map<string, string>()
  #cookies: string[] = []

  constructor(init?: HeadersInit | null) {
    if (!init) return
    if (init instanceof Headers) for (const [k, v] of init) this.append(k, v)
    else if (typeof (init as Iterable<unknown>)[Symbol.iterator] === 'function') {
      for (const [k, v] of init as Iterable<readonly [string, string]>) this.append(k, v)
    }
    else for (const k of Object.keys(init)) this.append(k, (init as Record<string, string>)[k]!)
  }

  append(name: string, value: string): void {
    const key = String(name).toLowerCase()
    const v = String(value).trim()
    if (key === 'set-cookie') return void this.#cookies.push(v)
    const old = this.#map.get(key)
    this.#map.set(key, old === undefined ? v : `${old}, ${v}`)
  }
  set(name: string, value: string): void {
    const key = String(name).toLowerCase()
    if (key === 'set-cookie') this.#cookies = [String(value).trim()]
    else this.#map.set(key, String(value).trim())
  }
  get(name: string): string | null {
    const key = String(name).toLowerCase()
    if (key === 'set-cookie') return this.#cookies.length ? this.#cookies.join(', ') : null
    return this.#map.get(key) ?? null
  }
  getSetCookie(): string[] {
    return [...this.#cookies]
  }
  has(name: string): boolean {
    const key = String(name).toLowerCase()
    return key === 'set-cookie' ? this.#cookies.length > 0 : this.#map.has(key)
  }
  delete(name: string): void {
    const key = String(name).toLowerCase()
    if (key === 'set-cookie') this.#cookies = []
    else this.#map.delete(key)
  }
  forEach(callback: (value: string, key: string, parent: Headers) => void, thisArg?: unknown): void {
    for (const [k, v] of this) callback.call(thisArg, v, k, this)
  }
  *entries(): IterableIterator<[string, string]> {
    const all: [string, string][] = [...this.#map, ...this.#cookies.map((v): [string, string] => ['set-cookie', v])]
    yield* all.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0))
  }
  *keys(): IterableIterator<string> {
    for (const [k] of this.entries()) yield k
  }
  *values(): IterableIterator<string> {
    for (const [, v] of this.entries()) yield v
  }
  [Symbol.iterator](): IterableIterator<[string, string]> {
    return this.entries()
  }
}
tag(Headers)
