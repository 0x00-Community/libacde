import { G, host } from './host'
import { observer, type ConsoleLevel } from './observe'

const enum Level {
  Log = 0,
  Warn = 1,
  Error = 2,
  Debug = 3,
}

type Console = Record<string, (...args: never[]) => void>
type Method = (...args: unknown[]) => void

const OURS = Symbol('acde console')

function inspectorConsole(): Record<string, Method | undefined> | null {
  if (!host.inspected?.()) return null
  const current = (G as { console?: Record<PropertyKey, unknown> }).console
  if (!current || current[OURS]) return null
  return current as Record<string, Method | undefined>
}

const NAMES: Readonly<Record<Level, ConsoleLevel>> = { [Level.Log]: 'log', [Level.Warn]: 'warn', [Level.Error]: 'error', [Level.Debug]: 'debug' }

export function describe(value: unknown): string {
  return typeof value === 'string' ? value : inspect(value)
}

export const inspectHooks = { node: null as null | ((value: object) => string | null) }

function inspect(value: unknown, depth = 2, seen: Set<object> = new Set()): string {
  switch (typeof value) {
    case 'string': return depth < 2 ? JSON.stringify(value) : value
    case 'number': return Object.is(value, -0) ? '-0' : String(value)
    case 'bigint': return `${value}n`
    case 'undefined': return 'undefined'
    case 'boolean': return String(value)
    case 'symbol': return value.toString()
    case 'function': return `[Function: ${value.name || '(anonymous)'}]`
    default:
  }
  if (value === null) return 'null'
  const object = value as object
  if (object instanceof Error) return object.stack ?? `${object.name}: ${object.message}`
  const node = inspectHooks.node?.(object)
  if (node) return node
  if (seen.has(object)) return '[Circular]'
  if (depth < 0) return Array.isArray(object) ? '[Array]' : '[Object]'
  seen.add(object)
  try {
    if (Array.isArray(object)) {
      const items = object.slice(0, 100).map((v) => inspect(v, depth - 1, seen))
      if (object.length > 100) items.push(`... ${object.length - 100} more items`)
      return `[ ${items.join(', ')} ]`
    }
    if (object instanceof Map) return `Map(${object.size}) { ${[...object].slice(0, 50).map(([k, v]) => `${inspect(k, depth - 1, seen)} => ${inspect(v, depth - 1, seen)}`).join(', ')} }`
    if (object instanceof Set) return `Set(${object.size}) { ${[...object].slice(0, 50).map((v) => inspect(v, depth - 1, seen)).join(', ')} }`
    if (ArrayBuffer.isView(object)) return `${object.constructor.name}(${(object as Uint8Array).length ?? object.byteLength})`
    if (object instanceof Date) return Number.isNaN(object.getTime()) ? 'Invalid Date' : object.toISOString()
    if (object instanceof RegExp) return String(object)
    if (object instanceof Promise) return 'Promise { ... }'
    const name = object.constructor && object.constructor !== Object ? `${object.constructor.name} ` : ''
    const keys = Object.keys(object)
    const parts = keys.slice(0, 50).map((k) => `${/^[A-Za-z_$][\w$]*$/.test(k) ? k : JSON.stringify(k)}: ${inspect((object as Record<string, unknown>)[k], depth - 1, seen)}`)
    if (keys.length > 50) parts.push(`... ${keys.length - 50} more`)
    return parts.length ? `${name}{ ${parts.join(', ')} }` : `${name}{}`
  } catch {
    return '[object]'
  } finally {
    seen.delete(object)
  }
}

function format(args: readonly unknown[]): string {
  if (args.length === 0) return ''
  const first = args[0]
  let i = 1
  let out = ''
  if (typeof first === 'string' && first.includes('%')) {
    out = first.replace(/%[sdifoOcj%]/g, (spec) => {
      if (spec === '%%') return '%'
      if (i >= args.length) return spec
      const arg = args[i++]
      switch (spec) {
        case '%s': return typeof arg === 'string' ? arg : inspect(arg, 1)
        case '%d':
        case '%i': return typeof arg === 'symbol' ? 'NaN' : String(spec === '%i' ? Math.trunc(Number(arg)) : Number(arg))
        case '%f': return String(Number(arg))
        case '%c': return ''
        default: return inspect(arg, 4)
      }
    })
  } else out = typeof first === 'string' ? first : inspect(first)
  for (; i < args.length; i++) out += ` ${typeof args[i] === 'string' ? args[i] : inspect(args[i])}`
  return out
}

export function createConsole(): Console {
  const inspector = inspectorConsole()
  let indent = ''
  const counts = new Map<string, number>()
  const times = new Map<string, number>()
  const print = (level: Level, args: readonly unknown[], name: ConsoleLevel = NAMES[level]) => {
    const text = format(args)
    const shown = indent ? text.replace(/^/gm, indent) : text
    host.print(level, shown)
    observer?.console?.(name, shown)
  }
  const noop = () => {}
  const group = (...args: unknown[]) => {
    if (args.length) print(Level.Log, args)
    indent += '  '
  }
  const console = {
    log: (...args: unknown[]) => print(Level.Log, args),
    info: (...args: unknown[]) => print(Level.Log, args, 'info'),
    debug: (...args: unknown[]) => print(Level.Debug, args),
    warn: (...args: unknown[]) => print(Level.Warn, args),
    error: (...args: unknown[]) => print(Level.Error, args),
    trace: (...args: unknown[]) => print(Level.Error, [`Trace: ${format(args)}\n${(new Error().stack ?? '').split('\n').slice(2).join('\n')}`]),
    dir: (value: unknown) => print(Level.Log, [inspect(value, 4)]),
    dirxml: (...args: unknown[]) => print(Level.Log, args),
    table: (value: unknown) => print(Level.Log, [inspect(value, 3)]),
    assert: (condition: unknown, ...args: unknown[]) => {
      if (!condition) print(Level.Error, ['Assertion failed:', ...args])
    },
    count: (label = 'default') => {
      const n = (counts.get(label) ?? 0) + 1
      counts.set(label, n)
      print(Level.Log, [`${label}: ${n}`])
    },
    countReset: (label = 'default') => void counts.delete(label),
    group: (...args: unknown[]) => group(...args),
    groupCollapsed: (...args: unknown[]) => group(...args),
    groupEnd: () => {
      indent = indent.slice(2)
    },
    time: (label = 'default') => void times.set(label, host.now()),
    timeLog: (label = 'default', ...args: unknown[]) => {
      const start = times.get(label)
      if (start !== undefined) print(Level.Log, [`${label}: ${(host.now() - start).toFixed(3)}ms`, ...args])
    },
    timeEnd: (label = 'default') => {
      const start = times.get(label)
      if (start === undefined) return
      times.delete(label)
      print(Level.Log, [`${label}: ${(host.now() - start).toFixed(3)}ms`])
    },
    clear: noop,
    timeStamp: noop,
    profile: noop,
    profileEnd: noop,
  }
  if (inspector) {
    const methods = console as unknown as Record<string, Method>
    for (const name of Object.keys(methods)) {
      const own = methods[name]!
      const native = inspector[name]
      if (typeof native !== 'function') continue
      methods[name] = (...args: unknown[]) => {
        try {
          native.apply(inspector, args)
        } catch {}
        own(...args)
      }
    }
  }
  Object.defineProperty(console, OURS, { value: true })
  Object.defineProperty(console, Symbol.toStringTag, { value: 'console', configurable: true })
  return console as unknown as Console
}
