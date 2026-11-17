import { Level, Log } from './log'
import { readTextOr } from './sys/fs'
import { sys } from './sys/natives'
import { xdgDirs } from './sys/os'
import { join } from './sys/path'

export interface MimeSource {
  readonly globs2: string | null
  readonly subclasses: string | null
}

interface Glob {
  readonly weight: number
  readonly mime: string
  readonly pattern: string
  readonly literal: boolean
  readonly cs: boolean
  readonly test: (name: string) => boolean
}

const WILDCARD = /[*?[]/

function matcher(pattern: string, caseSensitive: boolean): (name: string) => boolean {
  const fold = (text: string) => (caseSensitive ? text : text.toLowerCase())
  const wanted = fold(pattern)
  if (!WILDCARD.test(pattern)) return (name) => fold(name) === wanted
  if (wanted.startsWith('*') && !WILDCARD.test(wanted.slice(1))) {
    const suffix = wanted.slice(1)
    return (name) => fold(name).endsWith(suffix)
  }
  let source = ''
  for (let i = 0; i < wanted.length; i++) {
    const c = wanted[i]!
    if (c === '*') source += '.*'
    else if (c === '?') source += '.'
    else if (c === '[') {
      const end = wanted.indexOf(']', i + 2)
      if (end < 0) source += '\\['
      else {
        const body = wanted.slice(i + 1, end)
        source += `[${body.startsWith('!') ? `^${body.slice(1)}` : body}]`.replace(/\\/g, '\\\\')
        i = end
      }
    } else source += c.replace(/[.+^${}()|\\\]]/g, '\\$&')
  }
  const re = new RegExp(`^${source}$`, 's')
  return (name) => re.test(fold(name))
}

export class MimeDb {
  readonly #globs: readonly Glob[]
  readonly #parents: ReadonlyMap<string, readonly string[]>

  private constructor(globs: readonly Glob[], parents: ReadonlyMap<string, readonly string[]>) {
    this.#globs = globs
    this.#parents = parents
  }

  static parse(sources: readonly MimeSource[]): MimeDb | null {
    if (!sources.some((s) => s.globs2 !== null)) return null
    const globs: Glob[] = []
    const cleared = new Set<string>()
    const parents = new Map<string, string[]>()
    for (const source of sources) {
      const here: Glob[] = []
      const clearing = new Set<string>()
      for (const line of (source.globs2 ?? '').split('\n')) {
        if (!line || line.startsWith('#')) continue
        const [weight, mime, pattern, flags] = line.split(':')
        if (!weight || !mime || pattern === undefined || pattern === '') continue
        if (cleared.has(mime)) continue
        if (pattern === '__NOGLOBS__') {
          clearing.add(mime)
          continue
        }
        const literal = !WILDCARD.test(pattern)
        const cs = (flags ?? '').split(',').includes('cs')
        here.push({ weight: Number(weight) || 50, mime, pattern, literal, cs, test: matcher(pattern, cs) })
      }
      globs.push(...here)
      for (const mime of clearing) cleared.add(mime)
      for (const line of (source.subclasses ?? '').split('\n')) {
        const [child, parent] = line.trim().split(/\s+/)
        if (!child || !parent || child.startsWith('#')) continue
        const list = parents.get(child) ?? []
        if (!list.includes(parent)) list.push(parent)
        parents.set(child, list)
      }
    }
    return new MimeDb(globs, parents)
  }

  candidates(name: string): string[] {
    const matched = this.#globs.filter((g) => g.test(name))
    const literal = matched.filter((g) => g.literal)
    const pool = literal.length ? literal : matched
    if (!pool.length) return []
    const rank = (g: Glob) => [g.weight, g.pattern.length, g.cs ? 1 : 0] as const
    const better = (a: readonly number[], b: readonly number[]) => b[0]! - a[0]! || b[1]! - a[1]! || b[2]! - a[2]!
    const top = pool.map(rank).sort(better)[0]!
    return [...new Set(pool.filter((g) => better(rank(g), top) === 0).map((g) => g.mime))]
  }

  isText(mime: string): boolean {
    const seen = new Set<string>()
    const walk = (m: string): boolean => {
      if (m === 'text/plain' || m.startsWith('text/')) return true
      if (seen.has(m)) return false
      seen.add(m)
      return (this.#parents.get(m) ?? []).some(walk)
    }
    return walk(mime)
  }
}

export interface TypeSources {
  readonly registered: ((extension: string) => string | null) | null
  readonly database: () => MimeDb | null
}

export class FileTypes {
  static readonly UNKNOWN = ''
  static readonly #MIME = /^[\w.+-]+\/[\w.+-]+$/
  static #system: FileTypes | null = null

  readonly #sources: TypeSources

  constructor(sources: TypeSources) {
    this.#sources = sources
  }

  static valid(mime: string): boolean {
    return FileTypes.#MIME.test(mime)
  }

  static extension(name: string): string {
    const base = name.slice(Math.max(name.lastIndexOf('/'), name.lastIndexOf('\\')) + 1)
    const dot = base.lastIndexOf('.')
    const ext = dot < 0 ? '' : base.slice(dot).toLowerCase()
    return /^\.[^\s.\\/:*?"<>|]+$/.test(ext) ? ext : ''
  }

  get associations(): boolean {
    return this.#sources.registered !== null
  }

  byName(name: string): string {
    const registered = this.#sources.registered
    if (registered) {
      const ext = FileTypes.extension(name)
      const mime = ext ? (registered(ext)?.trim().toLowerCase() ?? '') : ''
      return FileTypes.valid(mime) ? mime : FileTypes.UNKNOWN
    }
    return this.#sources.database()?.candidates(name)[0] ?? FileTypes.UNKNOWN
  }

  database(): MimeDb | null {
    return this.#sources.database()
  }

  static system(): FileTypes {
    if (FileTypes.#system) return FileTypes.#system
    const contentType = sys().contentType
    let db: MimeDb | null | undefined
    const database = () => (db === undefined ? (db = FileTypes.#sharedMimeInfo()) : db)
    FileTypes.#system = new FileTypes({ registered: contentType ? (ext) => contentType(ext) : null, database })
    return FileTypes.#system
  }

  static #sharedMimeInfo(): MimeDb | null {
    const macos = sys().platform === 'darwin' ? ['/opt/homebrew/share/mime', '/usr/local/share/mime', '/opt/local/share/mime'] : []
    const dirs = [...new Set([...xdgDirs('data', 'mime'), ...macos])]
    const db = MimeDb.parse(dirs.map((dir) => ({ globs2: readTextOr(join(dir, 'globs2'), null), subclasses: readTextOr(join(dir, 'subclasses'), null) })))
    if (!db) {
      const hint = sys().platform === 'darwin' ? ' (brew install shared-mime-info)' : ''
      Log.write(Level.Warn, 'acde_host', `no shared-mime-info database${hint}: file types come from file(1) and may differ from Dopame OS`)
    }
    return db
  }
}
