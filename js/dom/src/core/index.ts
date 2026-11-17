import type { HostNative } from '../native'
import { AbortController, AbortSignal } from './abort'
import { Blob, File, FileReader } from './blob'
import { installClone } from './clone'
import { createConsole } from './console'
import { Crypto } from './crypto'
import { atob, btoa, TextDecoder, TextEncoder } from './encoding'
import { CloseEvent, CustomEvent, ErrorEvent, Event, EventTarget, MessageEvent, ProgressEvent, PromiseRejectionEvent } from './event'
import { DOMException } from './exception'
import { bindHost, globals, report } from './host'
import { BroadcastChannel, MessageChannel, MessagePort, structuredClone } from './message'
import { Performance, PerformanceEntry, PerformanceMark, PerformanceMeasure } from './performance'
import { timerGlobals } from './timers'
import { URL, URLSearchParams } from './url'

export interface CoreOptions {
  readonly nodeTimers?: boolean
}

export function installCore(global: typeof globalThis, native: HostNative, options: CoreOptions = {}): void {
  bindHost(global, native)
  installClone()
  globals(global, {
    console: createConsole(), performance: new Performance(), crypto: new Crypto(),
    ...timerGlobals(!!options.nodeTimers),
    structuredClone,
    reportError: (error: unknown) => report(error),
    atob, btoa,
    URL, URLSearchParams, TextEncoder, TextDecoder,
    Crypto, Performance, PerformanceEntry, PerformanceMark, PerformanceMeasure,
    AbortController, AbortSignal,
    Event, EventTarget, CustomEvent, ErrorEvent, MessageEvent, ProgressEvent, CloseEvent, PromiseRejectionEvent,
    DOMException,
    Blob, File, FileReader,
    MessageChannel, MessagePort, BroadcastChannel,
  })
}
