import { defineHandlers, Event, EventTarget, fire, MessageEvent } from './event'
import { DOMException } from './exception'
import { host, illegal, KEY, report, tag } from './host'

export function cloneFailure(error: unknown): unknown {
  const message = (error as Error | null)?.message
  if (error instanceof Error && !(error instanceof DOMException) && typeof message === 'string' && message.startsWith('DataCloneError: ')) {
    return new DOMException(message.slice('DataCloneError: '.length), 'DataCloneError')
  }
  return error
}

export function transferList(options: unknown): unknown[] {
  if (options === undefined || options === null) return []
  const list = typeof options === 'object' && Symbol.iterator in options ? options : (options as { transfer?: unknown }).transfer
  if (list === undefined) return []
  if (list === null || typeof list !== 'object' || !(Symbol.iterator in list)) throw new TypeError("Failed to execute 'postMessage': The transfer list is not a sequence.")
  const items = Array.from(list as Iterable<unknown>)
  if (items.some((item) => item === null || (typeof item !== 'object' && typeof item !== 'function'))) {
    throw new TypeError("Failed to execute 'postMessage': Failed to convert a transfer list value to 'object'.")
  }
  return items
}

export function structuredClone<T>(value: T, options?: { transfer?: unknown }): T {
  if (arguments.length < 1) throw new TypeError("Failed to execute 'structuredClone' on 'Window': 1 argument required, but only 0 present.")
  const list = options?.transfer === undefined ? [] : Array.from(options.transfer as Iterable<unknown>)
  const buffers: ArrayBuffer[] = []
  for (const item of list) {
    if (item instanceof MessagePort) continue
    if (!(item instanceof ArrayBuffer)) throw new DOMException('Value at index ' + list.indexOf(item) + ' does not have a transferable type.', 'DataCloneError')
    if (buffers.includes(item)) throw new DOMException('ArrayBuffer at index ' + list.indexOf(item) + ' is a duplicate of an earlier ArrayBuffer.', 'DataCloneError')
    if ((item as { detached?: boolean }).detached) throw new DOMException('An ArrayBuffer is detached and could not be cloned.', 'DataCloneError')
    buffers.push(item)
  }
  let result: T
  try {
    result = host.deserialize(host.serialize(value)) as T
  } catch (error) {
    throw cloneFailure(error)
  }
  for (const buffer of buffers) (buffer as ArrayBuffer & { transfer(): ArrayBuffer }).transfer()
  return result
}

const UNREADABLE = Symbol('unreadable')

let entangle: (a: MessagePort, b: MessagePort) => void
let sinkInto: (port: MessagePort, sink: (data: unknown) => void) => void
let receive: (port: MessagePort, data: unknown) => void
let disentangled: (port: MessagePort) => void

export class MessagePort extends EventTarget {
  #other: MessagePort | null = null
  #started = false
  #closed = false
  #queue: unknown[] = []
  #sink: ((data: unknown) => void) | null = null

  constructor(...key: unknown[]) {
    illegal(key[0])
    super()
  }

  static {
    entangle = (a, b) => {
      a.#other = b
      b.#other = a
    }
    sinkInto = (port, sink) => {
      port.#sink = sink
    }
    receive = (port, data) => {
      if (port.#closed) return
      if (!port.#started) return void port.#queue.push(data)
      host.setTimer(0, () => {
        if (port.#closed) return
        try {
          fire(port, data === UNREADABLE ? new MessageEvent('messageerror') : new MessageEvent('message', { data }))
        } catch (error) {
          report(error)
        }
      })
    }
    disentangled = (port) => {
      host.setTimer(0, () => {
        try {
          fire(port, new Event('close'))
        } catch (error) {
          report(error)
        }
      })
    }
  }

  postMessage(message: unknown, transfer?: unknown): void {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'postMessage' on 'MessagePort': 1 argument required, but only 0 present.")
    const data = structuredClone(message, { transfer: transferList(transfer) })
    const other = this.#other
    if (!other || this.#closed) return
    if (other.#sink) other.#sink(data)
    else receive(other, data)
  }

  start(): void {
    if (this.#started) return
    this.#started = true
    const queued = this.#queue
    this.#queue = []
    for (const data of queued) receive(this, data)
  }

  close(): void {
    this.#closed = true
    const other = this.#other
    this.#other = null
    if (!other) return
    other.#other = null
    disentangled(other)
  }

  get onmessage(): unknown {
    return this.#onmessage
  }
  set onmessage(value: unknown) {
    if (this.#onmessage) this.removeEventListener('message', this.#onmessage)
    this.#onmessage = typeof value === 'function' ? (value as (e: unknown) => void) : null
    if (this.#onmessage) {
      this.addEventListener('message', this.#onmessage)
      this.start()
    }
  }
  #onmessage: ((event: unknown) => void) | null = null
}
defineHandlers(MessagePort.prototype, ['messageerror', 'close'])
tag(MessagePort)

export class MessageChannel {
  readonly port1 = new MessagePort(KEY)
  readonly port2 = new MessagePort(KEY)

  constructor() {
    entangle(this.port1, this.port2)
  }
}
tag(MessageChannel)

export const PORT_LIMIT = 262144

export interface PortLine {
  send(line: string): void
  closed(): void
}

export interface LinePort {
  readonly port: MessagePort
  receive(line: string | null): void
  close(): void
}

export function linePort(line: PortLine): LinePort {
  const port = new MessagePort(KEY)
  const inner = new MessagePort(KEY)
  entangle(port, inner)
  let open = true
  sinkInto(inner, (data) => line.send(jsonLine(data)))
  inner.addEventListener('close', () => {
    open = false
    line.closed()
  })
  return {
    port,
    receive: (text) => {
      if (!open) return
      const data = text === null || utf8Length(text) > PORT_LIMIT ? UNREADABLE : parsed(text)
      if (data === UNREADABLE) receive(port, UNREADABLE)
      else inner.postMessage(data)
    },
    close: () => {
      open = false
      inner.close()
    },
  }
}

function parsed(text: string): unknown {
  try {
    return JSON.parse(text)
  } catch {
    return UNREADABLE
  }
}

function jsonLine(data: unknown): string {
  const why = notJson(data, new Set())
  if (why) throw new DOMException(`this port carries JSON only (${why})`, 'DataCloneError')
  const text = JSON.stringify(data)
  const size = utf8Length(text)
  if (size > PORT_LIMIT) throw new DOMException(`message is ${size} bytes, over the ${PORT_LIMIT}-byte limit of this port`, 'DataCloneError')
  return text
}

function notJson(value: unknown, seen: Set<object>): string | null {
  if (value === null || typeof value === 'string' || typeof value === 'boolean') return null
  if (typeof value === 'number') return Number.isFinite(value) ? null : `${value} is not a JSON number`
  if (typeof value !== 'object') return `${typeof value} values are not JSON`
  if (seen.has(value)) return 'it refers to itself'
  const proto = Object.getPrototypeOf(value)
  const plain = proto === null || (Object.getPrototypeOf(proto) === null && proto.constructor?.name === 'Object')
  if (!Array.isArray(value) && !plain) return `${proto?.constructor?.name ?? 'this object'} values are not JSON`
  seen.add(value)
  for (const item of Object.values(value)) {
    const why = notJson(item, seen)
    if (why) return why
  }
  seen.delete(value)
  return null
}

function utf8Length(text: string): number {
  let size = 0
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i)
    if (code < 0x80) size += 1
    else if (code < 0x800) size += 2
    else if (code >= 0xd800 && code < 0xdc00 && i + 1 < text.length) {
      size += 4
      i++
    } else size += 3
  }
  return size
}

export class BroadcastChannel extends EventTarget {
  #closed = false

  constructor(readonly name: string) {
    super()
  }

  postMessage(message: unknown): void {
    if (this.#closed) throw new DOMException('Channel is closed', 'InvalidStateError')
    structuredClone(message)
  }

  close(): void {
    this.#closed = true
  }
}
defineHandlers(BroadcastChannel.prototype, ['message', 'messageerror'])
tag(BroadcastChannel)
