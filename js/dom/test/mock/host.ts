import { randomFillSync, randomUUID } from 'node:crypto'
import { deserialize, serialize } from 'node:v8'
import vm from 'node:vm'
import type { HostNative } from '../../src/native.ts'

export class MockHost implements HostNative {
  readonly start = performance.now()
  readonly errors: unknown[] = []
  readonly logs: [number, string][] = []
  readonly assets = new Map<string, string | Uint8Array>()
  frameRequests = 0
  context: vm.Context | null = null
  afterTask: () => void = () => {}
  #timers = new Map<number, NodeJS.Timeout>()
  #next = 1

  now(): number {
    return performance.now() - this.start
  }
  timeOrigin(): number {
    return performance.timeOrigin + this.start
  }
  setTimer(ms: number, callback: () => void, refed = true): number {
    const handle = this.#next++
    const timer = setTimeout(() => {
      this.#timers.delete(handle)
      try {
        callback()
      } catch (error) {
        this.reportError(error)
      }
      setImmediate(() => this.afterTask())
    }, ms)
    if (!refed) timer.unref()
    this.#timers.set(handle, timer)
    return handle
  }
  clearTimer(handle: number): void {
    const timer = this.#timers.get(handle)
    if (timer) clearTimeout(timer)
    this.#timers.delete(handle)
  }
  refTimer(handle: number, refed: boolean): void {
    const timer = this.#timers.get(handle)
    if (refed) timer?.ref()
    else timer?.unref()
  }
  dispose(): void {
    for (const timer of this.#timers.values()) clearTimeout(timer)
    this.#timers.clear()
  }
  queueMicrotask(callback: () => void): void {
    queueMicrotask(callback)
  }
  gc(): void {
    ;(globalThis as { gc?: () => void }).gc?.()
  }
  requestFrame(): void {
    this.frameRequests++
  }
  reportError(error: unknown): void {
    this.errors.push(error)
  }
  print(level: number, text: string): void {
    this.logs.push([level, text])
  }
  log(): void {}
  logEnabled(): boolean {
    return false
  }
  clock(): number {
    return performance.now()
  }
  importModule(url: string): Promise<unknown> {
    return Promise.reject(new Error(`modules are not supported in tests: ${url}`))
  }
  defineModule(url: string): void {
    throw new Error(`modules are not supported in tests: ${url}`)
  }
  moduleSource: ((url: string) => string | null) | null = null
  setModuleSource(source: ((url: string) => string | null) | null): void {
    this.moduleSource = source
  }

  refuseModule(url: string): void {
    throw new Error(`modules are not supported in tests: ${url}`)
  }
  async runModule(source: string, url: string): Promise<unknown> {
    return vm.runInContext(`(async () => {\n${source}\n})()`, this.context!, { filename: url })
  }
  runScript(source: string, url: string): unknown {
    return vm.runInContext(source, this.context!, { filename: url })
  }
  readAsset(url: string): Uint8Array | null {
    const asset = this.assets.get(new URL(url).pathname)
    if (asset === undefined) return null
    return typeof asset === 'string' ? new TextEncoder().encode(asset) : asset
  }
  encodeUtf8(text: string): Uint8Array {
    return new TextEncoder().encode(text)
  }
  decode(bytes: Uint8Array, label: string, fatal: boolean, ignoreBom: boolean): string {
    return new TextDecoder(label, { fatal, ignoreBOM: ignoreBom }).decode(bytes)
  }
  readonly decoders = new Map<number, TextDecoder>()
  encodingName(label: string): string | null {
    if (label.trim() === '') return null
    try {
      return new TextDecoder(label).encoding
    } catch {
      return null
    }
  }
  openDecoder(encoding: string, fatal: boolean, ignoreBom: boolean): number {
    const id = this.#next++
    this.decoders.set(id, new TextDecoder(encoding, { fatal, ignoreBOM: ignoreBom }))
    return id
  }
  decodeChunk(id: number, bytes: Uint8Array, last: boolean): string {
    const decoder = this.decoders.get(id)
    if (!decoder) throw new TypeError('decodeChunk: no such decoder')
    try {
      return decoder.decode(bytes, { stream: !last })
    } catch (error) {
      this.decoders.delete(id)
      throw error
    } finally {
      if (last) this.decoders.delete(id)
    }
  }
  closeDecoder(id: number): void {
    this.decoders.delete(id)
  }
  parseUrl(input: string, base: string | null) {
    try {
      const u = base === null ? new URL(input) : new URL(input, base)
      return { href: u.href, origin: u.origin, protocol: u.protocol, username: u.username, password: u.password, host: u.host, hostname: u.hostname, port: u.port, pathname: u.pathname, search: u.search, hash: u.hash }
    } catch {
      return null
    }
  }
  serialize(value: unknown): Uint8Array {
    return serialize(value)
  }
  deserialize(bytes: Uint8Array): unknown {
    return deserialize(bytes)
  }
  randomBytes(buffer: Uint8Array): void {
    randomFillSync(buffer)
  }
  randomUUID(): string {
    return randomUUID()
  }
}
