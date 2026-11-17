import { setAttribute, type Element } from './element'
import { URL } from '../core/url'
import { resolveUrl } from './env'
import { dom, ID } from './realm'

type Proto = object

function define(proto: Proto, prop: string, get: (this: Element) => unknown, set: (this: Element, value: unknown) => void): void {
  Object.defineProperty(proto, prop, { get, set, enumerable: true, configurable: true })
}

export function string(proto: Proto, prop: string, attr = prop.toLowerCase()): void {
  define(proto, prop, function () {
    return dom.getAttribute(this[ID], attr) ?? ''
  }, function (value) {
    setAttribute(this, attr, String(value))
  })
}

export function nullable(proto: Proto, prop: string, attr = prop.toLowerCase()): void {
  define(proto, prop, function () {
    return dom.getAttribute(this[ID], attr)
  }, function (value) {
    setAttribute(this, attr, value === null ? null : String(value))
  })
}

export function bool(proto: Proto, prop: string, attr = prop.toLowerCase()): void {
  define(proto, prop, function () {
    return dom.hasAttribute(this[ID], attr)
  }, function (value) {
    const on = !!value
    if (on !== dom.hasAttribute(this[ID], attr)) setAttribute(this, attr, on ? '' : null)
  })
}

export function long(proto: Proto, prop: string, fallback: number, attr = prop.toLowerCase(), min = -Infinity): void {
  define(proto, prop, function () {
    const raw = dom.getAttribute(this[ID], attr)
    if (raw === null) return fallback
    const n = parseInt(raw, 10)
    return Number.isNaN(n) || n < min ? fallback : n
  }, function (value) {
    setAttribute(this, attr, String(Math.trunc(Number(value)) || 0))
  })
}

export function url(proto: Proto, prop: string, attr = prop.toLowerCase()): void {
  define(proto, prop, function () {
    const raw = dom.getAttribute(this[ID], attr)
    return raw === null ? '' : resolveUrl(raw)
  }, function (value) {
    setAttribute(this, attr, String(value))
  })
}

export function urlParts(proto: Proto, parts: readonly string[], read: (self: never) => URL | null, write: (self: never, url: URL) => void): void {
  for (const part of parts) {
    Object.defineProperty(proto, part, {
      get(this: never) {
        const u = read(this)
        return u ? u[part as 'href'] : ''
      },
      set: part === 'origin' ? undefined : function (this: never, value: unknown) {
        const u = read(this)
        if (!u) return
        const next = new URL(u.href)
        next[part as 'href'] = String(value)
        write(this, next)
      },
      enumerable: true,
      configurable: true,
    })
  }
}
