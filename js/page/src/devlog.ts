import type { ConsoleLevel, DialogType, PageErrorKind, RequestEvent, RequestKind } from '@dopame/acde-dom'

export type DevPost =
  | { readonly devlog: 'console'; readonly time: number; readonly level: ConsoleLevel; readonly text: string }
  | { readonly devlog: 'error'; readonly time: number; readonly kind: PageErrorKind; readonly text: string }
  | { readonly devlog: 'request'; readonly time: number; readonly event: RequestEvent }
  | { readonly devlog: 'dialog'; readonly time: number; readonly dialog: DialogEntry }

export interface DialogEntry {
  readonly type: DialogType
  readonly message: string
  readonly defaultValue: string | null
  readonly answered: boolean
  readonly accept: boolean
  readonly text: string | null
}

export interface ConsoleEntry {
  readonly seq: number
  readonly time: number
  readonly web: string
  readonly app: string | null
  readonly kind: 'console' | 'dialog' | PageErrorKind
  readonly level: ConsoleLevel
  readonly text: string
  readonly dialog?: DialogEntry
}

export interface RequestEntry {
  readonly seq: number
  readonly time: number
  readonly web: string
  readonly app: string | null
  readonly id: number
  readonly kind: RequestKind
  readonly method: string
  readonly url: string
  readonly requestBytes: number
  status: number | null
  statusText: string | null
  responseBytes: number | null
  ms: number | null
  error: string | null
  done: boolean
  readonly mocked?: true
}

export interface DevQuery {
  readonly web?: string
  readonly since?: number
  readonly limit?: number
}

export interface DevPage<T> {
  readonly entries: readonly T[]
  readonly next: number
  readonly dropped: number
}

interface Source {
  readonly name: string
  readonly app: string | null
}

export class DevLog {
  static readonly KEEP = 5000
  static readonly TEXT_MAX = 16_384
  static readonly LIMIT = 500

  readonly #console: ConsoleEntry[] = []
  readonly #requests: RequestEntry[] = []
  readonly #open = new Map<string, RequestEntry>()
  #nextConsole = 1
  #nextRequest = 1

  static isPost(message: unknown): message is DevPost {
    return typeof message === 'object' && message !== null && typeof (message as { devlog?: unknown }).devlog === 'string'
  }

  static text(text: string): string {
    return text.length > DevLog.TEXT_MAX ? `${text.slice(0, DevLog.TEXT_MAX)}… (${text.length - DevLog.TEXT_MAX} more characters)` : text
  }

  take(source: Source, post: DevPost): void {
    switch (post.devlog) {
      case 'console':
        return this.#log({ seq: this.#nextConsole++, time: post.time, web: source.name, app: source.app, kind: 'console', level: post.level, text: DevLog.text(post.text) })
      case 'error':
        return this.#log({ seq: this.#nextConsole++, time: post.time, web: source.name, app: source.app, kind: post.kind, level: 'error', text: DevLog.text(post.text) })
      case 'request':
        return this.#request(source, post.time, post.event)
      case 'dialog': {
        const d = post.dialog
        const outcome = d.type === 'alert' ? (d.answered ? 'dismissed' : 'shown') : d.accept ? `accepted${d.type === 'prompt' ? ` with ${JSON.stringify(d.text ?? '')}` : ''}` : 'dismissed'
        const text = `${d.type}(${JSON.stringify(d.message)}) ${outcome}${d.answered ? '' : ' (no answer was given)'}`
        return this.#log({ seq: this.#nextConsole++, time: post.time, web: source.name, app: source.app, kind: 'dialog', level: 'info', text: DevLog.text(text), dialog: { ...d, message: DevLog.text(d.message) } })
      }
    }
  }

  gone(web: string): void {
    for (const [key, entry] of this.#open) {
      if (entry.web !== web) continue
      entry.error ??= 'the page went away'
      entry.done = true
      this.#open.delete(key)
    }
  }

  console(query: DevQuery & { readonly level?: ConsoleLevel | 'errors' }): DevPage<ConsoleEntry> {
    const level = query.level
    return this.#page(this.#console, this.#nextConsole, query, (e) => level === undefined || (level === 'errors' ? e.level === 'error' : e.level === level))
  }

  requests(query: DevQuery & { readonly failed?: boolean }): DevPage<RequestEntry> {
    return this.#page(this.#requests, this.#nextRequest, query, (e) => !query.failed || e.error !== null || (e.status !== null && e.status >= 400))
  }

  #log(entry: ConsoleEntry): void {
    this.#console.push(entry)
    if (this.#console.length > DevLog.KEEP) this.#console.splice(0, this.#console.length - DevLog.KEEP)
  }

  #request(source: Source, time: number, event: RequestEvent): void {
    const key = `${source.name}#${event.id}`
    if (event.phase === 'start') {
      const entry: RequestEntry = {
        seq: this.#nextRequest++,
        time,
        web: source.name,
        app: source.app,
        id: event.id,
        kind: event.kind,
        method: event.method,
        url: DevLog.text(event.url),
        requestBytes: event.bytes,
        status: null,
        statusText: null,
        responseBytes: null,
        ms: null,
        error: null,
        done: false,
        ...(event.mocked && { mocked: true as const }),
      }
      this.#requests.push(entry)
      this.#open.set(key, entry)
      if (this.#requests.length > DevLog.KEEP) {
        for (const old of this.#requests.splice(0, this.#requests.length - DevLog.KEEP)) this.#open.delete(`${old.web}#${old.id}`)
      }
      return
    }
    const entry = this.#open.get(key)
    if (!entry) return
    switch (event.phase) {
      case 'response':
        entry.status = event.status
        entry.statusText = event.statusText
        return
      case 'end':
        entry.responseBytes = event.bytes
        break
      case 'error':
        entry.error = DevLog.text(event.message)
        break
    }
    entry.ms = Math.round((time - entry.time) * 1000) / 1000
    entry.done = true
    this.#open.delete(key)
  }

  #page<T extends { readonly seq: number; readonly web: string; readonly app: string | null }>(all: readonly T[], next: number, query: DevQuery, keep: (entry: T) => boolean): DevPage<T> {
    const since = query.since ?? 0
    const limit = Math.max(1, Math.min(query.limit ?? DevLog.LIMIT, DevLog.KEEP))
    const web = query.web
    const matching = all.filter((e) => e.seq >= since && (web === undefined || e.web === web || e.app === web || (web === 'desktop' && e.app === null)) && keep(e))
    const first = all[0]?.seq ?? next
    return { entries: matching.slice(-limit), next, dropped: since > 0 && since < first ? first - since : 0 }
  }
}

type Segment = readonly [column: number, source: number, line: number, sourceColumn: number]

interface SourceMap {
  readonly sources: readonly string[]
  readonly lines: readonly (readonly Segment[])[]
}

export class SourceMaps {
  static readonly #BASE64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
  static readonly #FRAME = /(https?:\/\/[^\s()]+?\.m?js)(?:\?[^\s():]*)?:(\d+):(\d+)/g
  readonly #read: (url: string) => Uint8Array | null
  readonly #readMap: (url: string) => Uint8Array | null
  readonly #maps = new Map<string, SourceMap | null>()

  constructor(read: (url: string) => Uint8Array | null, readMap: (url: string) => Uint8Array | null) {
    this.#read = read
    this.#readMap = readMap
  }

  stack(text: string): string {
    if (!text.includes('.js:')) return text
    return text.replace(SourceMaps.#FRAME, (whole, url: string, line: string, column: string) => {
      const map = this.#map(url)
      const found = map ? SourceMaps.#find(map, Number(line) - 1, Number(column) - 1) : null
      return found ?? whole
    })
  }

  #map(url: string): SourceMap | null {
    if (this.#maps.has(url)) return this.#maps.get(url)!
    let map: SourceMap | null = null
    try {
      map = this.#load(url)
    } catch {
      map = null
    }
    this.#maps.set(url, map)
    return map
  }

  #load(url: string): SourceMap | null {
    const code = this.#read(url)
    if (!code) return null
    const text = new TextDecoder().decode(code)
    const link = /\/\/[#@] sourceMapping[U]RL=(\S+)\s*$/.exec(text.slice(-4096))?.[1]
    if (!link) return null
    let json: string
    let base = url
    const inline = /^data:application\/json[^,]*;base64,(.+)$/.exec(link)
    if (inline) json = new TextDecoder().decode(Uint8Array.from(atob(inline[1]!), (c) => c.charCodeAt(0)))
    else {
      base = new URL(link, url).href
      const bytes = this.#readMap(base)
      if (!bytes) return null
      json = new TextDecoder().decode(bytes)
    }
    const raw = JSON.parse(json) as { sources?: string[]; sourceRoot?: string; mappings?: string }
    if (typeof raw.mappings !== 'string' || !Array.isArray(raw.sources)) return null
    const root = raw.sourceRoot ? `${raw.sourceRoot.replace(/\/?$/, '/')}` : ''
    const origin = new URL(url).origin
    const sources = raw.sources.map((source) => {
      const resolved = new URL(root + source, base)
      return resolved.origin === origin ? resolved.pathname.replace(/^\//, '') : resolved.href
    })
    return { sources, lines: SourceMaps.#decode(raw.mappings) }
  }

  static #decode(mappings: string): Segment[][] {
    const lines: Segment[][] = []
    let [source, line, sourceColumn] = [0, 0, 0]
    for (const text of mappings.split(';')) {
      const segments: Segment[] = []
      let column = 0
      for (const part of text.split(',')) {
        if (!part) continue
        const values: number[] = []
        let value = 0
        let shift = 0
        for (const char of part) {
          const digit = SourceMaps.#BASE64.indexOf(char)
          if (digit < 0) return lines
          value += (digit & 31) << shift
          if (digit & 32) shift += 5
          else {
            values.push(value & 1 ? -(value >>> 1) : value >>> 1)
            value = 0
            shift = 0
          }
        }
        column += values[0] ?? 0
        if (values.length < 4) continue
        source += values[1]!
        line += values[2]!
        sourceColumn += values[3]!
        segments.push([column, source, line, sourceColumn])
      }
      lines.push(segments)
    }
    return lines
  }

  static #find(map: SourceMap, line: number, column: number): string | null {
    const segments = map.lines[line]
    if (!segments?.length) return null
    let found: Segment | null = null
    for (const segment of segments) {
      if (segment[0] > column) break
      found = segment
    }
    if (!found) return null
    const source = map.sources[found[1]]
    return source === undefined ? null : `${source}:${found[2] + 1}:${found[3] + 1}`
  }
}
