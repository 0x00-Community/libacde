import { NO_NETWORK, type NetworkHeaders, type NetworkObserver, type RequestKind } from '@dopame/acde-dom'
import { DevToolsError, type DevToolsDomain, type DevToolsPage } from './index'

const TYPES: Record<RequestKind, string> = {
  fetch: 'Fetch',
  xhr: 'XHR',
  websocket: 'WebSocket',
  eventsource: 'EventSource',
  image: 'Image',
  stylesheet: 'Stylesheet',
  font: 'Font',
}
const TEXT = /^(text\/|application\/(json|javascript|ecmascript|xml|xhtml\+xml|x-www-form-urlencoded)|image\/svg\+xml)|\+json$|\+xml$/i

interface Open {
  readonly kind: RequestKind
  readonly type: string
  readonly url: string
  mimeType: string
  chunks: Uint8Array[]
  size: number
  keep: boolean
  readonly start: number
}

interface Kept {
  readonly body: Uint8Array
  readonly mimeType: string
}

export class NetworkDomain {
  static readonly BODIES = 16 << 20

  readonly #open = new Map<number, Open>()
  readonly #bodies = new Map<string, Kept>()
  #kept = 0

  constructor(
    readonly page: DevToolsPage,
    readonly documentUrl: () => string,
    readonly limit = NetworkDomain.BODIES,
    readonly clock: () => number = () => performance.now() / 1000,
  ) {}

  readonly observer: NetworkObserver = {
    recording: () => this.page.listening('Network'),
    start: (id, kind, method, url, headers, body) => this.#start(id, kind, method, url, headers, body),
    response: (id, status, statusText, headers, fromCache) => this.#response(id, status, statusText, headers, fromCache === true),
    data: (id, chunk) => this.#data(id, chunk),
    end: (id, bytes) => this.#end(id, bytes),
    error: (id, message) => this.#error(id, message),
    frame: (id, sent, data) => this.#frame(id, sent, data),
    message: (id, event, data, lastId) => this.#message(id, event, data, lastId),
  }

  domain(): DevToolsDomain {
    return {
      name: 'Network',
      methods: {
        getResponseBody: (params) => this.#body(String(params['requestId'] ?? '')),
      },
    }
  }

  static headers(list: NetworkHeaders): Record<string, string> {
    const out: Record<string, string> = {}
    for (const [name, value] of list) out[name] = name in out ? `${out[name]}\n${value}` : value
    return out
  }

  static mimeType(list: NetworkHeaders): string {
    const type = list.find(([name]) => name.toLowerCase() === 'content-type')?.[1] ?? ''
    return type.split(';')[0]!.trim().toLowerCase()
  }

  static timing(start: number, headers: number): Record<string, number> {
    const unknown = -1
    const received = Math.max(0, (headers - start) * 1000)
    return {
      requestTime: start,
      proxyStart: unknown,
      proxyEnd: unknown,
      dnsStart: unknown,
      dnsEnd: unknown,
      connectStart: unknown,
      connectEnd: unknown,
      sslStart: unknown,
      sslEnd: unknown,
      workerStart: unknown,
      workerReady: unknown,
      workerFetchStart: unknown,
      workerRespondWithSettled: unknown,
      sendStart: 0,
      sendEnd: 0,
      pushStart: 0,
      pushEnd: 0,
      receiveHeadersStart: received,
      receiveHeadersEnd: received,
    }
  }

  static base64(bytes: Uint8Array): string {
    let binary = ''
    for (let at = 0; at < bytes.length; at += 0x8000) binary += String.fromCharCode(...bytes.subarray(at, at + 0x8000))
    return btoa(binary)
  }

  #emit(method: string, params: Record<string, unknown>): void {
    this.page.emit('Network', method, params)
  }

  #live(id: number): Open | null {
    if (!this.page.listening('Network')) {
      this.#open.clear()
      return null
    }
    return this.#open.get(id) ?? null
  }

  #start(id: number, kind: RequestKind, method: string, url: string, headers: NetworkHeaders, body: Uint8Array | null): void {
    if (!this.page.listening('Network')) return
    const type = TYPES[kind]
    const streamed = kind === 'websocket' || kind === 'eventsource'
    const start = this.clock()
    this.#open.set(id, { kind, type, url, mimeType: '', chunks: [], size: 0, keep: !streamed, start })
    if (kind === 'websocket') {
      this.#emit('webSocketCreated', { requestId: String(id), url, initiator: { type: 'script' } })
      const request = { headers: NetworkDomain.headers(headers) }
      this.#emit('webSocketWillSendHandshakeRequest', { requestId: String(id), timestamp: start, wallTime: Date.now() / 1000, request })
      return
    }
    const request: Record<string, unknown> = {
      url,
      method,
      headers: NetworkDomain.headers(headers),
      initialPriority: 'High',
      referrerPolicy: 'strict-origin-when-cross-origin',
    }
    if (body?.length) {
      request['hasPostData'] = true
      request['postData'] = new TextDecoder().decode(body)
    }
    this.#emit('requestWillBeSent', {
      requestId: String(id),
      loaderId: this.page.frameId,
      documentURL: this.documentUrl(),
      request,
      timestamp: start,
      wallTime: Date.now() / 1000,
      initiator: { type: 'script' },
      redirectHasExtraInfo: false,
      type,
      frameId: this.page.frameId,
    })
  }

  #response(id: number, status: number, statusText: string, headers: NetworkHeaders, fromCache = false): void {
    const open = this.#live(id)
    if (!open) return
    open.mimeType = NetworkDomain.mimeType(headers)
    const now = this.clock()
    if (open.kind === 'websocket') {
      const response = { status, statusText, headers: NetworkDomain.headers(headers) }
      return this.#emit('webSocketHandshakeResponseReceived', { requestId: String(id), timestamp: now, response })
    }
    this.#emit('responseReceived', {
      requestId: String(id),
      loaderId: this.page.frameId,
      timestamp: now,
      type: open.type,
      frameId: this.page.frameId,
      hasExtraInfo: false,
      response: {
        url: open.url,
        status,
        statusText,
        headers: NetworkDomain.headers(headers),
        mimeType: open.mimeType,
        charset: '',
        connectionReused: false,
        connectionId: 0,
        encodedDataLength: 0,
        securityState: 'neutral',
        fromMemoryCache: fromCache,
        timing: NetworkDomain.timing(open.start, now),
      },
    })
  }

  #data(id: number, chunk: Uint8Array): void {
    const open = this.#live(id)
    if (!open || open.kind === 'websocket') return
    open.size += chunk.length
    if (open.keep && open.size <= this.limit) open.chunks.push(chunk.slice())
    else {
      open.keep = false
      open.chunks = []
    }
    this.#emit('dataReceived', { requestId: String(id), timestamp: this.clock(), dataLength: chunk.length, encodedDataLength: chunk.length })
  }

  #end(id: number, bytes: number): void {
    const open = this.#live(id)
    if (!open) return
    this.#open.delete(id)
    if (open.kind === 'websocket') return this.#emit('webSocketClosed', { requestId: String(id), timestamp: this.clock() })
    if (open.keep) this.#keep(String(id), open)
    this.#emit('loadingFinished', { requestId: String(id), timestamp: this.clock(), encodedDataLength: bytes })
  }

  #error(id: number, message: string): void {
    const open = this.#live(id)
    if (!open) return
    this.#open.delete(id)
    if (open.kind === 'websocket') {
      this.#emit('webSocketFrameError', { requestId: String(id), timestamp: this.clock(), errorMessage: message })
      return this.#emit('webSocketClosed', { requestId: String(id), timestamp: this.clock() })
    }
    const failed: Record<string, unknown> = { requestId: String(id), timestamp: this.clock(), type: open.type, errorText: message }
    if (message === 'aborted') failed['canceled'] = true
    if (message === NO_NETWORK) failed['blockedReason'] = 'other'
    this.#emit('loadingFailed', failed)
  }

  #frame(id: number, sent: boolean, data: string | Uint8Array): void {
    const open = this.#live(id)
    if (!open || open.kind !== 'websocket') return
    const text = typeof data === 'string'
    const response = { opcode: text ? 1 : 2, mask: sent, payloadData: text ? data : NetworkDomain.base64(data) }
    this.#emit(sent ? 'webSocketFrameSent' : 'webSocketFrameReceived', { requestId: String(id), timestamp: this.clock(), response })
  }

  #message(id: number, event: string, data: string, lastId: string): void {
    const open = this.#live(id)
    if (!open || open.kind !== 'eventsource') return
    this.#emit('eventSourceMessageReceived', { requestId: String(id), timestamp: this.clock(), eventName: event, eventId: lastId, data })
  }

  #keep(id: string, open: Open): void {
    const body = new Uint8Array(open.size)
    let at = 0
    for (const chunk of open.chunks) {
      body.set(chunk, at)
      at += chunk.length
    }
    this.#bodies.set(id, { body, mimeType: open.mimeType })
    this.#kept += body.length
    for (const [oldest, kept] of this.#bodies) {
      if (this.#kept <= this.limit) break
      this.#bodies.delete(oldest)
      this.#kept -= kept.body.length
    }
  }

  #body(id: string): { body: string; base64Encoded: boolean } {
    const kept = this.#bodies.get(id)
    if (!kept) throw new DevToolsError(-32000, 'No resource with given identifier found')
    if (TEXT.test(kept.mimeType) || kept.mimeType === '') {
      try {
        return { body: new TextDecoder('utf-8', { fatal: true }).decode(kept.body), base64Encoded: false }
      } catch {}
    }
    return { body: NetworkDomain.base64(kept.body), base64Encoded: true }
  }
}
