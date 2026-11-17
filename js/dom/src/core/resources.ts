import type { Header } from './backend'
import { concat } from './encoding'
import { host } from './host'
import { assetHead, exchange, headerOf, net, route, typeOfName, unreachable } from './net'
import { observer, Tracked, type RequestKind } from './observe'
import { URL } from './url'

const KINDS: Readonly<Record<string, RequestKind>> = { image: 'image', style: 'stylesheet', font: 'font' }
const ACCEPT: Readonly<Record<string, string>> = {
  image: 'image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8',
  style: 'text/css,*/*;q=0.1',
}

function failed(url: string, message: string): void {
  const text = `Failed to load resource: ${url}: ${message}`
  if (net.log) host.print(2, text)
  observer?.console?.('error', text)
}

function problem(destination: string, status: number, type: string): string | null {
  if (status < 200 || status > 299) return `the server responded with a status of ${status}`
  if (destination === 'style' && type.split(';')[0]!.trim().toLowerCase() !== 'text/css') return `its MIME type ('${type}') is not 'text/css'`
  return null
}

function served(kind: RequestKind, url: string, asset: { bytes: Uint8Array; type: string }): void {
  const tracked = Tracked.network(kind, 'GET', url)
  tracked?.response(200, 'OK', assetHead(asset))
  tracked?.received(asset.bytes.length, asset.bytes)
  tracked?.end(asset.bytes.length)
}

export function reportServed(url: string, destination: string, cached = false): void {
  if (!observer?.network?.recording()) return
  const kind = KINDS[destination] ?? 'fetch'
  if (cached) {
    const tracked = Tracked.network(kind, 'GET', url)
    const type = typeOfName(new URL(url).pathname)
    tracked?.response(200, 'OK', type ? [['content-type', type]] : [], true)
    tracked?.end(0)
    return
  }
  const routed = route(url, 'GET')
  if (routed && 'asset' in routed) return served(kind, url, routed.asset)
  const tracked = Tracked.network(kind, 'GET', url)
  tracked?.response(200, 'OK')
  tracked?.end(0)
}

export function loadResource(url: string, destination: string, cors: string, done: (bytes: Uint8Array | null, opaque: boolean) => void): void {
  const kind = KINDS[destination] ?? 'fetch'
  const routed = route(url, 'GET')
  if (!routed) {
    Tracked.start(kind, 'GET', url)?.error(unreachable(url))
    failed(url, unreachable(url))
    return done(null, false)
  }
  if ('asset' in routed) {
    served(kind, url, routed.asset)
    return done(routed.asset.bytes, false)
  }
  const font = destination === 'font'
  const accept = ACCEPT[destination]
  const headers: Header[] = accept ? [['accept', accept]] : []
  const credentials = font || cors === 'anonymous' ? 'same-origin' : 'include'
  const request = { url, method: 'GET', headers, body: null, redirect: 'follow', credentials, mode: font || cors ? 'cors' : 'script' }
  const chunks: Uint8Array[] = []
  const own = new URL(net.base()).origin
  let wrong: string | null = null
  let opaque = false
  exchange(routed.target, request, {
    head(head) {
      wrong = problem(destination, head.status, headerOf(head.headers, 'content-type') ?? '')
      opaque = request.mode !== 'cors' && (new URL(url).origin !== own || new URL(head.url).origin !== own)
    },
    chunk(data) {
      chunks.push(data)
    },
    end() {
      if (wrong) failed(url, wrong)
      done(wrong && destination !== 'image' ? null : concat(chunks), opaque)
    },
    error(message) {
      failed(url, message)
      done(null, false)
    },
  }, kind)
}
