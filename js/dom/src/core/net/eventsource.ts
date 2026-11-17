import type { Target } from '../backend'
import { TextDecoder } from '../encoding'
import { defineHandlers, Event, EventTarget, fire, MessageEvent } from '../event'
import { DOMException } from '../exception'
import { constants, host, tag } from '../host'
import { Tracked } from '../observe'
import { URL } from '../url'
import { backendFailed, exchange, headerOf, net, route, unreachable } from './index'

const ES_STATES = { CONNECTING: 0, OPEN: 1, CLOSED: 2 } as const

export class EventSource extends EventTarget {
  readonly url: string
  readonly withCredentials: boolean
  readyState = 0
  #target: Target | null = null
  #lastId = ''
  #retry = 3000
  #stop: (() => void) | null = null
  #timer = 0

  constructor(url: string | URL, init?: { withCredentials?: boolean }) {
    super()
    try {
      this.url = new URL(String(url), net.base()).href
    } catch {
      throw new DOMException(`Failed to construct 'EventSource': Cannot open an EventSource to '${String(url)}'. The URL is invalid.`, 'SyntaxError')
    }
    this.withCredentials = !!init?.withCredentials
    const routed = route(this.url, 'SSE')
    if (!routed || !('target' in routed)) {
      Tracked.start('eventsource', 'GET', this.url)?.error(unreachable(this.url))
      host.setTimer(0, () => {
        this.readyState = ES_STATES.CLOSED
        fire(this, new Event('error'))
      })
      return
    }
    this.#target = routed.target
    this.#connect()
  }

  #connect(): void {
    const decoder = new TextDecoder()
    const origin = new URL(this.url).origin
    let buffer = ''
    let started = false
    let data = ''
    let type = ''
    let id = this.#lastId
    let tracked: Tracked | null = null
    const dispatch = (): void => {
      this.#lastId = id
      if (!data) {
        type = ''
        return
      }
      const event = new MessageEvent(type || 'message', { data: data.endsWith('\n') ? data.slice(0, -1) : data, origin, lastEventId: this.#lastId })
      tracked?.message(event.type, String(event.data), this.#lastId)
      data = ''
      type = ''
      if (this.readyState !== ES_STATES.CLOSED) fire(this, event)
    }
    const line = (text: string): void => {
      if (!text) return dispatch()
      if (text.startsWith(':')) return
      const colon = text.indexOf(':')
      const field = colon < 0 ? text : text.slice(0, colon)
      let value = colon < 0 ? '' : text.slice(colon + 1)
      if (value.startsWith(' ')) value = value.slice(1)
      if (field === 'event') type = value
      else if (field === 'data') data += `${value}\n`
      else if (field === 'id') {
        if (!value.includes('\0')) id = value
      } else if (field === 'retry' && /^\d+$/.test(value)) this.#retry = Number(value)
    }
    const feed = (text: string, end: boolean): void => {
      if (!started && text) {
        started = true
        if (text.startsWith('\uFEFF')) text = text.slice(1)
      }
      buffer += text
      let at = 0
      for (;;) {
        const cr = buffer.indexOf('\r', at)
        const lf = buffer.indexOf('\n', at)
        const stop = cr < 0 ? lf : lf < 0 ? cr : Math.min(cr, lf)
        if (stop < 0) break
        if (stop === cr && cr === buffer.length - 1 && !end) break
        line(buffer.slice(at, stop))
        at = stop + (stop === cr && buffer[cr + 1] === '\n' ? 2 : 1)
        if (this.readyState === ES_STATES.CLOSED) return
      }
      buffer = buffer.slice(at)
    }
    const headers: [string, string][] = [['accept', 'text/event-stream'], ['cache-control', 'no-cache']]
    if (this.#lastId) headers.push(['last-event-id', this.#lastId])
    const credentials = this.withCredentials ? 'include' : 'same-origin'
    tracked = Tracked.start('eventsource', 'GET', this.url, 0, false, headers)
    this.#stop = exchange(this.#target!, { url: this.url, method: 'GET', headers, body: null, redirect: 'follow', credentials, mode: 'cors' }, {
      head: (head) => {
        if (this.readyState === ES_STATES.CLOSED) return
        const contentType = headerOf(head.headers, 'content-type') ?? ''
        if (head.status !== 200 || !/^text\/event-stream\s*(;|$)/i.test(contentType)) {
          backendFailed('SSE', this.url, `status ${head.status}, ${contentType || 'no content type'}`)
          return this.#fail()
        }
        this.readyState = ES_STATES.OPEN
        fire(this, new Event('open'))
      },
      chunk: (bytes) => {
        if (this.readyState === ES_STATES.OPEN) feed(decoder.decode(bytes, { stream: true }), false)
      },
      end: () => {
        if (this.readyState === ES_STATES.OPEN) feed(decoder.decode(), true)
        this.#reconnect()
      },
      error: (message) => {
        backendFailed('SSE', this.url, message)
        this.#reconnect()
      },
    }, 'eventsource', tracked)
  }

  #reconnect(): void {
    this.#stop = null
    if (this.readyState === ES_STATES.CLOSED) return
    this.readyState = ES_STATES.CONNECTING
    fire(this, new Event('error'))
    if (this.readyState === ES_STATES.CLOSED) return
    this.#timer = host.setTimer(this.#retry, () => {
      this.#timer = 0
      if (this.readyState === ES_STATES.CONNECTING) this.#connect()
    })
  }

  #fail(): void {
    this.#stop?.()
    this.#stop = null
    this.readyState = ES_STATES.CLOSED
    fire(this, new Event('error'))
  }

  close(): void {
    this.readyState = ES_STATES.CLOSED
    this.#stop?.()
    this.#stop = null
    if (this.#timer) host.clearTimer(this.#timer)
    this.#timer = 0
  }
}
constants(EventSource, ES_STATES)
defineHandlers(EventSource.prototype, ['open', 'message', 'error'])
tag(EventSource)
