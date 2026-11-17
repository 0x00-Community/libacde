import { hostTime } from '@dopame/acde-dom'
import type { WebInternals } from '@dopame/acde-dom'
import type { Rpc } from './rpc'
import { existsSync, readFileSync, stat, writeFileSync } from './sys/fs'
import { basename } from './sys/path'
import { Level, Log } from './log'
import { FileTypes } from './mimedb'
import { HostStrings } from './language'

type Env = WebInternals['env']
type Chooser = NonNullable<Env['files']>
type Picked = { readonly name: string; readonly path: string; readonly writable?: boolean }
type Kind = 'file' | 'directory'

export interface HandOver {
  handle(path: string, kind: Kind, writable: boolean): object | null
  held(handle: unknown): { readonly path: string; readonly kind: Kind } | null
}

export interface FolderPicker {
  directory(title: string, writable: boolean): Promise<string | null>
  entries(dir: string): Promise<{ name: string; kind: Kind }[]>
  entry(dir: string, name: string, create: Kind | null): Promise<Kind | 'other' | null>
  remove(dir: string, name: string, recursive: boolean): Promise<boolean>
  read(path: string): Promise<{ bytes: Uint8Array; lastModified: number } | null>
  write(path: string, bytes: Uint8Array): Promise<void>
  adopt?(path: string): string
  allowFile?(path: string, writable: boolean): void
  readRange?(path: string, offset: number, length: number, version: string | null): Promise<FileRange | null>
  openWrite?(path: string, keepExistingData: boolean): Promise<{ id: string; size: number }>
  writeChunk?(id: string, offset: number, bytes: Uint8Array): Promise<void>
  truncateWrite?(id: string, size: number): Promise<void>
  commitWrite?(id: string): Promise<void>
  abortWrite?(id: string): Promise<void>
}

type FileRange = { bytes: Uint8Array; size: number; lastModified: number; version: string }
type Ranged = NonNullable<Chooser['ranged']>

const WHOLE_LIMIT = 64 * 1024 * 1024
const CHUNK_LIMIT = 16 * 1024 * 1024

type Notice = { title: string; body: string; icon: string; tag: string }

interface FolderSetup {
  readonly picker: FolderPicker | null
  readonly label: string | null
  language(): string | undefined
  readonly privateDirectory: (() => string) | null
  readonly notify: boolean
}

export class PageCalls implements HandOver {
  readonly #rpc: Rpc
  readonly #name: string
  readonly #page: WebInternals
  readonly #granted = new Set<string>()
  readonly #folders = new Set<string>()
  readonly #setup: FolderSetup
  #warned = false

  constructor(page: WebInternals, rpc: Rpc, name: string, setup: FolderSetup) {
    this.#page = page
    this.#rpc = rpc
    this.#name = name
    this.#setup = setup
    page.env.files = this.#chooser()
    page.env.fileType = (name) => FileTypes.system().byName(name)
    page.env.download = (request) =>
      void this.#download(request).catch((error: Error) => this.#log(`download of ${request.url.slice(0, 120)}: ${error.message}`))
    const notify = (notification: Notice) => void this.#notify(notification).catch((error: Error) => this.#log(`notification: ${error.message}`))
    page.env.notify = setup.notify ? notify : null
  }

  handle(path: string, kind: Kind, writable: boolean): object | null {
    const picker = this.#setup.picker
    if (kind === 'file') return this.#page.handles.of(this.#grant(path), 'file', writable ? 'readwrite' : 'read')
    if (!picker?.adopt) return null
    try {
      const adopted = picker.adopt(path)
      this.#folders.add(adopted)
      return this.#page.handles.of({ name: basename(adopted) || adopted, path: adopted }, 'directory', writable ? 'readwrite' : 'read')
    } catch (error) {
      this.#log(`handing over ${kind}: ${(error as Error).message}`)
      return null
    }
  }

  held(handle: unknown): { readonly path: string; readonly kind: Kind; readonly writable: boolean } | null {
    const picked = this.#page.handles.picked(handle)
    if (!picked) return null
    const ok = this.#inFolder(picked.path) !== null || (picked.kind === 'file' && this.#granted.has(picked.path))
    return ok ? { path: picked.path, kind: picked.kind, writable: picked.mode === 'readwrite' } : null
  }

  #grant(path: string): Picked {
    this.#granted.add(path)
    const mode = stat(path)?.mode ?? 0o666
    const writable = (mode & 0o200) !== 0
    this.#setup.picker?.allowFile?.(path, writable)
    return { name: basename(path), path, writable }
  }

  #ranged(): Ranged | undefined {
    const picker = this.#setup.picker
    if (!picker?.allowFile || !picker.readRange || !picker.openWrite || !picker.writeChunk) return undefined
    if (!picker.truncateWrite || !picker.commitWrite || !picker.abortWrite) return undefined
    const { readRange, openWrite, writeChunk, truncateWrite, commitWrite, abortWrite } = picker
    return {
      wholeLimit: WHOLE_LIMIT,
      chunkLimit: CHUNK_LIMIT,
      readRange: (file, offset, length, version) => readRange.call(picker, file.path, offset, length, version),
      openWrite: (file, keep) => openWrite.call(picker, file.path, keep),
      writeChunk: (id, offset, bytes) => writeChunk.call(picker, id, offset, bytes),
      truncateWrite: (id, size) => truncateWrite.call(picker, id, size),
      commitWrite: (id) => commitWrite.call(picker, id),
      abortWrite: (id) => abortWrite.call(picker, id),
    }
  }

  #inFolder(path: string): FolderPicker | null {
    const picker = this.#setup.picker
    if (!picker) return null
    for (const folder of this.#folders) if (path === folder || path.startsWith(folder === '/' ? '/' : `${folder}/`)) return picker
    return null
  }

  #chooser(): Chooser {
    const grant = (path: string): Picked => this.#grant(path)
    const check = (file: Picked): string => {
      if (!this.#granted.has(file.path)) throw new Error('not a file the user picked')
      return file.path
    }
    const picker = this.#setup.picker
    const inFolder = (path: string): FolderPicker | null => this.#inFolder(path)
    const folder = (dir: Picked): FolderPicker => {
      const found = inFolder(dir.path)
      if (!found) throw new Error('not in a folder the user picked')
      return found
    }
    const child = (dir: Picked, name: string): string => (dir.path === '/' ? `/${name}` : `${dir.path}/${name}`)
    return {
      open: async (options) => {
        const paths = await this.#ask('open', options)
        return Array.isArray(paths) && paths.length ? paths.map((path) => grant(String(path))) : null
      },
      save: async (options) => {
        const path = await this.#ask('save', options)
        return typeof path === 'string' && path ? grant(path) : null
      },
      directory: async ({ mode }) => {
        if (!picker) return null
        try {
          const writable = mode === 'readwrite'
          const strings = HostStrings.of(this.#setup.language())
          const prompt = writable ? strings.writeFolder : strings.readFolder
          const path = await picker.directory(`${this.#setup.label ?? strings.desktop}: ${prompt}`, writable)
          if (!path) return null
          this.#folders.add(path)
          return { name: basename(path) || path, path }
        } catch (error) {
          if (!this.#warned) this.#log(`folder chooser: ${(error as Error).message}`)
          this.#warned = true
          return null
        }
      },
      privateDirectory: () => {
        if (!picker || !this.#setup.privateDirectory) return null
        const path = this.#setup.privateDirectory()
        this.#folders.add(path)
        return { name: '', path }
      },
      entries: async (dir) => (await folder(dir).entries(dir.path)).map(({ name, kind }) => ({ name, kind, path: child(dir, name) })),
      entry: async (dir, name, create) => {
        const kind = await folder(dir).entry(dir.path, name, create)
        return kind ? { name, kind, path: child(dir, name) } : null
      },
      remove: (dir, name, recursive) => folder(dir).remove(dir.path, name, recursive),
      read: async (file) => {
        const from = inFolder(file.path)
        if (from) return from.read(file.path)
        const path = check(file)
        if (!existsSync(path)) return null
        const bytes = readFileSync(path)
        return { bytes, lastModified: stat(path)?.mtimeMs ?? hostTime.wallNow() }
      },
      write: async (file, bytes) => {
        const to = inFolder(file.path)
        if (to) return to.write(file.path, bytes)
        writeFileSync(check(file), bytes)
      },
      ranged: this.#ranged(),
    }
  }

  async #ask(method: 'open' | 'save', options: unknown): Promise<unknown> {
    try {
      return await this.#rpc.call('page', method, [options])
    } catch (error) {
      if (!this.#warned) this.#log(`file chooser: ${(error as Error).message}`)
      this.#warned = true
      return null
    }
  }

  async #download(request: Parameters<NonNullable<Env['download']>>[0]): Promise<void> {
    let bytes: Uint8Array
    let type: string
    let name = request.name
    if (request.blob) {
      bytes = new Uint8Array(await request.blob.arrayBuffer())
      type = request.blob.type
    } else {
      const fetch = (this.#page.window as unknown as { fetch: typeof globalThis.fetch }).fetch
      const response = await fetch(request.url)
      if (!response.ok) throw new Error(`HTTP ${response.status}`)
      bytes = new Uint8Array(await response.arrayBuffer())
      type = response.headers.get('content-type') ?? ''
      name ||= PageCalls.dispositionName(response.headers.get('content-disposition'))
    }
    name ||= PageCalls.urlName(request.url)
    await this.#rpc.call('page', 'download', [name, type, bytes])
  }

  async #notify(notification: Notice): Promise<void> {
    let icon = notification.icon
    if (icon.startsWith('http://acde.local/')) {
      icon = ''
      try {
        const fetch = (this.#page.window as unknown as { fetch: typeof globalThis.fetch }).fetch
        const response = await fetch(notification.icon)
        const bytes = new Uint8Array(await response.arrayBuffer())
        if (response.ok && bytes.length <= PageCalls.ICON_BYTES) {
          let binary = ''
          for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
          icon = `data:${response.headers.get('content-type') || 'image/png'};base64,${btoa(binary)}`
        }
      } catch {
      }
    }
    await this.#rpc.call('page', 'notify', [{ ...notification, icon }])
  }

  static readonly ICON_BYTES = 128 * 1024

  static dispositionName(header: string | null): string {
    if (!header) return ''
    const star = /filename\*\s*=\s*(?:utf-8|UTF-8)''([^;]+)/.exec(header)
    if (star) {
      try {
        return decodeURIComponent(star[1]!.trim())
      } catch {
      }
    }
    return /filename\s*=\s*(?:"([^"]*)"|([^;]+))/i.exec(header)?.slice(1).find((v) => v !== undefined)?.trim() ?? ''
  }

  static urlName(url: string): string {
    if (!/^https?:/i.test(url)) return ''
    try {
      return decodeURIComponent(new URL(url).pathname.split('/').pop() ?? '')
    } catch {
      return ''
    }
  }

  #log(text: string): void {
    Log.write(Level.Warn, 'acde_host', `[${this.#name}] ${text}`)
  }
}
