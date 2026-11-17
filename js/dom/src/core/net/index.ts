import { Blob, blobBytes } from '../blob'
import {
  allowedHeaders,
  backendTarget,
  cookies,
  dialOf,
  isRemote,
  requestHeaders,
  transport,
  type Header,
  type HttpEvent,
  type RemoteTarget,
  type Target,
} from '../backend'
import { Cors } from '../cors'
import { mocked, Tracked, type MockReply, type RequestKind } from '../observe'
import { bytesOf, concat, TextDecoder, unbase64 } from '../encoding'
import { DOMException } from '../exception'
import { G, host, tag } from '../host'
import { isDisturbed, readAllBytes, ReadableStream, streamOfBytes, type ReadableStreamDefaultController } from '../streams'
import { objectUrls, URL, URLSearchParams } from '../url'
import { Headers, type HeadersInit } from './headers'

export const net = {
  base: () => 'http://acde.local/',
  log: true,
}

const TYPES: Record<string, string> = {
  html: 'text/html', htm: 'text/html', js: 'text/javascript', mjs: 'text/javascript', css: 'text/css', json: 'application/json',
  wasm: 'application/wasm', woff: 'font/woff', woff2: 'font/woff2', ttf: 'font/ttf', otf: 'font/otf', png: 'image/png', jpg: 'image/jpeg',
  jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp', svg: 'image/svg+xml', avif: 'image/avif', bmp: 'image/bmp', ico: 'image/x-icon',
  mp3: 'audio/mpeg', wav: 'audio/wav', ogg: 'audio/ogg', flac: 'audio/flac', m4a: 'audio/mp4', mp4: 'video/mp4', webm: 'video/webm',
}

export function typeOfName(name: string): string {
  const ext = /\.([a-z0-9]+)$/i.exec(name)?.[1]?.toLowerCase() ?? ''
  return TYPES[ext] ?? ''
}

const warned = new Set<string>()

function warnOnce(method: string, url: string, what: string): void {
  let key: string
  try {
    const parsed = new URL(url)
    key = `${method} ${parsed.origin === new URL(net.base()).origin ? '' : parsed.origin}${parsed.pathname}`
  } catch {
    key = `${method} ${url}`
  }
  if (!net.log || warned.has(`${key}\n${what}`)) return
  warned.add(`${key}\n${what}`)
  host.print(1, `acde-dom: ${key}: ${what}`)
}

export function backendFailed(method: string, url: string, message: string): void {
  let target: Target | null = null
  try {
    target = backendTarget(new URL(url), new URL(net.base()))
  } catch {
  }
  warnOnce(method, url, target && !isRemote(target) ? `the app's backend: ${message}` : message)
}

function quality(languages: readonly string[]): string {
  return languages.map((tag, i) => (i ? `${tag};q=${Math.max(0.1, 1 - i * 0.1).toFixed(1)}` : tag)).join(',')
}

export function browserHeaders(page: readonly Header[], accept: boolean): Header[] {
  const out: Header[] = [...page]
  const has = (name: string) => out.some(([n]) => n.toLowerCase() === name)
  const navigator = G['navigator'] as { userAgent?: string; languages?: readonly string[] } | undefined
  if (accept && !has('accept')) out.push(['accept', '*/*'])
  if (!has('accept-language') && navigator?.languages?.length) out.push(['accept-language', quality(navigator.languages)])
  if (!has('user-agent') && navigator?.userAgent) out.push(['user-agent', navigator.userAgent])
  return out
}

type Loaded = { bytes: Uint8Array; type: string }
type Routed = { target: Target } | { asset: Loaded } | null

export const NO_NETWORK = 'another host, and this page has no network'

export function unreachable(url: string): string {
  try {
    if (new URL(url).origin === new URL(net.base()).origin) return 'no such file in the app, and no backend to ask'
  } catch {
  }
  return NO_NETWORK

}

const JAVASCRIPT_TYPES = new Set([
  'application/ecmascript', 'application/javascript', 'application/x-ecmascript', 'application/x-javascript', 'text/ecmascript',
  'text/javascript', 'text/javascript1.0', 'text/javascript1.1', 'text/javascript1.2', 'text/javascript1.3', 'text/javascript1.4',
  'text/javascript1.5', 'text/jscript', 'text/livescript', 'text/x-ecmascript', 'text/x-javascript',
])

export function moduleSource(url: string): string | null {
  const found = inline(url)
  if (!found) return null
  const essence = found.type.split(';')[0]!.trim().toLowerCase()
  if (!JAVASCRIPT_TYPES.has(essence)) {
    throw new TypeError(`Failed to load module script: expected a JavaScript module script, but ${url} has the MIME type "${found.type}"`)
  }
  return host.decode(found.bytes, 'utf-8', false, false)
}

function inline(url: string): Loaded | null {
  if (url.startsWith('blob:')) {
    const hash = url.indexOf('#')
    const blob = objectUrls.get(hash < 0 ? url : url.slice(0, hash))
    return blob instanceof Blob ? { bytes: blobBytes(blob), type: blob.type } : null
  }
  const comma = url.indexOf(',')
  if (comma < 0) return null
  const meta = url.slice(5, comma)
  const body = url.slice(comma + 1)
  const base64 = /;base64$/i.test(meta)
  const bytes = base64 ? unbase64(decodeURIComponent(body)) : host.encodeUtf8(decodeURIComponent(body))
  return bytes ? { bytes, type: meta.replace(/;base64$/i, '') || 'text/plain;charset=US-ASCII' } : null
}

export function route(url: string, method: string): Routed {
  const connection = method === 'WS' || method === 'SSE'
  if (!connection && (url.startsWith('data:') || url.startsWith('blob:'))) {
    const found = inline(url)
    return found && { asset: found }
  }
  let parsed: URL
  try {
    parsed = new URL(url)
  } catch {
    return null
  }
  const own = new URL(net.base())
  if (!connection && parsed.origin === own.origin && (method === 'GET' || method === 'HEAD')) {
    const bytes = host.readAsset(parsed.href)
    if (bytes) return { asset: { bytes, type: typeOfName(parsed.pathname) || 'application/octet-stream' } }
  }
  const target = backendTarget(parsed, own)
  if (target) return { target }
  warnOnce(method, url, 'no network; refused')
  return null
}

export function assetHead(found: Loaded): Header[] {
  return [['content-length', String(found.bytes.length)], ['content-type', found.type]]
}

function visibleHeaders(headers: readonly Header[]): Header[] {
  return headers.filter(([name]) => !/^set-cookie2?$/i.test(name))
}

export function headerOf(headers: readonly Header[], name: string): string | null {
  const values = headers.filter(([n]) => n.toLowerCase() === name).map(([, v]) => v)
  return values.length ? values.join(', ') : null
}

interface FormDataLike {
  entries(): IterableIterator<[string, string | Blob]>
}

function isFormData(value: unknown): value is FormDataLike {
  return typeof value === 'object' && value !== null && Object.prototype.toString.call(value) === '[object FormData]'
}

export type BodyInit = string | ArrayBuffer | ArrayBufferView | Blob | URLSearchParams | ReadableStream<Uint8Array> | FormDataLike | null | undefined

function multipart(form: FormDataLike): { bytes: Uint8Array; type: string } {
  const random = new Uint8Array(12)
  host.randomBytes(random)
  const boundary = `----acdeFormBoundary${[...random].map((b) => b.toString(16).padStart(2, '0')).join('')}`
  const quote = (text: string) => text.replace(/\r/g, '%0D').replace(/\n/g, '%0A').replace(/"/g, '%22')
  const parts: Uint8Array[] = []
  for (const [name, value] of form.entries()) {
    let head = `--${boundary}\r\nContent-Disposition: form-data; name="${quote(name)}"`
    if (value instanceof Blob) {
      const filename = (value as Blob & { name?: string }).name ?? 'blob'
      head += `; filename="${quote(filename)}"\r\nContent-Type: ${value.type || 'application/octet-stream'}\r\n\r\n`
      parts.push(host.encodeUtf8(head), blobBytes(value), host.encodeUtf8('\r\n'))
    } else parts.push(host.encodeUtf8(`${head}\r\n\r\n${String(value).replace(/\r?\n|\r/g, '\r\n')}\r\n`))
  }
  parts.push(host.encodeUtf8(`--${boundary}--\r\n`))
  return { bytes: concat(parts), type: `multipart/form-data; boundary=${boundary}` }
}

export function extractBody(body: BodyInit): { bytes: Uint8Array | null; stream: ReadableStream<Uint8Array> | null; type: string | null } {
  if (body === null || body === undefined) return { bytes: null, stream: null, type: null }
  if (typeof body === 'string') return { bytes: host.encodeUtf8(body), stream: null, type: 'text/plain;charset=UTF-8' }
  if (body instanceof Blob) return { bytes: blobBytes(body), stream: null, type: body.type || null }
  if (body instanceof URLSearchParams) return { bytes: host.encodeUtf8(body.toString()), stream: null, type: 'application/x-www-form-urlencoded;charset=UTF-8' }
  if (body instanceof ReadableStream) return { bytes: null, stream: body, type: null }
  if (isFormData(body)) {
    const { bytes, type } = multipart(body)
    return { bytes, stream: null, type }
  }
  return { bytes: bytesOf(body as ArrayBuffer).slice(), stream: null, type: null }
}

const SEND = Symbol('send')

class Body {
  #bytes: Uint8Array | null
  #stream: ReadableStream<Uint8Array> | null
  #used = false

  constructor(bytes: Uint8Array | null, stream: ReadableStream<Uint8Array> | null = null) {
    this.#bytes = bytes
    this.#stream = stream
  }

  get body(): ReadableStream<Uint8Array> | null {
    if (!this.#stream && this.#bytes) {
      this.#stream = streamOfBytes(this.#bytes)
      this.#bytes = null
    }
    return this.#stream
  }
  get bodyUsed(): boolean {
    return this.#used || (!!this.#stream && isDisturbed(this.#stream))
  }

  protected bodyForClone(): { bytes: Uint8Array | null; stream: ReadableStream<Uint8Array> | null } {
    if (this.#stream) {
      const [mine, theirs] = this.#stream.tee()
      this.#stream = mine
      return { bytes: null, stream: theirs }
    }
    return { bytes: this.#bytes, stream: null }
  }

  async #consume(): Promise<Uint8Array> {
    if (this.bodyUsed) throw new TypeError('Failed to execute: body stream already read')
    const stream = this.body
    if (!stream) return new Uint8Array(0)
    if (stream.locked) throw new TypeError('Failed to execute: body stream is locked')
    this.#used = true
    return readAllBytes(stream, true)
  }

  [SEND](): Promise<Uint8Array | null> {
    if (!this.#stream && !this.#bytes) return Promise.resolve(null)
    return this.#consume()
  }

  async text(): Promise<string> {
    return new TextDecoder().decode(await this.#consume())
  }
  async json(): Promise<unknown> {
    return JSON.parse(await this.text())
  }
  async arrayBuffer(): Promise<ArrayBuffer> {
    return (await this.#consume()).slice().buffer
  }
  async bytes(): Promise<Uint8Array> {
    return (await this.#consume()).slice()
  }
  async blob(): Promise<Blob> {
    const bytes = await this.#consume()
    return new Blob([bytes], { type: (this as unknown as { headers: Headers }).headers.get('content-type') ?? '' })
  }
  async formData(): Promise<FormDataLike> {
    const type = (this as unknown as { headers: Headers }).headers.get('content-type') ?? ''
    const FormData = G['FormData'] as (new () => FormDataLike & { append(name: string, value: unknown, filename?: string): void }) | undefined
    if (!FormData) throw new TypeError('formData() needs FormData')
    const bytes = await this.#consume()
    const form = new FormData()
    if (/^application\/x-www-form-urlencoded\b/i.test(type)) {
      for (const [name, value] of new URLSearchParams(new TextDecoder().decode(bytes))) form.append(name, value)
      return form
    }
    const boundary = /^multipart\/form-data\b.*;\s*boundary=(?:"([^"]+)"|([^;\s]+))/i.exec(type)
    if (!boundary) throw new TypeError(`formData(): not a form (${type || 'no content type'})`)
    for (const part of multipartParts(bytes, boundary[1] ?? boundary[2]!)) {
      const disposition = /\bname="([^"]*)"(?:;\s*filename="([^"]*)")?/i.exec(part.headers['content-disposition'] ?? '')
      if (!disposition) continue
      const name = disposition[1]!
      if (disposition[2] === undefined) form.append(name, new TextDecoder().decode(part.body))
      else form.append(name, new Blob([part.body], { type: part.headers['content-type'] ?? 'application/octet-stream' }), disposition[2])
    }
    return form
  }
}

function multipartParts(bytes: Uint8Array, boundary: string): { headers: Record<string, string>; body: Uint8Array }[] {
  const find = (needle: Uint8Array, from: number): number => {
    outer: for (let i = from; i + needle.length <= bytes.length; i++) {
      for (let j = 0; j < needle.length; j++) if (bytes[i + j] !== needle[j]) continue outer
      return i
    }
    return -1
  }
  const delimiter = host.encodeUtf8(`--${boundary}`)
  const parts: { headers: Record<string, string>; body: Uint8Array }[] = []
  let at = find(delimiter, 0)
  while (at >= 0) {
    let start = at + delimiter.length
    if (bytes[start] === 0x2d && bytes[start + 1] === 0x2d) break
    if (bytes[start] === 0x0d) start += 2
    const headEnd = find(host.encodeUtf8('\r\n\r\n'), start)
    if (headEnd < 0) break
    const next = find(host.encodeUtf8(`\r\n--${boundary}`), headEnd + 4)
    if (next < 0) break
    const headers: Record<string, string> = {}
    for (const line of new TextDecoder().decode(bytes.subarray(start, headEnd)).split('\r\n')) {
      const colon = line.indexOf(':')
      if (colon > 0) headers[line.slice(0, colon).trim().toLowerCase()] = line.slice(colon + 1).trim()
    }
    parts.push({ headers, body: bytes.slice(headEnd + 4, next) })
    at = next + 2
  }
  return parts
}

interface RequestInit {
  method?: string
  headers?: HeadersInit
  body?: BodyInit
  signal?: unknown
  cache?: string
  credentials?: string
  redirect?: string
  duplex?: string
  mode?: string
  keepalive?: boolean
}

export class Request extends Body {
  readonly url: string
  readonly method: string
  readonly headers: Headers
  readonly signal: unknown
  readonly mode: string
  readonly credentials: string
  readonly cache: string
  readonly redirect: string
  readonly referrer = 'about:client'
  readonly referrerPolicy = ''
  readonly integrity = ''
  readonly keepalive = false
  readonly destination = ''

  constructor(input: string | URL | Request, init?: RequestInit) {
    const base = input instanceof Request ? input : null
    const { bytes, stream, type } = extractBody(init?.body)
    const inherited = init?.body === undefined && base ? base.bodyForClone() : null
    super(inherited ? inherited.bytes : bytes, inherited ? inherited.stream : stream)
    const parsed = base ? null : new URL(String(input), net.base())
    if (parsed && (parsed.username || parsed.password)) {
      throw new TypeError(`Failed to construct 'Request': Request cannot be constructed from a URL that includes credentials: ${String(input)}`)
    }
    this.url = base ? base.url : parsed!.href
    this.method = String(init?.method ?? base?.method ?? 'GET').toUpperCase()
    this.headers = new Headers(init?.headers ?? base?.headers)
    if (type && !this.headers.has('content-type')) this.headers.set('content-type', type)
    this.signal = init?.signal ?? base?.signal ?? null
    this.cache = init?.cache ?? 'default'
    this.credentials = init?.credentials ?? base?.credentials ?? 'same-origin'
    this.redirect = init?.redirect ?? base?.redirect ?? 'follow'
    this.mode = init?.mode ?? base?.mode ?? 'cors'
    if (!['cors', 'no-cors', 'same-origin'].includes(this.mode)) {
      throw new TypeError(`Failed to construct 'Request': Cannot construct a Request with a RequestInit whose mode member is set as '${this.mode}'.`)
    }
    if (this.mode === 'no-cors' && !Cors.simpleMethod(this.method)) {
      throw new TypeError(`Failed to construct 'Request': '${this.method}' is unsupported in no-cors mode.`)
    }
  }

  clone(): Request {
    return new Request(this)
  }
}
tag(Request)

type ResponseInit = { status?: number; statusText?: string; headers?: HeadersInit }

export class Response extends Body {
  readonly status: number
  readonly statusText: string
  readonly headers: Headers
  readonly type: string
  url = ''
  readonly redirected = false

  constructor(body?: BodyInit, init?: ResponseInit) {
    const { bytes, stream, type } = extractBody(body)
    super(bytes, stream)
    this.status = init?.status ?? 200
    if (this.status < 200 || this.status > 599) {
      throw new RangeError(`Failed to construct 'Response': The status provided (${this.status}) is outside the range [200, 599].`)
    }
    this.statusText = init?.statusText ?? ''
    this.headers = new Headers(init?.headers)
    if (type && !this.headers.has('content-type')) this.headers.set('content-type', type)
    this.type = 'default'
  }

  get ok(): boolean {
    return this.status >= 200 && this.status < 300
  }

  clone(): Response {
    if (this.bodyUsed) throw new TypeError("Failed to execute 'clone' on 'Response': Response body is already used")
    const { bytes, stream } = this.bodyForClone()
    const inRange = this.status >= 200 && this.status <= 599
    const copy = new Response(stream ?? bytes, { status: inRange ? this.status : 200, statusText: this.statusText, headers: this.headers })
    if (!inRange) Object.defineProperty(copy, 'status', { value: this.status })
    if (this.type !== 'default') Object.defineProperty(copy, 'type', { value: this.type })
    copy.url = this.url
    if (this.redirected) Object.defineProperty(copy, 'redirected', { value: true })
    return copy
  }

  static json(data: unknown, init?: ResponseInit): Response {
    const response = new Response(JSON.stringify(data), init)
    response.headers.set('content-type', 'application/json')
    return response
  }

  static error(): Response {
    const response = new Response(null)
    Object.defineProperty(response, 'status', { value: 0 })
    Object.defineProperty(response, 'type', { value: 'error' })
    return response
  }

  static redirect(url: string, status = 302): Response {
    return new Response(null, { status, headers: { location: new URL(url, net.base()).href } })
  }
}
tag(Response)

const REDIRECTS = new Set([301, 302, 303, 307, 308])
export const NULL_BODY = new Set([101, 103, 204, 205, 304])
const BODY_HEADERS = new Set(['content-type', 'content-encoding', 'content-language', 'content-location', 'content-length'])

interface ExchangeHead {
  readonly status: number
  readonly statusText: string
  readonly headers: Header[]
  readonly url: string
  readonly redirected: boolean
  readonly type: 'basic' | 'cors' | 'opaque'
}

interface ExchangeHandlers {
  head(head: ExchangeHead): void
  chunk(data: Uint8Array): void
  end(): void
  error(message: string): void
}

interface ExchangeRequest {
  readonly url: string
  readonly method: string
  readonly headers: Iterable<readonly [string, string]>
  readonly body: Uint8Array | null
  readonly redirect: string
  readonly credentials: string
  readonly mode: string
}

function tracking(tracked: Tracked | null, given: ExchangeHandlers): ExchangeHandlers {
  if (!tracked) return given
  return {
    head(head) {
      tracked.response(head.status, head.statusText, head.headers)
      given.head(head)
    },
    chunk(data) {
      tracked.received(data.length, data)
      given.chunk(data)
    },
    end() {
      tracked.end()
      given.end()
    },
    error(message) {
      tracked.error(message)
      given.error(message)
    },
  }
}

export function exchange(
  first: Target,
  request: ExchangeRequest,
  given: ExchangeHandlers,
  kind: RequestKind,
  tracked: Tracked | null = Tracked.start(kind, request.method, request.url, request.body?.length ?? 0, false, request.headers, request.body),
): () => void {
  const backend = transport!
  const on = tracking(tracked, given)
  const withCookies = request.credentials !== 'omit'
  const include = request.credentials === 'include'
  const own = new URL(net.base()).origin
  let origin = own
  let current: number | null = null
  let finished = false
  let redirects = 0
  const fail = (message: string): void => {
    if (finished) return
    finished = true
    on.error(message)
  }
  const stop = (id: number | null): void => {
    if (id !== null) backend.abort(id)
  }
  const blocked = (problem: string): void => fail(`CORS: ${problem}`)
  const ask = (target: Target, method: string, headers: Header[], body: Uint8Array | null, onEvent: (event: HttpEvent) => void): number | null => {
    const [to, path] = dialOf(target)
    try {
      return backend.request(to, method, path, headers, body, onEvent)
    } catch (error) {
      host.queueMicrotask(() => fail(String((error as Error)?.message ?? error)))
      return null
    }
  }
  const listen = (
    target: Target, url: string, method: string, pageHeaders: Iterable<readonly [string, string]>, body: Uint8Array | null, mine: () => number | null,
    tainting: 'basic' | 'cors' | 'opaque',
  ) => (event: HttpEvent): void => {
    if (finished || current !== mine()) return
    switch (event.type) {
      case 'head': {
        const all = event.headers ?? []
        if (!isRemote(target) && withCookies) for (const [name, value] of all) if (name.toLowerCase() === 'set-cookie') cookies.store(value, target.path)
        const location = headerOf(all, 'location')
        const redirecting = REDIRECTS.has(event.status) && location !== null && request.redirect !== 'manual'
        if (tainting === 'cors') {
          const problem = Cors.check(all, origin, include)
          if (problem) {
            stop(current)
            current = null
            return blocked(problem)
          }
        }
        if (!redirecting) {
          if (tainting === 'opaque') {
            stop(current)
            current = null
            finished = true
            on.head({ status: 0, statusText: '', headers: [], url: '', redirected: false, type: 'opaque' })
            on.end()
            return
          }
          const headers = tainting === 'cors' ? Cors.exposed(all, include) : visibleHeaders(all)
          on.head({ status: event.status, statusText: event.statusText ?? '', headers, url, redirected: redirects > 0, type: tainting })
          return
        }
        stop(current)
        current = null
        if (request.redirect === 'error') return fail(`redirected to ${location}`)
        if (++redirects > 20) return fail('too many redirects')
        let next: string
        try {
          next = new URL(location!, url).href
        } catch {
          return fail(`redirected to an invalid URL ${location}`)
        }
        const toGet = (event.status === 303 && method !== 'HEAD') || ((event.status === 301 || event.status === 302) && method === 'POST')
        const nextMethod = toGet ? 'GET' : method
        const nextHeaders = toGet ? [...pageHeaders].filter(([n]) => !BODY_HEADERS.has(n.toLowerCase())) : pageHeaders
        const routed = route(next, nextMethod)
        if (routed && 'target' in routed) {
          if (isRemote(target) && isRemote(routed.target) && routed.target.origin !== target.origin) origin = 'null'
          return send(routed.target, next, nextMethod, nextHeaders, toGet ? null : body)
        }
        if (!routed) return fail(`redirected to ${next}, which cannot be reached`)
        finished = true
        on.head({ status: 200, statusText: 'OK', headers: assetHead(routed.asset), url: next, redirected: true, type: 'basic' })
        if (nextMethod !== 'HEAD') on.chunk(routed.asset.bytes)
        on.end()
        return
      }
      case 'chunk':
        return on.chunk(event.data)
      case 'end':
        finished = true
        return on.end()
      case 'error':
        return fail(event.message)
    }
  }
  const remote = (target: RemoteTarget, url: string, method: string, pageHeaders: Iterable<readonly [string, string]>, body: Uint8Array | null): void => {
    if (request.mode === 'same-origin') return fail(`mode 'same-origin' does not allow a request to ${target.origin}`)
    const page = allowedHeaders(pageHeaders)
    if (request.mode === 'script') {
      let mine: number | null = null
      const headers = browserHeaders(page, true).concat([['referer', `${own}/`]])
      current = mine = ask(target, method, headers, body, listen(target, url, method, pageHeaders, body, () => mine, 'basic'))
      return
    }
    const unsafe = Cors.unsafeNames(page)
    const simple = Cors.simpleMethod(method) && !unsafe.length
    const opaque = request.mode === 'no-cors'
    if (opaque && !simple) return fail("mode 'no-cors' allows only GET, HEAD and POST with CORS-safelisted headers")
    const headers = browserHeaders(page, true)
    if (!opaque || (method !== 'GET' && method !== 'HEAD')) headers.push(['origin', origin])
    headers.push(['referer', `${own}/`])
    const actual = (): void => {
      let mine: number | null = null
      current = mine = ask(target, method, headers, body, listen(target, url, method, pageHeaders, body, () => mine, opaque ? 'opaque' : 'cors'))
    }
    if (simple || opaque) return actual()
    const preflight: Header[] = [['accept', '*/*'], ['origin', origin], ['access-control-request-method', method]]
    if (unsafe.length) preflight.push(['access-control-request-headers', unsafe.join(',')])
    let mine: number | null = null
    const onPreflight = (event: HttpEvent): void => {
      if (finished || current !== mine) return
      if (event.type === 'error') return fail(event.message)
      if (event.type !== 'head') return
      stop(mine)
      current = null
      const all = event.headers ?? []
      const problem = event.status < 200 || event.status > 299
        ? `It does not have HTTP ok status (${event.status}).`
        : (Cors.check(all, origin, include) ?? Cors.preflight(all, method, unsafe, include))
      if (problem) return blocked(`Response to preflight request doesn't pass access control check: ${problem}`)
      actual()
    }
    current = mine = ask(target, 'OPTIONS', browserHeaders(preflight, false).concat([['referer', `${own}/`]]), null, onPreflight)
  }
  const send = (target: Target, url: string, method: string, pageHeaders: Iterable<readonly [string, string]>, body: Uint8Array | null): void => {
    if (isRemote(target)) return remote(target, url, method, pageHeaders, body)
    const headers = requestHeaders(pageHeaders, target, { origin: method !== 'GET' && method !== 'HEAD', cookies: withCookies })
    if (!headers.some(([n]) => n.toLowerCase() === 'accept')) headers.push(['accept', '*/*'])
    let mine: number | null = null
    current = mine = ask(target, method, headers, body, listen(target, url, method, pageHeaders, body, () => mine, 'basic'))
  }
  send(first, request.url, request.method, request.headers, request.body)
  return () => {
    if (finished) return
    finished = true
    tracked?.error('aborted')
    stop(current)
  }
}

type SignalLike = {
  aborted?: boolean
  reason?: unknown
  addEventListener?: (t: string, f: () => void) => void
  removeEventListener?: (t: string, f: () => void) => void
}

function aborted(signal: SignalLike): unknown {
  return signal.reason ?? new DOMException('The user aborted a request.', 'AbortError')
}

function assetResponse(request: Request, found: Loaded): Response {
  const response = new Response(request.method === 'HEAD' ? null : found.bytes, { status: 200, statusText: 'OK', headers: assetHead(found) })
  response.url = request.url
  return response
}

async function fetchBackend(request: Request, target: Target, signal: SignalLike | null, mode = request.mode): Promise<Response> {
  const body = request.method === 'GET' || request.method === 'HEAD' ? null : await request[SEND]()
  if (signal?.aborted) throw aborted(signal)
  return new Promise((resolve, reject) => {
    let controller: ReadableStreamDefaultController<Uint8Array> | null = null
    let response: Response | null = null
    const settle = (): void => signal?.removeEventListener?.('abort', onAbort)
    const { url, method, headers, redirect, credentials } = request
    const sent = { url, method, headers, body, redirect, credentials, mode }
    const abort = exchange(target, sent, {
      head(head) {
        if (head.type === 'opaque') {
          settle()
          response = new Response(null)
          Object.defineProperty(response, 'status', { value: 0 })
          Object.defineProperty(response, 'type', { value: 'opaque' })
          return resolve(response)
        }
        if (head.status < 200 || head.status > 599) {
          abort()
          settle()
          return reject(new TypeError('Failed to fetch', { cause: `status ${head.status}` }))
        }
        const empty = request.method === 'HEAD' || NULL_BODY.has(head.status)
        const stream = empty ? null : new ReadableStream<Uint8Array>({
          start: (c) => void (controller = c),
          cancel: () => {
            abort()
            settle()
          },
        }, { highWaterMark: 0 })
        response = new Response(stream, { status: head.status, statusText: head.statusText, headers: head.headers })
        response.url = head.url
        Object.defineProperty(response, 'type', { value: head.type })
        if (head.redirected) Object.defineProperty(response, 'redirected', { value: true })
        if (empty) settle()
        resolve(response)
      },
      chunk(data) {
        try {
          controller?.enqueue(data)
        } catch {
        }
      },
      end() {
        settle()
        try {
          controller?.close()
        } catch {
        }
      },
      error(message) {
        settle()
        backendFailed(request.method, request.url, message)
        const error = new TypeError(response ? 'network error' : 'Failed to fetch', { cause: message })
        if (!response) reject(error)
        else controller?.error(error)
      },
    }, 'fetch')
    const onAbort = (): void => {
      abort()
      settle()
      const reason = aborted(signal!)
      if (!response) reject(reason)
      else controller?.error(reason)
    }
    signal?.addEventListener?.('abort', onAbort)
  })
}

export async function scriptSource(url: string): Promise<string> {
  const routed = route(url, 'GET')
  if (!routed) throw new Error(`<script src=${url}>: ${unreachable(url)}`)
  if ('asset' in routed) return new TextDecoder().decode(routed.asset.bytes)
  const response = await fetchBackend(new Request(url), routed.target, null, 'script')
  if (!response.ok) throw new Error(`<script src=${url}>: HTTP ${response.status}`)
  return response.text()
}

function fetchMocked(request: Request, mock: MockReply, signal: SignalLike | null): Promise<Response> {
  const tracked = Tracked.start('fetch', request.method, request.url, 0, true, request.headers)
  return new Promise((resolve, reject) => {
    host.setTimer(0, () => {
      if (signal?.aborted) {
        tracked?.error('aborted')
        return reject(aborted(signal))
      }
      if ('error' in mock) {
        tracked?.error(mock.error)
        return reject(new TypeError('Failed to fetch', { cause: mock.error }))
      }
      const empty = request.method === 'HEAD' || NULL_BODY.has(mock.status)
      tracked?.response(mock.status, mock.statusText, mock.headers)
      if (!empty) tracked?.received(mock.body.length, mock.body)
      tracked?.end(empty ? 0 : mock.body.length)
      const response = new Response(empty ? null : mock.body, { status: mock.status, statusText: mock.statusText, headers: mock.headers as [string, string][] })
      response.url = request.url
      resolve(response)
    })
  })
}

export function fetch(input: string | URL | Request, init?: RequestInit): Promise<Response> {
  let request: Request
  try {
    request = new Request(input, init)
  } catch (error) {
    return Promise.reject(error instanceof TypeError ? error : new TypeError(String((error as Error)?.message ?? error)))
  }
  const signal = request.signal as SignalLike | null
  if (signal?.aborted) return Promise.reject(aborted(signal))
  const mock = mocked('fetch', request.method, request.url)
  if (mock) return fetchMocked(request, mock, signal)
  const routed = route(request.url, request.method)
  if (routed && 'target' in routed) return fetchBackend(request, routed.target, signal)
  const tracked = Tracked.start('fetch', request.method, request.url, 0, false, request.headers)
  return new Promise((resolve, reject) => {
    host.setTimer(0, () => {
      if (signal?.aborted) {
        tracked?.error('aborted')
        return reject(aborted(signal))
      }
      if (!routed) {
        tracked?.error(unreachable(request.url))
        return reject(new TypeError('Failed to fetch'))
      }
      tracked?.response(200, 'OK', assetHead(routed.asset))
      tracked?.received(routed.asset.bytes.length, routed.asset.bytes)
      tracked?.end(routed.asset.bytes.length)
      resolve(assetResponse(request, routed.asset))
    })
  })
}
