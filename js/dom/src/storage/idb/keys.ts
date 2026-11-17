import { blobBytes, Blob, File } from '../../core/blob'
import { DOMException } from '../../core/exception'
import { host, tag } from '../../core/host'

export type Key = Uint8Array
export type KeyPath = string | string[]

const enum Tag {
  End = 0x00,
  Number = 0x10,
  Date = 0x20,
  String = 0x30,
  Binary = 0x40,
  Array = 0x50,
}

export const INVALID = Symbol('invalid')

class Writer {
  bytes = new Uint8Array(64)
  length = 0

  room(n: number): void {
    if (this.length + n <= this.bytes.length) return
    let size = this.bytes.length * 2
    while (size < this.length + n) size *= 2
    const next = new Uint8Array(size)
    next.set(this.bytes.subarray(0, this.length))
    this.bytes = next
  }

  byte(b: number): void {
    this.room(1)
    this.bytes[this.length++] = b
  }

  escaped(b: number): void {
    if (b === 0) {
      this.room(2)
      this.bytes[this.length++] = 0
      this.bytes[this.length++] = 0xff
    } else this.byte(b)
  }

  number(n: number): void {
    const view = new DataView(new ArrayBuffer(8))
    view.setFloat64(0, n === 0 ? 0 : n)
    const raw = new Uint8Array(view.buffer)
    if (raw[0]! & 0x80) for (let i = 0; i < 8; i++) raw[i] = ~raw[i]! & 0xff
    else raw[0] = raw[0]! | 0x80
    this.room(8)
    this.bytes.set(raw, this.length)
    this.length += 8
  }

  done(): Key {
    return this.bytes.slice(0, this.length)
  }
}

function isDate(value: object): boolean {
  try {
    Date.prototype.getTime.call(value)
    return true
  } catch {
    return false
  }
}

function bufferBytes(value: object): Uint8Array | null {
  if (value instanceof ArrayBuffer) return (value as { detached?: boolean }).detached ? null : new Uint8Array(value)
  if (ArrayBuffer.isView(value)) {
    const buffer = value.buffer as ArrayBuffer
    if ((buffer as { detached?: boolean }).detached) return null
    return new Uint8Array(buffer, value.byteOffset, value.byteLength)
  }
  return null
}

function isBuffer(value: object): boolean {
  return value instanceof ArrayBuffer || ArrayBuffer.isView(value)
}

function write(w: Writer, input: unknown, seen: unknown[]): boolean {
  if (typeof input === 'number') {
    if (Number.isNaN(input)) return false
    w.byte(Tag.Number)
    w.number(input)
    return true
  }
  if (typeof input === 'string') {
    w.byte(Tag.String)
    for (let i = 0; i < input.length; i++) {
      const unit = input.charCodeAt(i)
      w.escaped(unit >> 8)
      w.escaped(unit & 0xff)
    }
    w.byte(0)
    w.byte(1)
    return true
  }
  if (input === null || typeof input !== 'object') return false
  if (isDate(input)) {
    const time = Date.prototype.getTime.call(input)
    if (Number.isNaN(time)) return false
    w.byte(Tag.Date)
    w.number(time)
    return true
  }
  if (isBuffer(input)) {
    const bytes = bufferBytes(input)
    if (!bytes) return false
    w.byte(Tag.Binary)
    for (let i = 0; i < bytes.length; i++) w.escaped(bytes[i]!)
    w.byte(0)
    w.byte(1)
    return true
  }
  if (Array.isArray(input)) {
    if (host.isProxy?.(input)) return false
    if (seen.includes(input)) return false
    seen.push(input)
    const length = toLength((input as { length: unknown }).length)
    w.byte(Tag.Array)
    for (let i = 0; i < length; i++) {
      if (!Object.prototype.hasOwnProperty.call(input, i)) return false
      if (!write(w, (input as unknown[])[i], seen)) return false
    }
    w.byte(Tag.End)
    return true
  }
  return false
}

function toLength(value: unknown): number {
  const n = Math.trunc(Number(value))
  if (!(n > 0)) return 0
  return Math.min(n, Number.MAX_SAFE_INTEGER)
}

export function toKey(input: unknown): Key | typeof INVALID {
  const w = new Writer()
  return write(w, input, []) ? w.done() : INVALID
}

export function toMultiEntryKeys(input: unknown): Key[] | typeof INVALID {
  if (!Array.isArray(input) || host.isProxy?.(input)) {
    const key = toKey(input)
    return key === INVALID ? INVALID : [key]
  }
  const keys: Key[] = []
  const length = toLength((input as { length: unknown }).length)
  for (let i = 0; i < length; i++) {
    if (!Object.prototype.hasOwnProperty.call(input, i)) continue
    const w = new Writer()
    let ok: boolean
    try {
      ok = write(w, (input as unknown[])[i], [input])
    } catch {
      ok = false
    }
    if (!ok) continue
    const key = w.done()
    if (!keys.some((k) => compare(k, key) === 0)) keys.push(key)
  }
  return keys
}

export function compare(a: Key, b: Key): number {
  const n = Math.min(a.length, b.length)
  for (let i = 0; i < n; i++) if (a[i] !== b[i]) return a[i]! < b[i]! ? -1 : 1
  return a.length === b.length ? 0 : a.length < b.length ? -1 : 1
}

class Reader {
  at = 0
  constructor(readonly bytes: Key) {}

  number(): number {
    const raw = this.bytes.slice(this.at, this.at + 8)
    this.at += 8
    if (raw[0]! & 0x80) raw[0] = raw[0]! & 0x7f
    else for (let i = 0; i < 8; i++) raw[i] = ~raw[i]! & 0xff
    return new DataView(raw.buffer).getFloat64(0)
  }

  escaped(): number[] {
    const out: number[] = []
    for (;;) {
      const b = this.bytes[this.at++]!
      if (b !== 0) {
        out.push(b)
        continue
      }
      const next = this.bytes[this.at++]!
      if (next === 1) return out
      out.push(0)
    }
  }

  value(): unknown {
    const t = this.bytes[this.at++]
    switch (t) {
      case Tag.Number:
        return this.number()
      case Tag.Date:
        return new Date(this.number())
      case Tag.String: {
        const bytes = this.escaped()
        let s = ''
        for (let i = 0; i < bytes.length; i += 2) s += String.fromCharCode((bytes[i]! << 8) | bytes[i + 1]!)
        return s
      }
      case Tag.Binary:
        return new Uint8Array(this.escaped()).buffer
      case Tag.Array: {
        const items: unknown[] = []
        while (this.bytes[this.at] !== Tag.End) Object.defineProperty(items, items.length, { value: this.value(), writable: true, enumerable: true, configurable: true })
        this.at++
        return items
      }
      default:
        throw new DOMException('A stored key is damaged.', 'UnknownError')
    }
  }
}

export function fromKey(key: Key): unknown {
  return new Reader(key).value()
}

export function isNumberKey(key: Key): boolean {
  return key[0] === Tag.Number
}

export function numberOfKey(key: Key): number {
  return new Reader(key.subarray(1)).number()
}

export function dataError(message: string): DOMException {
  return new DOMException(message, 'DataError')
}

export function keyOrThrow(input: unknown, what = 'The parameter is not a valid key.'): Key {
  const key = toKey(input)
  if (key === INVALID) throw dataError(what)
  return key
}

const IDENTIFIER = /^[\p{ID_Start}$_][\p{ID_Continue}$‌‍]*$/u

export function validKeyPath(path: KeyPath): boolean {
  if (Array.isArray(path)) return path.length > 0 && path.every((p) => typeof p === 'string' && validKeyPath(p))
  if (path === '') return true
  return path.split('.').every((part) => IDENTIFIER.test(part))
}

export function keyPathFromArgument(value: unknown): KeyPath | null {
  if (value === null || value === undefined) return null
  if (typeof value === 'object' && typeof (value as { [Symbol.iterator]?: unknown })[Symbol.iterator] === 'function' && typeof value !== 'string') {
    const text = (item: unknown): string => (Array.isArray(item) ? Array.from(item, text).join(',') : `${item as string}`)
    return Array.from(value as Iterable<unknown>, text)
  }
  return `${value as string}`
}

export function keyPathText(path: KeyPath | null): string | null {
  return path === null ? null : JSON.stringify(path)
}

export function keyPathOf(text: string | null): KeyPath | null {
  return text === null || text === '' ? null : (JSON.parse(text) as KeyPath)
}

const FAILURE = Symbol('failure')

function evaluate(value: unknown, path: string): unknown {
  if (path === '') return value
  for (const identifier of path.split('.')) {
    if (typeof value === 'string' && identifier === 'length') value = value.length
    else if (Array.isArray(value) && identifier === 'length') value = toLength(value.length)
    else if (value instanceof Blob && identifier === 'size') value = value.size
    else if (value instanceof Blob && identifier === 'type') value = value.type
    else if (value instanceof File && identifier === 'name') value = value.name
    else if (value instanceof File && identifier === 'lastModified') value = value.lastModified
    else {
      if (value === null || typeof value !== 'object') return FAILURE
      if (!Object.prototype.hasOwnProperty.call(value, identifier)) return FAILURE
      value = (value as Record<string, unknown>)[identifier]
    }
  }
  return value
}

export function evaluateKeyPath(value: unknown, path: KeyPath): unknown {
  if (Array.isArray(path)) {
    const out: unknown[] = []
    for (const item of path) {
      const r = evaluate(value, item)
      if (r === FAILURE) return FAILURE
      out.push(r)
    }
    return out
  }
  return evaluate(value, path)
}

export function isFailure(value: unknown): boolean {
  return value === FAILURE
}

export function extractKey(value: unknown, path: KeyPath): Key | typeof INVALID | typeof FAILURE {
  const r = evaluateKeyPath(value, path)
  if (r === FAILURE) return FAILURE
  return toKey(r)
}

export function extractIndexKeys(value: unknown, path: KeyPath, multiEntry: boolean): Key[] {
  const r = evaluateKeyPath(value, path)
  if (r === FAILURE) return []
  if (multiEntry && Array.isArray(r)) {
    const keys = toMultiEntryKeys(r)
    return keys === INVALID ? [] : keys
  }
  const key = toKey(r)
  return key === INVALID ? [] : [key]
}

export function canInjectKey(value: unknown, path: string): boolean {
  const identifiers = path.split('.')
  identifiers.pop()
  for (const identifier of identifiers) {
    if (value === null || typeof value !== 'object') return false
    if (!Object.prototype.hasOwnProperty.call(value, identifier)) return true
    value = (value as Record<string, unknown>)[identifier]
  }
  return value !== null && typeof value === 'object'
}

export function injectKey(value: unknown, key: Key, path: string): void {
  const identifiers = path.split('.')
  const last = identifiers.pop()!
  let target = value as Record<string, unknown>
  for (const identifier of identifiers) {
    if (!Object.prototype.hasOwnProperty.call(target, identifier)) {
      const fresh = {}
      Object.defineProperty(target, identifier, { value: fresh, writable: true, enumerable: true, configurable: true })
      target = fresh as Record<string, unknown>
    } else target = target[identifier] as Record<string, unknown>
  }
  Object.defineProperty(target, last, { value: fromKey(key), writable: true, enumerable: true, configurable: true })
}

const BOUNDS = Symbol('bounds')

export interface Bounds {
  readonly lower: Key | null
  readonly upper: Key | null
  readonly lowerOpen: boolean
  readonly upperOpen: boolean
}

let making = false

export class IDBKeyRange {
  declare readonly [BOUNDS]: Bounds

  constructor() {
    if (!making) throw new TypeError('Illegal constructor')
  }

  static #make(bounds: Bounds): IDBKeyRange {
    making = true
    try {
      const range = new IDBKeyRange()
      Object.defineProperty(range, BOUNDS, { value: bounds })
      return range
    } finally {
      making = false
    }
  }

  get lower(): unknown {
    const k = this[BOUNDS].lower
    return k === null ? undefined : fromKey(k)
  }
  get upper(): unknown {
    const k = this[BOUNDS].upper
    return k === null ? undefined : fromKey(k)
  }
  get lowerOpen(): boolean {
    return this[BOUNDS].lowerOpen
  }
  get upperOpen(): boolean {
    return this[BOUNDS].upperOpen
  }

  static only(value: unknown): IDBKeyRange {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'only' on 'IDBKeyRange': 1 argument required, but only 0 present.")
    const key = keyOrThrow(value)
    return IDBKeyRange.#make({ lower: key, upper: key, lowerOpen: false, upperOpen: false })
  }

  static lowerBound(lower: unknown, open = false): IDBKeyRange {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'lowerBound' on 'IDBKeyRange': 1 argument required, but only 0 present.")
    return IDBKeyRange.#make({ lower: keyOrThrow(lower), upper: null, lowerOpen: !!open, upperOpen: true })
  }

  static upperBound(upper: unknown, open = false): IDBKeyRange {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'upperBound' on 'IDBKeyRange': 1 argument required, but only 0 present.")
    return IDBKeyRange.#make({ lower: null, upper: keyOrThrow(upper), lowerOpen: true, upperOpen: !!open })
  }

  static bound(lower: unknown, upper: unknown, lowerOpen = false, upperOpen = false): IDBKeyRange {
    if (arguments.length < 2) throw new TypeError(`Failed to execute 'bound' on 'IDBKeyRange': 2 arguments required, but only ${arguments.length} present.`)
    const lo = keyOrThrow(lower)
    const hi = keyOrThrow(upper)
    const c = compare(lo, hi)
    if (c > 0 || (c === 0 && (lowerOpen || upperOpen))) throw dataError('The lower key is greater than the upper key.')
    return IDBKeyRange.#make({ lower: lo, upper: hi, lowerOpen: !!lowerOpen, upperOpen: !!upperOpen })
  }

  includes(key: unknown): boolean {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'includes' on 'IDBKeyRange': 1 argument required, but only 0 present.")
    return inRange(this[BOUNDS], keyOrThrow(key))
  }

}
tag(IDBKeyRange)

export function boundsOf(range: IDBKeyRange): Bounds {
  return range[BOUNDS]
}

export function inRange(b: Bounds, key: Key): boolean {
  if (b.lower) {
    const c = compare(key, b.lower)
    if (c < 0 || (c === 0 && b.lowerOpen)) return false
  }
  if (b.upper) {
    const c = compare(key, b.upper)
    if (c > 0 || (c === 0 && b.upperOpen)) return false
  }
  return true
}

export const UNBOUNDED: Bounds = { lower: null, upper: null, lowerOpen: false, upperOpen: false }

export function toBounds(query: unknown, nullDisallowed = false): Bounds {
  if (query instanceof IDBKeyRange) return boundsOf(query)
  if (query === undefined || query === null) {
    if (nullDisallowed) throw dataError('No key or key range specified.')
    return UNBOUNDED
  }
  const key = keyOrThrow(query)
  return { lower: key, upper: key, lowerOpen: false, upperOpen: false }
}

export function bytesOfBlob(blob: Blob): Uint8Array {
  return blobBytes(blob)
}
