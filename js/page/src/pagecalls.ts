import type { WebInternals } from '@acde/dom'
import type { Rpc } from './rpc'
import { Level, Log } from './log'
import { FileTypes } from './mimedb'
import { HostStrings } from './language'

type Env = WebInternals['env']
type Chooser = NonNullable<Env['files']>
type Picked = { readonly name: string; readonly path: string; readonly writable?: boolean }
type Kind = 'file' | 'directory'
type LentFile = { readonly name: string; readonly label: string; readonly writable: boolean }
export type LentEntry = LentFile & { readonly kind: Kind }

const LENT = 'lent:'
const LENT_FILE = 'lent-file:'

export interface HandOver {
  lent(entry: LentEntry): object | null
  dropped(uris: string | null): Promise<readonly Dropped[]>
  handed(handle: unknown): Handed | null
}

export type Handed = { readonly name: string; readonly path?: string; readonly writable?: false }

export interface FolderPicker {
  entries(dir: string): Promise<{ name: string; kind: Kind }[]>
  entry(dir: string, name: string, create: Kind | null): Promise<Kind | 'other' | null>
  remove(dir: string, name: string, recursive: boolean): Promise<boolean>
  read(path: string): Promise<{ bytes: Uint8Array; lastModified: number } | null>
  write(path: string, bytes: Uint8Array): Promise<void>
  dropped?(): Promise<LentEntry[]>
  readRange?(path: string, offset: number, length: number, version: string | null): Promise<FileRange | null>
  openWrite?(path: string, keepExistingData: boolean): Promise<{ id: string; size: number }>
  writeChunk?(id: string, offset: number, bytes: Uint8Array): Promise<void>
  truncateWrite?(id: string, size: number): Promise<void>
  commitWrite?(id: string): Promise<void>
  abortWrite?(id: string): Promise<void>
}

export type Dropped = { readonly handle: object; readonly file: File | null }
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
  readonly #folders = new Set<string>()
  readonly #files = new Set<string>()
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

  lent(entry: LentEntry): object | null {
    const mode = entry.writable ? 'readwrite' : 'read'
    if (entry.kind === 'file') return this.#page.handles.of(this.#lent(entry), 'file', mode)
    const path = `${LENT}${entry.name}`
    this.#folders.add(path)
    return this.#page.handles.of({ name: entry.label || entry.name, path }, 'directory', mode)
  }

  handed(handle: unknown): Handed | null {
    const given = this.#entry(handle)
    if (!given) return null
    return { name: given.name, ...(given.path && { path: given.path }), ...(!given.writable && { writable: false as const }) }
  }

  lentFile(handle: unknown): string | null {
    const given = this.#entry(handle)
    return given?.kind === 'file' && !given.path ? given.name : null
  }

  #entry(handle: unknown): { readonly name: string; readonly path: string; readonly kind: Kind; readonly writable: boolean } | null {
    const picked = this.#page.handles.picked(handle)
    if (!picked) return null
    const writable = picked.mode === 'readwrite'
    if (picked.kind === 'file' && this.#files.has(picked.path)) return { name: picked.path.slice(LENT_FILE.length), path: '', kind: 'file', writable }
    if (!picked.path.startsWith(LENT) || !this.#inFolder(picked.path)) return null
    const [name = '', ...inner] = picked.path.slice(LENT.length).split('/')
    return name ? { name, path: inner.join('/'), kind: picked.kind, writable } : null
  }

  #lent(file: LentFile): Picked {
    const path = `${LENT_FILE}${file.name}`
    this.#files.add(path)
    return { name: file.label || file.name, path, writable: file.writable }
  }

  async dropped(uris: string | null): Promise<readonly Dropped[]> {
    const dropped: Dropped[] = []
    for (const entry of await this.#droppedEntries(uris)) {
      const handle = this.lent(entry)
      if (handle) dropped.push({ handle, file: entry.kind === 'file' ? await this.#small(entry) : null })
    }
    return dropped
  }

  async #droppedEntries(uris: string | null): Promise<readonly LentEntry[]> {
    try {
      if (uris) return ((await this.#rpc.call('page', 'dropped', [{ uris }])) as LentEntry[] | null) ?? []
      return (await this.#setup.picker?.dropped?.()) ?? []
    } catch (error) {
      this.#log(`drop: ${(error as Error).message}`)
      return []
    }
  }

  async #small(entry: LentEntry): Promise<File | null> {
    const picker = this.#setup.picker
    const path = `${LENT_FILE}${entry.name}`
    const head = await picker?.readRange?.(path, 0, 0, null)
    if (!picker || !head || head.size > WHOLE_LIMIT) return null
    const read = await picker.read(path)
    if (!read) return null
    const window = this.#page.window as unknown as { File: typeof File }
    const name = entry.label || entry.name
    return new window.File([read.bytes as Uint8Array<ArrayBuffer>], name, { type: this.#page.env.fileType(name), lastModified: read.lastModified })
  }

  #lentFile(path: string): FolderPicker | null {
    return this.#files.has(path) ? this.#setup.picker : null
  }

  #ranged(): Ranged | undefined {
    const picker = this.#setup.picker
    if (!picker?.readRange || !picker.openWrite || !picker.writeChunk) return undefined
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
    const picker = this.#setup.picker
    const inFolder = (path: string): FolderPicker | null => this.#inFolder(path)
    const reach = (path: string): FolderPicker | null => this.#inFolder(path) ?? this.#lentFile(path)
    const lent = (file: LentFile): Picked => this.#lent(file)
    const folder = (dir: Picked): FolderPicker => {
      const found = inFolder(dir.path)
      if (!found) throw new Error('not in a folder the user picked')
      return found
    }
    const child = (dir: Picked, name: string): string => (dir.path === '/' ? `/${name}` : `${dir.path}/${name}`)
    return {
      open: async (options) => {
        const files = await this.#ask('open', options)
        return Array.isArray(files) && files.length ? (files as LentFile[]).map(lent) : null
      },
      save: async (options) => {
        const file = (await this.#ask('save', options)) as LentFile | null
        return file && typeof file.name === 'string' ? lent(file) : null
      },
      directory: async ({ mode }) => {
        if (!picker) return null
        try {
          const writable = mode === 'readwrite'
          const strings = HostStrings.of(this.#setup.language())
          const prompt = writable ? strings.writeFolder : strings.readFolder
          const title = `${this.#setup.label ?? strings.desktop}: ${prompt}`
          const lent = (await this.#rpc.call('page', 'folder', [{ title, writable }])) as { name?: unknown; label?: unknown } | null
          if (!lent || typeof lent.name !== 'string' || !lent.name) return null
          const path = `${LENT}${lent.name}`
          this.#folders.add(path)
          return { name: typeof lent.label === 'string' && lent.label ? lent.label : lent.name, path }
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
        const from = reach(file.path)
        if (!from) throw new Error('not a file or folder this page was given')
        return from.read(file.path)
      },
      write: async (file, bytes) => {
        const to = reach(file.path)
        if (!to) throw new Error('not a file or folder this page was given')
        return to.write(file.path, bytes)
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
