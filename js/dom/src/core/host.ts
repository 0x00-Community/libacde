import type { HostNative } from '../native'

export let host: HostNative = null as unknown as HostNative
export let G: Record<PropertyKey, any> = globalThis as never

export function bindHost(global: object, native: HostNative): void {
  G = global as never
  host = native
}

export let report: (error: unknown) => void = (error) => host.reportError(error)

export function setReport(fn: (error: unknown) => void): void {
  report = fn
}

export function guarded(fn: () => void): void {
  try {
    fn()
  } catch (error) {
    report(error)
  }
}

export function globals(global: object, values: Record<string, unknown>): void {
  for (const name of Object.keys(values)) Object.defineProperty(global, name, { value: values[name], writable: true, configurable: true, enumerable: false })
}

export function queueTask(fn: () => void): void {
  host.setTimer(0, () => guarded(fn), true)
}

export function tag(cls: { prototype: object; name: string }, name = cls.name): void {
  Object.defineProperty(cls.prototype, Symbol.toStringTag, { value: name, configurable: true })
}

export function constants(cls: { prototype: object }, table: Record<string, number>): void {
  for (const [name, value] of Object.entries(table)) {
    Object.defineProperty(cls, name, { value, enumerable: true })
    Object.defineProperty(cls.prototype, name, { value, enumerable: true })
  }
}

export const KEY = Symbol('key')

export function illegal(token: unknown): void {
  if (token !== KEY) throw new TypeError('Illegal constructor')
}

export function getters(target: object, table: Record<string, unknown>): void {
  for (const [name, value] of Object.entries(table)) {
    Object.defineProperty(target, name, { get: typeof value === 'function' ? (value as () => unknown) : () => value, enumerable: true, configurable: true })
  }
}

export function listClass<T>(name: string) {
  const cls = {
    [name]: class {
      readonly #items: T[]
      constructor(items: T[] = []) {
        this.#items = items
        items.forEach((value, i) => Object.defineProperty(this, i, { value, enumerable: true }))
      }
      get length(): number {
        return this.#items.length
      }
      item(index: number): T | null {
        return this.#items[index] ?? null
      }
      [Symbol.iterator](): IterableIterator<T> {
        return this.#items[Symbol.iterator]()
      }
    },
  }[name]!
  tag(cls, name)
  return cls
}
