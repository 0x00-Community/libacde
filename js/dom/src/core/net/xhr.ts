import { Blob } from '../blob'
import type { Header, Target } from '../backend'
import { mocked, Tracked, type MockReply } from '../observe'
import { concat, TextDecoder } from '../encoding'
import { defineHandlers, Event, EventTarget, fire, ProgressEvent } from '../event'
import { DOMException } from '../exception'
import { constants, getters, host, tag } from '../host'
import { readAllBytes } from '../streams'
import { URL } from '../url'
import { Headers } from './headers'
import { type BodyInit, NULL_BODY, assetHead, backendFailed, exchange, extractBody, headerOf, net, route, unreachable } from './index'

export class XMLHttpRequestEventTarget extends EventTarget {}
defineHandlers(XMLHttpRequestEventTarget.prototype, ['loadstart', 'progress', 'abort', 'error', 'load', 'timeout', 'loadend'])
tag(XMLHttpRequestEventTarget)

export class XMLHttpRequestUpload extends XMLHttpRequestEventTarget {}
tag(XMLHttpRequestUpload)

export class XMLHttpRequest extends XMLHttpRequestEventTarget {
  readyState = 0
  status = 0
  statusText = ''
  responseType = ''
  responseURL = ''
  timeout = 0
  withCredentials = false
  readonly upload = new XMLHttpRequestUpload()
  #method = 'GET'
  #url = ''
  #async = true
  #chunks: Uint8Array[] = []
  #bytes: Uint8Array | null = null
  #type = ''
  #responseHeaders: Header[] | null = null
  #headers = new Headers()
  #sent = 0
  #stop: (() => void) | null = null

  open(method: string, url: string, async = true): void {
    this.#stop?.()
    this.#stop = null
    this.#sent++
    this.#method = String(method).toUpperCase()
    this.#url = new URL(String(url), net.base()).href
    this.#async = async !== false
    this.#bytes = null
    this.#chunks = []
    this.#responseHeaders = null
    this.#headers = new Headers()
    this.status = 0
    this.statusText = ''
    this.#state(1)
  }

  setRequestHeader(name: string, value: string): void {
    if (this.readyState !== 1) {
      throw new DOMException("Failed to execute 'setRequestHeader' on 'XMLHttpRequest': The object's state must be OPENED.", 'InvalidStateError')
    }
    this.#headers.append(name, value)
  }

  #state(state: number): void {
    this.readyState = state
    fire(this, new Event('readystatechange'))
  }

  send(body?: BodyInit): void {
    if (this.readyState !== 1) throw new DOMException("Failed to execute 'send' on 'XMLHttpRequest': The object's state must be OPENED.", 'InvalidStateError')
    const token = ++this.#sent
    const mock = mocked('xhr', this.#method, this.#url)
    if (mock) return this.#mocked(mock, token)
    const routed = route(this.#url, this.#method)
    if (routed && 'target' in routed) {
      if (!this.#async) {
        const what = `synchronous requests to the app's backend or the network are not supported ('${this.#url}').`
        throw new DOMException(`Failed to execute 'send' on 'XMLHttpRequest': ${what}`, 'NetworkError')
      }
      fire(this, new ProgressEvent('loadstart'))
      return this.#backend(routed.target, token, body)
    }
    const tracked = Tracked.start('xhr', this.#method, this.#url, 0, false, this.#headers)
    const finish = () => {
      if (token !== this.#sent) return tracked?.error('aborted')
      const found = routed?.asset
      if (!found) {
        tracked?.error(unreachable(this.#url))
        this.#state(4)
        if (!this.#async) throw new DOMException(`Failed to execute 'send' on 'XMLHttpRequest': Failed to load '${this.#url}'.`, 'NetworkError')
        fire(this, new ProgressEvent('error'))
        fire(this, new ProgressEvent('loadend'))
        return
      }
      tracked?.response(200, 'OK', assetHead(found))
      tracked?.received(found.bytes.length, found.bytes)
      tracked?.end(found.bytes.length)
      this.#bytes = found.bytes
      this.#type = found.type
      this.#responseHeaders = assetHead(found)
      this.status = 200
      this.statusText = 'OK'
      this.responseURL = this.#url
      this.#state(2)
      this.#state(3)
      this.#state(4)
      const total = found.bytes.length
      fire(this, new ProgressEvent('load', { lengthComputable: true, loaded: total, total }))
      fire(this, new ProgressEvent('loadend', { lengthComputable: true, loaded: total, total }))
    }
    if (!this.#async) return finish()
    fire(this, new ProgressEvent('loadstart'))
    host.setTimer(0, finish)
  }

  #mocked(mock: MockReply, token: number): void {
    const tracked = Tracked.start('xhr', this.#method, this.#url, 0, true, this.#headers)
    const finish = () => {
      if (token !== this.#sent) return tracked?.error('aborted')
      if ('error' in mock) {
        tracked?.error(mock.error)
        this.#state(4)
        if (!this.#async) throw new DOMException(`Failed to execute 'send' on 'XMLHttpRequest': Failed to load '${this.#url}'.`, 'NetworkError')
        fire(this, new ProgressEvent('error'))
        fire(this, new ProgressEvent('loadend'))
        return
      }
      const body = this.#method === 'HEAD' || NULL_BODY.has(mock.status) ? new Uint8Array(0) : mock.body
      tracked?.response(mock.status, mock.statusText, mock.headers)
      tracked?.received(body.length, body)
      tracked?.end(body.length)
      this.#bytes = body
      this.#responseHeaders = mock.headers.map(([name, value]) => [name, value] as Header)
      this.#type = headerOf(this.#responseHeaders, 'content-type') ?? ''
      this.status = mock.status
      this.statusText = mock.statusText
      this.responseURL = this.#url
      this.#state(2)
      this.#state(3)
      this.#state(4)
      const total = body.length
      fire(this, new ProgressEvent('load', { lengthComputable: true, loaded: total, total }))
      fire(this, new ProgressEvent('loadend', { lengthComputable: true, loaded: total, total }))
    }
    if (!this.#async) return finish()
    fire(this, new ProgressEvent('loadstart'))
    host.setTimer(0, finish)
  }

  #backend(target: Target, token: number, init: BodyInit): void {
    const { bytes, stream, type } = this.#method === 'GET' || this.#method === 'HEAD' ? { bytes: null, stream: null, type: null } : extractBody(init)
    if (type && !this.#headers.has('content-type')) this.#headers.set('content-type', type)
    const live = () => token === this.#sent
    let loaded = 0
    let total = 0
    const finish = (event: 'load' | 'error' | 'timeout', message = 'the request failed'): void => {
      this.#stop = null
      this.#bytes = this.#joined()
      if (event !== 'load') {
        this.status = 0
        this.statusText = ''
        this.#responseHeaders = null
        this.#bytes = null
        if (event === 'error') backendFailed(this.#method, this.#url, message)
      }
      this.#state(4)
      const sizes = { lengthComputable: total > 0, loaded, total }
      fire(this, new ProgressEvent(event, sizes))
      fire(this, new ProgressEvent('loadend', sizes))
    }
    const start = (sent: Uint8Array | null): void => {
      if (!live()) return
      const credentials = this.withCredentials ? 'include' : 'same-origin'
      const request = { url: this.#url, method: this.#method, headers: this.#headers, body: sent, redirect: 'follow' as const, credentials }
      this.#stop = exchange(target, { ...request, mode: 'cors' as const }, {
        head: (head) => {
          if (!live()) return
          this.status = head.status
          this.statusText = head.statusText
          this.responseURL = head.url
          this.#responseHeaders = head.headers
          this.#type = headerOf(head.headers, 'content-type') ?? ''
          total = Number(headerOf(head.headers, 'content-length') ?? 0) || 0
          this.#state(2)
        },
        chunk: (data) => {
          if (!live()) return
          this.#chunks.push(data)
          this.#bytes = null
          loaded += data.length
          this.#state(3)
          fire(this, new ProgressEvent('progress', { lengthComputable: total > 0, loaded, total }))
        },
        end: () => {
          if (live()) finish('load')
        },
        error: (message) => {
          if (live()) finish('error', message)
        },
      }, 'xhr')
      if (this.timeout > 0) {
        host.setTimer(this.timeout, () => {
          if (!live() || this.readyState === 4) return
          this.#stop?.()
          finish('timeout')
        })
      }
    }
    if (stream) readAllBytes(stream).then(start, () => live() && finish('error'))
    else start(bytes)
  }

  #joined(): Uint8Array | null {
    if (this.#bytes) return this.#bytes
    if (!this.#chunks.length) return this.#responseHeaders ? new Uint8Array(0) : null
    if (this.#chunks.length === 1) return (this.#bytes = this.#chunks[0]!)
    const out = concat(this.#chunks)
    this.#chunks = [out]
    return (this.#bytes = out)
  }

  abort(): void {
    this.#sent++
    this.#stop?.()
    this.#stop = null
    if (this.readyState === 0 || this.readyState === 4) return
    this.#state(4)
    fire(this, new ProgressEvent('abort'))
    fire(this, new ProgressEvent('loadend'))
    this.readyState = 0
  }

  get responseText(): string {
    const bytes = this.#joined()
    return bytes ? new TextDecoder().decode(bytes) : ''
  }

  get response(): unknown {
    const text = this.responseType === '' || this.responseType === 'text'
    if (text && this.readyState === 3) return this.responseText
    const bytes = this.readyState === 4 ? this.#joined() : null
    if (!bytes) return text ? '' : null
    switch (this.responseType) {
      case 'json':
        try {
          return JSON.parse(this.responseText)
        } catch {
          return null
        }
      case 'arraybuffer': return bytes.slice().buffer
      case 'blob': return new Blob([bytes], { type: this.#type })
      case 'document': return null
      default: return this.responseText
    }
  }

  getResponseHeader(name: string): string | null {
    if (this.readyState < 2 || !this.#responseHeaders) return null
    return headerOf(this.#responseHeaders, String(name).toLowerCase())
  }

  getAllResponseHeaders(): string {
    if (this.readyState < 2 || !this.#responseHeaders) return ''
    const names = [...new Set(this.#responseHeaders.map(([n]) => n.toLowerCase()))].sort()
    return names.map((n) => `${n}: ${headerOf(this.#responseHeaders!, n)}\r\n`).join('')
  }

  overrideMimeType(type: string): void {
    this.#type = String(type)
  }
}
getters(XMLHttpRequest.prototype, { responseXML: null })
defineHandlers(XMLHttpRequest.prototype, ['readystatechange'])
constants(XMLHttpRequest, { UNSENT: 0, OPENED: 1, HEADERS_RECEIVED: 2, LOADING: 3, DONE: 4 })
tag(XMLHttpRequest)
