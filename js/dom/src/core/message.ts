import { defineHandlers, EventTarget, fire, MessageEvent } from './event'
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
  if (typeof options === 'object' && Symbol.iterator in options) return Array.from(options as Iterable<unknown>)
  const list = (options as { transfer?: unknown }).transfer
  return list === undefined ? [] : Array.from(list as Iterable<unknown>)
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

let entangle: (a: MessagePort, b: MessagePort) => void
let receive: (port: MessagePort, data: unknown) => void

export class MessagePort extends EventTarget {
  #other: MessagePort | null = null
  #started = false
  #closed = false
  #queue: unknown[] = []

  constructor(...key: unknown[]) {
    illegal(key[0])
    super()
  }

  static {
    entangle = (a, b) => {
      a.#other = b
      b.#other = a
    }
    receive = (port, data) => {
      if (port.#closed) return
      if (!port.#started) return void port.#queue.push(data)
      host.setTimer(0, () => {
        if (port.#closed) return
        try {
          fire(port, new MessageEvent('message', { data }))
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
    receive(other, data)
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
    if (this.#other) this.#other.#other = null
    this.#other = null
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
defineHandlers(MessagePort.prototype, ['messageerror'])
tag(MessagePort)

export class MessageChannel {
  readonly port1 = new MessagePort(KEY)
  readonly port2 = new MessagePort(KEY)

  constructor() {
    entangle(this.port1, this.port2)
  }
}
tag(MessageChannel)

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
