import { basename, join } from '../../../page/src/sys/path'

export const DIRECTORY = 'inode/directory'
export const UNKNOWN = 'application/octet-stream'
const OWN_LIST = 'acde-mimeapps.list'
const DEFAULTS = '[Default Applications]'
const ENTRY = '[Desktop Entry]'
const ACDE_PREFIX = 'acde-'
export const DESKTOP_SUFFIX = '.desktop'

const EXTENSIONS: Readonly<Record<string, string>> = {
  txt: 'text/plain', log: 'text/plain', conf: 'text/plain', ini: 'text/plain', md: 'text/markdown', markdown: 'text/markdown',
  html: 'text/html', htm: 'text/html', css: 'text/css', csv: 'text/csv', js: 'text/javascript', mjs: 'text/javascript',
  ts: 'text/x-typescript', rs: 'text/x-rust', py: 'text/x-python', sh: 'application/x-shellscript', json: 'application/json',
  xml: 'application/xml', pdf: 'application/pdf', zip: 'application/zip', png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg',
  gif: 'image/gif', webp: 'image/webp', svg: 'image/svg+xml', mp3: 'audio/mpeg', ogg: 'audio/ogg', wav: 'audio/wav', flac: 'audio/flac',
  mp4: 'video/mp4', webm: 'video/webm', mkv: 'video/x-matroska', doc: 'application/msword',
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
  pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
}

export interface OpenFiles {
  readText(path: string): string | null
  readDir(path: string): string[]
  isDirectory(path: string): boolean
  exists(path: string): boolean
  writeText(path: string, text: string): void
}

export interface OpenPlaces {
  readonly data: readonly string[]
  readonly config: string
  readonly configDirs: readonly string[]
  readonly applications: readonly string[]
}

export interface Native {
  readonly id: string
  readonly label: string
  readonly file: string
  readonly icon: string | null
}

export type Chosen = { readonly kind: 'acde'; readonly app: string } | { readonly kind: 'native'; readonly native: Native }

interface Glob {
  readonly weight: number
  readonly mime: string
  readonly key: string
  readonly exactCase: boolean
}

function parseGlob(line: string): Glob | null {
  if (line.startsWith('#')) return null
  const [weight, mime, pattern, flags] = line.split(':', 4)
  const value = Number(weight)
  if (!Number.isInteger(value) || !mime || !pattern) return null
  const exactCase = (flags ?? '').split(',').includes('cs')
  return { weight: value, mime, key: exactCase ? pattern : pattern.toLowerCase(), exactCase }
}

function globMatches(glob: Glob, name: string, lower: string): boolean {
  const wild = (s: string) => /[*?[]/.test(s)
  const subject = glob.exactCase ? name : lower
  if (glob.key.startsWith('*')) {
    const suffix = glob.key.slice(1)
    return !wild(suffix) && subject.endsWith(suffix)
  }
  return !wild(glob.key) && subject === glob.key
}

export function group(text: string, name: string): [string, string][] {
  const out: [string, string][] = []
  let inside = false
  for (const raw of text.split('\n')) {
    const line = raw.trim()
    if (line.startsWith('[')) {
      inside = line === name
      continue
    }
    if (!inside || line.startsWith('#')) continue
    const at = line.indexOf('=')
    if (at > 0) out.push([line.slice(0, at).trim(), line.slice(at + 1).trim()])
  }
  return out
}

export function withDefault(text: string, mime: string, id: string): string {
  const out: string[] = []
  let inside = false
  let written = false
  for (const line of text.split('\n').filter((l, i, all) => i < all.length - 1 || l !== '')) {
    const trimmed = line.trim()
    if (trimmed.startsWith('[')) {
      if (inside && !written) {
        out.push(`${mime}=${id};`)
        written = true
      }
      inside = trimmed === DEFAULTS
    } else if (inside && trimmed.includes('=') && trimmed.slice(0, trimmed.indexOf('=')).trim() === mime) {
      if (!written) {
        out.push(`${mime}=${id};`)
        written = true
      }
      continue
    }
    out.push(line)
  }
  if (!written) {
    if (!inside) out.push(DEFAULTS)
    out.push(`${mime}=${id};`)
  }
  return `${out.join('\n')}\n`
}

export class LocalOpener {
  #globs: readonly Glob[] | null = null

  constructor(
    readonly files: OpenFiles,
    readonly places: OpenPlaces,
  ) {}

  #loadGlobs(): readonly Glob[] {
    if (this.#globs) return this.#globs
    const globs: Glob[] = []
    for (const dir of this.places.data) {
      const text = this.files.readText(join(dir, 'mime', 'globs2'))
      if (text !== null) for (const line of text.split('\n')) {
        const glob = parseGlob(line.trim())
        if (glob) globs.push(glob)
      }
    }
    this.#globs = globs
    return globs
  }

  mimeOf(path: string): string {
    if (this.files.isDirectory(path)) return DIRECTORY
    const name = basename(path)
    const lower = name.toLowerCase()
    let best: Glob | null = null
    for (const glob of this.#loadGlobs()) {
      if (!globMatches(glob, name, lower)) continue
      if (!best || glob.weight > best.weight || (glob.weight === best.weight && glob.key.length > best.key.length)) best = glob
    }
    if (best) return best.mime
    const dot = lower.lastIndexOf('.')
    return (dot > 0 && EXTENSIONS[lower.slice(dot + 1)]) || UNKNOWN
  }

  #lists(): string[] {
    return [
      join(this.places.config, OWN_LIST), join(this.places.config, 'mimeapps.list'),
      ...this.places.configDirs.map((dir) => join(dir, 'mimeapps.list')),
      ...this.places.applications.map((dir) => join(dir, 'mimeapps.list')),
    ]
  }

  remembered(mime: string): string[] {
    const found: string[] = []
    for (const list of this.#lists()) {
      const text = this.files.readText(list)
      if (text === null) continue
      const value = group(text, DEFAULTS).find(([key]) => key === mime)?.[1]
      const id = value?.split(';').map((s) => s.trim()).find(Boolean)
      if (id) found.push(id)
    }
    return found
  }

  remember(mime: string, id: string): void {
    const list = join(this.places.config, OWN_LIST)
    this.files.writeText(list, withDefault(this.files.readText(list) ?? '', mime, id))
  }

  natives(mime: string): Native[] {
    const seen = new Set<string>()
    const found: Native[] = []
    for (const dir of this.places.applications) {
      for (const name of [...this.files.readDir(dir)].sort()) {
        if (seen.has(name)) continue
        seen.add(name)
        if (name.startsWith(ACDE_PREFIX) || !name.endsWith(DESKTOP_SUFFIX)) continue
        const file = join(dir, name)
        const keys = new Map(group(this.files.readText(file) ?? '', ENTRY))
        const shown = keys.get('Type') === 'Application' && keys.get('NoDisplay') !== 'true' && keys.get('Hidden') !== 'true'
        const label = keys.get('Name')
        if (!shown || !label) continue
        const types = (keys.get('MimeType') ?? '').split(';').filter(Boolean)
        if (types.includes(mime)) found.push({ id: name, label, file, icon: keys.get('Icon') || null })
      }
    }
    return found
  }

  static acdeId(desktopId: string): string | null {
    return desktopId.startsWith(ACDE_PREFIX) && desktopId.endsWith(DESKTOP_SUFFIX) ? desktopId.slice(ACDE_PREFIX.length, -DESKTOP_SUFFIX.length) : null
  }

  static desktopId(app: string): string {
    return `${ACDE_PREFIX}${app}${DESKTOP_SUFFIX}`
  }

  chosen(mime: string, natives: readonly Native[], isApp: (id: string) => boolean, forType: (mime: string) => string | null): Chosen | null {
    for (const id of this.remembered(mime)) {
      const app = LocalOpener.acdeId(id)
      if (app && isApp(app)) return { kind: 'acde', app }
      const native = natives.find((n) => n.id === id)
      if (native) return { kind: 'native', native }
    }
    const app = forType(mime)
    if (app) return { kind: 'acde', app }
    return natives[0] ? { kind: 'native', native: natives[0] } : null
  }
}
