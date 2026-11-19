import { defineHandlers, ErrorEvent, Event, EventTarget, fire, MessageEvent } from './event'
import { backendPort } from './backend'
import { DOMException } from './exception'
import { G, host, report, tag } from './host'
import { cloneFailure, MessagePort, transferList } from './message'
import { moduleSource, net, scriptSource } from './net'
import { URL } from './url'

export interface WorkersNative {
  spawn(id: number, name: string): void
  load(id: number, url: string, source: string, module: boolean, env: string): void
  post(id: number, message: unknown, transfer: unknown[]): void
  terminate(id: number): void
  onEvent(fn: (id: number, kind: string, payload: unknown) => void): void
}

export interface WorkerEnv {
  readonly url: string
  readonly name: string
  readonly origin: string
  readonly userAgent: string
  readonly platform: string
  readonly language: string
  readonly languages: readonly string[]
  readonly hardwareConcurrency: number
  readonly backendPort: number | null
}

const RECEIVE = Symbol('receive')
const live = new Map<number, Worker>()
let native: WorkersNative | null = null
let nextId = 1

export function installWorkers(workers: WorkersNative | undefined): void {
  native = workers ?? null
  native?.onEvent((id, kind, payload) => live.get(id)?.[RECEIVE](kind, payload))
}

function environment(url: string, name: string): WorkerEnv {
  const navigator = (G.navigator ?? {}) as Partial<WorkerEnv>
  const parsed = new URL(url)
  return {
    url,
    name,
    origin: parsed.protocol === 'data:' ? 'null' : parsed.protocol === 'blob:' ? new URL(net.base()).origin : parsed.origin,
    userAgent: String(navigator.userAgent ?? ''),
    platform: String(navigator.platform ?? ''),
    language: String(navigator.language ?? 'en-US'),
    languages: Array.from(navigator.languages ?? ['en-US'], String),
    hardwareConcurrency: Number(navigator.hardwareConcurrency ?? 4),
    backendPort: backendPort(),
  }
}

export class Worker extends EventTarget {
  readonly #id: number
  #terminated = false

  constructor(scriptURL: string | URL, options?: { type?: string; name?: string; credentials?: string }) {
    const what = "Failed to construct 'Worker'"
    if (arguments.length < 1) throw new TypeError(`${what}: 1 argument required, but only 0 present.`)
    super()
    const type = options?.type === undefined ? 'classic' : String(options.type)
    if (type !== 'classic' && type !== 'module') throw new TypeError(`${what}: Failed to read the 'type' property from 'WorkerOptions': The provided value '${type}' is not a valid enum value of type WorkerType.`)
    const name = options?.name === undefined ? '' : String(options.name)
    let url: URL
    try {
      url = new URL(String(scriptURL), net.base())
    } catch {
      throw new DOMException(`${what}: The script URL ${String(scriptURL)} is invalid.`, 'SyntaxError')
    }
    const own = new URL(net.base()).origin
    if (url.protocol !== 'blob:' && url.protocol !== 'data:' && url.origin !== own) {
      throw new DOMException(`${what}: Script at '${url.href}' cannot be accessed from origin '${own}'.`, 'SecurityError')
    }
    if (!native) throw new DOMException(`${what}: workers are not available in this web.`, 'NotSupportedError')
    this.#id = nextId++
    live.set(this.#id, this)
    native.spawn(this.#id, name)
    void this.#load(url.href, type === 'module', name)
  }

  async #load(url: string, module: boolean, name: string): Promise<void> {
    let source: string
    try {
      const inline = module && (url.startsWith('blob:') || url.startsWith('data:'))
      source = inline ? (moduleSource(url) ?? (await scriptSource(url))) : await scriptSource(url)
    } catch (error) {
      if (this.#terminated) return
      host.log(2, 'worker', `${url}: ${String((error as Error | null)?.message ?? error)}`)
      this.terminate()
      fire(this, new Event('error', { cancelable: true }))
      return
    }
    if (this.#terminated) return
    native!.load(this.#id, url, source, module, JSON.stringify(environment(url, name)))
  }

  postMessage(message: unknown, options?: unknown): void {
    if (arguments.length < 1) throw new TypeError("Failed to execute 'postMessage' on 'Worker': 1 argument required, but only 0 present.")
    const transfer = transferList(options)
    if (transfer.some((item) => item instanceof MessagePort)) {
      throw new DOMException("Failed to execute 'postMessage' on 'Worker': transferring a MessagePort to a worker is not supported yet.", 'DataCloneError')
    }
    if (this.#terminated) return
    try {
      native!.post(this.#id, message, transfer)
    } catch (error) {
      throw cloneFailure(error)
    }
  }

  terminate(): void {
    if (this.#terminated) return
    this.#end()
    native?.terminate(this.#id)
  }

  #end(): void {
    this.#terminated = true
    live.delete(this.#id)
  }

  [RECEIVE](kind: string, payload: unknown): void {
    if (kind === 'message') {
      fire(this, new MessageEvent('message', { data: payload }))
    } else if (kind === 'messageerror') {
      fire(this, new MessageEvent('messageerror'))
    } else if (kind === 'error') {
      const { message, filename, lineno, colno } = payload as { message: string; filename: string; lineno: number; colno: number }
      const event = new ErrorEvent('error', { message, filename, lineno, colno, cancelable: true })
      if (fire(this, event)) {
        const error = new Error(message)
        Object.defineProperty(error, 'stack', { value: `Error: ${message}\n    at ${filename || 'worker'}:${lineno}:${colno}` })
        report(error)
      }
    } else if (kind === 'exit') {
      this.#end()
    }
  }
}
defineHandlers(Worker.prototype, ['message', 'messageerror', 'error'])
tag(Worker)
