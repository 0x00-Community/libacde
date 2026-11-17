import { Blob, blobInMemory, File, RangedSource } from '../core/blob'
import { bytesOf } from '../core/encoding'
import { DOMException } from '../core/exception'
import { getters, host as native, tag } from '../core/host'
import { env, type FileFilter, type PickedEntry, type PickedFile, type RangedFiles } from './env'

export function acceptFilters(accept: string): FileFilter[] {
  const tokens = accept.split(',').map((t) => t.trim()).filter(Boolean)
  const globs = tokens.filter((t) => t.startsWith('.')).map((t) => `*${t}`)
  const mimes = tokens.filter((t) => t.includes('/'))
  return globs.length || mimes.length ? [{ name: tokens.join(', '), globs, mimes }] : []
}

function typeFilters(types: unknown): FileFilter[] {
  if (!Array.isArray(types)) return []
  return types.flatMap((type: { description?: unknown; accept?: Record<string, unknown> }) => {
    const accept = type?.accept && typeof type.accept === 'object' ? type.accept : {}
    const mimes = Object.keys(accept)
    const globs = Object.values(accept).flatMap((exts) => (Array.isArray(exts) ? exts : [exts])).map((ext) => `*${String(ext)}`)
    return mimes.length || globs.length ? [{ name: String(type?.description ?? [...mimes, ...globs].join(', ')), globs, mimes }] : []
  })
}

export async function readPicked(file: PickedFile): Promise<File> {
  const ranged = env.files!.ranged
  const head = ranged ? await ranged.readRange(file, 0, 0, null) : null
  if (ranged && !head) throw notFound()
  if (ranged && head && head.size > ranged.wholeLimit) {
    const source = new RangedSource(async (offset, length) => {
      const range = await host(() => ranged.readRange(file, offset, length, head.version))
      if (!range) throw notReadable()
      return range.bytes
    }, head.size)
    return new File([source], file.name, { type: env.fileType(file.name), lastModified: head.lastModified })
  }
  const read = await env.files!.read(file)
  if (!read) throw notFound()
  return new File([read.bytes], file.name, { type: env.fileType(file.name), lastModified: read.lastModified })
}

function aborted(): DOMException {
  return new DOMException('The user aborted a request.', 'AbortError')
}

function notFound(): DOMException {
  return new DOMException('A requested file or directory could not be found at the time an operation was processed.', 'NotFoundError')
}

function notReadable(): DOMException {
  const what = 'The requested file could not be read, typically due to permission problems that have occurred after a reference to a file was acquired.'
  return new DOMException(what, 'NotReadableError')
}

function mismatch(): DOMException {
  return new DOMException('The path supplied exists, but was not an entry of requested type.', 'TypeMismatchError')
}

function notAllowed(): DOMException {
  return new DOMException('The request is not allowed by the user agent or the platform in the current context.', 'NotAllowedError')
}

function failed(error: unknown): DOMException {
  const code = (error as { code?: unknown })?.code
  if (code === 'ENOENT') return notFound()
  if (code === 'ENOTEMPTY') return new DOMException('The object can not be modified in this way.', 'InvalidModificationError')
  if (code === 'EACCES') return notAllowed()
  if (code === 'ECONFLICT') return notReadable()
  return new DOMException((error as Error)?.message ?? String(error), 'InvalidStateError')
}

function host<T>(call: () => Promise<T>): Promise<T> {
  return call().catch((error: unknown) => {
    throw error instanceof DOMException ? error : failed(error)
  })
}

export type Mode = 'read' | 'readwrite'

function permissionMode(descriptor: unknown, method: string): Mode {
  const mode = descriptor !== null && typeof descriptor === 'object' ? (descriptor as { mode?: unknown }).mode : undefined
  if (mode === undefined) return 'read'
  const text = String(mode)
  if (text !== 'read' && text !== 'readwrite') {
    const what = `The provided value '${text}' is not a valid enum value of type FileSystemPermissionMode.`
    throw new TypeError(`Failed to execute '${method}' on 'FileSystemHandle': ${what}`)
  }
  return text
}

function validName(name: unknown, method: string): string {
  const text = String(name)
  if (text === '' || text === '.' || text === '..' || text.includes('/') || text.includes('\0')) {
    throw new TypeError(`Failed to execute '${method}' on 'FileSystemDirectoryHandle': Name is not allowed.`)
  }
  return text
}

const PICKED = Symbol('picked')
const MODE = Symbol('mode')
const KIND = Symbol('kind')

export class FileSystemHandle {
  [PICKED]: PickedFile;
  [MODE]: Mode;
  [KIND]: 'file' | 'directory'
  constructor(picked?: PickedFile, mode: Mode = 'readwrite', kind: 'file' | 'directory' = 'file') {
    if (!picked) throw new TypeError('Illegal constructor')
    this[PICKED] = picked
    this[MODE] = mode
    this[KIND] = kind
  }
  get kind(): string {
    return this[KIND]
  }
  get name(): string {
    return this[PICKED].name
  }
  async isSameEntry(other: unknown): Promise<boolean> {
    return other instanceof FileSystemHandle && other[KIND] === this[KIND] && other[PICKED].path === this[PICKED].path
  }
  async queryPermission(descriptor?: unknown): Promise<string> {
    return permissionMode(descriptor, 'queryPermission') === 'readwrite' && this[MODE] === 'read' ? 'prompt' : 'granted'
  }
  async requestPermission(descriptor?: unknown): Promise<string> {
    return permissionMode(descriptor, 'requestPermission') === 'readwrite' && this[MODE] === 'read' ? 'denied' : 'granted'
  }
}
tag(FileSystemHandle)

export class FileSystemFileHandle extends FileSystemHandle {
  getFile(): Promise<File> {
    return host(() => readPicked(this[PICKED]))
  }
  async createWritable(options?: { keepExistingData?: boolean }): Promise<FileSystemWritableFileStream> {
    if (this[MODE] !== 'readwrite') throw notAllowed()
    const picked = this[PICKED]
    const ranged = env.files!.ranged
    if (ranged) {
      const { id, size } = await host(() => ranged.openWrite(picked, !!options?.keepExistingData))
      return new FileSystemWritableFileStream(SINK, rangedSink(ranged, id), size)
    }
    let existing: Uint8Array = new Uint8Array(0)
    if (options?.keepExistingData) {
      const read = await host(() => env.files!.read(picked))
      if (!read) throw notFound()
      existing = read.bytes
    }
    return new FileSystemWritableFileStream(SINK, memorySink(picked, existing), existing.length)
  }
}
tag(FileSystemFileHandle)

type Entry = [string, FileSystemFileHandle | FileSystemDirectoryHandle]

export class FileSystemDirectoryHandle extends FileSystemHandle {
  async *entries(): AsyncGenerator<Entry, undefined, undefined> {
    const dir = this[PICKED]
    const children = await host(() => env.files!.entries(dir))
    for (const child of children) {
      if (child.kind === 'file') yield [child.name, new FileSystemFileHandle(child, this[MODE], 'file')]
      else if (child.kind === 'directory') yield [child.name, directoryHandle(child, this[MODE])]
    }
    return undefined
  }
  async *keys(): AsyncGenerator<string, undefined, undefined> {
    for await (const [name] of this.entries()) yield name
    return undefined
  }
  async *values(): AsyncGenerator<Entry[1], undefined, undefined> {
    for await (const [, handle] of this.entries()) yield handle
    return undefined
  }
  [Symbol.asyncIterator](): AsyncGenerator<Entry, undefined, undefined> {
    return this.entries()
  }
  async getFileHandle(name: unknown, options?: { create?: boolean }): Promise<FileSystemFileHandle> {
    const child = await childOf(this, validName(name, 'getFileHandle'), options?.create ? 'file' : null)
    if (child.kind !== 'file') throw mismatch()
    return new FileSystemFileHandle(child, this[MODE], 'file')
  }
  async getDirectoryHandle(name: unknown, options?: { create?: boolean }): Promise<FileSystemDirectoryHandle> {
    const child = await childOf(this, validName(name, 'getDirectoryHandle'), options?.create ? 'directory' : null)
    if (child.kind !== 'directory') throw mismatch()
    return directoryHandle(child, this[MODE])
  }
  async removeEntry(name: unknown, options?: { recursive?: boolean }): Promise<void> {
    const text = validName(name, 'removeEntry')
    if (this[MODE] !== 'readwrite') throw notAllowed()
    const dir = this[PICKED]
    if (!(await host(() => env.files!.remove(dir, text, !!options?.recursive)))) throw notFound()
  }
  async resolve(possibleDescendant: unknown): Promise<string[] | null> {
    if (!(possibleDescendant instanceof FileSystemHandle)) {
      throw new TypeError("Failed to execute 'resolve' on 'FileSystemDirectoryHandle': parameter 1 is not of type 'FileSystemHandle'.")
    }
    const here = this[PICKED].path
    const there = possibleDescendant[PICKED].path
    if (there === here) return possibleDescendant[KIND] === 'directory' ? [] : null
    const prefix = here === '/' ? '/' : `${here}/`
    return there.startsWith(prefix) ? there.slice(prefix.length).split('/') : null
  }
}
tag(FileSystemDirectoryHandle)

function directoryHandle(picked: PickedFile, mode: Mode): FileSystemDirectoryHandle {
  return new FileSystemDirectoryHandle(picked, mode, 'directory')
}

export function handedOver(entry: PickedFile, kind: 'file' | 'directory', mode: Mode): FileSystemFileHandle | FileSystemDirectoryHandle {
  return kind === 'directory' ? directoryHandle(entry, mode) : new FileSystemFileHandle(entry, mode, 'file')
}

export function pickedOf(handle: unknown): (PickedFile & { readonly kind: 'file' | 'directory'; readonly mode: Mode }) | null {
  return handle instanceof FileSystemHandle ? { ...handle[PICKED], kind: handle[KIND], mode: handle[MODE] } : null
}

export function storageRootHandle(path: string): FileSystemDirectoryHandle {
  return directoryHandle({ name: '', path }, 'readwrite')
}

async function childOf(handle: FileSystemDirectoryHandle, name: string, create: 'file' | 'directory' | null): Promise<PickedEntry> {
  if (create && handle[MODE] !== 'readwrite') throw notAllowed()
  const dir = handle[PICKED]
  const child = await host(() => env.files!.entry(dir, name, create))
  if (!child) throw notFound()
  return child
}

type WriteParams = { type: 'write'; data: unknown; position?: number } | { type: 'seek'; position: number } | { type: 'truncate'; size: number }

interface Sink {
  readonly limit: number
  write(position: number, bytes: Uint8Array): Promise<void>
  truncate(size: number): Promise<void>
  close(): Promise<void>
  abort(): Promise<void>
}

function memorySink(picked: PickedFile, existing: Uint8Array): Sink {
  let bytes = existing.slice()
  let size = existing.length
  const resize = (length: number) => {
    if (length <= bytes.length) return
    const grown = new Uint8Array(Math.max(length, bytes.length * 2))
    grown.set(bytes.subarray(0, size))
    bytes = grown
  }
  return {
    limit: Number.MAX_SAFE_INTEGER,
    async write(position, chunk) {
      resize(position + chunk.length)
      if (position > size) bytes.fill(0, size, position)
      bytes.set(chunk, position)
      size = Math.max(size, position + chunk.length)
    },
    async truncate(length) {
      resize(length)
      if (length > size) bytes.fill(0, size, length)
      size = length
    },
    close: () => host(() => env.files!.write(picked, bytes.slice(0, size))),
    async abort() {},
  }
}

function rangedSink(ranged: RangedFiles, id: string): Sink {
  return {
    limit: ranged.chunkLimit,
    write: (position, bytes) => host(() => ranged.writeChunk(id, position, bytes)),
    truncate: (size) => host(() => ranged.truncateWrite(id, size)),
    close: () => host(() => ranged.commitWrite(id)),
    abort: () => host(() => ranged.abortWrite(id)),
  }
}

export class FileSystemWritableFileStream {
  readonly #sink: Sink
  #size: number
  #position = 0
  #closed = false
  #failed: unknown = null
  #queue: Promise<void> = Promise.resolve()

  constructor(token: unknown, sink: Sink, size: number) {
    if (token !== SINK) throw new TypeError('Illegal constructor')
    this.#sink = sink
    this.#size = size
  }

  write(data: unknown): Promise<void> {
    if (this.#closed) return Promise.reject(new TypeError('The stream is closed.'))
    return this.#then(() => this.#write(data))
  }

  seek(position: number): Promise<void> {
    return this.write({ type: 'seek', position })
  }

  truncate(size: number): Promise<void> {
    return this.write({ type: 'truncate', size })
  }

  close(): Promise<void> {
    if (this.#closed) return Promise.reject(new TypeError('The stream is closed.'))
    this.#closed = true
    return this.#then(() => this.#sink.close())
  }

  abort(): Promise<void> {
    const open = !this.#closed
    this.#closed = true
    const aborting = this.#queue.then(() => (open ? this.#sink.abort() : undefined))
    this.#queue = aborting.catch(() => {})
    return aborting
  }

  getWriter(): { write(data: unknown): Promise<void>; close(): Promise<void>; abort(): Promise<void>; releaseLock(): void; ready: Promise<void> } {
    return { write: (data) => this.write(data), close: () => this.close(), abort: () => this.abort(), releaseLock: () => {}, ready: Promise.resolve() }
  }

  #then(step: () => Promise<void>): Promise<void> {
    const run = this.#queue.then(() => {
      if (this.#failed !== null) throw this.#failed
      return step().catch((error: unknown) => {
        this.#failed = error
        throw error
      })
    })
    this.#queue = run.catch(() => {})
    return run
  }

  async #write(data: unknown): Promise<void> {
    const typed = data !== null && typeof data === 'object' && 'type' in (data as object) && typeof (data as { type: unknown }).type === 'string'
    const params = typed && !(data instanceof Blob) ? (data as WriteParams) : ({ type: 'write', data } as WriteParams)
    if (params.type === 'seek') return void (this.#position = Math.max(0, Math.trunc(params.position)))
    if (params.type === 'truncate') {
      const size = Math.max(0, Math.trunc(params.size))
      await this.#sink.truncate(size)
      this.#size = size
      this.#position = Math.min(this.#position, size)
      return
    }
    if (params.position !== undefined) this.#position = Math.max(0, Math.trunc(params.position))
    for await (const chunk of chunksOf(params.data)) {
      for (let at = 0; at < chunk.length; at += this.#sink.limit) {
        const piece = chunk.subarray(at, Math.min(chunk.length, at + this.#sink.limit))
        await this.#sink.write(this.#position, piece)
        this.#position += piece.length
        this.#size = Math.max(this.#size, this.#position)
      }
    }
  }
}
getters(FileSystemWritableFileStream.prototype, { locked: false })
tag(FileSystemWritableFileStream)

const SINK = Symbol('sink')

async function* chunksOf(data: unknown): AsyncGenerator<Uint8Array, void, undefined> {
  if (typeof data === 'string') return yield native.encodeUtf8(data)
  if (!(data instanceof Blob)) return yield bytesOf(data as ArrayBuffer)
  const memory = blobInMemory(data)
  if (memory) return yield memory
  const reader = data.stream().getReader()
  for (;;) {
    const { value, done } = await reader.read()
    if (done) return
    yield value
  }
}

export async function showOpenFilePicker(options?: { multiple?: boolean; types?: unknown; excludeAcceptAllOption?: boolean }): Promise<FileSystemFileHandle[]> {
  const picked = env.files ? await env.files.open({ multiple: !!options?.multiple, directory: false, filters: typeFilters(options?.types) }) : null
  if (!picked || picked.length === 0) throw aborted()
  return picked.map((file) => new FileSystemFileHandle(file, file.writable === false ? 'read' : 'readwrite'))
}

export async function showSaveFilePicker(options?: { suggestedName?: string; types?: unknown }): Promise<FileSystemFileHandle> {
  const picked = env.files ? await env.files.save({ name: String(options?.suggestedName ?? ''), filters: typeFilters(options?.types) }) : null
  if (!picked) throw aborted()
  return new FileSystemFileHandle(picked)
}

const WELL_KNOWN = new Set(['desktop', 'documents', 'downloads', 'music', 'pictures', 'videos'])

export async function showDirectoryPicker(options?: { id?: unknown; mode?: unknown; startIn?: unknown }): Promise<FileSystemDirectoryHandle> {
  const method = "Failed to execute 'showDirectoryPicker' on 'Window'"
  const mode = options?.mode === undefined ? 'read' : String(options.mode)
  const invalid = (field: string, value: string, type: string) => {
    const what = `The provided value '${value}' is not a valid enum value of type ${type}.`
    return new TypeError(`${method}: Failed to read the '${field}' property from 'DirectoryPickerOptions': ${what}`)
  }
  if (mode !== 'read' && mode !== 'readwrite') throw invalid('mode', mode, 'FileSystemPermissionMode')
  if (options?.id !== undefined && !/^[\w-]{0,32}$/.test(String(options.id))) {
    throw new TypeError(`${method}: ID must be at most 32 characters long and contain only alphanumeric characters, underscores, or dashes.`)
  }
  const start = options?.startIn
  if (start !== undefined && !(start instanceof FileSystemHandle) && !WELL_KNOWN.has(String(start))) {
    throw invalid('startIn', String(start), 'WellKnownDirectory')
  }
  const picked = env.files ? await env.files.directory({ mode }) : null
  if (!picked) throw aborted()
  return directoryHandle(picked, mode)
}
