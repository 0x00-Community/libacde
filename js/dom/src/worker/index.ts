import type { HostNative } from '../native'
import { installCore } from '../core'
import { installBackendTransport, type BackendTransport } from '../core/backend'
import { defineHandlers, ErrorEvent, EventTarget, fire, MessageEvent, PromiseRejectionEvent } from '../core/event'
import { DOMException } from '../core/exception'
import { globals, host, illegal, KEY, setReport, tag } from '../core/host'
import { cloneFailure, MessagePort, transferList } from '../core/message'
import { fetch, moduleSource, net, Request, Response, route, unreachable } from '../core/net'
import { EventSource } from '../core/net/eventsource'
import { Headers } from '../core/net/headers'
import { WebSocket } from '../core/net/websocket'
import { XMLHttpRequest, XMLHttpRequestEventTarget, XMLHttpRequestUpload } from '../core/net/xhr'
import {
  ReadableStream,
  ReadableStreamDefaultController,
  ReadableStreamDefaultReader,
  TextDecoderStream,
  TextEncoderStream,
  TransformStream,
  TransformStreamDefaultController,
  WritableStream,
  WritableStreamDefaultController,
  WritableStreamDefaultWriter,
} from '../core/streams'
import { objectUrlOrigin, URL } from '../core/url'
import type { WorkerEnv } from '../core/worker'

export interface WorkerSideNative {
  post(message: unknown, transfer: unknown[]): void
  onEvent(fn: (kind: string, payload: unknown) => void): void
  close(): void
  error(message: string, filename: string, lineno: number, colno: number): void
}

const ENV = Symbol('env')

type ScopeEnv = WorkerEnv & { readonly module: boolean }

export class WorkerLocation {
  readonly #url: URL

  constructor(token: unknown, href: string) {
    illegal(token)
    this.#url = new URL(href)
  }

  get href(): string {
    return this.#url.href
  }
  get origin(): string {
    return this.#url.origin
  }
  get protocol(): string {
    return this.#url.protocol
  }
  get host(): string {
    return this.#url.host
  }
  get hostname(): string {
    return this.#url.hostname
  }
  get port(): string {
    return this.#url.port
  }
  get pathname(): string {
    return this.#url.pathname
  }
  get search(): string {
    return this.#url.search
  }
  get hash(): string {
    return this.#url.hash
  }
  toString(): string {
    return this.#url.href
  }
}
tag(WorkerLocation)

export class WorkerNavigator {
  readonly [ENV]: WorkerEnv

  constructor(token: unknown, env: WorkerEnv) {
    illegal(token)
    this[ENV] = env
  }

  get userAgent(): string {
    return this[ENV].userAgent
  }
  get appVersion(): string {
    return this[ENV].userAgent.replace(/^Mozilla\//, '')
  }
  get appName(): string {
    return 'Netscape'
  }
  get appCodeName(): string {
    return 'Mozilla'
  }
  get product(): string {
    return 'Gecko'
  }
  get platform(): string {
    return this[ENV].platform
  }
  get language(): string {
    return this[ENV].language
  }
  get languages(): readonly string[] {
    return Object.freeze([...this[ENV].languages])
  }
  get hardwareConcurrency(): number {
    return this[ENV].hardwareConcurrency
  }
  get onLine(): boolean {
    return true
  }
}
tag(WorkerNavigator)

export class WorkerGlobalScope extends EventTarget {
  constructor(token?: unknown) {
    illegal(token)
    super()
  }
}
defineHandlers(WorkerGlobalScope.prototype, ['error', 'languagechange', 'offline', 'online', 'rejectionhandled', 'unhandledrejection'])
tag(WorkerGlobalScope)

export class DedicatedWorkerGlobalScope extends WorkerGlobalScope {}
defineHandlers(DedicatedWorkerGlobalScope.prototype, ['message', 'messageerror'])
tag(DedicatedWorkerGlobalScope)

function location(stack: string | undefined): { filename: string; lineno: number; colno: number } {
  const at = /\(?((?:https?|blob|data|file|acde-runtime):[^\s)]*?):(\d+):(\d+)\)?\s*$/m.exec(stack ?? '')
  return at ? { filename: at[1]!, lineno: Number(at[2]), colno: Number(at[3]) } : { filename: '', lineno: 0, colno: 0 }
}

export interface WorkerScope {
  load(env: ScopeEnv, source: string): Promise<void>
}

export function installWorkerScope(
  global: typeof globalThis,
  hostNative: HostNative,
  worker: WorkerSideNative,
  backend: Omit<BackendTransport, 'port'> | undefined,
): WorkerScope {
  installCore(global, hostNative)
  Object.setPrototypeOf(global, DedicatedWorkerGlobalScope.prototype)
  let env: ScopeEnv | null = null
  let workerLocation: WorkerLocation | null = null
  let workerNavigator: WorkerNavigator | null = null
  let closing = false
  const self = () => global
  for (const [name, get] of [
    ['self', self],
    ['location', () => workerLocation],
    ['navigator', () => workerNavigator],
    ['name', () => env?.name ?? ''],
  ] as const) {
    Object.defineProperty(global, name, { get, configurable: true, enumerable: true })
  }
  globals(global, {
    WorkerGlobalScope, DedicatedWorkerGlobalScope, WorkerLocation, WorkerNavigator,
    fetch, Headers, Request, Response, XMLHttpRequest, XMLHttpRequestEventTarget, XMLHttpRequestUpload, WebSocket, EventSource,
    ReadableStream, ReadableStreamDefaultReader, ReadableStreamDefaultController, WritableStream, WritableStreamDefaultWriter,
    WritableStreamDefaultController, TransformStream, TransformStreamDefaultController, TextDecoderStream, TextEncoderStream,
    postMessage(message: unknown, options?: unknown): void {
      if (arguments.length < 1) throw new TypeError("Failed to execute 'postMessage' on 'DedicatedWorkerGlobalScope': 1 argument required, but only 0 present.")
      const transfer = transferList(options)
      if (transfer.some((item) => item instanceof MessagePort)) {
        throw new DOMException("Failed to execute 'postMessage' on 'DedicatedWorkerGlobalScope': transferring a MessagePort is not supported yet.", 'DataCloneError')
      }
      try {
        worker.post(message, transfer)
      } catch (error) {
        throw cloneFailure(error)
      }
    },
    close(): void {
      closing = true
      worker.close()
    },
    importScripts(...urls: unknown[]): void {
      const base = workerLocation?.href ?? net.base()
      const resolved = urls.map((url) => {
        try {
          return new URL(String(url), base).href
        } catch {
          throw new DOMException(`Failed to execute 'importScripts' on 'WorkerGlobalScope': The URL '${String(url)}' is invalid.`, 'SyntaxError')
        }
      })
      if (env && resolved.length > 0 && env.module) {
        throw new TypeError("Failed to execute 'importScripts' on 'WorkerGlobalScope': Module scripts don't support importScripts().")
      }
      for (const url of resolved) {
        const routed = route(url, 'GET')
        if (!routed || !('asset' in routed)) {
          throw new DOMException(
            `Failed to execute 'importScripts' on 'WorkerGlobalScope': The script at '${url}' failed to load${routed ? ' (scripts served by the backend are not loaded synchronously yet)' : `: ${unreachable(url)}`}.`,
            'NetworkError',
          )
        }
        host.runScript(host.decode(routed.asset.bytes, 'utf-8', false, false), url)
      }
    },
  })
  hostNative.setModuleSource?.(moduleSource)

  const uncaught = (error: unknown): void => {
    const message = error instanceof Error ? `Uncaught ${error.name}: ${error.message}` : `Uncaught ${String(error)}`
    const where = location((error as { stack?: string } | null)?.stack)
    const event = new ErrorEvent('error', { message, error, cancelable: true, ...where })
    if (fire(global as unknown as EventTarget, event)) worker.error(message, where.filename, where.lineno, where.colno)
  }
  setReport(uncaught)
  ;(hostNative as HostNative & { setReporter?(fn: (error: unknown, promise?: Promise<unknown>) => void): void }).setReporter?.((error, promise) => {
    if (!promise) return uncaught(error)
    const event = new PromiseRejectionEvent('unhandledrejection', { promise, reason: error, cancelable: true })
    if (fire(global as unknown as EventTarget, event)) hostNative.reportError(error)
  })

  let ready = false
  const early: [string, unknown][] = []
  const deliver = (kind: string, payload: unknown): void => {
    if (closing) return
    if (kind === 'message') fire(global as unknown as EventTarget, new MessageEvent('message', { data: payload }))
    else if (kind === 'messageerror') fire(global as unknown as EventTarget, new MessageEvent('messageerror'))
  }
  worker.onEvent((kind, payload) => {
    if (kind === 'load') {
      const { url, source, module, env: given } = payload as { url: string; source: string; module: boolean; env: string }
      void scope.load({ ...(JSON.parse(given) as WorkerEnv), url, module }, source)
      return
    }
    if (!ready) early.push([kind, payload])
    else deliver(kind, payload)
  })

  const scope: WorkerScope = {
    async load(given, source) {
      env = given
      workerLocation = new WorkerLocation(KEY, given.url)
      workerNavigator = new WorkerNavigator(KEY, given)
      objectUrlOrigin.value = given.origin
      net.base = () => given.url
      if (backend) installBackendTransport({ ...backend, port: given.backendPort })
      const flush = (): void => {
        ready = true
        for (const [kind, payload] of early.splice(0)) deliver(kind, payload)
      }
      try {
        if (given.module) {
          const evaluation = host.runModule(source, given.url)
          flush()
          await evaluation
        } else {
          host.runScript(source, given.url)
          flush()
        }
      } catch (error) {
        flush()
        uncaught(error)
      }
    },
  }
  return scope
}
