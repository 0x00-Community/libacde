import type { Blob } from '../core/blob'
import { host } from '../core/host'

let documentUrl = 'http://acde.local/'

export const documentUrlWatch = { changed: (_href: string): void => {} }

export const env = {
  get url(): string {
    return documentUrl
  },
  set url(href: string) {
    if (href === documentUrl) return
    documentUrl = href
    documentUrlWatch.changed(href)
  },
  navigate: (_url: string, _target: string) => {},
  reload: () => {},
  close: () => {},
  clipboard: null as null | ClipboardAccess,
  pointerLock: null as null | PointerLockAccess,
  alert: null as null | ((message: string) => void),
  download: null as null | ((request: DownloadRequest) => void),
  files: null as null | FileChooser,
  notify: null as null | ((notification: { title: string; body: string; icon: string; tag: string }) => void),
  fileType: (_name: string): string => '',
}

export interface ClipboardBytes {
  readonly type: string
  readonly data: Uint8Array
}

export interface PointerLockAccess {
  lock(): void
  unlock(): void
}

export interface ClipboardAccess {
  readText(): Promise<string>
  writeText(text: string): Promise<void>
  types(): Promise<string[]>
  read(type: string): Promise<Uint8Array | null>
  write(entries: readonly ClipboardBytes[]): Promise<void>
}

interface DownloadRequest {
  readonly url: string
  readonly name: string
  readonly blob?: Blob
}

export interface PickedFile {
  readonly name: string
  readonly path: string
  readonly writable?: boolean
}

export interface FileFilter {
  readonly name: string
  readonly globs: readonly string[]
  readonly mimes: readonly string[]
}

export interface PickedEntry extends PickedFile {
  readonly kind: 'file' | 'directory' | 'other'
}

export interface FileChooser {
  open(options: {
    readonly multiple: boolean
    readonly directory: boolean
    readonly filters: readonly FileFilter[]
    readonly title?: string
  }): Promise<PickedFile[] | null>
  save(options: { readonly name: string; readonly filters: readonly FileFilter[]; readonly title?: string }): Promise<PickedFile | null>
  read(file: PickedFile): Promise<{ bytes: Uint8Array; lastModified: number } | null>
  write(file: PickedFile, bytes: Uint8Array): Promise<void>
  directory(options: { readonly mode: 'read' | 'readwrite' }): Promise<PickedFile | null>
  privateDirectory(): PickedFile | null
  entries(dir: PickedFile): Promise<PickedEntry[]>
  entry(dir: PickedFile, name: string, create: 'file' | 'directory' | null): Promise<PickedEntry | null>
  remove(dir: PickedFile, name: string, recursive: boolean): Promise<boolean>
  readonly ranged?: RangedFiles
}

export interface FileRange {
  readonly bytes: Uint8Array
  readonly size: number
  readonly lastModified: number
  readonly version: string
}

export interface RangedFiles {
  readonly wholeLimit: number
  readonly chunkLimit: number
  readRange(file: PickedFile, offset: number, length: number, version: string | null): Promise<FileRange | null>
  openWrite(file: PickedFile, keepExistingData: boolean): Promise<{ readonly id: string; readonly size: number }>
  writeChunk(id: string, offset: number, bytes: Uint8Array): Promise<void>
  truncateWrite(id: string, size: number): Promise<void>
  commitWrite(id: string): Promise<void>
  abortWrite(id: string): Promise<void>
}

export function resolveUrl(url: string): string {
  return host.parseUrl(url, env.url)?.href ?? url
}

export const loads = {
  expected: false,
  deliver: (): void => {},
  expect(): void {
    if (loads.expected) return
    loads.expected = true
    host.setTimer(0, () => {
      loads.expected = false
      loads.deliver()
    })
  },
}

export const frames = {
  wanted: false,
  request(): void {
    if (frames.wanted) return
    frames.wanted = true
    host.requestFrame()
  },
}
