import { installCore, type CoreOptions } from './core/index'
import { describe, inspectHooks } from './core/console'
import { CustomEvent, ErrorEvent, Event, fire, PromiseRejectionEvent, type EventTarget } from './core/event'
import { globals, host, KEY, report, setReport } from './core/host'
import { observer, type PageErrorKind } from './core/observe'
import { EventSource } from './core/net/eventsource'
import { Headers } from './core/net/headers'
import { fetch, moduleSource, net, Request, Response } from './core/net'
import { WebSocket } from './core/net/websocket'
import { XMLHttpRequest, XMLHttpRequestEventTarget, XMLHttpRequestUpload } from './core/net/xhr'
import { loadResource, reportServed } from './core/resources'
import {
  ReadableStream,
  ReadableStreamDefaultController,
  ReadableStreamDefaultReader,
  TextDecoderStream,
  TextEncoderStream,
  TransformStream,
  TransformStreamDefaultController,
  WritableStream,
  WritableStreamDefaultController,
  WritableStreamDefaultWriter,
} from './core/streams'
import { objectUrlOrigin, URL } from './core/url'
import type { AudioNative, CaptureNative, DomNative, GpuNative, HostNative, StorageNative } from './native'
import { installDevice, type DeviceNative } from './device'
import { installStorage, Storage } from './storage/webstorage'
import { idbTaskEnded, installIndexedDB } from './storage/idb/idb'
import { StorageManager } from './storage/manager'
import { installCaches } from './storage/cache'
import { contexts as gpuContexts, installGpu } from './gpu/index'
import { installWebgl, webglContexts, type WebglNative } from './webgl/index'
import { installAudio } from './audio/index'
import { NodeType } from './native'
import { Animation, AnimationEffect, AnimationPlaybackEvent, AnimationTimeline, animate, animationsOf, DocumentTimeline, KeyframeEffect } from './dom/animation'
import { CanvasContext2d, CanvasGradient, CanvasPattern, canvases, ImageData, Path2D, TextMetrics } from './dom/canvas'
import { bitmaps, ImageBitmap } from './dom/bitmap'
import { CustomElementRegistry } from './dom/custom'
import * as editing from './dom/editing'
import { Document, DOMImplementation, DOMParser, documentState, FontFace, FontFaceSet, HTMLDocument, XMLDocument, XMLSerializer } from './dom/document'
import { Attr, DOMStringMap, DOMTokenList, Element, flags, LOCAL, NamedNodeMap } from './dom/element'
import { env, frames, loads } from './dom/env'
import * as events from './dom/events'
import { focus } from './dom/focus'
import * as forms from './dom/forms'
import { DOMMatrix, DOMMatrixReadOnly, DOMPoint, DOMPointReadOnly, DOMQuad, DOMRect, DOMRectList, DOMRectReadOnly } from './dom/geometry'
import * as html from './dom/html'
import { createHtml, HTMLElement } from './dom/html'
import { Input } from './dom/input'
import * as media from './dom/media'
import { deliverLoads, presentVideos } from './dom/media'
import { cssEventsWanted, deliverCssEvents, onCssEventsWanted } from './dom/cssevents'
import { escape, lightDismiss } from './dom/popover'
import {
  InputDeviceInfo,
  installCapture,
  MediaDeviceInfo,
  MediaDevices,
  MediaStream,
  MediaStreamTrack,
  OverconstrainedError,
  Permissions,
  PermissionStatus,
} from './media/stream'
import { MutationObserver, MutationRecord } from './dom/mutation'
import { CDATASection, CharacterData, Comment, DocumentFragment, DocumentType, HTMLCollection, Node, NodeList, ProcessingInstruction, Text } from './dom/node'
import { deliverObservations, IntersectionObserver, IntersectionObserverEntry, ResizeObserver, ResizeObserverEntry, watching } from './dom/observers'
import { loadPage } from './dom/page'
import { handedOver, pickedOf, type Mode } from './dom/picker'
import { AbstractRange, CaretPosition, getSelection, Range, Selection, StaticRange } from './dom/range'
import { bindDom, changed, documentId, dom, existing, idOf, moved, rootElementId, version, wrap, wrapperStats, inner } from './dom/realm'
import { attachShadow, shadowRootOf, ShadowRoot } from './dom/shadow'
import { CSS, CSSRule, CSSRuleList, CSSStyleDeclaration, CSSStyleRule, CSSStyleSheet, StyleSheetList } from './dom/style'
import * as svg from './dom/svg'
import { NodeFilter, NodeIterator, TreeWalker } from './dom/traversal'
import {
  Clipboard,
  framesPending,
  History,
  installWindow,
  Location,
  mediaChanged,
  MediaQueryList,
  Navigator,
  readViewport,
  runAnimationFrames,
  Screen,
  screenSize,
  ScreenOrientation,
  setNavigatorLanguages,
  setNavigatorStorage,
  VisualViewport,
  Window,
  type NavigatorOptions,
} from './dom/window'
import { clockHandle, now as clockNow, type ClockRequest, type ClockState } from './core/clock'
import { adoptTimers } from './core/timers'

export { hostTime, type ClockRequest, type ClockState } from './core/clock'
export type { DeviceNative } from './device'
export type { AudioNative, CaptureNative, DomNative, GpuNative, HostNative, StorageAreaNative, StorageNative } from './native'
export type { WebglNative } from './webgl/index'
export { installBackendTransport, type BackendTransport, type Header as BackendHeader, type HttpEvent, type WsEvent } from './core/backend'
export { NO_NETWORK } from './core/net'
export {
  observe,
  type ConsoleLevel,
  type DialogAnswer,
  type DialogEvent,
  type DialogType,
  type MockReply,
  type NetworkHeaders,
  type NetworkObserver,
  type Observer,
  type PageErrorKind,
  type RequestEvent,
  type RequestKind,
} from './core/observe'

export interface InstallOptions extends CoreOptions, NavigatorOptions {
  readonly url: string
  readonly storage?: StorageNative
  readonly logNetwork?: boolean
  readonly gpu?: GpuNative
  readonly webgl?: WebglNative
  readonly audio?: AudioNative
  readonly capture?: CaptureNative
  readonly device?: DeviceNative
  readonly screen?: { width: number; height: number }
}

export interface WebInternals {
  readonly window: typeof globalThis
  readonly document: Document
  readonly input: Input
  readonly env: typeof env
  readonly editing: typeof editing
  readonly handles: Handles
  idOf(node: unknown): number
  nodeOf(id: number): unknown
  loadPage(html: string): Promise<void>
  frame(time: number): void
  endTask(): void
  resize(): void
  mediaChanged(): void
  setLanguages(languages: { language?: string; languages?: readonly string[] }): void
  setScreen(size: { width: number; height: number }): void
  setVisible(visible: boolean): void
  setActive(active: boolean): void
  clock(request: ClockRequest): Promise<ClockState> | ClockState
  reportError(error: unknown): void
  unhandledRejection(promise: Promise<unknown>, reason: unknown): boolean
  stats(): { strong: number; weak: number }
}

export interface Handles {
  of(entry: { readonly name: string; readonly path: string }, kind: 'file' | 'directory', mode: Mode): object
  picked(handle: unknown): { readonly name: string; readonly path: string; readonly kind: 'file' | 'directory'; readonly mode: Mode } | null
  transfer(transfer: object, file: object | null, handle: object): void
}

let installed: WebInternals | null = null

function errorText(error: unknown): string {
  const e = error as { stack?: unknown; name?: unknown; message?: unknown } | null
  if (typeof e === 'object' && e !== null) {
    if (typeof e.stack === 'string' && e.stack) return e.stack
    if (typeof e.message === 'string') return `${typeof e.name === 'string' ? e.name : 'Error'}: ${e.message}`
  }
  return describe(error)
}

export function install(global: typeof globalThis, domNative: DomNative, hostNative: HostNative, options: InstallOptions): WebInternals {
  if (installed) throw new Error('acde-dom is installed in this context already')
  installCore(global, hostNative, options)
  bindDom(domNative)
  domNative.setLoader((id, url, destination, cors, served, cached) => {
    if (served) return reportServed(url, destination, cached === true)
    loadResource(url, destination, cors, (bytes, opaque) => {
      domNative.loaded(id, bytes, opaque)
      if (destination === 'style') loads.expect()
    })
  })
  env.url = hostNative.parseUrl(options.url, null)?.href ?? options.url
  objectUrlOrigin.value = new URL(env.url).origin
  net.base = () => env.url
  net.log = options.logNetwork ?? true
  readViewport()
  if (options.screen) Object.assign(screenSize, options.screen)

  const unhandled = (kind: PageErrorKind, error: unknown) => {
    host.reportError(error)
    observer?.error?.(kind, errorText(error))
  }
  let reporting = false
  const reportError = (error: unknown) => {
    if (reporting) return unhandled('error', error)
    reporting = true
    try {
      const message = error instanceof Error ? error.message : String(error)
      if (fire(global as unknown as EventTarget, new ErrorEvent('error', { message, error, cancelable: true }))) unhandled('error', error)
    } finally {
      reporting = false
    }
  }
  setReport(reportError)

  const document = wrap<Document>(documentId)!
  const input = new Input()
  const { local, session } = installStorage(global as unknown as EventTarget, options.storage)
  const registry = new CustomElementRegistry()

  extendElements(input)
  const navigator = new Navigator(options)
  setNavigatorStorage(navigator, new StorageManager(KEY, options.storage))
  installWindow(global, {
    document,
    location: new Location(),
    history: new History(),
    navigator,
    screen: new Screen(),
    visualViewport: new VisualViewport(),
    localStorage: local,
    sessionStorage: session,
    customElements: registry,
    getSelection,
  })
  globals(global, interfaces())
  globals(global, legacyFactories())
  globals(global, { fetch })
  globals(global, installIndexedDB(options.storage?.idb))
  globals(global, { StorageManager })
  globals(global, installCaches(options.storage?.caches))
  if (options.gpu) globals(global, installGpu(global, options.gpu, externalPixels))
  if (options.webgl) globals(global, installWebgl(options.webgl, () => frames.request()))
  if (options.audio) globals(global, installAudio(options.audio))
  installCapture(options.capture)
  installDevice(global, options.device)
  globals(global, { MediaStream, MediaStreamTrack, MediaDeviceInfo, InputDeviceInfo, OverconstrainedError, Permissions, PermissionStatus })
  inspectHooks.node = (value) => {
    if (!(value instanceof Node)) return null
    if (value instanceof Element) {
      const id = inner(value, 'id'), cls = inner(value, 'getAttribute').call(value, 'class')
      return `<${value[LOCAL]}${id ? `#${id}` : ''}${cls ? `.${cls.trim().split(/\s+/).join('.')}` : ''}>`
    }
    return inner(value, 'nodeType') === NodeType.Text ? `#text ${JSON.stringify((value as Text).data.slice(0, 40))}` : inner(value, 'nodeName')
  }

  let frameVersion = -1
  onCssEventsWanted(() => frames.request())
  host.setModuleSource?.(moduleSource)
  inner(document, 'addEventListener').call(document, 'pointerdown', (event: Event) => lightDismiss(event.target as never), true)
  inner(document, 'addEventListener').call(document, 'keydown', (event: Event & { readonly key?: string }) => {
    if (event.key === 'Escape' && !event.defaultPrevented) escape()
  })

  const deliverScrolls = () => {
    const scrolled = dom.takeScrolled()
    if (scrolled.length === 0) return
    moved()
    const root = rootElementId()
    for (const id of scrolled) {
      if (id === root || id === documentId) fire(document, new Event('scroll', { bubbles: true }))
      else {
        let target = existing(id) as Element | undefined
        if (!target && flags.inlineHandlers && dom.hasAttribute(id, 'onscroll')) target = wrap<Element>(id)!
        if (target) fire(target, new Event('scroll'))
      }
    }
  }

  installed = {
    window: global,
    document,
    input,
    env,
    editing,
    handles: {
      of: handedOver,
      picked: pickedOf,
      transfer: (transfer, file, handle) => events.transferEntry(transfer as never, file as never, handle as never),
    },
    idOf,
    nodeOf: (id: number) => wrap(id),
    loadPage,
    frame() {
      frames.wanted = false
      let animating = false
      moved()
      try {
        deliverScrolls()
        if (!gpuContexts.behind()) runAnimationFrames(clockNow(), report)
        deliverObservations()
        animating = deliverCssEvents()
      } catch (error) {
        report(error)
      }
      const loading = deliverLoads()
      const playing = presentVideos()
      webglContexts.present()
      webglContexts.endTask()
      canvases.flush()
      gpuContexts.present()
      frameVersion = version
      dom.commit()
      if (framesPending() || loading || playing || animating) frames.request()
    },
    endTask() {
      idbTaskEnded()
      webglContexts.endTask()
      moved()
      canvases.flush()
      gpuContexts.present()
      if (version !== frameVersion && (watching() || cssEventsWanted())) frames.request()
      dom.commit()
    },
    resize() {
      input.selects.close()
      readViewport()
      moved()
      changed()
      fire(global as unknown as EventTarget, new Event('resize'))
      fire((global as unknown as { visualViewport: EventTarget }).visualViewport, new Event('resize'))
      mediaChanged()
      if (watching()) frames.request()
    },
    mediaChanged,
    setLanguages(languages) {
      if (setNavigatorLanguages(navigator, languages)) fire(global as unknown as EventTarget, new Event('languagechange'))
    },
    setScreen(size) {
      Object.assign(screenSize, size)
    },
    setVisible(visible) {
      if (documentState.hidden === !visible) return
      if (!visible) input.selects.close()
      documentState.hidden = !visible
      fire(document, new Event('visibilitychange', { bubbles: true }))
    },
    setActive(active) {
      if (focus.windowActive === active) return
      if (!active) input.selects.close()
      focus.windowActive = active
      focus.revision++
      fire(global as unknown as EventTarget, new events.FocusEvent(active ? 'focus' : 'blur'))
    },
    clock: (request) => clockHandle(request, adoptTimers),
    reportError,
    unhandledRejection(promise, reason) {
      const event = new PromiseRejectionEvent('unhandledrejection', { promise, reason, cancelable: true })
      const missed = fire(global as unknown as EventTarget, event)
      if (missed) unhandled('rejection', reason)
      return missed
    },
    stats: wrapperStats,
  }
  return installed
}

function externalPixels(source: unknown): { width: number; height: number; rgba: Uint8Array } | null {
  if (source instanceof ImageBitmap) return externalPixels(bitmaps.canvas(source))
  if (source instanceof ImageData) {
    return { width: source.width, height: source.height, rgba: new Uint8Array(source.data.buffer, source.data.byteOffset, source.data.byteLength) }
  }
  if (source instanceof media.HTMLCanvasElement) {
    const webgl = webglContexts.pixels(source)
    if (webgl !== undefined) return webgl
    const gpu = gpuContexts.pixels(source)
    if (gpu !== undefined) return gpu
    const { width, height } = source
    if (!width || !height || !canvases.has(source as never)) return null
    const rgba = canvases.pixels(source as never, 0, 0, width, height)
    return rgba && { width, height, rgba }
  }
  if (source instanceof media.HTMLImageElement) {
    const { naturalWidth: width, naturalHeight: height } = source as unknown as { naturalWidth: number; naturalHeight: number }
    if (!width || !height) return null
    const canvas = createHtml('canvas') as unknown as media.HTMLCanvasElement
    canvas.width = width
    canvas.height = height
    const context = canvas.getContext('2d') as CanvasContext2d
    context.drawImage(source as never, 0, 0)
    const rgba = canvases.pixels(canvas as never, 0, 0, width, height)
    return rgba && { width, height, rgba }
  }
  return null
}

function extendElements(input: Input): void {
  const proto = Element.prototype as unknown as Record<string, unknown>
  Object.defineProperties(proto, {
    animate: {
      value(this: Element, keyframes: unknown, options?: number | Record<string, unknown>) {
        return animate(this, keyframes, options)
      },
      writable: true,
      configurable: true,
    },
    getAnimations: {
      value(this: Element, options?: { subtree?: boolean }) {
        return animationsOf(this, !!options?.subtree)
      },
      writable: true,
      configurable: true,
    },
    attachShadow: {
      value(this: Element, init: { mode?: string; delegatesFocus?: boolean }) {
        return attachShadow(this, init)
      },
      writable: true,
      configurable: true,
    },
    shadowRoot: { get(this: Element) { return shadowRootOf(this) }, enumerable: true, configurable: true },
    setPointerCapture: { value(this: Element, pointerId: number) { input.setPointerCapture(this, pointerId) }, writable: true, configurable: true },
    releasePointerCapture: { value(this: Element, pointerId: number) { input.releasePointerCapture(this, pointerId) }, writable: true, configurable: true },
    hasPointerCapture: { value(this: Element) { return input.capture === this }, writable: true, configurable: true },
  })
}

function legacyFactories(): Record<string, unknown> {
  const factory = (name: string, proto: object, make: (...args: any[]) => Element) => {
    const F = function (...args: unknown[]) {
      if (!new.target) throw new TypeError(`Failed to construct '${name}': Please use the 'new' operator.`)
      return make(...args)
    }
    Object.defineProperty(F, 'name', { value: name })
    F.prototype = proto
    return F
  }
  return {
    Image: factory('Image', media.HTMLImageElement.prototype, (width?: number, height?: number) => {
      const img = createHtml('img')
      if (width !== undefined) img.setAttribute('width', String(width))
      if (height !== undefined) img.setAttribute('height', String(height))
      return img
    }),
    Option: factory('Option', forms.HTMLOptionElement.prototype, (text?: string, value?: string, defaultSelected?: boolean, selected?: boolean) => {
      const option = createHtml('option') as unknown as forms.HTMLOptionElement
      if (text !== undefined) option.textContent = String(text)
      if (value !== undefined) option.setAttribute('value', String(value))
      if (defaultSelected) option.setAttribute('selected', '')
      if (selected) option.selected = true
      return option
    }),
    Audio: factory('Audio', media.HTMLAudioElement.prototype, (src?: string) => {
      const audio = createHtml('audio')
      audio.setAttribute('preload', 'auto')
      if (src !== undefined) audio.setAttribute('src', String(src))
      return audio
    }),
  }
}

function interfaces(): Record<string, unknown> {
  const out: Record<string, unknown> = {
    Window, Node, CharacterData, Text, CDATASection, Comment, ProcessingInstruction, DocumentType, DocumentFragment, ShadowRoot,
    Document, HTMLDocument, XMLDocument, Element, HTMLElement, Attr, NamedNodeMap, NodeList, HTMLCollection, DOMTokenList, DOMStringMap,
    CSSStyleDeclaration, CSSStyleSheet, CSSRule, CSSRuleList, CSSStyleRule, StyleSheetList, CSS,
    MutationObserver, MutationRecord, ResizeObserver, ResizeObserverEntry, IntersectionObserver, IntersectionObserverEntry,
    AbstractRange, Range, StaticRange, Selection, CaretPosition, TreeWalker, NodeIterator, NodeFilter,
    DOMParser, XMLSerializer, DOMImplementation, FontFace, FontFaceSet,
    DOMRect, DOMRectReadOnly, DOMRectList, DOMPoint, DOMPointReadOnly, DOMMatrix, DOMMatrixReadOnly, DOMQuad,
    SVGMatrix: DOMMatrix, SVGPoint: DOMPoint, SVGRect: DOMRect,
    CanvasRenderingContext2D: CanvasContext2d, CanvasGradient, CanvasPattern, Path2D, ImageData, TextMetrics, ImageBitmap,
    Animation, AnimationEffect, KeyframeEffect, AnimationTimeline, DocumentTimeline, AnimationPlaybackEvent,
    CustomElementRegistry, Location, History, Navigator, Screen, ScreenOrientation, VisualViewport, Storage, MediaQueryList, Clipboard, MediaDevices,
    Headers, Request, Response, XMLHttpRequest, XMLHttpRequestEventTarget, XMLHttpRequestUpload, WebSocket, EventSource,
    ReadableStream, ReadableStreamDefaultReader, ReadableStreamDefaultController, WritableStream, WritableStreamDefaultWriter,
    WritableStreamDefaultController, TransformStream, TransformStreamDefaultController, TextDecoderStream, TextEncoderStream,
    CustomEvent, ErrorEvent,
  }
  for (const module of [events, forms, html, media, svg] as Record<string, unknown>[]) {
    for (const [name, value] of Object.entries(module)) {
      if (typeof value === 'function' && /^[A-Z]/.test(name) && value.prototype) out[name] = value
    }
  }
  delete out['HTMLOrSVGElementMixin']
  return out
}
