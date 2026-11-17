import { DOMException } from '../core/exception'
import { fire, type EventTarget } from '../core/event'
import { tag } from '../core/host'
import { useCookieJar } from '../core/backend'
import { env } from '../dom/env'
import { StorageEvent } from '../dom/events'
import type { StorageAreaNative, StorageNative } from '../native'

export const QUOTA = 5 * 1024 * 1024

export class MemoryArea implements StorageAreaNative {
  readonly #items = new Map<string, string>()
  #used = 0

  length(): number {
    return this.#items.size
  }
  key(index: number): string | null {
    if (index >= this.#items.size) return null
    let i = 0
    for (const key of this.#items.keys()) if (i++ === index) return key
    return null
  }
  get(key: string): string | null {
    return this.#items.get(key) ?? null
  }
  set(key: string, value: string): boolean {
    const old = this.#items.get(key)
    if (old === value) return true
    const used = this.#used - (old === undefined ? 0 : key.length + old.length) + key.length + value.length
    if (used > QUOTA && used > this.#used) return false
    this.#used = used
    this.#items.set(key, value)
    return true
  }
  remove(key: string): void {
    const old = this.#items.get(key)
    if (old === undefined) return
    this.#used -= key.length + old.length
    this.#items.delete(key)
  }
  clear(): void {
    this.#items.clear()
    this.#used = 0
  }
  keys(): string[] {
    return [...this.#items.keys()]
  }
}

const AREA = Symbol('area')
let constructing = false

export class Storage {
  declare [AREA]: StorageAreaNative

  constructor() {
    if (!constructing) throw new TypeError('Illegal constructor')
  }

  get length(): number {
    return this[AREA].length()
  }
  key(index: number): string | null {
    if (arguments.length < 1) throw new TypeError(`Failed to execute 'key' on 'Storage': 1 argument required, but only 0 present.`)
    return this[AREA].key(toUnsignedLong(index))
  }
  getItem(key: string): string | null {
    if (arguments.length < 1) throw new TypeError(`Failed to execute 'getItem' on 'Storage': 1 argument required, but only 0 present.`)
    return this[AREA].get(`${key}`)
  }
  setItem(key: string, value: string): void {
    if (arguments.length < 2) throw new TypeError(`Failed to execute 'setItem' on 'Storage': 2 arguments required, but only ${arguments.length} present.`)
    const k = `${key}`, v = `${value}`
    if (!this[AREA].set(k, v, env.url)) {
      throw new DOMException(`Failed to execute 'setItem' on 'Storage': Setting the value of '${k}' exceeded the quota.`, 'QuotaExceededError')
    }
  }
  removeItem(key: string): void {
    if (arguments.length < 1) throw new TypeError(`Failed to execute 'removeItem' on 'Storage': 1 argument required, but only 0 present.`)
    this[AREA].remove(`${key}`, env.url)
  }
  clear(): void {
    this[AREA].clear(env.url)
  }
}
tag(Storage)

function toUnsignedLong(value: unknown): number {
  const n = Number(value)
  if (!Number.isFinite(n)) return 0
  const t = Math.trunc(n) % 2 ** 32
  return t < 0 ? t + 2 ** 32 : t
}

function visible(target: Storage, key: string | symbol): key is string {
  return typeof key === 'string' && !(key in target) && target[AREA].get(key) !== null
}

export function storageOver(area: StorageAreaNative): Storage {
  constructing = true
  let target: Storage
  try {
    target = new Storage()
  } finally {
    constructing = false
  }
  Object.defineProperty(target, AREA, { value: area })
  const proxy: Storage = new Proxy(target, {
    get(t, key, receiver) {
      if (visible(t, key)) return t[AREA].get(key)
      return Reflect.get(t, key, receiver)
    },
    set(t, key, value, receiver) {
      if (typeof key === 'string' && receiver === proxy) {
        t.setItem(key, value)
        return true
      }
      return Reflect.set(t, key, value, receiver)
    },
    deleteProperty(t, key) {
      if (visible(t, key)) {
        t.removeItem(key)
        return true
      }
      return Reflect.deleteProperty(t, key)
    },
    has(t, key) {
      return visible(t, key) || Reflect.has(t, key)
    },
    defineProperty(t, key, descriptor) {
      if (typeof key === 'string') {
        if ('get' in descriptor || 'set' in descriptor) return false
        t.setItem(key, descriptor.value)
        return true
      }
      return Reflect.defineProperty(t, key, descriptor)
    },
    ownKeys(t) {
      return [...t[AREA].keys().filter((key) => !(key in t)), ...Reflect.ownKeys(t)]
    },
    getOwnPropertyDescriptor(t, key) {
      if (visible(t, key)) return { value: t[AREA].get(key), writable: true, enumerable: true, configurable: true }
      return Reflect.getOwnPropertyDescriptor(t, key)
    },
  })
  return proxy
}

export function installStorage(window: EventTarget, native: StorageNative | undefined): { local: Storage; session: Storage } {
  useCookieJar(native?.cookies)
  const local = storageOver(native?.local ?? new MemoryArea())
  const session = storageOver(new MemoryArea())
  native?.onChange((key, oldValue, newValue, url) => {
    fire(window, new StorageEvent('storage', { key, oldValue, newValue, url, storageArea: local }))
  })
  return { local, session }
}
