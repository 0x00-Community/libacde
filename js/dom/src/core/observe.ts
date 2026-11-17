export type ConsoleLevel = 'log' | 'info' | 'warn' | 'error' | 'debug'
export type PageErrorKind = 'error' | 'rejection'
export type RequestKind = 'fetch' | 'xhr' | 'websocket' | 'eventsource' | 'image' | 'stylesheet' | 'font'

export type RequestEvent =
  | {
      readonly phase: 'start'
      readonly id: number
      readonly kind: RequestKind
      readonly method: string
      readonly url: string
      readonly bytes: number
      readonly mocked?: true
    }
  | { readonly phase: 'response'; readonly id: number; readonly status: number; readonly statusText: string }
  | { readonly phase: 'end'; readonly id: number; readonly bytes: number }
  | { readonly phase: 'error'; readonly id: number; readonly message: string }

export type MockReply =
  | { readonly status: number; readonly statusText: string; readonly headers: readonly (readonly [string, string])[]; readonly body: Uint8Array }
  | { readonly error: string }

export type DialogType = 'alert' | 'confirm' | 'prompt'

export interface DialogEvent {
  readonly type: DialogType
  readonly message: string
  readonly defaultValue: string | null
}

export interface DialogAnswer {
  readonly accept: boolean
  readonly text?: string
}

export type NetworkHeaders = readonly (readonly [string, string])[]

export interface NetworkObserver {
  recording(): boolean
  start(id: number, kind: RequestKind, method: string, url: string, headers: NetworkHeaders, body: Uint8Array | null): void
  response(id: number, status: number, statusText: string, headers: NetworkHeaders, fromCache?: boolean): void
  data(id: number, chunk: Uint8Array): void
  end(id: number, bytes: number): void
  error(id: number, message: string): void
  frame?(id: number, sent: boolean, data: string | Uint8Array): void
  message?(id: number, event: string, data: string, lastId: string): void
}

export interface Observer {
  console?(level: ConsoleLevel, text: string): void
  error?(kind: PageErrorKind, text: string): void
  request?(event: RequestEvent): void
  route?(kind: 'fetch' | 'xhr', method: string, url: string): MockReply | null
  dialog?(event: DialogEvent): DialogAnswer | null
  readonly network?: NetworkObserver | null
}

export let observer: Observer | null = null

export function observe(next: Observer | null): void {
  observer = next
}

export function mocked(kind: 'fetch' | 'xhr', method: string, url: string): MockReply | null {
  return observer?.route?.call(observer, kind, method, url) ?? null
}

export function dialogAnswer(type: DialogType, message: string, defaultValue: string | null): DialogAnswer | null {
  return observer?.dialog?.call(observer, { type, message, defaultValue }) ?? null
}

let nextRequest = 1

export class Tracked {
  readonly id = nextRequest++
  #received = 0
  #done = false

  private constructor(
    readonly sink: (event: RequestEvent) => void,
    readonly network: NetworkObserver | null,
  ) {}

  static start(
    kind: RequestKind,
    method: string,
    url: string,
    bytes = 0,
    mocked = false,
    headers?: Iterable<readonly [string, string]>,
    body?: Uint8Array | null,
  ): Tracked | null {
    const sink = observer?.request
    const network = observer?.network?.recording() ? observer.network : null
    if (!sink && !network) return null
    const tracked = new Tracked((event) => sink?.call(observer, event), network)
    tracked.sink({ phase: 'start', id: tracked.id, kind, method, url, bytes, ...(mocked && { mocked: true as const }) })
    network?.start(tracked.id, kind, method, url, headers ? [...headers] : [], body ?? null)
    return tracked
  }

  static network(kind: RequestKind, method: string, url: string, headers: Iterable<readonly [string, string]> = []): Tracked | null {
    const network = observer?.network?.recording() ? observer.network : null
    if (!network) return null
    const tracked = new Tracked(() => {}, network)
    network.start(tracked.id, kind, method, url, [...headers], null)
    return tracked
  }

  response(status: number, statusText: string, headers: NetworkHeaders = [], fromCache = false): void {
    if (this.#done) return
    this.sink({ phase: 'response', id: this.id, status, statusText })
    this.network?.response(this.id, status, statusText, headers, fromCache)
  }

  received(bytes: number, data?: Uint8Array): void {
    this.#received += bytes
    if (data && !this.#done) this.network?.data(this.id, data)
  }

  frame(sent: boolean, data: string | Uint8Array): void {
    if (!this.#done) this.network?.frame?.(this.id, sent, data)
  }

  message(event: string, data: string, lastId: string): void {
    if (!this.#done) this.network?.message?.(this.id, event, data, lastId)
  }

  end(bytes = this.#received): void {
    if (this.#done) return
    this.#done = true
    this.sink({ phase: 'end', id: this.id, bytes })
    this.network?.end(this.id, bytes)
  }

  error(message: string): void {
    if (this.#done) return
    this.#done = true
    this.sink({ phase: 'error', id: this.id, message })
    this.network?.error(this.id, message)
  }
}
