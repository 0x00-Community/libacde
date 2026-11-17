import type { CookieNative } from '../native'
export type Header = [name: string, value: string]

export type HttpEvent =
  | { type: 'head'; status: number; statusText: string; headers: Header[] }
  | { type: 'chunk'; data: Uint8Array }
  | { type: 'end' }
  | { type: 'error'; message: string }

export type WsEvent =
  | { type: 'open'; protocol: string; extensions: string }
  | { type: 'message'; text?: string; data?: Uint8Array }
  | { type: 'close'; code: number; reason: string; wasClean: boolean }
  | { type: 'error'; message: string }

export interface BackendTransport {
  readonly port: number | null
  request(to: number | string, method: string, path: string, headers: Header[], body: Uint8Array | null, onEvent: (event: HttpEvent) => void): number
  abort(id: number): unknown
  websocket(to: number | string, path: string, headers: Header[], protocols: string[], onEvent: (event: WsEvent) => void): number
  send(id: number, data: string | Uint8Array): unknown
  close(id: number, code?: number, reason?: string): unknown
}

export let transport: BackendTransport | null = null

export function installBackendTransport(given: BackendTransport | null): void {
  transport = given
}

export interface BackendTarget {
  readonly port: number
  readonly path: string
  readonly host: string
}

export interface RemoteTarget {
  readonly url: string
  readonly origin: string
}

export type Target = BackendTarget | RemoteTarget

export function isRemote(target: Target): target is RemoteTarget {
  return 'url' in target
}

export function dialOf(target: Target): [to: number | string, path: string] {
  return isRemote(target) ? [target.url, ''] : [target.port, target.path]
}

type UrlParts = { readonly protocol: string; readonly host: string; readonly hostname: string; readonly port: string; readonly pathname: string; readonly search: string; readonly origin: string; readonly href: string; readonly hash: string }

const LOOPBACK = new Set(['localhost', '127.0.0.1', '[::1]', '0.0.0.0'])
const SECURE: Record<string, boolean> = { 'http:': false, 'ws:': false, 'https:': true, 'wss:': true }

export function backendTarget(url: UrlParts, own: UrlParts): Target | null {
  if (!transport || !(url.protocol in SECURE)) return null
  const path = (url.pathname || '/') + url.search
  const backend = transport.port
  if (url.hostname.toLowerCase() === own.hostname.toLowerCase()) {
    return url.host === own.host && SECURE[url.protocol] === SECURE[own.protocol] && backend !== null ? { port: backend, path, host: 'localhost' } : null
  }
  if (backend !== null && LOOPBACK.has(url.hostname.toLowerCase())) {
    const port = url.port ? Number(url.port) : SECURE[url.protocol] ? 443 : 80
    return { port, path, host: url.port ? `localhost:${port}` : 'localhost' }
  }
  return { url: url.hash ? url.href.slice(0, -url.hash.length) : url.href, origin: url.origin }
}

const FORBIDDEN = new Set([
  'host', 'connection', 'content-length', 'transfer-encoding', 'upgrade', 'keep-alive', 'te', 'trailer', 'cookie', 'cookie2', 'origin', 'expect', 'proxy-connection',
  'accept-charset', 'accept-encoding', 'access-control-request-headers', 'access-control-request-method', 'date', 'dnt', 'referer', 'via',
  'sec-websocket-key', 'sec-websocket-version', 'sec-websocket-extensions', 'sec-websocket-protocol',
])

export function allowedHeaders(page: Iterable<readonly [string, string]>): Header[] {
  const out: Header[] = []
  for (const [name, value] of page) {
    const lower = name.toLowerCase()
    if (!FORBIDDEN.has(lower) && !lower.startsWith('proxy-') && !lower.startsWith('sec-')) out.push([name, value])
  }
  return out
}

export function requestHeaders(page: Iterable<readonly [string, string]>, target: BackendTarget, options: { origin: boolean; cookies: boolean }): Header[] {
  const out: Header[] = [['host', target.host], ...allowedHeaders(page)]
  if (options.origin) out.push(['origin', `http://${target.host}`])
  const cookie = options.cookies ? cookies.header(target.path) : ''
  if (cookie) out.push(['cookie', cookie])
  return out
}

interface Cookie {
  readonly name: string
  readonly value: string
  readonly path: string
  readonly expires: number | null
  readonly httpOnly: boolean
  readonly created: number
}

function pathOf(target: string): string {
  const q = target.search(/[?#]/)
  return q < 0 ? target : target.slice(0, q)
}

function defaultPath(target: string): string {
  const path = pathOf(target)
  if (!path.startsWith('/')) return '/'
  const slash = path.lastIndexOf('/')
  return slash <= 0 ? '/' : path.slice(0, slash)
}

function pathMatches(path: string, cookiePath: string): boolean {
  if (path === cookiePath) return true
  if (!path.startsWith(cookiePath)) return false
  return cookiePath.endsWith('/') || path[cookiePath.length] === '/'
}

let created = 0
let shared: CookieNative | null = null

export function useCookieJar(jar: CookieNative | undefined): void {
  shared = jar ?? null
}

export const cookies = {
  jar: new Map<string, Cookie>(),

  store(line: string, target: string, script = false): void {
    const [pair = '', ...attributes] = String(line).split(';')
    const eq = pair.indexOf('=')
    const name = eq < 0 ? '' : pair.slice(0, eq).trim()
    const value = (eq < 0 ? pair : pair.slice(eq + 1)).trim()
    if (!name && !value) return
    let path: string | null = null
    let expires: number | null = null
    let maxAge: number | null = null
    let httpOnly = false
    for (const attribute of attributes) {
      const at = attribute.indexOf('=')
      const key = (at < 0 ? attribute : attribute.slice(0, at)).trim().toLowerCase()
      const val = at < 0 ? '' : attribute.slice(at + 1).trim()
      if (key === 'path' && val.startsWith('/')) path = val
      else if (key === 'expires') {
        const time = Date.parse(val)
        if (!Number.isNaN(time)) expires = time
      } else if (key === 'max-age' && /^-?\d+$/.test(val)) maxAge = Number(val)
      else if (key === 'httponly') httpOnly = true
    }
    if (maxAge !== null) expires = maxAge <= 0 ? 0 : Date.now() + maxAge * 1000
    path ??= defaultPath(target)
    if (shared) return shared.set(name, value, path, expires, httpOnly, script)
    const key = `${name}\n${path}`
    const old = cookies.jar.get(key)
    if (script && (httpOnly || old?.httpOnly)) return
    if (expires !== null && expires <= Date.now()) {
      cookies.jar.delete(key)
      return
    }
    cookies.jar.set(key, { name, value, path, expires, httpOnly, created: old?.created ?? created++ })
  },

  header(target: string, script = false): string {
    const path = pathOf(target) || '/'
    if (shared) return shared.matching(path, script).map(([name, value]) => (name ? `${name}=${value}` : value)).join('; ')
    const now = Date.now()
    const out: Cookie[] = []
    for (const [key, cookie] of cookies.jar) {
      if (cookie.expires !== null && cookie.expires <= now) {
        cookies.jar.delete(key)
        continue
      }
      if ((script && cookie.httpOnly) || !pathMatches(path, cookie.path)) continue
      out.push(cookie)
    }
    out.sort((a, b) => b.path.length - a.path.length || a.created - b.created)
    return out.map((c) => (c.name ? `${c.name}=${c.value}` : c.value)).join('; ')
  },
}
