import { AbortController, type AbortSignal } from './abort'
import { bytesOf, concat, TextDecoder, TextEncoder } from './encoding'
import { getters, illegal, KEY, tag } from './host'

interface Deferred<T> {
  promise: Promise<T>
  resolve: (value: T) => void
  reject: (reason: unknown) => void
  settled: boolean
}

export function deferred<T>(): Deferred<T> {
  const d = { settled: false } as Deferred<T>
  d.promise = new Promise<T>((resolve, reject) => {
    d.resolve = (value) => {
      if (d.settled) return
      d.settled = true
      resolve(value)
    }
    d.reject = (reason) => {
      if (d.settled) return
      d.settled = true
      reject(reason)
    }
  })
  d.promise.catch(() => undefined)
  return d
}

function attempt(fn: (() => unknown) | undefined): Promise<unknown> {
  try {
    return Promise.resolve(fn?.())
  } catch (error) {
    return Promise.reject(error)
  }
}

interface Strategy<T> {
  readonly highWaterMark?: number
  readonly size?: (chunk: T) => number
}

function strategy<T>(given: Strategy<T> | undefined, fallback: number): { hwm: number; size: (chunk: T) => number } {
  const hwm = given?.highWaterMark === undefined ? fallback : Number(given.highWaterMark)
  if (Number.isNaN(hwm) || hwm < 0) throw new RangeError('The highWaterMark of a queuing strategy must be a non-negative number')
  const size = given?.size
  return { hwm, size: size ? (chunk) => Number(size(chunk)) : () => 1 }
}

interface UnderlyingSource<R> {
  start?(controller: ReadableStreamDefaultController<R>): unknown
  pull?(controller: ReadableStreamDefaultController<R>): unknown
  cancel?(reason: unknown): unknown
  type?: string
}

type ReadResult<R> = { value: R; done: false } | { value: undefined; done: true }

interface ReadState<R> {
  state: 'readable' | 'closed' | 'errored'
  error: unknown
  queue: { chunk: R; size: number }[]
  total: number
  hwm: number
  size: (chunk: R) => number
  closeRequested: boolean
  started: boolean
  pulling: boolean
  pullAgain: boolean
  disturbed: boolean
  source: UnderlyingSource<R>
  controller: ReadableStreamDefaultController<R>
  reader: ReadableStreamDefaultReader<R> | null
  reads: Deferred<ReadResult<R>>[]
  closed: Deferred<undefined> | null
}

const readStates = new WeakMap<object, ReadState<any>>()

function rs<R>(stream: ReadableStream<R>): ReadState<R> {
  const s = readStates.get(stream)
  if (!s) throw new TypeError('Illegal invocation')
  return s
}

function desiredSize(s: { readonly state: string; readonly hwm: number; readonly total: number }): number | null {
  if (s.state === 'errored') return null
  if (s.state === 'closed') return 0
  return s.hwm - s.total
}

function finishClose(s: ReadState<any>): void {
  if (s.state !== 'readable') return
  s.state = 'closed'
  s.queue = []
  s.total = 0
  for (const read of s.reads.splice(0)) read.resolve({ value: undefined, done: true })
  s.closed?.resolve(undefined)
}

function errorStream(s: ReadState<any>, error: unknown): void {
  if (s.state !== 'readable') return
  s.state = 'errored'
  s.error = error
  s.queue = []
  s.total = 0
  for (const read of s.reads.splice(0)) read.reject(error)
  s.closed?.reject(error)
}

function pullIfNeeded(s: ReadState<any>): void {
  if (!s.started || s.closeRequested || s.state !== 'readable') return
  const wanted = (s.reader !== null && s.reads.length > 0) || (desiredSize(s) ?? 0) > 0
  if (!wanted) return
  if (s.pulling) {
    s.pullAgain = true
    return
  }
  if (!s.source.pull) return
  s.pulling = true
  attempt(() => s.source.pull!.call(s.source, s.controller)).then(() => {
    s.pulling = false
    if (s.pullAgain) {
      s.pullAgain = false
      pullIfNeeded(s)
    }
  }, (error) => errorStream(s, error))
}

function enqueue<R>(s: ReadState<R>, chunk: R): void {
  if (s.closeRequested || s.state !== 'readable') throw new TypeError('Cannot enqueue a chunk into a closed or errored stream')
  const read = s.reads.shift()
  if (read) read.resolve({ value: chunk, done: false })
  else {
    let size: number
    try {
      size = s.size(chunk)
      if (!(size >= 0) || size === Infinity) throw new RangeError('The size of a chunk must be a finite, non-negative number')
    } catch (error) {
      errorStream(s, error)
      throw error
    }
    s.queue.push({ chunk, size })
    s.total += size
  }
  pullIfNeeded(s)
}

function cancelStream(s: ReadState<any>, reason: unknown): Promise<undefined> {
  s.disturbed = true
  if (s.state === 'closed') return Promise.resolve(undefined)
  if (s.state === 'errored') return Promise.reject(s.error)
  finishClose(s)
  return attempt(() => s.source.cancel?.call(s.source, reason)).then(() => undefined)
}

export class ReadableStreamDefaultController<R = unknown> {
  readonly #stream: ReadableStream<R>

  constructor(token: unknown, stream: ReadableStream<R>) {
    illegal(token)
    this.#stream = stream
  }

  get desiredSize(): number | null {
    return desiredSize(rs(this.#stream))
  }

  enqueue(chunk: R): void {
    enqueue(rs(this.#stream), chunk)
  }

  close(): void {
    const s = rs(this.#stream)
    if (s.closeRequested || s.state !== 'readable') throw new TypeError('The stream is not in a state that permits close')
    s.closeRequested = true
    if (!s.queue.length) finishClose(s)
  }

  error(error?: unknown): void {
    errorStream(rs(this.#stream), error)
  }
}
tag(ReadableStreamDefaultController)

function readerReleased(): TypeError {
  return new TypeError('This readable stream reader has been released')
}

export class ReadableStreamDefaultReader<R = unknown> {
  #stream: ReadableStream<R> | null
  #closed: Deferred<undefined>

  constructor(stream: ReadableStream<R>) {
    const s = rs(stream)
    if (s.reader) throw new TypeError("Failed to construct 'ReadableStreamDefaultReader': ReadableStream is locked")
    this.#stream = stream
    s.reader = this
    this.#closed = deferred()
    s.closed = this.#closed
    if (s.state === 'closed') this.#closed.resolve(undefined)
    else if (s.state === 'errored') this.#closed.reject(s.error)
  }

  get closed(): Promise<undefined> {
    return this.#closed.promise
  }

  read(): Promise<ReadResult<R>> {
    if (!this.#stream) return Promise.reject(readerReleased())
    const s = rs(this.#stream)
    s.disturbed = true
    if (s.state === 'closed') return Promise.resolve({ value: undefined, done: true })
    if (s.state === 'errored') return Promise.reject(s.error)
    const head = s.queue.shift()
    if (head) {
      s.total -= head.size
      if (s.closeRequested && !s.queue.length) finishClose(s)
      else pullIfNeeded(s)
      return Promise.resolve({ value: head.chunk, done: false })
    }
    const read = deferred<ReadResult<R>>()
    s.reads.push(read)
    pullIfNeeded(s)
    return read.promise
  }

  releaseLock(): void {
    if (!this.#stream) return
    const s = rs(this.#stream)
    const released = readerReleased()
    for (const read of s.reads.splice(0)) read.reject(released)
    if (this.#closed.settled) this.#closed = deferred()
    this.#closed.reject(released)
    s.reader = null
    s.closed = null
    this.#stream = null
  }

  cancel(reason?: unknown): Promise<undefined> {
    if (!this.#stream) return Promise.reject(readerReleased())
    return cancelStream(rs(this.#stream), reason)
  }
}
tag(ReadableStreamDefaultReader)

export class ReadableStream<R = unknown> {
  constructor(source: UnderlyingSource<R> | null = null, given?: Strategy<R>) {
    const src = source ?? {}
    const { hwm, size } = strategy(given, 1)
    const s: ReadState<R> = {
      state: 'readable', error: undefined, queue: [], total: 0, hwm, size, closeRequested: false, started: false, pulling: false,
      pullAgain: false, disturbed: false, source: src, controller: null as never, reader: null, reads: [], closed: null,
    }
    readStates.set(this, s)
    s.controller = new ReadableStreamDefaultController<R>(KEY, this)
    attempt(() => src.start?.call(src, s.controller)).then(() => {
      s.started = true
      pullIfNeeded(s)
    }, (error) => errorStream(s, error))
  }

  get locked(): boolean {
    return rs(this).reader !== null
  }

  getReader(options?: { mode?: string }): ReadableStreamDefaultReader<R> {
    if (options?.mode !== undefined) throw new TypeError(`Failed to execute 'getReader' on 'ReadableStream': mode '${options.mode}' is not supported`)
    return new ReadableStreamDefaultReader(this)
  }

  cancel(reason?: unknown): Promise<undefined> {
    if (this.locked) return Promise.reject(new TypeError('Cannot cancel a locked stream'))
    return cancelStream(rs(this), reason)
  }

  tee(): [ReadableStream<R>, ReadableStream<R>] {
    const reader = this.getReader()
    let reading = false
    let pending = false
    const canceled = [false, false]
    const reasons: unknown[] = [undefined, undefined]
    const controllers: ReadableStreamDefaultController<R>[] = []
    const cancelled = deferred<undefined>()
    const pull = (): Promise<void> => {
      if (reading) {
        pending = true
        return Promise.resolve()
      }
      reading = true
      reader.read().then(({ value, done }) => {
        reading = false
        for (let i = 0; i < 2; i++) {
          if (canceled[i]) continue
          try {
            if (done) controllers[i]!.close()
            else controllers[i]!.enqueue(value as R)
          } catch {
          }
        }
        if (done) cancelled.resolve(undefined)
        else if (pending) {
          pending = false
          void pull()
        }
      }, (error) => {
        reading = false
        for (const c of controllers) c.error(error)
        cancelled.resolve(undefined)
      })
      return Promise.resolve()
    }
    const branch = (i: number) => new ReadableStream<R>({
      start: (c) => void (controllers[i] = c),
      pull,
      cancel: (reason) => {
        canceled[i] = true
        reasons[i] = reason
        if (canceled[0] && canceled[1]) cancelled.resolve(reader.cancel(reasons) as never)
        return cancelled.promise
      },
    })
    return [branch(0), branch(1)]
  }

  async pipeTo(destination: WritableStream<R>, options: { preventClose?: boolean; preventAbort?: boolean; preventCancel?: boolean; signal?: AbortSignalLike } = {}): Promise<void> {
    if (this.locked) throw new TypeError("Failed to execute 'pipeTo' on 'ReadableStream': Cannot pipe a locked stream")
    if (destination.locked) throw new TypeError("Failed to execute 'pipeTo' on 'ReadableStream': Cannot pipe to a locked stream")
    const reader = this.getReader()
    const writer = destination.getWriter()
    const signal = options.signal
    const aborted = deferred<never>()
    const onAbort = () => aborted.reject(signal!.reason)
    if (signal?.aborted) onAbort()
    else signal?.addEventListener?.('abort', onAbort)
    const race = <T>(promise: Promise<T>): Promise<T> => (signal ? Promise.race([promise, aborted.promise]) : promise)
    let side: 'source' | 'dest' | 'signal' = 'source'
    try {
      for (;;) {
        side = 'dest'
        await race(writer.ready)
        side = 'source'
        const read = reader.read()
        read.catch(() => undefined)
        const { value, done } = await race(read)
        if (done) {
          side = 'dest'
          if (!options.preventClose) await writer.close()
          return
        }
        side = 'dest'
        await race(writer.write(value as R))
      }
    } catch (error) {
      if (aborted.settled) side = 'signal'
      if (side !== 'source' && !options.preventCancel) await reader.cancel(error).catch(() => undefined)
      if (side !== 'dest' && !options.preventAbort) await writer.abort(error).catch(() => undefined)
      throw error
    } finally {
      signal?.removeEventListener?.('abort', onAbort)
      reader.releaseLock()
      writer.releaseLock()
    }
  }

  pipeThrough<T>(transform: { writable: WritableStream<R>; readable: ReadableStream<T> }, options?: { preventClose?: boolean; preventAbort?: boolean; preventCancel?: boolean; signal?: AbortSignalLike }): ReadableStream<T> {
    if (this.locked) throw new TypeError("Failed to execute 'pipeThrough' on 'ReadableStream': Cannot pipe a locked stream")
    this.pipeTo(transform.writable, options).catch(() => undefined)
    return transform.readable
  }

  values(options?: { preventCancel?: boolean }): AsyncIterableIterator<R> {
    const reader = this.getReader()
    const preventCancel = !!options?.preventCancel
    const iterator: AsyncIterableIterator<R> = {
      async next() {
        const result = await reader.read().catch((error) => {
          reader.releaseLock()
          throw error
        })
        if (result.done) reader.releaseLock()
        return result.done ? { value: undefined, done: true } : { value: result.value, done: false }
      },
      async return(value?: unknown) {
        if (!preventCancel) await reader.cancel(value)
        reader.releaseLock()
        return { value: value as R, done: true }
      },
      [Symbol.asyncIterator]() {
        return iterator
      },
    }
    return iterator
  }

  [Symbol.asyncIterator](options?: { preventCancel?: boolean }): AsyncIterableIterator<R> {
    return this.values(options)
  }

  static from<R>(iterable: Iterable<R> | AsyncIterable<R>): ReadableStream<R> {
    if (iterable instanceof ReadableStream) return iterable as ReadableStream<R>
    const asyncIt = (iterable as AsyncIterable<R>)[Symbol.asyncIterator]
    const iterator = asyncIt ? asyncIt.call(iterable) : (iterable as Iterable<R>)[Symbol.iterator]() as unknown as AsyncIterator<R>
    return new ReadableStream<R>({
      async pull(controller) {
        const { value, done } = await iterator.next()
        if (done) controller.close()
        else controller.enqueue(await value)
      },
      async cancel(reason) {
        await iterator.return?.(reason)
      },
    }, { highWaterMark: 0 })
  }
}
tag(ReadableStream)

type AbortSignalLike = { readonly aborted: boolean; readonly reason: unknown; addEventListener?(type: 'abort', fn: () => void): void; removeEventListener?(type: 'abort', fn: () => void): void }

export function isDisturbed(stream: ReadableStream<any>): boolean {
  return rs(stream).disturbed
}

export function streamOfBytes(bytes: Uint8Array): ReadableStream<Uint8Array> {
  return new ReadableStream<Uint8Array>({
    start(controller) {
      if (bytes.length) controller.enqueue(bytes)
      controller.close()
    },
  })
}

export async function readAllBytes(stream: ReadableStream<any>, keepLock = false): Promise<Uint8Array> {
  const reader = stream.getReader()
  const parts: Uint8Array[] = []
  const encoder = new TextEncoder()
  try {
    for (;;) {
      const { value, done } = await reader.read()
      if (done) break
      if (value === null || value === undefined) throw new TypeError('The body stream gave a chunk that is not a Uint8Array')
      parts.push(typeof value === 'string' ? encoder.encode(value) : bytesOf(value))
    }
  } finally {
    if (!keepLock) reader.releaseLock()
  }
  return parts.length === 1 ? parts[0]!.slice() : concat(parts)
}

interface UnderlyingSink<W> {
  start?(controller: WritableStreamDefaultController): unknown
  write?(chunk: W, controller: WritableStreamDefaultController): unknown
  close?(): unknown
  abort?(reason: unknown): unknown
}

const CLOSE = Symbol('close')

interface WriteState<W> {
  state: 'writable' | 'closed' | 'errored'
  error: unknown
  queue: { chunk: W | typeof CLOSE; size: number; done: Deferred<undefined> }[]
  total: number
  hwm: number
  size: (chunk: W) => number
  started: boolean
  busy: boolean
  closeRequested: boolean
  sink: UnderlyingSink<W>
  controller: WritableStreamDefaultController
  abort: AbortController
  writer: WritableStreamDefaultWriter<W> | null
  ready: Deferred<undefined>
  closed: Deferred<undefined>
}

const writeStates = new WeakMap<object, WriteState<any>>()

function ws<W>(stream: WritableStream<W>): WriteState<W> {
  const s = writeStates.get(stream)
  if (!s) throw new TypeError('Illegal invocation')
  return s
}

function backpressure(s: WriteState<any>): void {
  if (s.state !== 'writable') return
  const full = !s.closeRequested && s.hwm - s.total <= 0
  if (full && s.ready.settled) s.ready = deferred()
  else if (!full) s.ready.resolve(undefined)
}

function failWritable(s: WriteState<any>, error: unknown): void {
  if (s.state !== 'writable') return
  s.state = 'errored'
  s.error = error
  s.abort.abort(error)
  for (const item of s.queue.splice(0)) item.done.reject(error)
  s.total = 0
  if (s.ready.settled) s.ready = deferred()
  s.ready.reject(error)
  s.closed.reject(error)
}

function advance(s: WriteState<any>): void {
  if (!s.started || s.busy || s.state !== 'writable') return
  const head = s.queue.shift()
  if (!head) return
  s.total -= head.size
  s.busy = true
  const fail = (error: unknown) => {
    s.busy = false
    head.done.reject(error)
    failWritable(s, error)
  }
  if (head.chunk === CLOSE) {
    attempt(() => s.sink.close?.call(s.sink)).then(() => {
      s.busy = false
      if (s.state !== 'writable') return
      s.state = 'closed'
      head.done.resolve(undefined)
      s.closed.resolve(undefined)
    }, fail)
    return
  }
  attempt(() => s.sink.write?.call(s.sink, head.chunk, s.controller)).then(() => {
    s.busy = false
    head.done.resolve(undefined)
    backpressure(s)
    advance(s)
  }, fail)
}

export class WritableStreamDefaultController {
  readonly #stream: WritableStream<any>

  constructor(token: unknown, stream: WritableStream<any>) {
    illegal(token)
    this.#stream = stream
  }

  get signal(): AbortSignal {
    return ws(this.#stream).abort.signal as unknown as AbortSignal
  }

  error(error?: unknown): void {
    failWritable(ws(this.#stream), error)
  }
}
tag(WritableStreamDefaultController)

function writerReleased(): TypeError {
  return new TypeError('This writable stream writer has been released')
}

export class WritableStreamDefaultWriter<W = unknown> {
  #stream: WritableStream<W> | null

  constructor(stream: WritableStream<W>) {
    const s = ws(stream)
    if (s.writer) throw new TypeError("Failed to construct 'WritableStreamDefaultWriter': WritableStream is locked")
    s.writer = this
    this.#stream = stream
  }

  #state(): WriteState<W> {
    if (!this.#stream) throw writerReleased()
    return ws(this.#stream)
  }

  #held(fn: (s: WriteState<W>) => Promise<undefined>): Promise<undefined> {
    return this.#stream ? fn(ws(this.#stream)) : Promise.reject(writerReleased())
  }

  get closed(): Promise<undefined> {
    return this.#held((s) => s.closed.promise)
  }

  get ready(): Promise<undefined> {
    return this.#held((s) => s.ready.promise)
  }

  get desiredSize(): number | null {
    return desiredSize(this.#state())
  }

  write(chunk: W): Promise<undefined> {
    return this.#held((s) => {
      if (s.state === 'errored') return Promise.reject(s.error)
      if (s.state === 'closed' || s.closeRequested) return Promise.reject(new TypeError('Cannot write to a closing or closed stream'))
      let size: number
      try {
        size = s.size(chunk)
      } catch (error) {
        failWritable(s, error)
        return Promise.reject(error)
      }
      const done = deferred<undefined>()
      s.queue.push({ chunk, size, done })
      s.total += size
      backpressure(s)
      advance(s)
      return done.promise
    })
  }

  close(): Promise<undefined> {
    return this.#held(closeWritable)
  }

  abort(reason?: unknown): Promise<undefined> {
    return this.#held((s) => abortWritable(s, reason))
  }

  releaseLock(): void {
    if (!this.#stream) return
    ws(this.#stream).writer = null
    this.#stream = null
  }
}
tag(WritableStreamDefaultWriter)

function closeWritable(s: WriteState<any>): Promise<undefined> {
  if (s.state === 'errored') return Promise.reject(s.error)
  if (s.state === 'closed' || s.closeRequested) return Promise.reject(new TypeError('Cannot close a closing or closed stream'))
  s.closeRequested = true
  const done = deferred<undefined>()
  s.queue.push({ chunk: CLOSE, size: 0, done })
  backpressure(s)
  advance(s)
  return done.promise
}

function abortWritable(s: WriteState<any>, reason: unknown): Promise<undefined> {
  if (s.state !== 'writable') return Promise.resolve(undefined)
  failWritable(s, reason)
  return attempt(() => s.sink.abort?.call(s.sink, reason)).then(() => undefined)
}

export class WritableStream<W = unknown> {
  constructor(sink: UnderlyingSink<W> | null = null, given?: Strategy<W>) {
    const snk = sink ?? {}
    const { hwm, size } = strategy(given, 1)
    const s: WriteState<W> = {
      state: 'writable', error: undefined, queue: [], total: 0, hwm, size, started: false, busy: false, closeRequested: false,
      sink: snk, controller: null as never, abort: new AbortController(), writer: null, ready: deferred(), closed: deferred(),
    }
    writeStates.set(this, s)
    s.controller = new WritableStreamDefaultController(KEY, this)
    backpressure(s)
    attempt(() => snk.start?.call(snk, s.controller)).then(() => {
      s.started = true
      advance(s)
    }, (error) => failWritable(s, error))
  }

  get locked(): boolean {
    return ws(this).writer !== null
  }

  getWriter(): WritableStreamDefaultWriter<W> {
    return new WritableStreamDefaultWriter(this)
  }

  close(): Promise<undefined> {
    if (this.locked) return Promise.reject(new TypeError('Cannot close a locked stream'))
    return closeWritable(ws(this))
  }

  abort(reason?: unknown): Promise<undefined> {
    if (this.locked) return Promise.reject(new TypeError('Cannot abort a locked stream'))
    return abortWritable(ws(this), reason)
  }
}
tag(WritableStream)

interface Transformer<I, O> {
  start?(controller: TransformStreamDefaultController<O>): unknown
  transform?(chunk: I, controller: TransformStreamDefaultController<O>): unknown
  flush?(controller: TransformStreamDefaultController<O>): unknown
  cancel?(reason: unknown): unknown
}

export class TransformStreamDefaultController<O = unknown> {
  readonly #readable: ReadableStreamDefaultController<O>
  readonly #writable: () => WritableStreamDefaultController

  constructor(token: unknown, readable: ReadableStreamDefaultController<O>, writable: () => WritableStreamDefaultController) {
    illegal(token)
    this.#readable = readable
    this.#writable = writable
  }

  get desiredSize(): number | null {
    return this.#readable.desiredSize
  }

  enqueue(chunk: O): void {
    this.#readable.enqueue(chunk)
  }

  error(reason?: unknown): void {
    this.#readable.error(reason)
    this.#writable().error(reason)
  }

  terminate(): void {
    try {
      this.#readable.close()
    } catch {
    }
    this.#writable().error(new TypeError('The transform stream was terminated'))
  }
}
tag(TransformStreamDefaultController)

export class TransformStream<I = unknown, O = unknown> {
  readonly readable: ReadableStream<O>
  readonly writable: WritableStream<I>

  constructor(transformer: Transformer<I, O> | null = null, writableStrategy?: Strategy<I>, readableStrategy?: Strategy<O>) {
    const t = transformer ?? {}
    let rc!: ReadableStreamDefaultController<O>
    let wc!: WritableStreamDefaultController
    this.readable = new ReadableStream<O>({
      start: (c) => void (rc = c),
      cancel: (reason) => {
        wc.error(reason)
        return t.cancel?.call(t, reason)
      },
    }, readableStrategy ?? { highWaterMark: 0 })
    const tc = new TransformStreamDefaultController<O>(KEY, rc, () => wc)
    this.writable = new WritableStream<I>({
      start: (c) => {
        wc = c
        return t.start?.call(t, tc)
      },
      write: (chunk) => (t.transform ? t.transform.call(t, chunk, tc) : tc.enqueue(chunk as unknown as O)),
      close: async () => {
        await t.flush?.call(t, tc)
        try {
          rc.close()
        } catch {
        }
      },
      abort: (reason) => {
        rc.error(reason)
        return t.cancel?.call(t, reason)
      },
    }, writableStrategy ?? { highWaterMark: 1 })
  }
}
tag(TransformStream)

export class TextDecoderStream {
  readonly readable: ReadableStream<string>
  readonly writable: WritableStream<ArrayBuffer | ArrayBufferView>
  readonly #decoder: TextDecoder

  constructor(label = 'utf-8', options?: { fatal?: boolean; ignoreBOM?: boolean }) {
    const decoder = (this.#decoder = new TextDecoder(label, options))
    const t = new TransformStream<ArrayBuffer | ArrayBufferView, string>({
      transform(chunk, controller) {
        const text = decoder.decode(chunk, { stream: true })
        if (text) controller.enqueue(text)
      },
      flush(controller) {
        const text = decoder.decode()
        if (text) controller.enqueue(text)
      },
    })
    this.readable = t.readable
    this.writable = t.writable
  }

  get encoding(): string {
    return this.#decoder.encoding
  }
  get fatal(): boolean {
    return this.#decoder.fatal
  }
  get ignoreBOM(): boolean {
    return this.#decoder.ignoreBOM
  }
}
tag(TextDecoderStream)

export class TextEncoderStream {
  readonly readable: ReadableStream<Uint8Array>
  readonly writable: WritableStream<string>

  constructor() {
    const encoder = new TextEncoder()
    let lead = ''
    const t = new TransformStream<string, Uint8Array>({
      transform(chunk, controller) {
        let text = lead + String(chunk)
        lead = ''
        const last = text.charCodeAt(text.length - 1)
        if (last >= 0xd800 && last <= 0xdbff) {
          lead = text.slice(-1)
          text = text.slice(0, -1)
        }
        if (text) controller.enqueue(encoder.encode(text))
      },
      flush(controller) {
        if (lead) controller.enqueue(encoder.encode('\uFFFD'))
      },
    })
    this.readable = t.readable
    this.writable = t.writable
  }
}
getters(TextEncoderStream.prototype, { encoding: 'utf-8' })
tag(TextEncoderStream)
