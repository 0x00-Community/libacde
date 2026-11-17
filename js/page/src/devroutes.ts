import type { MockReply } from '@dopame/acde-dom'

export type RouteUrl = string | { readonly regex: string; readonly flags?: string }

export interface DevRoute {
  readonly id: number
  readonly url: RouteUrl
  readonly method: string | null
  readonly web: string | null
  readonly times: number | null
  readonly status: number
  readonly statusText: string
  readonly headers: readonly (readonly [string, string])[]
  readonly body: string
  readonly abort: string | null
}

interface Rule {
  readonly route: DevRoute
  readonly test: (url: string) => boolean
  readonly reply: MockReply
  left: number | null
}

const STATUS_TEXT: Readonly<Record<number, string>> = {
  200: 'OK', 201: 'Created', 202: 'Accepted', 204: 'No Content', 301: 'Moved Permanently', 302: 'Found', 304: 'Not Modified',
  400: 'Bad Request', 401: 'Unauthorized', 403: 'Forbidden', 404: 'Not Found', 409: 'Conflict', 422: 'Unprocessable Content',
  429: 'Too Many Requests', 500: 'Internal Server Error', 502: 'Bad Gateway', 503: 'Service Unavailable', 504: 'Gateway Timeout',
}

export class DevRoutes {
  #rules: Rule[] = []
  readonly #used: (id: number) => void

  constructor(routes: readonly DevRoute[], used: (id: number) => void) {
    this.#used = used
    this.set(routes)
  }

  set(routes: readonly DevRoute[]): void {
    this.#rules = routes.map((route) => ({ route, test: DevRoutes.matcher(route.url), reply: DevRoutes.#reply(route), left: route.times }))
  }

  match(method: string, url: string): MockReply | null {
    for (let i = this.#rules.length - 1; i >= 0; i--) {
      const rule = this.#rules[i]!
      if (rule.left === 0) continue
      if (rule.route.method !== null && rule.route.method !== method.toUpperCase()) continue
      if (!rule.test(url)) continue
      if (rule.left !== null) {
        rule.left--
        this.#used(rule.route.id)
      }
      return rule.reply
    }
    return null
  }

  static matcher(url: RouteUrl): (href: string) => boolean {
    if (typeof url !== 'string') {
      const regex = new RegExp(url.regex, (url.flags ?? '').replace(/[gy]/g, ''))
      return (href) => regex.test(href)
    }
    const glob = DevRoutes.glob(url)
    if (!url.startsWith('/')) return (href) => glob.test(href)
    return (href) => {
      try {
        const parsed = new URL(href)
        return glob.test(`${parsed.pathname}${parsed.search}`)
      } catch {
        return false
      }
    }
  }

  static glob(pattern: string): RegExp {
    let source = ''
    for (let i = 0; i < pattern.length; i++) {
      const c = pattern[i]!
      if (c === '*') {
        if (pattern[i + 1] === '*') {
          source += '.*'
          i++
        } else source += '[^/]*'
      } else source += c.replace(/[.+?^${}()|[\]\\]/g, '\\$&')
    }
    return new RegExp(`^${source}$`)
  }

  static statusText(status: number): string {
    return STATUS_TEXT[status] ?? ''
  }

  static parse(args: Readonly<Record<string, unknown>>, id: number): DevRoute {
    const fail = (message: string): never => {
      throw Object.assign(new Error(`route: ${message}`), { status: 400 })
    }
    const raw = args['url']
    let url: RouteUrl
    if (typeof raw === 'string' && raw) url = raw
    else if (typeof raw === 'object' && raw !== null && typeof (raw as { regex?: unknown }).regex === 'string') {
      const { regex, flags } = raw as { regex: string; flags?: unknown }
      if (flags !== undefined && typeof flags !== 'string') fail('url.flags is a string of RegExp flags')
      url = { regex, ...(typeof flags === 'string' && { flags }) }
    } else url = fail('url is a glob ("**/api/*", "/todos?*") or { regex, flags }')
    try {
      DevRoutes.matcher(url)
    } catch (error) {
      fail(`url: ${(error as Error).message}`)
    }
    const method = args['method'] === undefined || args['method'] === null ? null : typeof args['method'] === 'string' && /^[A-Za-z]+$/.test(args['method']) ? args['method'].toUpperCase() : fail('method is an HTTP method like GET')
    const web = args['web'] === undefined || args['web'] === null ? null : typeof args['web'] === 'string' && args['web'] ? args['web'] : fail('web is a web name or app id')
    const times = args['times'] === undefined || args['times'] === null ? null : Number.isInteger(args['times']) && (args['times'] as number) > 0 ? (args['times'] as number) : fail('times is a whole number above 0')
    const abort = args['abort'] === undefined || args['abort'] === null ? null : typeof args['abort'] === 'string' ? args['abort'] || 'aborted' : args['abort'] === true ? 'aborted' : fail('abort is true or the error text')
    const status = args['status'] === undefined ? 200 : Number.isInteger(args['status']) && (args['status'] as number) >= 200 && (args['status'] as number) <= 599 ? (args['status'] as number) : fail('status is a whole number from 200 to 599')
    const statusText = args['statusText'] === undefined ? DevRoutes.statusText(status) : typeof args['statusText'] === 'string' ? args['statusText'] : fail('statusText is a string')
    const rawHeaders = args['headers'] ?? {}
    if (typeof rawHeaders !== 'object' || rawHeaders === null || Array.isArray(rawHeaders)) fail('headers is an object of header names to values')
    const headers = Object.entries(rawHeaders as Record<string, unknown>).map(([name, value]) => {
      if (!/^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/.test(name)) fail(`header name ${JSON.stringify(name)} is not a token`)
      if (typeof value !== 'string') fail(`header ${name} is a string`)
      return [name.toLowerCase(), value as string] as const
    })
    const body = args['body'] === undefined || args['body'] === null ? '' : typeof args['body'] === 'string' ? args['body'] : fail('body is base64')
    if (body && !/^[A-Za-z0-9+/]*={0,2}$/.test(body)) fail('body is base64')
    if (abort !== null && (args['status'] !== undefined || body || headers.length)) fail('abort cannot come with status, headers or body')
    return { id, url, method, web, times, status, statusText, headers, body, abort }
  }

  static #reply(route: DevRoute): MockReply {
    if (route.abort !== null) return { error: route.abort }
    return { status: route.status, statusText: route.statusText, headers: route.headers, body: DevRoutes.#bytes(route.body) }
  }

  static #bytes(base64: string): Uint8Array {
    const text = atob(base64)
    const bytes = new Uint8Array(text.length)
    for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i)
    return bytes
  }
}
