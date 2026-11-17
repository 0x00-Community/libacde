import { Blob, blobBytes, File } from './blob'
import { Event, EventTarget } from './event'
import { DOMException } from './exception'
import { host } from './host'

const CLONE = Symbol('clone')

type Payload = readonly unknown[]

const revivers = new Map<string, (payload: Payload) => object>()

export function serializable<T extends object>(cls: { prototype: T }, kind: string, write: (value: T) => unknown[], revive: (payload: Payload) => object): void {
  Object.defineProperty(cls.prototype, CLONE, {
    configurable: true,
    value(this: T) {
      return [kind, ...write(this)]
    },
  })
  revivers.set(kind, revive)
}

function unserializable(cls: { prototype: object; name: string }): void {
  Object.defineProperty(cls.prototype, CLONE, {
    configurable: true,
    value(this: object) {
      throw new DOMException(`${Object.prototype.toString.call(this).slice(8, -1)} object could not be cloned.`, 'DataCloneError')
    },
  })
}

function revive(payload: unknown): object {
  const list = payload as Payload
  const reviver = revivers.get(String(list?.[0]))
  if (!reviver) throw new DOMException('Unknown platform object in a clone.', 'DataCloneError')
  return reviver(list.slice(1))
}

function reviveAll(root: unknown, pairs: readonly [object, unknown][]): unknown {
  const made = new Map<object, object>()
  for (const [placeholder, payload] of pairs) made.set(placeholder, revive(payload))
  const seen = new Set<object>()
  const fix = (value: unknown): unknown => {
    if (value === null || typeof value !== 'object') return value
    const replacement = made.get(value)
    if (replacement) return replacement
    visit(value)
    return value
  }
  const visit = (object: object): void => {
    if (seen.has(object) || ArrayBuffer.isView(object) || object instanceof ArrayBuffer || object instanceof Date || object instanceof RegExp) return
    seen.add(object)
    if (object instanceof Map) {
      const entries = [...object].map(([k, v]) => [fix(k), fix(v)] as const)
      object.clear()
      for (const [k, v] of entries) object.set(k, v)
      return
    }
    if (object instanceof Set) {
      const values = [...object].map(fix)
      object.clear()
      for (const v of values) object.add(v)
      return
    }
    for (const key of Reflect.ownKeys(object)) {
      const descriptor = Object.getOwnPropertyDescriptor(object, key)
      if (!descriptor || !('value' in descriptor)) continue
      const next = fix(descriptor.value)
      if (next !== descriptor.value) (object as Record<PropertyKey, unknown>)[key] = next
    }
  }
  return fix(root)
}

export function installClone(): void {
  serializable(File, 'File', (f) => [blobBytes(f).slice(), f.type, f.name, f.lastModified], ([bytes, type, name, lastModified]) =>
    new File([bytes as Uint8Array], name as string, { type: type as string, lastModified: lastModified as number }))
  serializable(Blob, 'Blob', (b) => [blobBytes(b).slice(), b.type], ([bytes, type]) => new Blob([bytes as Uint8Array], { type: type as string }))
  unserializable(EventTarget)
  unserializable(Event)
  host.cloneHooks?.(CLONE, reviveAll)
}
