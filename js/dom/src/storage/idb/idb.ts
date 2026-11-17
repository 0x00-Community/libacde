import { BUILD_PATH, defineHandlers, Event, EventTarget, fireFromTask as fire, hooks, listenerThrows, type EventInit } from '../../core/event'
import { DOMException } from '../../core/exception'
import { host, illegal, KEY, queueTask, tag } from '../../core/host'
import { cloneFailure } from '../../core/message'
import type { IdbError, IdbEvent, IdbNative, IdbRecord, IdbSchema } from '../../native'
import * as keys from './keys'
import { IDBKeyRange, INVALID, type Bounds, type Key, type KeyPath } from './keys'

let native: IdbNative
let nextId = 1
const opening = new Map<number, { request: IDBOpenDBRequest; name: string }>()
const connections = new Map<number, Connection>()
const transactions = new Map<number, Txn>()
const touched = new Set<Txn>()
const created = new Set<Txn>()
const acks: (() => void)[] = []
const abortAcks = new Map<number, () => void>()

const MAX_GENERATOR = 2 ** 53

function id(): number {
  return nextId++
}

function domError(name: string, message: string): DOMException {
  return new DOMException(message, name)
}

function fromNative(error: IdbError): DOMException {
  return domError(error.name, error.message)
}

function abortError(): DOMException {
  return domError('AbortError', 'The transaction was aborted, so the request cannot be fulfilled.')
}

function requireArgs(count: number, needed: number, method: string, iface: string): void {
  if (count < needed) throw new TypeError(`Failed to execute '${method}' on '${iface}': ${needed} argument${needed > 1 ? 's' : ''} required, but only ${count} present.`)
}

function enforceRange(value: unknown, max: number, method: string, iface: string): number {
  const n = Number(value)
  if (!Number.isFinite(n)) throw new TypeError(`Failed to execute '${method}' on '${iface}': Value is not of type 'unsigned long'.`)
  const t = Math.trunc(n)
  if (t < 0 || t > max) throw new TypeError(`Failed to execute '${method}' on '${iface}': Value is outside the 'unsigned long' value range.`)
  return t
}

function serialize(value: unknown, txn: Txn): Uint8Array {
  const state = txn.state
  txn.state = 'inactive'
  try {
    return host.serialize(value)
  } catch (error) {
    throw cloneFailure(error)
  } finally {
    if (txn.state === 'inactive') txn.state = state
  }
}

function deserialize(bytes: Uint8Array): unknown {
  return host.deserialize(bytes)
}

function sortedNames(names: Iterable<string>): DOMStringList {
  return new DOMStringList(KEY, [...names].sort())
}

export class DOMStringList {
  readonly #items: string[]

  constructor(...key: unknown[]) {
    illegal(key[0])
    this.#items = key[1] as string[]
    this.#items.forEach((value, i) => Object.defineProperty(this, i, { value, enumerable: true }))
  }

  get length(): number {
    return this.#items.length
  }
  item(index: number): string | null {
    return this.#items[index] ?? null
  }
  contains(string: string): boolean {
    return this.#items.includes(`${string}`)
  }
  [Symbol.iterator](): IterableIterator<string> {
    return this.#items[Symbol.iterator]()
  }
}
tag(DOMStringList)

export class IDBVersionChangeEvent extends Event {
  readonly #old: number
  readonly #new: number | null

  constructor(type: string, init?: EventInit & { oldVersion?: number; newVersion?: number | null }) {
    super(type, init)
    this.#old = Number(init?.oldVersion ?? 0)
    this.#new = init?.newVersion === undefined || init.newVersion === null ? null : Number(init.newVersion)
  }

  get oldVersion(): number {
    return this.#old
  }
  get newVersion(): number | null {
    return this.#new
  }
}
tag(IDBVersionChangeEvent)

const SETTLE = Symbol('settle')
const RESET = Symbol('reset')

export class IDBRequest extends EventTarget {
  #done = false
  #result: unknown = undefined
  #error: DOMException | null = null
  #source: unknown
  #transaction: IDBTransaction | null

  constructor(...key: unknown[]) {
    illegal(key[0])
    super()
    this.#source = key[1] ?? null
    this.#transaction = (key[2] as IDBTransaction | null) ?? null
  }

  get result(): unknown {
    if (!this.#done) throw domError('InvalidStateError', "Failed to read the 'result' property from 'IDBRequest': The request has not finished.")
    return this.#result
  }
  get error(): DOMException | null {
    if (!this.#done) throw domError('InvalidStateError', "Failed to read the 'error' property from 'IDBRequest': The request has not finished.")
    return this.#error
  }
  get source(): unknown {
    return this.#source
  }
  get transaction(): IDBTransaction | null {
    return this.#transaction
  }
  get readyState(): 'pending' | 'done' {
    return this.#done ? 'done' : 'pending'
  }

  [SETTLE](result: unknown, error: DOMException | null): void {
    this.#done = true
    this.#result = result
    this.#error = error
  }

  [RESET](source?: unknown, transaction?: IDBTransaction | null): void {
    this.#done = false
    this.#result = undefined
    this.#error = null
    if (source !== undefined) this.#source = source
    if (transaction !== undefined) this.#transaction = transaction
  }

  [BUILD_PATH](_event: Event, path: EventTarget[]): void {
    const tx = this.#transaction
    if (tx) path.push(tx, tx.db)
  }
}
defineHandlers(IDBRequest.prototype, ['success', 'error'])
tag(IDBRequest)

export class IDBOpenDBRequest extends IDBRequest {
  constructor(...key: unknown[]) {
    illegal(key[0])
    super(KEY, null, null)
  }
}
defineHandlers(IDBOpenDBRequest.prototype, ['blocked', 'upgradeneeded'])
tag(IDBOpenDBRequest)

export class IDBRecord {
  readonly #key: unknown
  readonly #primaryKey: unknown
  readonly #value: unknown

  constructor(...key: unknown[]) {
    illegal(key[0])
    this.#key = key[1]
    this.#primaryKey = key[2]
    this.#value = key[3]
  }

  get key(): unknown {
    return this.#key
  }
  get primaryKey(): unknown {
    return this.#primaryKey
  }
  get value(): unknown {
    return this.#value
  }
}
tag(IDBRecord)

interface IndexMeta {
  id: number
  name: string
  keyPath: KeyPath
  unique: boolean
  multiEntry: boolean
  deleted: boolean
}

interface StoreMeta {
  id: number
  name: string
  keyPath: KeyPath | null
  autoIncrement: boolean
  indexes: Map<string, IndexMeta>
  deleted: boolean
}

interface Snapshot {
  version: number
  stores: { meta: StoreMeta; name: string; indexes: { meta: IndexMeta; name: string }[] }[]
}

function schemaOf(schema: IdbSchema): Map<string, StoreMeta> {
  const stores = new Map<string, StoreMeta>()
  for (const s of schema.stores) {
    const indexes = new Map<string, IndexMeta>()
    for (const i of s.indexes) indexes.set(i.name, { id: i.id, name: i.name, keyPath: keys.keyPathOf(i.keyPath)!, unique: i.unique, multiEntry: i.multiEntry, deleted: false })
    stores.set(s.name, { id: s.id, name: s.name, keyPath: keys.keyPathOf(s.keyPath), autoIncrement: s.autoIncrement, indexes, deleted: false })
  }
  return stores
}

class Connection {
  readonly db: IDBDatabase
  version: number
  stores: Map<string, StoreMeta>
  closePending = false
  upgrade: Txn | null = null
  nextStore: number
  nextIndex: number
  readonly live = new Set<Txn>()

  constructor(readonly name: string, readonly conn: number, readonly handle: number, schema: IdbSchema) {
    this.version = schema.version
    this.stores = schemaOf(schema)
    this.nextStore = Math.max(0, ...schema.stores.map((s) => s.id)) + 1
    this.nextIndex = Math.max(0, ...schema.stores.flatMap((s) => s.indexes.map((i) => i.id))) + 1
    this.db = new IDBDatabase(KEY, this)
    connections.set(conn, this)
  }

  close(): void {
    if (this.closePending) return
    this.closePending = true
    native.close(this.handle, this.conn)
    if (!this.upgrade) connections.delete(this.conn)
  }
}

const CONN = Symbol('connection')

export class IDBDatabase extends EventTarget {
  readonly [CONN]: Connection

  constructor(...key: unknown[]) {
    illegal(key[0])
    super()
    this[CONN] = key[1] as Connection
  }

  get name(): string {
    return this[CONN].name
  }
  get version(): number {
    return this[CONN].version
  }
  get objectStoreNames(): DOMStringList {
    return sortedNames(this[CONN].stores.keys())
  }

  transaction(storeNames: unknown, mode: unknown = 'readonly', options?: { durability?: unknown }): IDBTransaction {
    requireArgs(arguments.length, 1, 'transaction', 'IDBDatabase')
    const c = this[CONN]
    const names = typeof storeNames === 'string' || storeNames === null || typeof storeNames !== 'object' || typeof (storeNames as { [Symbol.iterator]?: unknown })[Symbol.iterator] !== 'function'
      ? [`${storeNames as string}`]
      : Array.from(storeNames as Iterable<unknown>, (n) => `${n as string}`)
    const modeText = `${mode as string}`
    if (!['readonly', 'readwrite', 'versionchange'].includes(modeText)) throw new TypeError(`Failed to execute 'transaction' on 'IDBDatabase': The provided value '${modeText}' is not a valid enum value of type IDBTransactionMode.`)
    const durability = options?.durability === undefined ? 'default' : `${options.durability as string}`
    if (!['default', 'strict', 'relaxed'].includes(durability)) throw new TypeError(`Failed to execute 'transaction' on 'IDBDatabase': The provided value '${durability}' is not a valid enum value of type IDBTransactionDurability.`)
    if (c.upgrade) throw domError('InvalidStateError', "Failed to execute 'transaction' on 'IDBDatabase': A version change transaction is running.")
    if (c.closePending) throw domError('InvalidStateError', "Failed to execute 'transaction' on 'IDBDatabase': The database connection is closing.")
    const scope: StoreMeta[] = []
    for (const name of new Set(names)) {
      const meta = c.stores.get(name)
      if (!meta) throw domError('NotFoundError', "Failed to execute 'transaction' on 'IDBDatabase': One of the specified object stores was not found.")
      scope.push(meta)
    }
    if (scope.length === 0) throw domError('InvalidAccessError', "Failed to execute 'transaction' on 'IDBDatabase': The storeNames parameter was empty.")
    if (modeText === 'versionchange') throw new TypeError("Failed to execute 'transaction' on 'IDBDatabase': The mode provided ('versionchange') is not one of 'readonly' or 'readwrite'.")
    const txn = new Txn(c, id(), modeText as 'readonly' | 'readwrite', scope, durability)
    native.begin(c.handle, txn.id, c.conn, scope.map((s) => s.id), modeText === 'readwrite' ? 1 : 0)
    return txn.tx
  }

  close(): void {
    this[CONN].close()
  }

  createObjectStore(name: unknown, options?: { keyPath?: unknown; autoIncrement?: unknown }): IDBObjectStore {
    requireArgs(arguments.length, 1, 'createObjectStore', 'IDBDatabase')
    const c = this[CONN]
    const txn = c.upgrade
    if (!txn) throw domError('InvalidStateError', "Failed to execute 'createObjectStore' on 'IDBDatabase': The database is not running a version change transaction.")
    if (txn.state !== 'active') throw domError('TransactionInactiveError', "Failed to execute 'createObjectStore' on 'IDBDatabase': The transaction is not active.")
    const storeName = `${name as string}`
    const keyPath = keys.keyPathFromArgument(options?.keyPath)
    const autoIncrement = !!options?.autoIncrement
    if (keyPath !== null && !keys.validKeyPath(keyPath)) throw domError('SyntaxError', "Failed to execute 'createObjectStore' on 'IDBDatabase': The keyPath option is not a valid key path.")
    if (c.stores.has(storeName)) throw domError('ConstraintError', "Failed to execute 'createObjectStore' on 'IDBDatabase': An object store with the specified name already exists.")
    if (autoIncrement && (keyPath === '' || Array.isArray(keyPath))) {
      throw domError('InvalidAccessError', "Failed to execute 'createObjectStore' on 'IDBDatabase': The autoIncrement option was set but the keyPath option was empty or an array.")
    }
    const meta: StoreMeta = { id: c.nextStore++, name: storeName, keyPath, autoIncrement, indexes: new Map(), deleted: false }
    c.stores.set(storeName, meta)
    txn.generators.set(meta.id, 1)
    txn.internal((op) => native.createStore(c.handle, txn.id, op, meta.id, storeName, keys.keyPathText(keyPath), autoIncrement))
    return txn.store(meta).handle
  }

  deleteObjectStore(name: unknown): void {
    requireArgs(arguments.length, 1, 'deleteObjectStore', 'IDBDatabase')
    const c = this[CONN]
    const txn = c.upgrade
    if (!txn) throw domError('InvalidStateError', "Failed to execute 'deleteObjectStore' on 'IDBDatabase': The database is not running a version change transaction.")
    if (txn.state !== 'active') throw domError('TransactionInactiveError', "Failed to execute 'deleteObjectStore' on 'IDBDatabase': The transaction is not active.")
    const meta = c.stores.get(`${name as string}`)
    if (!meta) throw domError('NotFoundError', "Failed to execute 'deleteObjectStore' on 'IDBDatabase': The specified object store was not found.")
    c.stores.delete(meta.name)
    meta.deleted = true
    for (const index of meta.indexes.values()) index.deleted = true
    txn.internal((op) => native.deleteStore(c.handle, txn.id, op, meta.id))
  }
}
defineHandlers(IDBDatabase.prototype, ['abort', 'close', 'error', 'versionchange'])
tag(IDBDatabase)

type Outcome = { value: unknown } | { error: DOMException }
type ResultMessage = Extract<IdbEvent, { type: 'result' }>
type Finish = (message: ResultMessage) => Outcome
type Mode = 'readonly' | 'readwrite' | 'versionchange'

interface Pending {
  request: IDBRequest | null
  finish: Finish
}

class Txn {
  readonly tx: IDBTransaction
  state: 'active' | 'inactive' | 'committing' | 'finished' = 'active'
  started = false
  paused = false
  readonly queue: { run: () => void; ready: (() => boolean) | null }[] = []
  readonly inflight = new Map<number, number>()
  readonly pending = new Map<number, Pending>()
  readonly order: IDBRequest[] = []
  readonly generators = new Map<number, number>()
  readonly stores = new Map<StoreMeta, StoreCore>()
  error: DOMException | null = null
  open: IDBOpenDBRequest | null = null
  snapshot: Snapshot | null = null
  nextOp = 1
  commitSent = false

  constructor(readonly connection: Connection, readonly id: number, readonly mode: Mode, readonly scope: StoreMeta[], readonly durability: string) {
    this.tx = new IDBTransaction(KEY, this)
    transactions.set(id, this)
    connection.live.add(this)
    touched.add(this)
    if (mode !== 'versionchange') created.add(this)
  }

  static upgrade(connection: Connection, txId: number, open: IDBOpenDBRequest, oldVersion: number): Txn {
    const txn = new Txn(connection, txId, 'versionchange', [], 'default')
    txn.open = open
    txn.snapshot = {
      version: oldVersion,
      stores: [...connection.stores.values()].map((meta) => ({ meta, name: meta.name, indexes: [...meta.indexes.values()].map((i) => ({ meta: i, name: i.name })) })),
    }
    connection.upgrade = txn
    return txn
  }

  store(meta: StoreMeta): StoreCore {
    let store = this.stores.get(meta)
    if (!store) {
      store = new StoreCore(meta, this)
      this.stores.set(meta, store)
    }
    return store
  }

  enqueue(fn: () => void, ready: (() => boolean) | null = null): void {
    if (this.started && !this.paused && this.queue.length === 0 && (!ready || ready())) fn()
    else this.queue.push({ run: fn, ready })
  }

  drain(): void {
    while (this.started && !this.paused && this.queue.length) {
      const head = this.queue[0]!
      if (head.ready && !head.ready()) return
      this.queue.shift()
      head.run()
    }
  }

  pause(): void {
    this.paused = true
  }

  resume(): void {
    this.paused = false
    this.drain()
  }

  place(request: IDBRequest | null, send: (op: number) => void, finish: Finish, ready: (() => boolean) | null = null): void {
    const op = this.nextOp++
    this.pending.set(op, { request, finish })
    if (request) this.order.push(request)
    this.enqueue(() => send(op), ready)
  }

  placeNow(send: (op: number) => void, finish: Finish): void {
    const op = this.nextOp++
    this.pending.set(op, { request: null, finish })
    send(op)
  }

  internal(send: (op: number) => void): void {
    this.place(null, send, (message) => {
      if (!message.error) return { value: undefined }
      const error = fromNative(message.error)
      this.fail(error, true)
      return { error }
    })
  }

  request(source: unknown, send: (op: number) => void, finish: Finish, reuse?: IDBRequest, ready: (() => boolean) | null = null): IDBRequest {
    const request = reuse ?? new IDBRequest(KEY, source, this.tx)
    this.place(request, send, finish, ready)
    return request
  }

  begin(generators: readonly [number, number][]): void {
    for (const [store, n] of generators) if (!this.generators.has(store)) this.generators.set(store, n)
    this.started = true
    this.drain()
  }

  result(message: ResultMessage): void {
    const entry = this.pending.get(message.op)
    if (!entry) return
    this.pending.delete(message.op)
    if (this.state === 'finished') return
    const outcome = entry.finish(message)
    this.drain()
    const request = entry.request
    if (!request || this.finished()) {
      touched.add(this)
      this.sendCommit()
      return
    }
    this.order.splice(this.order.indexOf(request), 1)
    if ('error' in outcome) {
      request[SETTLE](undefined, outcome.error)
      this.fireError(request)
    } else {
      request[SETTLE](outcome.value, null)
      this.fireSuccess(request)
    }
    this.sendCommit()
  }

  finished(): boolean {
    return this.state === 'finished'
  }

  activate(): void {
    if (this.state === 'inactive') this.state = 'active'
    touched.add(this)
  }

  fireSuccess(request: IDBRequest): void {
    this.activate()
    const before = listenerThrows()
    fire(request, new Event('success', { bubbles: false, cancelable: false }))
    if (this.state !== 'active') return
    this.state = 'inactive'
    if (listenerThrows() !== before) this.fail(domError('AbortError', 'An exception was thrown in an event handler.'), true)
  }

  fireError(request: IDBRequest): void {
    this.activate()
    const before = listenerThrows()
    const notCanceled = fire(request, new Event('error', { bubbles: true, cancelable: true }))
    const threw = listenerThrows() !== before
    if (this.state === 'active') {
      this.state = 'inactive'
      if (threw) this.fail(domError('AbortError', 'An exception was thrown in an event handler.'), true)
      else if (notCanceled) this.fail(request.error, true)
    } else if (this.state === 'committing' && notCanceled && !threw) this.fail(request.error, true)
  }

  deactivate(): void {
    if (this.state === 'active') this.state = 'inactive'
  }

  taskEnded(): void {
    this.deactivate()
    if (this.state === 'inactive' && this.pending.size === 0) this.commit()
  }

  commit(): void {
    this.state = 'committing'
    this.sendCommit()
  }

  sendCommit(): void {
    if (this.commitSent || this.state !== 'committing' || this.pending.size > 0) return
    this.commitSent = true
    this.enqueue(() => native.commit(this.connection.handle, this.id))
  }

  completed(): void {
    if (this.state === 'finished') return
    this.state = 'finished'
    this.forget()
    const c = this.connection
    if (this.mode === 'versionchange') c.upgrade = null
    fire(this.tx, new Event('complete', { bubbles: false, cancelable: false }))
    const open = this.open
    if (this.mode !== 'versionchange' || !open) return
    open[RESET](null, null)
    if (c.closePending) {
      connections.delete(c.conn)
      open[SETTLE](undefined, abortError())
      fire(open, new Event('error', { bubbles: true, cancelable: true }))
    } else {
      open[SETTLE](c.db, null)
      fire(open, new Event('success', { bubbles: false, cancelable: false }))
    }
  }

  fail(error: DOMException | null, tellNative: boolean): void {
    if (this.state === 'finished') return
    const c = this.connection
    this.state = 'finished'
    if (tellNative) native.abort(c.handle, this.id)
    this.queue.length = 0
    this.error = error
    if (this.mode === 'versionchange') this.revert()
    const outstanding = [...this.order]
    this.order.length = 0
    this.pending.clear()
    this.forget()
    for (const request of outstanding) request[SETTLE](undefined, abortError())
    const requestErrors = () => {
      for (const request of outstanding) fire(request, new Event('error', { bubbles: true, cancelable: true }))
    }
    const aborted = () => {
      if (this.mode === 'versionchange') c.upgrade = null
      fire(this.tx, new Event('abort', { bubbles: true, cancelable: false }))
      const open = this.open
      if (this.mode !== 'versionchange' || !open) return
      connections.delete(c.conn)
      open[RESET](null, null)
      open[SETTLE](undefined, abortError())
      fire(open, new Event('error', { bubbles: true, cancelable: true }))
    }
    if (this.mode === 'versionchange' && tellNative) {
      abortAcks.set(this.id, () => {
        requestErrors()
        aborted()
      })
      return
    }
    for (const request of outstanding) queueTask(() => fire(request, new Event('error', { bubbles: true, cancelable: true })))
    queueTask(aborted)
  }

  revert(): void {
    const snapshot = this.snapshot
    if (!snapshot) return
    const c = this.connection
    const known = new Set(snapshot.stores.map((s) => s.meta))
    const drop = (meta: StoreMeta) => {
      meta.deleted = true
      for (const index of meta.indexes.values()) index.deleted = true
    }
    for (const meta of c.stores.values()) if (!known.has(meta)) drop(meta)
    for (const meta of this.stores.keys()) if (!known.has(meta)) drop(meta)
    c.stores = new Map()
    for (const entry of snapshot.stores) {
      entry.meta.name = entry.name
      entry.meta.deleted = false
      const knownIndexes = new Set(entry.indexes.map((i) => i.meta))
      for (const index of entry.meta.indexes.values()) if (!knownIndexes.has(index)) index.deleted = true
      entry.meta.indexes = new Map()
      for (const index of entry.indexes) {
        index.meta.name = index.name
        index.meta.deleted = false
        entry.meta.indexes.set(index.name, index.meta)
      }
      c.stores.set(entry.name, entry.meta)
    }
    c.version = snapshot.version
    c.closePending = true
  }

  forget(): void {
    transactions.delete(this.id)
    this.connection.live.delete(this)
    touched.delete(this)
    created.delete(this)
  }
}

const TXN = Symbol('transaction')

export class IDBTransaction extends EventTarget {
  readonly [TXN]: Txn

  constructor(...key: unknown[]) {
    illegal(key[0])
    super()
    this[TXN] = key[1] as Txn
  }

  get db(): IDBDatabase {
    return this[TXN].connection.db
  }
  get mode(): string {
    return this[TXN].mode
  }
  get durability(): string {
    return this[TXN].durability
  }
  get error(): DOMException | null {
    return this[TXN].error
  }
  get objectStoreNames(): DOMStringList {
    const t = this[TXN]
    if (t.mode === 'versionchange') return sortedNames(t.connection.stores.keys())
    return sortedNames(t.scope.map((s) => s.name))
  }

  objectStore(name: unknown): IDBObjectStore {
    requireArgs(arguments.length, 1, 'objectStore', 'IDBTransaction')
    const t = this[TXN]
    if (t.state === 'finished') throw domError('InvalidStateError', "Failed to execute 'objectStore' on 'IDBTransaction': The transaction has finished.")
    const storeName = `${name as string}`
    const meta = t.mode === 'versionchange' ? t.connection.stores.get(storeName) : t.scope.find((s) => s.name === storeName && !s.deleted)
    if (!meta) throw domError('NotFoundError', "Failed to execute 'objectStore' on 'IDBTransaction': The specified object store was not found.")
    return t.store(meta).handle
  }

  abort(): void {
    const t = this[TXN]
    if (t.state === 'committing' || t.state === 'finished') throw domError('InvalidStateError', "Failed to execute 'abort' on 'IDBTransaction': The transaction has already been committed or aborted.")
    t.fail(null, true)
  }

  commit(): void {
    const t = this[TXN]
    if (t.state !== 'active') throw domError('InvalidStateError', "Failed to execute 'commit' on 'IDBTransaction': The transaction is not active.")
    t.commit()
  }

  [BUILD_PATH](_event: Event, path: EventTarget[]): void {
    path.push(this[TXN].connection.db)
  }
}
defineHandlers(IDBTransaction.prototype, ['abort', 'complete', 'error'])
tag(IDBTransaction)

function rangeArgs(b: Bounds): [Key | null, boolean, Key | null, boolean] {
  return [b.lower, b.lowerOpen, b.upper, b.upperOpen]
}

const DIRECTIONS = ['next', 'nextunique', 'prev', 'prevunique'] as const
type Direction = (typeof DIRECTIONS)[number]

function direction(value: unknown, method: string, iface: string): Direction {
  const d = value === undefined ? 'next' : `${value as string}`
  if (!(DIRECTIONS as readonly string[]).includes(d)) throw new TypeError(`Failed to execute '${method}' on '${iface}': The provided value '${d}' is not a valid enum value of type IDBCursorDirection.`)
  return d as Direction
}

function potentiallyValidKey(value: unknown): boolean {
  if (value === null || typeof value !== 'object') return true
  if (value instanceof IDBKeyRange || Array.isArray(value) || value instanceof ArrayBuffer || ArrayBuffer.isView(value)) return true
  try {
    Date.prototype.getTime.call(value)
    return true
  } catch {
    return false
  }
}

interface GetAllArgs {
  bounds: Bounds
  count: number
  direction: Direction
}

function getAllArgs(args: ArrayLike<unknown>, method: string, iface: string, optionsOnly: boolean): GetAllArgs {
  const query = args[0]
  if (optionsOnly || (query !== null && query !== undefined && !potentiallyValidKey(query))) {
    const options = (query ?? {}) as { query?: unknown; count?: unknown; direction?: unknown }
    const count = options.count === undefined ? 0 : enforceRange(options.count, 2 ** 32 - 1, method, iface)
    return { bounds: keys.toBounds(options.query), count, direction: direction(options.direction, method, iface) }
  }
  const count = args[1] === undefined ? 0 : enforceRange(args[1], 2 ** 32 - 1, method, iface)
  return { bounds: keys.toBounds(query), count, direction: 'next' }
}

abstract class SourceCore {
  abstract readonly txn: Txn
  abstract readonly isIndex: boolean
  abstract readonly iface: string
  abstract readonly handle: IDBObjectStore | IDBIndex
  abstract sourceId(): number
  abstract gone(): boolean
  abstract storeCore(): StoreCore

  check(method: string): void {
    if (this.gone()) throw domError('InvalidStateError', `Failed to execute '${method}' on '${this.iface}': The object store or index has been deleted.`)
    if (this.txn.state !== 'active') throw domError('TransactionInactiveError', `Failed to execute '${method}' on '${this.iface}': The transaction is not active.`)
  }

  ids(): { db: number; tx: number } {
    return { db: this.txn.connection.handle, tx: this.txn.id }
  }

  read(method: string, query: unknown, what: 'value' | 'key' | 'primaryKey'): IDBRequest {
    this.check(method)
    const bounds = keys.toBounds(query, true)
    const { db, tx } = this.ids()
    return this.txn.request(this.handle, (op) => native.get(db, tx, op, this.isIndex, this.sourceId(), ...rangeArgs(bounds), what === 'value', 1, 0), (m) => {
      if (m.error) return { error: fromNative(m.error) }
      const r = m.records?.[0]
      if (!r) return { value: undefined }
      if (what === 'value') return { value: deserialize(r.value!) }
      return { value: keys.fromKey(what === 'key' ? r.key : r.primaryKey) }
    })
  }

  readAll(method: string, args: ArrayLike<unknown>, what: 'value' | 'primaryKey' | 'record', optionsOnly = false): IDBRequest {
    this.check(method)
    const options = getAllArgs(args, method, this.iface, optionsOnly)
    const { db, tx } = this.ids()
    return this.txn.request(this.handle, (op) => native.get(db, tx, op, this.isIndex, this.sourceId(), ...rangeArgs(options.bounds), what !== 'primaryKey', options.count, DIRECTIONS.indexOf(options.direction)), (m) => {
      if (m.error) return { error: fromNative(m.error) }
      const records = m.records ?? []
      if (what === 'value') return { value: records.map((r) => deserialize(r.value!)) }
      if (what === 'primaryKey') return { value: records.map((r) => keys.fromKey(r.primaryKey)) }
      return { value: records.map((r) => new IDBRecord(KEY, keys.fromKey(r.key), keys.fromKey(r.primaryKey), deserialize(r.value!))) }
    })
  }

  count(query: unknown): IDBRequest {
    this.check('count')
    const bounds = keys.toBounds(query)
    const { db, tx } = this.ids()
    return this.txn.request(this.handle, (op) => native.count(db, tx, op, this.isIndex, this.sourceId(), ...rangeArgs(bounds)), (m) => (m.error ? { error: fromNative(m.error) } : { value: m.count ?? 0 }))
  }

  cursor(method: string, query: unknown, dir: unknown, withValue: boolean): IDBRequest {
    this.check(method)
    const d = direction(dir, method, this.iface)
    const bounds = keys.toBounds(query)
    const cursor = new CursorCore(this, d, bounds, !withValue)
    const request = new IDBRequest(KEY, this.handle, this.txn.tx)
    cursor.request = request
    cursor.step(null, null, 1)
    return request
  }
}

class StoreCore extends SourceCore {
  readonly handle: IDBObjectStore
  readonly indexes = new Map<IndexMeta, IndexCore>()
  readonly isIndex = false
  readonly iface = 'IDBObjectStore'
  keyPathValue: { value: unknown } | null = null

  constructor(readonly meta: StoreMeta, readonly txn: Txn) {
    super()
    this.handle = new IDBObjectStore(KEY, this)
  }

  sourceId(): number {
    return this.meta.id
  }
  gone(): boolean {
    return this.meta.deleted
  }
  storeCore(): StoreCore {
    return this
  }

  index(meta: IndexMeta): IndexCore {
    let index = this.indexes.get(meta)
    if (!index) {
      index = new IndexCore(meta, this)
      this.indexes.set(meta, index)
    }
    return index
  }

  put(method: string, value: unknown, keyArg: unknown, overwrite: boolean, cursor?: CursorCore, cloned?: { bytes: Uint8Array; clone: unknown; key: Key }): IDBRequest {
    const { meta, txn } = this
    const inline = meta.keyPath !== null
    let key: Key | null = null
    if (cloned) key = cloned.key
    else {
      this.check(method)
      if (txn.mode === 'readonly') throw domError('ReadOnlyError', `Failed to execute '${method}' on 'IDBObjectStore': The transaction is read-only.`)
      if (inline && keyArg !== undefined) throw keys.dataError(`Failed to execute '${method}' on 'IDBObjectStore': The object store uses in-line keys and the key parameter was provided.`)
      if (!inline && !meta.autoIncrement && keyArg === undefined) throw keys.dataError(`Failed to execute '${method}' on 'IDBObjectStore': The object store uses out-of-line keys and has no key generator and the key parameter was not provided.`)
      if (keyArg !== undefined) key = keys.keyOrThrow(keyArg, `Failed to execute '${method}' on 'IDBObjectStore': The parameter is not a valid key.`)
    }
    const bytes = cloned?.bytes ?? serialize(value, txn)
    const indexes = [...meta.indexes.values()]
    const clone = cloned ? cloned.clone : inline || indexes.length ? deserialize(bytes) : undefined
    if (inline && !cloned) {
      const extracted = keys.extractKey(clone, meta.keyPath!)
      if (extracted === INVALID) throw keys.dataError(`Failed to execute '${method}' on 'IDBObjectStore': Evaluating the object store's key path yielded a value that is not a valid key.`)
      if (keys.isFailure(extracted)) {
        if (!meta.autoIncrement) throw keys.dataError(`Failed to execute '${method}' on 'IDBObjectStore': Evaluating the object store's key path did not yield a value.`)
        if (!keys.canInjectKey(clone, meta.keyPath as string)) throw keys.dataError(`Failed to execute '${method}' on 'IDBObjectStore': A generated key could not be inserted into the value.`)
      } else key = extracted as Key
    }
    const { db, tx } = this.ids()
    let stored: Key | null = null
    let failure: DOMException | null = null
    let restore: number | null = null
    const ready = meta.autoIncrement ? () => !txn.inflight.get(meta.id) : null
    return txn.request(cursor?.cursor ?? this.handle, (op) => {
      let generated: number | undefined
      let finalBytes = bytes
      let finalKey = key
      if (meta.autoIncrement) {
        const current = txn.generators.get(meta.id) ?? 1
        if (finalKey === null) {
          if (!(current <= MAX_GENERATOR)) {
            failure = domError('ConstraintError', 'The key generator of the object store has reached its maximum value.')
            native.count(db, tx, op, false, meta.id, null, false, null, false)
            return
          }
          finalKey = keys.toKey(current) as Key
          generated = current >= MAX_GENERATOR ? Infinity : current + 1
          if (inline) {
            keys.injectKey(clone, finalKey, meta.keyPath as string)
            finalBytes = host.serialize(clone)
          }
        } else if (keys.isNumberKey(finalKey)) {
          const n = keys.numberOfKey(finalKey)
          if (n >= current) generated = n >= MAX_GENERATOR ? Infinity : Math.floor(n) + 1
        }
        if (generated !== undefined) {
          restore = current
          txn.generators.set(meta.id, generated)
        }
        txn.inflight.set(meta.id, (txn.inflight.get(meta.id) ?? 0) + 1)
      }
      stored = finalKey
      const entries: (number | Uint8Array)[] = []
      for (const index of indexes) {
        for (const k of keys.extractIndexKeys(clone, index.keyPath, index.multiEntry)) entries.push(index.id, k)
      }
      native.put(db, tx, op, meta.id, finalKey!, finalBytes, entries, overwrite, generated)
    }, (m) => {
      if (failure) return { error: failure }
      if (meta.autoIncrement) txn.inflight.set(meta.id, (txn.inflight.get(meta.id) ?? 1) - 1)
      if (m.error) {
        if (restore !== null) txn.generators.set(meta.id, restore)
        return { error: fromNative(m.error) }
      }
      return { value: keys.fromKey(stored!) }
    }, undefined, ready)
  }

  update(value: unknown, primaryKey: Key, cursor: CursorCore): IDBRequest {
    const bytes = serialize(value, this.txn)
    const clone = deserialize(bytes)
    if (this.meta.keyPath !== null) {
      const extracted = keys.extractKey(clone, this.meta.keyPath)
      if (extracted === INVALID || keys.isFailure(extracted) || keys.compare(extracted as Key, primaryKey) !== 0) {
        throw keys.dataError("Failed to execute 'update' on 'IDBCursor': The effective object store of this cursor uses in-line keys and evaluating the key path of the value parameter results in a different value than the cursor's effective key.")
      }
    }
    return this.put('update', value, undefined, true, cursor, { bytes, clone, key: primaryKey })
  }

  remove(bounds: Bounds, source: unknown): IDBRequest {
    const { db, tx } = this.ids()
    return this.txn.request(source, (op) => native.remove(db, tx, op, this.meta.id, ...rangeArgs(bounds)), (m) => (m.error ? { error: fromNative(m.error) } : { value: undefined }))
  }

  clear(): IDBRequest {
    this.check('clear')
    if (this.txn.mode === 'readonly') throw domError('ReadOnlyError', "Failed to execute 'clear' on 'IDBObjectStore': The transaction is read-only.")
    const { db, tx } = this.ids()
    return this.txn.request(this.handle, (op) => native.clear(db, tx, op, this.meta.id), (m) => (m.error ? { error: fromNative(m.error) } : { value: undefined }))
  }

  rename(value: unknown): void {
    const { meta, txn } = this
    const name = `${value as string}`
    if (txn.mode !== 'versionchange') throw domError('InvalidStateError', "Failed to set the 'name' property on 'IDBObjectStore': The database is not running a version change transaction.")
    if (meta.deleted) throw domError('InvalidStateError', "Failed to set the 'name' property on 'IDBObjectStore': The object store has been deleted.")
    if (txn.state !== 'active') throw domError('TransactionInactiveError', "Failed to set the 'name' property on 'IDBObjectStore': The transaction is not active.")
    if (name === meta.name) return
    const stores = txn.connection.stores
    if (stores.has(name)) throw domError('ConstraintError', "Failed to set the 'name' property on 'IDBObjectStore': An object store with the specified name already exists.")
    stores.delete(meta.name)
    meta.name = name
    stores.set(name, meta)
    const { db, tx } = this.ids()
    txn.internal((op) => native.renameStore(db, tx, op, meta.id, name))
  }

  createIndex(name: unknown, keyPathArg: unknown, options?: { unique?: unknown; multiEntry?: unknown }): IDBIndex {
    const { meta, txn } = this
    if (txn.mode !== 'versionchange') throw domError('InvalidStateError', "Failed to execute 'createIndex' on 'IDBObjectStore': The database is not running a version change transaction.")
    if (meta.deleted) throw domError('InvalidStateError', "Failed to execute 'createIndex' on 'IDBObjectStore': The object store has been deleted.")
    if (txn.state !== 'active') throw domError('TransactionInactiveError', "Failed to execute 'createIndex' on 'IDBObjectStore': The transaction is not active.")
    const indexName = `${name as string}`
    if (meta.indexes.has(indexName)) throw domError('ConstraintError', "Failed to execute 'createIndex' on 'IDBObjectStore': An index with the specified name already exists.")
    const keyPath = keys.keyPathFromArgument(keyPathArg)
    if (keyPath === null || !keys.validKeyPath(keyPath)) throw domError('SyntaxError', "Failed to execute 'createIndex' on 'IDBObjectStore': The keyPath argument contains an invalid key path.")
    const unique = !!options?.unique
    const multiEntry = !!options?.multiEntry
    if (Array.isArray(keyPath) && multiEntry) throw domError('InvalidAccessError', "Failed to execute 'createIndex' on 'IDBObjectStore': The keyPath argument was an array and the multiEntry option is true.")
    const c = txn.connection
    const index: IndexMeta = { id: c.nextIndex++, name: indexName, keyPath, unique, multiEntry, deleted: false }
    meta.indexes.set(indexName, index)
    const { db, tx } = this.ids()
    txn.internal((op) => native.createIndex(db, tx, op, index.id, meta.id, indexName, keys.keyPathText(keyPath)!, unique, multiEntry))
    txn.enqueue(() => {
      txn.pause()
      txn.placeNow((op) => native.get(db, tx, op, false, meta.id, null, false, null, false, true, 0, 0), (m) => {
        if (m.error) return this.#populateFailed(m.error)
        const entries: Uint8Array[] = []
        for (const record of m.records ?? []) {
          const value = deserialize(record.value!)
          for (const k of keys.extractIndexKeys(value, keyPath, multiEntry)) entries.push(k, record.primaryKey)
        }
        txn.placeNow((op) => native.indexEntries(db, tx, op, index.id, entries), (r) => (r.error ? this.#populateFailed(r.error) : { value: undefined }))
        txn.resume()
        return { value: undefined }
      })
    })
    return this.index(index).handle
  }

  #populateFailed(failure: IdbError): Outcome {
    const error = fromNative(failure)
    this.txn.fail(error, true)
    return { error }
  }

  deleteIndex(name: unknown): void {
    const { meta, txn } = this
    if (txn.mode !== 'versionchange') throw domError('InvalidStateError', "Failed to execute 'deleteIndex' on 'IDBObjectStore': The database is not running a version change transaction.")
    if (meta.deleted) throw domError('InvalidStateError', "Failed to execute 'deleteIndex' on 'IDBObjectStore': The object store has been deleted.")
    if (txn.state !== 'active') throw domError('TransactionInactiveError', "Failed to execute 'deleteIndex' on 'IDBObjectStore': The transaction is not active.")
    const index = meta.indexes.get(`${name as string}`)
    if (!index) throw domError('NotFoundError', "Failed to execute 'deleteIndex' on 'IDBObjectStore': The specified index was not found.")
    meta.indexes.delete(index.name)
    index.deleted = true
    const { db, tx } = this.ids()
    txn.internal((op) => native.deleteIndex(db, tx, op, index.id))
  }
}

const SRC = Symbol('source')

export class IDBObjectStore {
  readonly [SRC]: StoreCore

  constructor(...key: unknown[]) {
    illegal(key[0])
    this[SRC] = key[1] as StoreCore
  }

  get name(): string {
    return this[SRC].meta.name
  }
  set name(value: string) {
    this[SRC].rename(value)
  }
  get keyPath(): unknown {
    const core = this[SRC]
    const path = core.meta.keyPath
    core.keyPathValue ??= { value: Array.isArray(path) ? [...path] : path }
    return core.keyPathValue.value
  }
  get indexNames(): DOMStringList {
    const meta = this[SRC].meta
    return sortedNames(meta.deleted ? [] : meta.indexes.keys())
  }
  get transaction(): IDBTransaction {
    return this[SRC].txn.tx
  }
  get autoIncrement(): boolean {
    return this[SRC].meta.autoIncrement
  }

  put(value: unknown, key?: unknown): IDBRequest {
    requireArgs(arguments.length, 1, 'put', 'IDBObjectStore')
    return this[SRC].put('put', value, arguments.length > 1 ? key : undefined, true)
  }

  add(value: unknown, key?: unknown): IDBRequest {
    requireArgs(arguments.length, 1, 'add', 'IDBObjectStore')
    return this[SRC].put('add', value, arguments.length > 1 ? key : undefined, false)
  }

  delete(query: unknown): IDBRequest {
    requireArgs(arguments.length, 1, 'delete', 'IDBObjectStore')
    const core = this[SRC]
    core.check('delete')
    if (core.txn.mode === 'readonly') throw domError('ReadOnlyError', "Failed to execute 'delete' on 'IDBObjectStore': The transaction is read-only.")
    return core.remove(keys.toBounds(query, true), this)
  }

  clear(): IDBRequest {
    return this[SRC].clear()
  }

  get(query: unknown): IDBRequest {
    requireArgs(arguments.length, 1, 'get', 'IDBObjectStore')
    return this[SRC].read('get', query, 'value')
  }

  getKey(query: unknown): IDBRequest {
    requireArgs(arguments.length, 1, 'getKey', 'IDBObjectStore')
    return this[SRC].read('getKey', query, 'key')
  }

  getAll(_query?: unknown, _count?: number): IDBRequest {
    return this[SRC].readAll('getAll', arguments, 'value')
  }

  getAllKeys(_query?: unknown, _count?: number): IDBRequest {
    return this[SRC].readAll('getAllKeys', arguments, 'primaryKey')
  }

  getAllRecords(_options?: unknown): IDBRequest {
    return this[SRC].readAll('getAllRecords', arguments, 'record', true)
  }

  count(query?: unknown): IDBRequest {
    return this[SRC].count(query)
  }

  openCursor(query?: unknown, dir?: unknown): IDBRequest {
    return this[SRC].cursor('openCursor', query, dir, true)
  }

  openKeyCursor(query?: unknown, dir?: unknown): IDBRequest {
    return this[SRC].cursor('openKeyCursor', query, dir, false)
  }

  index(name: unknown): IDBIndex {
    requireArgs(arguments.length, 1, 'index', 'IDBObjectStore')
    const core = this[SRC]
    if (core.meta.deleted) throw domError('InvalidStateError', "Failed to execute 'index' on 'IDBObjectStore': The object store has been deleted.")
    if (core.txn.state === 'finished') throw domError('InvalidStateError', "Failed to execute 'index' on 'IDBObjectStore': The transaction has finished.")
    const index = core.meta.indexes.get(`${name as string}`)
    if (!index) throw domError('NotFoundError', "Failed to execute 'index' on 'IDBObjectStore': The specified index was not found.")
    return core.index(index).handle
  }

  createIndex(name: unknown, keyPath: unknown, options?: { unique?: unknown; multiEntry?: unknown }): IDBIndex {
    requireArgs(arguments.length, 2, 'createIndex', 'IDBObjectStore')
    return this[SRC].createIndex(name, keyPath, options)
  }

  deleteIndex(name: unknown): void {
    requireArgs(arguments.length, 1, 'deleteIndex', 'IDBObjectStore')
    this[SRC].deleteIndex(name)
  }
}
tag(IDBObjectStore)

class IndexCore extends SourceCore {
  readonly handle: IDBIndex
  readonly isIndex = true
  readonly iface = 'IDBIndex'
  keyPathValue: { value: unknown } | null = null

  constructor(readonly meta: IndexMeta, readonly store: StoreCore) {
    super()
    this.handle = new IDBIndex(KEY, this)
  }

  get txn(): Txn {
    return this.store.txn
  }
  sourceId(): number {
    return this.meta.id
  }
  gone(): boolean {
    return this.meta.deleted || this.store.gone()
  }
  storeCore(): StoreCore {
    return this.store
  }

  rename(value: unknown): void {
    const { meta, store } = this
    const txn = store.txn
    const name = `${value as string}`
    if (txn.mode !== 'versionchange') throw domError('InvalidStateError', "Failed to set the 'name' property on 'IDBIndex': The database is not running a version change transaction.")
    if (this.gone()) throw domError('InvalidStateError', "Failed to set the 'name' property on 'IDBIndex': The index or its object store has been deleted.")
    if (txn.state !== 'active') throw domError('TransactionInactiveError', "Failed to set the 'name' property on 'IDBIndex': The transaction is not active.")
    if (name === meta.name) return
    const indexes = store.meta.indexes
    if (indexes.has(name)) throw domError('ConstraintError', "Failed to set the 'name' property on 'IDBIndex': An index with the specified name already exists.")
    indexes.delete(meta.name)
    meta.name = name
    indexes.set(name, meta)
    const { db, tx } = this.ids()
    txn.internal((op) => native.renameIndex(db, tx, op, meta.id, name))
  }
}

export class IDBIndex {
  readonly [SRC]: IndexCore

  constructor(...key: unknown[]) {
    illegal(key[0])
    this[SRC] = key[1] as IndexCore
  }

  get name(): string {
    return this[SRC].meta.name
  }
  set name(value: string) {
    this[SRC].rename(value)
  }
  get objectStore(): IDBObjectStore {
    return this[SRC].store.handle
  }
  get keyPath(): unknown {
    const core = this[SRC]
    const path = core.meta.keyPath
    core.keyPathValue ??= { value: Array.isArray(path) ? [...path] : path }
    return core.keyPathValue.value
  }
  get multiEntry(): boolean {
    return this[SRC].meta.multiEntry
  }
  get unique(): boolean {
    return this[SRC].meta.unique
  }

  get(query: unknown): IDBRequest {
    requireArgs(arguments.length, 1, 'get', 'IDBIndex')
    return this[SRC].read('get', query, 'value')
  }

  getKey(query: unknown): IDBRequest {
    requireArgs(arguments.length, 1, 'getKey', 'IDBIndex')
    return this[SRC].read('getKey', query, 'primaryKey')
  }

  getAll(_query?: unknown, _count?: number): IDBRequest {
    return this[SRC].readAll('getAll', arguments, 'value')
  }

  getAllKeys(_query?: unknown, _count?: number): IDBRequest {
    return this[SRC].readAll('getAllKeys', arguments, 'primaryKey')
  }

  getAllRecords(_options?: unknown): IDBRequest {
    return this[SRC].readAll('getAllRecords', arguments, 'record', true)
  }

  count(query?: unknown): IDBRequest {
    return this[SRC].count(query)
  }

  openCursor(query?: unknown, dir?: unknown): IDBRequest {
    return this[SRC].cursor('openCursor', query, dir, true)
  }

  openKeyCursor(query?: unknown, dir?: unknown): IDBRequest {
    return this[SRC].cursor('openKeyCursor', query, dir, false)
  }
}
tag(IDBIndex)

class CursorCore {
  readonly cursor: IDBCursor
  request: IDBRequest | null = null
  position: Key | null = null
  storePosition: Key | null = null
  key: Key | null = null
  primaryKey: Key | null = null
  valueBytes: Uint8Array | null = null
  keyValue: { value: unknown } | null = null
  primaryValue: { value: unknown } | null = null
  value: { value: unknown } | null = null
  gotValue = false

  constructor(readonly source: SourceCore, readonly direction: Direction, readonly bounds: Bounds, readonly keyOnly: boolean) {
    this.cursor = keyOnly ? new IDBCursor(KEY, this) : new IDBCursorWithValue(KEY, this)
  }

  step(target: Key | null, targetPrimary: Key | null, count: number): void {
    const source = this.source
    const { db, tx } = source.ids()
    const position = this.position
    const storePosition = source.isIndex ? this.storePosition : null
    source.txn.request(source.handle, (op) => native.cursor(db, tx, op, source.isIndex, source.sourceId(), ...rangeArgs(this.bounds), DIRECTIONS.indexOf(this.direction), position, storePosition, target, targetPrimary, count, !this.keyOnly), (m) => {
      if (m.error) return { error: fromNative(m.error) }
      const record: IdbRecord | undefined = m.records?.[0]
      this.keyValue = null
      this.primaryValue = null
      this.value = null
      if (!record) {
        this.key = null
        this.primaryKey = null
        this.valueBytes = null
        this.gotValue = false
        return { value: null }
      }
      this.position = record.key
      this.storePosition = record.primaryKey
      this.key = record.key
      this.primaryKey = record.primaryKey
      this.valueBytes = record.value
      this.gotValue = true
      return { value: this.cursor }
    }, this.request!)
  }

  ready(method: string): void {
    if (this.source.txn.state !== 'active') throw domError('TransactionInactiveError', `Failed to execute '${method}' on 'IDBCursor': The transaction is not active.`)
    if (this.source.gone()) throw domError('InvalidStateError', `Failed to execute '${method}' on 'IDBCursor': The cursor's source or effective object store has been deleted.`)
    if (!this.gotValue) throw domError('InvalidStateError', `Failed to execute '${method}' on 'IDBCursor': The cursor is being iterated or has iterated past its end.`)
  }

  iterate(key: Key | null, primaryKey: Key | null, count: number): void {
    this.gotValue = false
    this.request![RESET]()
    this.step(key, primaryKey, count)
  }

  writable(method: string): StoreCore {
    if (this.source.txn.state !== 'active') throw domError('TransactionInactiveError', `Failed to execute '${method}' on 'IDBCursor': The transaction is not active.`)
    if (this.source.txn.mode === 'readonly') throw domError('ReadOnlyError', `Failed to execute '${method}' on 'IDBCursor': The record may not be changed inside a read-only transaction.`)
    if (this.source.gone()) throw domError('InvalidStateError', `Failed to execute '${method}' on 'IDBCursor': The cursor's source or effective object store has been deleted.`)
    if (!this.gotValue || this.keyOnly) throw domError('InvalidStateError', `Failed to execute '${method}' on 'IDBCursor': The cursor is being iterated or has iterated past its end, or is a key cursor.`)
    return this.source.storeCore()
  }
}

const CUR = Symbol('cursor')

export class IDBCursor {
  readonly [CUR]: CursorCore

  constructor(...key: unknown[]) {
    illegal(key[0])
    this[CUR] = key[1] as CursorCore
  }

  get source(): unknown {
    return this[CUR].source.handle
  }
  get direction(): string {
    return this[CUR].direction
  }
  get key(): unknown {
    const c = this[CUR]
    if (c.key === null) return undefined
    c.keyValue ??= { value: keys.fromKey(c.key) }
    return c.keyValue.value
  }
  get primaryKey(): unknown {
    const c = this[CUR]
    if (c.primaryKey === null) return undefined
    c.primaryValue ??= { value: keys.fromKey(c.primaryKey) }
    return c.primaryValue.value
  }
  get request(): IDBRequest | null {
    return this[CUR].request
  }

  advance(count: unknown): void {
    requireArgs(arguments.length, 1, 'advance', 'IDBCursor')
    const n = enforceRange(count, 2 ** 32 - 1, 'advance', 'IDBCursor')
    if (n === 0) throw new TypeError("Failed to execute 'advance' on 'IDBCursor': A count argument with value 0 (zero) was supplied, must be greater than 0.")
    const c = this[CUR]
    c.ready('advance')
    c.iterate(null, null, n)
  }

  continue(key?: unknown): void {
    const c = this[CUR]
    c.ready('continue')
    let target: Key | null = null
    if (key !== undefined) {
      target = keys.keyOrThrow(key, "Failed to execute 'continue' on 'IDBCursor': The parameter is not a valid key.")
      const order = keys.compare(target, c.position!)
      const forward = c.direction === 'next' || c.direction === 'nextunique'
      if ((forward && order <= 0) || (!forward && order >= 0)) throw keys.dataError("Failed to execute 'continue' on 'IDBCursor': The parameter is not past the cursor's position.")
    }
    c.iterate(target, null, 1)
  }

  continuePrimaryKey(key: unknown, primaryKey: unknown): void {
    requireArgs(arguments.length, 2, 'continuePrimaryKey', 'IDBCursor')
    const c = this[CUR]
    if (c.source.txn.state !== 'active') throw domError('TransactionInactiveError', "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The transaction is not active.")
    if (c.source.gone()) throw domError('InvalidStateError', "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The cursor's source or effective object store has been deleted.")
    if (!c.source.isIndex) throw domError('InvalidAccessError', "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The cursor's source is not an index.")
    if (c.direction !== 'next' && c.direction !== 'prev') throw domError('InvalidAccessError', "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The cursor's direction is not 'next' or 'prev'.")
    if (!c.gotValue) throw domError('InvalidStateError', "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The cursor is being iterated or has iterated past its end.")
    const k = keys.keyOrThrow(key, "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The parameter is not a valid key.")
    const pk = keys.keyOrThrow(primaryKey, "Failed to execute 'continuePrimaryKey' on 'IDBCursor': The parameter is not a valid key.")
    const order = keys.compare(k, c.position!)
    const orderPrimary = keys.compare(pk, c.storePosition!)
    if (c.direction === 'next' && (order < 0 || (order === 0 && orderPrimary <= 0))) throw keys.dataError("Failed to execute 'continuePrimaryKey' on 'IDBCursor': The parameters are not past the cursor's position.")
    if (c.direction === 'prev' && (order > 0 || (order === 0 && orderPrimary >= 0))) throw keys.dataError("Failed to execute 'continuePrimaryKey' on 'IDBCursor': The parameters are not past the cursor's position.")
    c.iterate(k, pk, 1)
  }

  update(value: unknown): IDBRequest {
    requireArgs(arguments.length, 1, 'update', 'IDBCursor')
    const c = this[CUR]
    return c.writable('update').update(value, c.primaryKey!, c)
  }

  delete(): IDBRequest {
    const c = this[CUR]
    const store = c.writable('delete')
    const pk = c.primaryKey!
    return store.remove({ lower: pk, upper: pk, lowerOpen: false, upperOpen: false }, this)
  }
}
tag(IDBCursor)

export class IDBCursorWithValue extends IDBCursor {
  get value(): unknown {
    const c = this[CUR]
    if (c.valueBytes === null) return undefined
    c.value ??= { value: deserialize(c.valueBytes) }
    return c.value.value
  }
}
tag(IDBCursorWithValue)

export class IDBFactory {
  constructor(...key: unknown[]) {
    illegal(key[0])
  }

  open(name: unknown, version?: unknown): IDBOpenDBRequest {
    requireArgs(arguments.length, 1, 'open', 'IDBFactory')
    const dbName = `${name as string}`
    let wanted: number | undefined
    if (version !== undefined) {
      const n = Number(version)
      if (!Number.isFinite(n) || Math.trunc(n) < 0 || Math.trunc(n) > Number.MAX_SAFE_INTEGER) throw new TypeError("Failed to execute 'open' on 'IDBFactory': Value is outside the 'unsigned long long' value range.")
      wanted = Math.trunc(n)
      if (wanted === 0) throw new TypeError("Failed to execute 'open' on 'IDBFactory': The version provided must not be 0.")
    }
    const request = new IDBOpenDBRequest(KEY)
    const req = id()
    opening.set(req, { request, name: dbName })
    native.open(dbName, req, wanted)
    return request
  }

  deleteDatabase(name: unknown): IDBOpenDBRequest {
    requireArgs(arguments.length, 1, 'deleteDatabase', 'IDBFactory')
    const dbName = `${name as string}`
    const request = new IDBOpenDBRequest(KEY)
    const req = id()
    opening.set(req, { request, name: dbName })
    native.deleteDatabase(dbName, req)
    return request
  }

  databases(): Promise<{ name: string; version: number }[]> {
    try {
      return Promise.resolve(native.databases().map(([name, version]) => ({ name, version })))
    } catch (error) {
      return Promise.reject(domError('UnknownError', String((error as Error)?.message ?? error).replace(/^UnknownError: /, '')))
    }
  }

  cmp(first: unknown, second: unknown): number {
    requireArgs(arguments.length, 2, 'cmp', 'IDBFactory')
    const a = keys.keyOrThrow(first, "Failed to execute 'cmp' on 'IDBFactory': The parameter is not a valid key.")
    const b = keys.keyOrThrow(second, "Failed to execute 'cmp' on 'IDBFactory': The parameter is not a valid key.")
    return keys.compare(a, b)
  }
}
tag(IDBFactory)

function onEvent(event: IdbEvent): void {
  switch (event.type) {
    case 'opened': {
      const pending = opening.get(event.req)
      opening.delete(event.req)
      if (!pending) return
      const c = new Connection(pending.name, event.conn, event.db, event.schema)
      pending.request[SETTLE](c.db, null)
      fire(pending.request, new Event('success', { bubbles: false, cancelable: false }))
      return
    }
    case 'upgrade': {
      const pending = opening.get(event.req)
      opening.delete(event.req)
      if (!pending) return
      const { request } = pending
      const c = new Connection(pending.name, event.conn, event.db, event.schema)
      const txn = Txn.upgrade(c, event.tx, request, event.old)
      request[RESET](null, txn.tx)
      request[SETTLE](c.db, null)
      const before = listenerThrows()
      fire(request, new IDBVersionChangeEvent('upgradeneeded', { oldVersion: event.old, newVersion: event.schema.version }))
      if (txn.state === 'active') {
        txn.state = 'inactive'
        if (listenerThrows() !== before) txn.fail(domError('AbortError', 'An exception was thrown in an upgradeneeded handler.'), true)
      }
      touched.add(txn)
      return
    }
    case 'openFailed': {
      const pending = opening.get(event.req)
      opening.delete(event.req)
      if (!pending) return
      pending.request[SETTLE](undefined, fromNative(event.error))
      fire(pending.request, new Event('error', { bubbles: true, cancelable: true }))
      return
    }
    case 'blocked': {
      const pending = opening.get(event.req)
      if (pending) fire(pending.request, new IDBVersionChangeEvent('blocked', { oldVersion: event.old, newVersion: event.new }))
      return
    }
    case 'deleted': {
      const pending = opening.get(event.req)
      opening.delete(event.req)
      if (!pending) return
      pending.request[SETTLE](undefined, null)
      fire(pending.request, new IDBVersionChangeEvent('success', { oldVersion: event.old, newVersion: null }))
      return
    }
    case 'versionchange': {
      const c = connections.get(event.conn)
      if (!c) return
      if (!c.closePending) fire(c.db, new IDBVersionChangeEvent('versionchange', { oldVersion: event.old, newVersion: event.new }))
      acks.push(() => native.acknowledge(c.handle, event.conn))
      return
    }
    case 'forcedClose': {
      const c = connections.get(event.conn)
      if (!c) return
      c.closePending = true
      connections.delete(event.conn)
      for (const txn of [...c.live]) txn.fail(domError('UnknownError', 'The database connection was closed.'), false)
      fire(c.db, new Event('close'))
      return
    }
    case 'started':
      transactions.get(event.tx)?.begin(event.generators)
      return
    case 'result':
      transactions.get(event.tx)?.result(event)
      return
    case 'complete':
      transactions.get(event.tx)?.completed()
      return
    case 'aborted': {
      const ack = abortAcks.get(event.tx)
      if (ack) {
        abortAcks.delete(event.tx)
        ack()
        return
      }
      transactions.get(event.tx)?.fail(event.error ? fromNative(event.error) : null, false)
      return
    }
  }
}

function deactivateCreated(): void {
  if (created.size === 0) return
  const list = [...created]
  created.clear()
  for (const txn of list) txn.deactivate()
}

export function idbTaskEnded(): void {
  deactivateCreated()
  for (const ack of acks.splice(0)) ack()
  if (touched.size === 0) return
  const list = [...touched]
  touched.clear()
  for (const txn of list) txn.taskEnded()
}

export function installIndexedDB(idb: IdbNative | undefined): Record<string, unknown> {
  const classes = { IDBFactory, IDBDatabase, IDBTransaction, IDBObjectStore, IDBIndex, IDBCursor, IDBCursorWithValue, IDBKeyRange, IDBRequest, IDBOpenDBRequest, IDBVersionChangeEvent, IDBRecord, DOMStringList }
  if (!idb) return classes
  native = idb
  idb.onEvent(onEvent)
  hooks.checkpoint = deactivateCreated
  return { ...classes, indexedDB: new IDBFactory(KEY) }
}
