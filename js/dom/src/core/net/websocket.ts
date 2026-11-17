import { Blob, blobBytes } from '../blob'
import { cookies, dialOf, isRemote, requestHeaders, transport, type Target, type WsEvent } from '../backend'
import { Tracked } from '../observe'
import { bytesOf } from '../encoding'
import { CloseEvent, defineHandlers, Event, EventTarget, fire, MessageEvent } from '../event'
import { DOMException } from '../exception'
import { constants, host, tag } from '../host'
import { URL } from '../url'
import { backendFailed, browserHeaders, net, route, unreachable } from './index'

const WS_STATES = { CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 } as const

export class WebSocket extends EventTarget {
  readonly url: string
  readyState = 0
  protocol = ''
  extensions = ''
  bufferedAmount = 0
  readonly withCredentials = false
  #binaryType: 'blob' | 'arraybuffer' = 'blob'
  #id: number | null = null
  #finished = false
  #tracked: Tracked | null = null
  readonly #origin: string

  constructor(url: string | URL, protocols?: string | Iterable<string>) {
    super()
    let parsed: URL
    try {
      parsed = new URL(String(url), net.base())
    } catch {
      throw new DOMException(`Failed to construct 'WebSocket': The URL '${String(url)}' is invalid.`, 'SyntaxError')
    }
    if (parsed.protocol === 'http:' || parsed.protocol === 'https:') parsed = new URL(`ws${parsed.href.slice(4)}`)
    if (parsed.protocol !== 'ws:' && parsed.protocol !== 'wss:') {
      const what = `The URL's scheme must be either 'http', 'https', 'ws', or 'wss'. '${parsed.protocol}' is not allowed.`
      throw new DOMException(`Failed to construct 'WebSocket': ${what}`, 'SyntaxError')
    }
    if (parsed.hash) {
      const what = `The URL contains a fragment identifier ('${parsed.hash}'). Fragment identifiers are not allowed in WebSocket URLs.`
      throw new DOMException(`Failed to construct 'WebSocket': ${what}`, 'SyntaxError')
    }
    this.url = parsed.href
    this.#origin = parsed.origin
    const list = protocols === undefined ? [] : typeof protocols === 'string' ? [protocols] : [...protocols].map(String)
    if (new Set(list).size !== list.length) throw new DOMException("Failed to construct 'WebSocket': The subprotocol is duplicated.", 'SyntaxError')
    this.#tracked = Tracked.start('websocket', 'GET', this.url)
    const routed = route(this.url, 'WS')
    if (!routed || !('target' in routed)) {
      this.#tracked?.error(unreachable(this.url))
      host.setTimer(0, () => this.#finish(1006, '', false))
      return
    }
    this.#connect(routed.target, list)
  }

  #connect(target: Target, protocols: string[]): void {
    const headers = isRemote(target)
      ? browserHeaders([['origin', new URL(net.base()).origin]], false)
      : requestHeaders([], target, { origin: true, cookies: true })
    const onEvent = (event: WsEvent): void => {
      if (this.#finished) return
      switch (event.type) {
        case 'open':
          if (this.readyState !== WS_STATES.CONNECTING) return
          this.readyState = WS_STATES.OPEN
          this.protocol = event.protocol ?? ''
          this.extensions = event.extensions ?? ''
          this.#tracked?.response(101, 'Switching Protocols')
          fire(this, new Event('open'))
          return
        case 'message': {
          if (this.readyState !== WS_STATES.OPEN) return
          this.#tracked?.received(event.text !== undefined ? event.text.length : (event.data?.length ?? 0))
          this.#tracked?.frame(false, event.text ?? event.data ?? new Uint8Array(0))
          let data: unknown
          if (event.text !== undefined) data = event.text
          else {
            const bytes = event.data ?? new Uint8Array(0)
            data = this.#binaryType === 'arraybuffer' ? bytes.slice().buffer : new Blob([bytes])
          }
          fire(this, new MessageEvent('message', { data, origin: this.#origin }))
          return
        }
        case 'close':
          return this.#finish(event.code ?? 1005, event.reason ?? '', !!event.wasClean)
        case 'error':
          backendFailed('WS', this.url, event.message)
          this.#tracked?.error(event.message)
          return this.#finish(1006, '', false)
      }
    }
    try {
      const [to, path] = dialOf(target)
      this.#id = transport!.websocket(to, path, headers, protocols, onEvent)
    } catch (error) {
      backendFailed('WS', this.url, String((error as Error)?.message ?? error))
      this.#tracked?.error(String((error as Error)?.message ?? error))
      host.queueMicrotask(() => this.#finish(1006, '', false))
    }
  }

  #finish(code: number, reason: string, wasClean: boolean): void {
    if (this.#finished) return
    this.#finished = true
    if (wasClean) this.#tracked?.end()
    else this.#tracked?.error(`closed ${code}${reason ? ` ${reason}` : ''}`)
    this.readyState = WS_STATES.CLOSED
    if (!wasClean) fire(this, new Event('error'))
    fire(this, new CloseEvent('close', { code, reason, wasClean }))
  }

  get binaryType(): 'blob' | 'arraybuffer' {
    return this.#binaryType
  }
  set binaryType(value: string) {
    if (value === 'blob' || value === 'arraybuffer') this.#binaryType = value
  }

  send(data: string | ArrayBuffer | ArrayBufferView | Blob): void {
    if (this.readyState === WS_STATES.CONNECTING) {
      throw new DOMException("Failed to execute 'send' on 'WebSocket': Still in CONNECTING state.", 'InvalidStateError')
    }
    let payload: string | Uint8Array
    if (typeof data === 'string') payload = data
    else if (data instanceof Blob) payload = blobBytes(data).slice()
    else if (typeof data === 'object' && data !== null) payload = bytesOf(data as ArrayBuffer).slice()
    else payload = String(data)
    if (this.readyState !== WS_STATES.OPEN || this.#id === null) {
      this.bufferedAmount += typeof payload === 'string' ? host.encodeUtf8(payload).length : payload.length
      return
    }
    transport?.send(this.#id, payload)
    this.#tracked?.frame(true, payload)
  }

  close(code?: number, reason?: string): void {
    if (code !== undefined) {
      code = Number(code)
      if (code !== 1000 && !(code >= 3000 && code <= 4999)) {
        const what = `The close code must be either 1000, or between 3000 and 4999. ${code} is neither.`
        throw new DOMException(`Failed to execute 'close' on 'WebSocket': ${what}`, 'InvalidAccessError')
      }
    }
    if (reason !== undefined && host.encodeUtf8(String(reason)).length > 123) {
      throw new DOMException("Failed to execute 'close' on 'WebSocket': The close reason must not be greater than 123 UTF-8 bytes.", 'SyntaxError')
    }
    if (this.readyState === WS_STATES.CLOSING || this.readyState === WS_STATES.CLOSED) return
    if (this.#id === null) {
      this.readyState = WS_STATES.CLOSED
      return
    }
    this.readyState = WS_STATES.CLOSING
    transport?.close(this.#id, code, reason === undefined ? undefined : String(reason))
  }
}
constants(WebSocket, WS_STATES)
defineHandlers(WebSocket.prototype, ['open', 'message', 'error', 'close'])
tag(WebSocket)
