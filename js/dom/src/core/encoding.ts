import { DOMException } from './exception'
import { getters, host, tag } from './host'

type BufferSourceLike = ArrayBuffer | ArrayBufferView

export function bytesOf(input: BufferSourceLike | SharedArrayBuffer | null | undefined): Uint8Array {
  if (input === null || input === undefined) return new Uint8Array(0)
  if (ArrayBuffer.isView(input)) return new Uint8Array(input.buffer, input.byteOffset, input.byteLength)
  if (input instanceof ArrayBuffer || Object.prototype.toString.call(input) === '[object ArrayBuffer]' || Object.prototype.toString.call(input) === '[object SharedArrayBuffer]') return new Uint8Array(input as ArrayBuffer)
  throw new TypeError("The provided value is not of type '(ArrayBuffer or ArrayBufferView)'")
}

export class TextEncoder {
  encode(input = ''): Uint8Array {
    return host.encodeUtf8(String(input))
  }

  encodeInto(input: string, destination: Uint8Array): { read: number; written: number } {
    const text = String(input)
    const bytes = host.encodeUtf8(text)
    if (bytes.length <= destination.length) {
      destination.set(bytes)
      return { read: text.length, written: bytes.length }
    }
    let written = destination.length
    while (written > 0 && (bytes[written]! & 0xc0) === 0x80) written--
    destination.set(bytes.subarray(0, written))
    let read = 0
    for (let i = 0; i < written; ) {
      const b = bytes[i]!
      const n = b < 0x80 ? 1 : b < 0xe0 ? 2 : b < 0xf0 ? 3 : 4
      i += n
      read += n === 4 ? 2 : 1
    }
    return { read, written }
  }
}
getters(TextEncoder.prototype, { encoding: 'utf-8' })
tag(TextEncoder)

const streams = new FinalizationRegistry<number>((id) => host.closeDecoder(id))

export class TextDecoder {
  readonly #encoding: string
  readonly #fatal: boolean
  readonly #ignoreBOM: boolean
  #stream = 0

  constructor(label = 'utf-8', options?: { fatal?: boolean; ignoreBOM?: boolean }) {
    const encoding = host.encodingName(String(label))
    if (encoding === null) throw new RangeError(`Failed to construct 'TextDecoder': The encoding label provided ('${label}') is invalid.`)
    this.#encoding = encoding
    this.#fatal = !!options?.fatal
    this.#ignoreBOM = !!options?.ignoreBOM
  }

  get encoding(): string {
    return this.#encoding
  }
  get fatal(): boolean {
    return this.#fatal
  }
  get ignoreBOM(): boolean {
    return this.#ignoreBOM
  }

  decode(input?: BufferSourceLike | null, options?: { stream?: boolean }): string {
    const bytes = bytesOf(input)
    let stream = !!options?.stream
    if (!stream && !this.#stream) return host.decode(bytes, this.#encoding, this.#fatal, this.#ignoreBOM)
    if (!this.#stream) {
      this.#stream = host.openDecoder(this.#encoding, this.#fatal, this.#ignoreBOM)
      streams.register(this, this.#stream, this)
    }
    const id = this.#stream
    try {
      return host.decodeChunk(id, bytes, !stream)
    } catch (error) {
      host.closeDecoder(id)
      stream = false
      throw error
    } finally {
      if (!stream) {
        streams.unregister(this)
        this.#stream = 0
      }
    }
  }
}
tag(TextDecoder)

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
const B64_INDEX = new Int16Array(128).fill(-1)
for (let i = 0; i < B64.length; i++) B64_INDEX[B64.charCodeAt(i)] = i

export function base64(bytes: Uint8Array): string {
  let out = ''
  let i = 0
  for (; i + 2 < bytes.length; i += 3) {
    const n = (bytes[i]! << 16) | (bytes[i + 1]! << 8) | bytes[i + 2]!
    out += B64[n >> 18]! + B64[(n >> 12) & 63]! + B64[(n >> 6) & 63]! + B64[n & 63]!
  }
  if (i < bytes.length) {
    const n = (bytes[i]! << 16) | ((bytes[i + 1] ?? 0) << 8)
    out += B64[n >> 18]! + B64[(n >> 12) & 63]! + (i + 1 < bytes.length ? B64[(n >> 6) & 63]! : '=') + '='
  }
  return out
}

export function unbase64(text: string): Uint8Array | null {
  const clean = text.replace(/[\t\n\f\r ]/g, '')
  const body = clean.length % 4 === 0 ? clean.replace(/={1,2}$/, '') : clean
  if (body.length % 4 === 1 || /[^A-Za-z0-9+/]/.test(body)) return null
  const out = new Uint8Array(Math.floor((body.length * 3) / 4))
  let bits = 0, value = 0, o = 0
  for (let i = 0; i < body.length; i++) {
    value = (value << 6) | B64_INDEX[body.charCodeAt(i)]!
    bits += 6
    if (bits >= 8) {
      bits -= 8
      out[o++] = (value >> bits) & 255
    }
  }
  return out
}

export function concat(parts: readonly Uint8Array[]): Uint8Array {
  let size = 0
  for (const part of parts) size += part.length
  const out = new Uint8Array(size)
  let at = 0
  for (const part of parts) {
    out.set(part, at)
    at += part.length
  }
  return out
}

export function latin1(bytes: Uint8Array): string {
  let out = ''
  for (let i = 0; i < bytes.length; i += 8192) out += String.fromCharCode(...bytes.subarray(i, i + 8192))
  return out
}

export function btoa(data: string): string {
  const text = String(data)
  const bytes = new Uint8Array(text.length)
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i)
    if (c > 255) throw new DOMException("Failed to execute 'btoa': The string to be encoded contains characters outside of the Latin1 range.", 'InvalidCharacterError')
    bytes[i] = c
  }
  return base64(bytes)
}

export function atob(data: string): string {
  const bytes = unbase64(String(data))
  if (!bytes) throw new DOMException("Failed to execute 'atob': The string to be decoded is not correctly encoded.", 'InvalidCharacterError')
  return latin1(bytes)
}
