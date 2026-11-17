import { defineHandlers, Event, EventTarget, fire, ProgressEvent } from './event'
import { DOMException } from './exception'
import { base64, bytesOf, concat, latin1, TextDecoder } from './encoding'
import { constants, getters, host, tag } from './host'
import { ReadableStream, streamOfBytes } from './streams'

export class RangedSource {
  readonly read: (offset: number, length: number) => Promise<Uint8Array>
  readonly size: number
  constructor(read: (offset: number, length: number) => Promise<Uint8Array>, size: number) {
    this.read = read
    this.size = size
  }
}

type BlobPart = Blob | ArrayBuffer | ArrayBufferView | string | RangedSource
type Ranged = { readonly source: RangedSource; readonly start: number; readonly end: number }
type Part = Uint8Array | Ranged

const PIECE = 4 * 1024 * 1024

export let blobBytes: (blob: Blob) => Uint8Array
export let blobInMemory: (blob: Blob) => Uint8Array | null
export let blobRead: (blob: Blob) => Promise<Uint8Array>

function isRanged(part: Part): part is Ranged {
  return (part as Ranged).source instanceof RangedSource
}

function partSize(part: Part): number {
  return isRanged(part) ? part.end - part.start : part.length
}

async function* pieces(parts: readonly Part[]): AsyncGenerator<Uint8Array, void, undefined> {
  for (const part of parts) {
    if (!isRanged(part)) {
      if (part.length) yield part
      continue
    }
    for (let at = part.start; at < part.end; ) {
      const piece = await part.source.read(at, Math.min(PIECE, part.end - at))
      if (!piece.length) throw new DOMException('The requested file could not be read.', 'NotReadableError')
      at += piece.length
      yield piece
    }
  }
}

export class Blob {
  #bytes: Uint8Array
  #parts: readonly Part[] | null = null
  #size: number
  #type: string

  constructor(parts: Iterable<BlobPart> = [], options?: { type?: string; endings?: string }) {
    const chunks: Part[] = []
    for (const part of parts) {
      if (part instanceof RangedSource) chunks.push({ source: part, start: 0, end: part.size })
      else if (part instanceof Blob) chunks.push(...(part.#parts ?? [part.#bytes]))
      else if (typeof part === 'object' && part !== null) chunks.push(bytesOf(part))
      else chunks.push(host.encodeUtf8(String(part)))
    }
    const inMemory = chunks.every((chunk): chunk is Uint8Array => !isRanged(chunk))
    this.#bytes = inMemory ? concat(chunks as Uint8Array[]) : new Uint8Array(0)
    this.#parts = inMemory ? null : chunks
    this.#size = chunks.reduce((sum, chunk) => sum + partSize(chunk), 0)
    const type = String(options?.type ?? '')
    this.#type = /^[\x20-\x7e]*$/.test(type) ? type.toLowerCase() : ''
  }

  static {
    blobInMemory = (blob) => (blob.#parts ? null : blob.#bytes)
    blobBytes = (blob) => {
      if (blob.#parts) throw new DOMException('This file is read on demand; read it with arrayBuffer(), text() or stream().', 'NotReadableError')
      return blob.#bytes
    }
    blobRead = (blob) => blob.#all()
  }

  get size(): number {
    return this.#size
  }

  get type(): string {
    return this.#type
  }

  slice(start = 0, end = this.#size, type = ''): Blob {
    const size = this.#size
    const from = start < 0 ? Math.max(size + start, 0) : Math.min(start, size)
    const to = Math.max(from, end < 0 ? Math.max(size + end, 0) : Math.min(end, size))
    if (!this.#parts) return new Blob([this.#bytes.subarray(from, to)], { type })
    const kept: Part[] = []
    let at = 0
    for (const part of this.#parts) {
      const length = partSize(part)
      const [a, b] = [Math.max(from - at, 0), Math.min(to - at, length)]
      if (a < b) kept.push(isRanged(part) ? { source: part.source, start: part.start + a, end: part.start + b } : part.subarray(a, b))
      at += length
    }
    const sliced = new Blob([], { type })
    sliced.#parts = kept
    sliced.#size = to - from
    return sliced
  }

  async #all(): Promise<Uint8Array> {
    if (!this.#parts) return this.#bytes
    const out = new Uint8Array(this.#size)
    let at = 0
    for await (const piece of pieces(this.#parts)) {
      out.set(piece.subarray(0, out.length - at), at)
      at += piece.length
    }
    return out
  }

  async text(): Promise<string> {
    return new TextDecoder().decode(await this.#all())
  }

  async arrayBuffer(): Promise<ArrayBuffer> {
    return (await this.#all()).slice().buffer
  }

  async bytes(): Promise<Uint8Array> {
    return (await this.#all()).slice()
  }

  stream(): ReadableStream<Uint8Array> {
    if (!this.#parts) return streamOfBytes(this.#bytes.slice())
    const reading = pieces(this.#parts)
    return new ReadableStream<Uint8Array>({
      async pull(controller) {
        const { value, done } = await reading.next()
        if (done) controller.close()
        else controller.enqueue(value.slice())
      },
      async cancel() {
        await reading.return()
      },
    })
  }
}
tag(Blob)

export class File extends Blob {
  readonly #name: string
  readonly #lastModified: number

  constructor(parts: Iterable<BlobPart>, name: string, options?: { type?: string; lastModified?: number }) {
    super(parts, options)
    this.#name = String(name)
    this.#lastModified = options?.lastModified ?? Date.now()
  }

  get name(): string {
    return this.#name
  }

  get lastModified(): number {
    return this.#lastModified
  }
}
getters(File.prototype, { webkitRelativePath: '' })
tag(File)

export class FileReader extends EventTarget {
  readyState = 0
  result: string | ArrayBuffer | null = null
  error: DOMException | null = null
  #token = 0

  #read(blob: Blob, as: (bytes: Uint8Array) => string | ArrayBuffer): void {
    if (this.readyState === 1) throw new DOMException('The object is already busy reading Blobs.', 'InvalidStateError')
    if (!(blob instanceof Blob)) throw new TypeError("Failed to execute 'read' on 'FileReader': parameter 1 is not of type 'Blob'.")
    this.readyState = 1
    this.result = null
    const token = ++this.#token
    const total = blob.size
    const progress = (type: string, loaded: number) => fire(this, new ProgressEvent(type, { lengthComputable: true, loaded, total }))
    const reading = blobRead(blob)
    host.queueMicrotask(() => {
      if (token !== this.#token) return
      progress('loadstart', 0)
      reading.then(
        (bytes) => {
          if (token !== this.#token) return
          this.result = as(bytes)
          this.readyState = 2
          progress('load', bytes.length)
          progress('loadend', bytes.length)
        },
        (error: unknown) => {
          if (token !== this.#token) return
          this.error = error instanceof DOMException ? error : new DOMException(String(error), 'NotReadableError')
          this.readyState = 2
          progress('error', 0)
          progress('loadend', 0)
        },
      )
    })
  }

  readAsText(blob: Blob, encoding = 'utf-8'): void {
    this.#read(blob, (bytes) => new TextDecoder(encoding).decode(bytes))
  }

  readAsDataURL(blob: Blob): void {
    this.#read(blob, (bytes) => `data:${blob.type || 'application/octet-stream'};base64,${base64(bytes)}`)
  }

  readAsArrayBuffer(blob: Blob): void {
    this.#read(blob, (bytes) => bytes.slice().buffer)
  }

  readAsBinaryString(blob: Blob): void {
    this.#read(blob, latin1)
  }

  abort(): void {
    if (this.readyState !== 1) return
    this.#token++
    this.readyState = 2
    this.result = null
    fire(this, new Event('abort'))
    fire(this, new Event('loadend'))
  }
}
defineHandlers(FileReader.prototype, ['loadstart', 'progress', 'load', 'abort', 'error', 'loadend'])
constants(FileReader, { EMPTY: 0, LOADING: 1, DONE: 2 })
tag(FileReader)
