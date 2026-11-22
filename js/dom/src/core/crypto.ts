import { DOMException } from './exception'
import { host, illegal, KEY, tag } from './host'

type AlgorithmIdentifier = string | { name: string }
type BufferSource = ArrayBuffer | ArrayBufferView

const DIGESTS: Readonly<Record<string, string>> = { 'sha-1': 'SHA-1', 'sha-256': 'SHA-256', 'sha-384': 'SHA-384', 'sha-512': 'SHA-512' }

function failed(what: string): TypeError {
  return new TypeError(`Failed to execute 'digest' on 'SubtleCrypto': ${what}`)
}

const byteLength = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'byteLength')!.get!
const resizable = Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, 'resizable')?.get

function plain(buffer: unknown): buffer is ArrayBuffer {
  try {
    byteLength.call(buffer)
  } catch {
    return false
  }
  return !resizable?.call(buffer)
}

function bufferSource(data: unknown): BufferSource {
  if (plain(data)) return data
  if (ArrayBuffer.isView(data) && plain(data.buffer)) return data
  throw failed("The provided value is not of type '(ArrayBuffer or ArrayBufferView)'.")
}

function normalizeDigest(algorithm: unknown): string {
  let name: string
  if ((typeof algorithm === 'object' && algorithm !== null) || typeof algorithm === 'function') {
    const given = (algorithm as { name?: unknown }).name
    if (given === undefined) throw failed('Algorithm: required member name is undefined.')
    name = String(given)
  } else name = String(algorithm)
  const found = DIGESTS[name.replace(/[A-Z]/g, (c) => c.toLowerCase())]
  if (!found) throw new DOMException('Algorithm: Unrecognized name', 'NotSupportedError')
  return found
}

export class SubtleCrypto {
  readonly #brand = true

  constructor(...key: unknown[]) {
    illegal(key[0])
  }

  digest(algorithm: AlgorithmIdentifier, data: BufferSource): Promise<ArrayBuffer> {
    try {
      if (typeof this !== 'object' || this === null || !(#brand in this)) throw new TypeError('Illegal invocation')
      if (arguments.length < 2) throw failed(`2 arguments required, but only ${arguments.length} present.`)
      const name = (algorithm !== null && typeof algorithm === 'object') || typeof algorithm === 'function' ? algorithm : String(algorithm)
      const bytes = bufferSource(data)
      const normalized = normalizeDigest(name)
      return host.digest(normalized, bytes)
    } catch (error) {
      return Promise.reject(error)
    }
  }
}
tag(SubtleCrypto)

export class Crypto {
  #subtle: SubtleCrypto | null = null

  get subtle(): SubtleCrypto {
    return (this.#subtle ??= new SubtleCrypto(KEY))
  }

  getRandomValues<T extends ArrayBufferView | null>(array: T): T {
    if (!ArrayBuffer.isView(array) || array instanceof Float32Array || array instanceof Float64Array || array instanceof DataView) {
      throw new DOMException("Failed to execute 'getRandomValues' on 'Crypto': The provided ArrayBufferView is of type that is not an integer array.", 'TypeMismatchError')
    }
    if (array.byteLength > 65536) throw new DOMException(`Failed to execute 'getRandomValues' on 'Crypto': The ArrayBufferView's byte length (${array.byteLength}) exceeds the number of bytes of entropy available via this API (65536).`, 'QuotaExceededError')
    host.randomBytes(new Uint8Array(array.buffer, array.byteOffset, array.byteLength))
    return array
  }

  randomUUID(): string {
    return host.randomUUID()
  }
}
tag(Crypto)
