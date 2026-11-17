import type { CookieNative, StorageAreaNative, StorageNative } from '../../src/native.ts'

type Listener = (key: string | null, oldValue: string | null, newValue: string | null, url: string) => void

export class MockStorage implements StorageNative {
  readonly items: Map<string, string>
  readonly writes: [string, string | null, string][] = []
  quota = Infinity
  #listener: Listener | null = null

  constructor(initial: Record<string, string> = {}) {
    this.items = new Map(Object.entries(initial))
  }

  readonly local: StorageAreaNative = {
    length: () => this.items.size,
    key: (index) => [...this.items.keys()][index] ?? null,
    get: (key) => this.items.get(key) ?? null,
    set: (key, value, url) => {
      if (key.length + value.length > this.quota) return false
      this.items.set(key, value)
      this.writes.push([key, value, url])
      return true
    },
    remove: (key, url) => {
      if (this.items.delete(key)) this.writes.push([key, null, url])
    },
    clear: (url) => {
      this.items.clear()
      this.writes.push(['', null, url])
    },
    keys: () => [...this.items.keys()],
  }

  readonly jar: { name: string; value: string; path: string; expires: number | null; httpOnly: boolean }[] = []

  readonly cookies: CookieNative = {
    set: (name, value, path, expires, httpOnly, fromScript) => {
      const at = this.jar.findIndex((c) => c.name === name && c.path === path)
      if (fromScript && (httpOnly || (at >= 0 && this.jar[at]!.httpOnly))) return
      if (at >= 0) this.jar.splice(at, 1)
      if (expires !== null && expires <= Date.now()) return
      this.jar.push({ name, value, path, expires, httpOnly })
    },
    matching: (path, script) => this.jar.filter((c) => !(script && c.httpOnly) && path.startsWith(c.path)).map((c): [string, string] => [c.name, c.value]),
  }

  onChange(callback: Listener): void {
    this.#listener = callback
  }

  fromElsewhere(key: string | null, newValue: string | null, url = 'http://acde.local/other'): void {
    const oldValue = key === null ? null : (this.items.get(key) ?? null)
    if (key === null) this.items.clear()
    else if (newValue === null) this.items.delete(key)
    else this.items.set(key, newValue)
    this.#listener?.(key, oldValue, newValue, url)
  }
}
