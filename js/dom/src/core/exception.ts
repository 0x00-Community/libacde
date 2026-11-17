import { constants, tag } from './host'

const CODES: Record<string, number> = {
  IndexSizeError: 1, HierarchyRequestError: 3, WrongDocumentError: 4, InvalidCharacterError: 5, NoModificationAllowedError: 7,
  NotFoundError: 8, NotSupportedError: 9, InUseAttributeError: 10, InvalidStateError: 11, SyntaxError: 12, InvalidModificationError: 13,
  NamespaceError: 14, InvalidAccessError: 15, TypeMismatchError: 17, SecurityError: 18, NetworkError: 19, AbortError: 20,
  URLMismatchError: 21, QuotaExceededError: 22, TimeoutError: 23, InvalidNodeTypeError: 24, DataCloneError: 25,
}

export class DOMException extends Error {
  readonly #name: string

  constructor(message = '', name = 'Error') {
    super(String(message))
    this.#name = String(name)
  }

  override get name(): string {
    return this.#name
  }

  get code(): number {
    return CODES[this.#name] ?? 0
  }
}
tag(DOMException)
constants(DOMException, Object.fromEntries(('INDEX_SIZE DOMSTRING_SIZE HIERARCHY_REQUEST WRONG_DOCUMENT INVALID_CHARACTER NO_DATA_ALLOWED NO_MODIFICATION_ALLOWED NOT_FOUND ' +
  'NOT_SUPPORTED INUSE_ATTRIBUTE INVALID_STATE SYNTAX INVALID_MODIFICATION NAMESPACE INVALID_ACCESS VALIDATION TYPE_MISMATCH SECURITY NETWORK ABORT ' +
  'URL_MISMATCH QUOTA_EXCEEDED TIMEOUT INVALID_NODE_TYPE DATA_CLONE').split(' ').map((name, i) => [`${name}_ERR`, i + 1])))

const NATIVE = /^([A-Z][A-Za-z]*Error): ([\s\S]*)$/

export function nativeException(message: string, fallback?: string): DOMException | null {
  const m = NATIVE.exec(message)
  if (m && (fallback || m[1]! in CODES)) return new DOMException(m[2], m[1])
  return fallback ? new DOMException(message, fallback) : null
}

function domError(error: unknown): unknown {
  if (error !== null && typeof error === 'object' && !(error instanceof DOMException) && typeof (error as Error).message === 'string') return nativeException((error as Error).message) ?? error
  return error
}

export function guard<T>(call: () => T): T {
  try {
    return call()
  } catch (error) {
    throw domError(error)
  }
}
