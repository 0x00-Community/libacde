export function indexKey(key: string | symbol): number {
  if (typeof key !== 'string') return -1
  const n = key.length
  if (n === 0 || n > 10) return -1
  const first = key.charCodeAt(0)
  if (first < 48 || first > 57 || (first === 48 && n > 1)) return -1
  for (let i = 1; i < n; i++) {
    const c = key.charCodeAt(i)
    if (c < 48 || c > 57) return -1
  }
  const index = Number(key)
  return index < 4294967295 ? index : -1
}

export interface LegacyShape<T extends object> {
  readonly length?: (target: T) => number
  readonly item?: (target: T, index: number) => unknown
  readonly setItem?: (target: T, index: number, value: unknown) => void
  readonly named?: (target: T, key: string) => unknown
  readonly names?: (target: T) => Iterable<string>
  readonly override?: boolean
  readonly identity?: boolean
}

export function legacy<T extends object>(shape: LegacyShape<T>): ProxyHandler<T> {
  const { length, item, setItem, named, names, override = false, identity = false } = shape
  const visible = (target: T, key: string): unknown => {
    if (!named || (override ? Object.hasOwn(target, key) : key in target)) return undefined
    return named(target, key)
  }
  return {
    get(target, key, receiver) {
      if (typeof key === 'string') {
        if (item) {
          const i = indexKey(key)
          if (i >= 0) return item(target, i) ?? undefined
        }
        const value = visible(target, key)
        if (value !== undefined) return value
      }
      return Reflect.get(target, key, identity ? receiver : target)
    },
    has(target, key) {
      if (typeof key === 'string') {
        if (length) {
          const i = indexKey(key)
          if (i >= 0) return i < length(target)
        }
        if (visible(target, key) !== undefined) return true
      }
      return Reflect.has(target, key)
    },
    getOwnPropertyDescriptor(target, key) {
      if (typeof key === 'string') {
        if (item && length) {
          const i = indexKey(key)
          if (i >= 0) return i < length(target) ? { value: item(target, i), writable: !!setItem, enumerable: true, configurable: true } : undefined
        }
        const own = Reflect.getOwnPropertyDescriptor(target, key)
        if (own) return own
        const value = visible(target, key)
        return value === undefined ? undefined : { value, writable: false, enumerable: false, configurable: true }
      }
      return Reflect.getOwnPropertyDescriptor(target, key)
    },
    defineProperty(target, key, descriptor) {
      if (typeof key === 'string') {
        if (item) {
          const i = indexKey(key)
          if (i >= 0) {
            if (!setItem || !('value' in descriptor)) return false
            setItem(target, i, descriptor.value)
            return true
          }
        }
        if (named && (override || !Object.hasOwn(target, key)) && named(target, key) !== undefined) return false
      }
      return Reflect.defineProperty(target, key, descriptor)
    },
    deleteProperty(target, key) {
      if (typeof key === 'string') {
        if (length) {
          const i = indexKey(key)
          if (i >= 0) return i >= length(target)
        }
        if (!Object.hasOwn(target, key) && visible(target, key) !== undefined) return false
      }
      return Reflect.deleteProperty(target, key)
    },
    ownKeys(target) {
      const keys = new Set<string | symbol>()
      if (length) for (let i = 0, n = length(target); i < n; i++) keys.add(String(i))
      if (names) for (const name of names(target)) if (indexKey(name) < 0 && visible(target, name) !== undefined) keys.add(name)
      for (const key of Reflect.ownKeys(target)) keys.add(key)
      return [...keys]
    },
    preventExtensions() {
      return false
    },
  }
}
