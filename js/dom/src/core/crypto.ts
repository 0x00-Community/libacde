import { DOMException } from './exception'
import { host, tag } from './host'

export class Crypto {
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
