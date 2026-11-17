declare const ACDEV_RUNTIME_VERSION: string | undefined
const VERSION = typeof ACDEV_RUNTIME_VERSION === 'string' ? ACDEV_RUNTIME_VERSION : '0.0.0'
import { defineHandlers, Event, EventTarget, fire, LISTENERS, windowEvent } from '../core/event'
import { DOMException } from '../core/exception'
import { dialogAnswer } from '../core/observe'
import { G, getters, globals, host, KEY, tag } from '../core/host'
import { capturePermission, MediaDevices, Permissions } from '../media/stream'
import { idlePermission } from '../device/idle'
import { structuredClone, transferList } from '../core/message'
import { objectUrls, URL } from '../core/url'
import { Blob } from '../core/blob'
import { NONE, NodeType } from '../native'
import type { Element } from './element'
import { scrollArgs, scrollElement, type ScrollToOptions } from './element'
import { env, frames, resolveUrl } from './env'
import type { Storage } from '../storage/webstorage'
import { HashChangeEvent, MediaQueryListEvent, PopStateEvent } from './events'
import { HANDLER_TYPES } from './element'
import { createImageBitmap } from './bitmap'
import { rootScroll } from './layout'
import { cssString } from './node'
import { FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemHandle, FileSystemWritableFileStream, showDirectoryPicker, showOpenFilePicker, showSaveFilePicker } from './picker'
import { documentId, dom, rootElementId, version, wrap } from './realm'
import { urlParts } from './reflect'
import { computedStyle, type CSSStyleDeclaration } from './style'

export const viewport = { width: 0, height: 0, scale: 1 }
export const screenSize = { width: 0, height: 0 }
const screenWidth = (): number => screenSize.width || viewport.width
const screenHeight = (): number => screenSize.height || viewport.height

export function readViewport(): void {
  const v = dom.viewport()
  viewport.width = v.width
  viewport.height = v.height
  viewport.scale = v.scale
}

export class Window extends EventTarget {}
tag(Window)

const WINDOW_HANDLERS = ['afterprint', 'beforeprint', 'beforeunload', 'hashchange', 'languagechange', 'message', 'messageerror', 'offline', 'online', 'pagehide', 'pageshow', 'popstate', 'rejectionhandled', 'storage', 'unhandledrejection', 'unload', 'devicemotion', 'deviceorientation']

function sameDocument(a: URL, b: URL): boolean {
  return a.origin === b.origin && a.pathname === b.pathname && a.search === b.search && a.username === b.username && a.password === b.password
}

interface Entry {
  url: string
  state: unknown
}

const entries: Entry[] = []
let index = 0
let parsedFrom = ''
let parsed: URL | null = null

function here(): URL {
  if (parsedFrom !== env.url || !parsed) {
    parsed = new URL(env.url)
    parsedFrom = env.url
  }
  return parsed
}

function navigate(url: string, replace: boolean): void {
  const next = new URL(url, env.url)
  const now = here()
  if (saves(next.href)) return saveUrl(next.href, '')
  if (!sameDocument(next, now) || (next.hash === now.hash && next.href !== now.href)) {
    env.navigate(next.href, '_self')
    return
  }
  if (next.href === now.href) return void (next.hash && scrollToFragment(next.hash))
  const oldURL = env.url
  env.url = next.href
  if (replace) entries[index] = { url: env.url, state: null }
  else {
    entries.splice(index + 1)
    entries.push({ url: env.url, state: null })
    index = entries.length - 1
  }
  if (next.hash !== now.hash) {
    scrollToFragment(next.hash)
    host.setTimer(0, () => {
      fire(G as unknown as EventTarget, new HashChangeEvent('hashchange', { oldURL, newURL: next.href }))
    })
  }
}

function saves(url: string): boolean {
  return url.startsWith('blob:') || url.startsWith('data:')
}

function saveUrl(url: string, name: string): void {
  if (!env.download) return void host.print(1, `acde-dom: not saved (no downloads here): ${url.slice(0, 120)}`)
  const blob = url.startsWith('blob:') ? objectUrls.get(url) : undefined
  env.download({ url, name, ...(blob instanceof Blob ? { blob } : {}) })
}

export function followLink(url: string, target: string, download: string | null): void {
  let next: URL
  try {
    next = new URL(url, env.url)
  } catch {
    return
  }
  if (download !== null && (saves(next.href) || next.origin === here().origin)) return saveUrl(next.href, download)
  const where = target.trim().toLowerCase()
  if (where && where !== '_self' && where !== '_parent' && where !== '_top') {
    if (saves(next.href)) return saveUrl(next.href, '')
    return env.navigate(next.href, target)
  }
  navigate(next.href, false)
}

function scrollToFragment(hash: string): void {
  let fragment = hash.replace(/^#/, '')
  try {
    fragment = decodeURIComponent(fragment)
  } catch {
  }
  let id = fragment ? dom.getElementById(fragment) : NONE
  if (id === NONE && fragment) id = dom.querySelectorAll(documentId, 'a[name]').find((a) => dom.getAttribute(a, 'name') === fragment) ?? NONE
  if (id !== NONE) return (wrap<Element>(id) as unknown as { scrollIntoView(options: object): void }).scrollIntoView({ block: 'start' })
  if (!fragment || fragment.toLowerCase() === 'top') scrollViewport(0, 0, false)
}

export class Location {
  get href(): string {
    return env.url
  }
  set href(value: string) {
    navigate(String(value), false)
  }
  assign(url: string): void {
    navigate(String(url), false)
  }
  replace(url: string): void {
    navigate(String(url), true)
  }
  reload(): void {
    env.reload()
  }
  toString(): string {
    return env.url
  }
  valueOf(): this {
    return this
  }
}
urlParts(Location.prototype, ['origin', 'protocol', 'host', 'hostname', 'port', 'pathname', 'search', 'hash'], here, (_, u) => navigate(u.href, false))
getters(Location.prototype, { ancestorOrigins: () => ({ length: 0 }) })
tag(Location)

export class History {
  scrollRestoration = 'auto'

  get length(): number {
    return entries.length
  }
  get state(): unknown {
    return entries[index]?.state ?? null
  }

  #url(url: string | URL | null | undefined): string {
    if (url === null || url === undefined) return env.url
    const next = new URL(String(url), env.url)
    if (next.origin !== here().origin) throw new DOMException(`Failed to execute 'pushState' on 'History': A history state object with URL '${next.href}' cannot be created in a document with origin '${here().origin}'.`, 'SecurityError')
    return next.href
  }

  pushState(state: unknown, _unused: string, url?: string | URL | null): void {
    const href = this.#url(url)
    entries.splice(index + 1)
    entries.push({ url: href, state: structuredClone(state) })
    index = entries.length - 1
    env.url = href
  }

  replaceState(state: unknown, _unused: string, url?: string | URL | null): void {
    const href = this.#url(url)
    entries[index] = { url: href, state: structuredClone(state) }
    env.url = href
  }

  go(delta = 0): void {
    if (delta === 0) return env.reload()
    const target = index + Math.trunc(delta)
    if (target < 0 || target >= entries.length) return
    host.setTimer(0, () => {
      const oldURL = env.url
      index = target
      const entry = entries[index]!
      env.url = entry.url
      fire(G as unknown as EventTarget, new PopStateEvent('popstate', { state: entry.state }))
      if (new URL(oldURL).hash !== new URL(entry.url).hash) {
        if (entry.state === null) scrollToFragment(new URL(entry.url).hash)
        fire(G as unknown as EventTarget, new HashChangeEvent('hashchange', { oldURL, newURL: entry.url }))
      }
    })
  }

  back(): void {
    this.go(-1)
  }

  forward(): void {
    this.go(1)
  }
}
tag(History)

const watched = new Set<MediaQueryList>()
let recheckList: (list: MediaQueryList) => void

export class MediaQueryList extends EventTarget {
  readonly #media: string
  #last: boolean | null = null

  constructor(media: string) {
    super()
    this.#media = media
  }

  get media(): string {
    return this.#media
  }

  get matches(): boolean {
    const now = dom.matchMedia(this.#media)
    this.#last = now
    return now
  }

  override addEventListener(type: string, callback: unknown, options?: unknown): void {
    if (type === 'change') {
      if (this.#last === null) this.#last = dom.matchMedia(this.#media)
      watched.add(this)
    }
    super.addEventListener(type, callback, options)
  }

  override removeEventListener(type: string, callback: unknown, options?: unknown): void {
    super.removeEventListener(type, callback, options)
    if (!this[LISTENERS]?.has('change')) watched.delete(this)
  }

  addListener(callback: unknown): void {
    this.addEventListener('change', callback)
  }

  removeListener(callback: unknown): void {
    this.removeEventListener('change', callback)
  }

  get onchange(): unknown {
    return this.#onchange
  }
  set onchange(value: unknown) {
    if (this.#onchange) this.removeEventListener('change', this.#onchange)
    this.#onchange = typeof value === 'function' ? value : null
    if (this.#onchange) this.addEventListener('change', this.#onchange)
  }
  #onchange: unknown = null

  static {
    recheckList = (list) => {
      if (!list[LISTENERS]?.has('change')) return void watched.delete(list)
      const now = dom.matchMedia(list.#media)
      if (now === list.#last) return
      list.#last = now
      fire(list, new MediaQueryListEvent('change', { media: list.#media, matches: now }))
    }
  }
}
tag(MediaQueryList)

export function mediaChanged(): void {
  for (const list of [...watched]) recheckList(list)
}

let nextFrame = 1
let pending = new Map<number, (time: number) => void>()

export function requestAnimationFrame(callback: (time: number) => void): number {
  if (typeof callback !== 'function') throw new TypeError("Failed to execute 'requestAnimationFrame' on 'Window': The callback provided as parameter 1 is not a function.")
  const id = nextFrame++
  pending.set(id, callback)
  frames.request()
  return id
}

export function cancelAnimationFrame(id: number): void {
  pending.delete(id)
}

export function runAnimationFrames(time: number, report: (error: unknown) => void): void {
  if (pending.size === 0) return
  const batch = pending
  pending = new Map()
  for (const callback of batch.values()) {
    try {
      callback(time)
    } catch (error) {
      report(error)
    }
  }
}

export function framesPending(): boolean {
  return pending.size > 0
}

export class Clipboard extends EventTarget {
  readText(): Promise<string> {
    return env.clipboard ? env.clipboard.readText() : Promise.reject(new DOMException('Read permission denied.', 'NotAllowedError'))
  }
  writeText(text: string): Promise<void> {
    return env.clipboard ? env.clipboard.writeText(String(text)) : Promise.reject(new DOMException('Write permission denied.', 'NotAllowedError'))
  }
  read(): Promise<unknown[]> {
    return Promise.reject(new DOMException('Clipboard items are not supported', 'NotAllowedError'))
  }
  write(): Promise<void> {
    return Promise.reject(new DOMException('Clipboard items are not supported', 'NotAllowedError'))
  }
}
tag(Clipboard)

export class Notification extends EventTarget {
  static get permission(): string {
    return env.notify ? 'granted' : 'denied'
  }
  static requestPermission(callback?: (permission: string) => void): Promise<string> {
    const permission = env.notify ? 'granted' : 'denied'
    if (typeof callback === 'function') host.setTimer(0, () => callback(permission))
    return Promise.resolve(permission)
  }

  readonly title: string
  readonly body: string
  readonly tag: string
  readonly icon: string
  readonly data: unknown

  constructor(title: string, options: { body?: string; tag?: string; icon?: string; data?: unknown } = {}) {
    super()
    if (arguments.length === 0) throw new TypeError("Failed to construct 'Notification': 1 argument required, but only 0 present.")
    this.title = String(title)
    this.body = String(options?.body ?? '')
    this.tag = String(options?.tag ?? '')
    this.icon = options?.icon ? resolveUrl(String(options.icon)) : ''
    this.data = options?.data ?? null
    const notify = env.notify
    if (notify) notify({ title: this.title, body: this.body, icon: this.icon, tag: this.tag })
    host.setTimer(0, () => fire(this, new Event(notify ? 'show' : 'error')))
  }

  close(): void {}
}
defineHandlers(Notification.prototype, ['click', 'close', 'error', 'show'])
getters(Notification, { maxActions: 0 })
tag(Notification)

export interface NavigatorOptions {
  readonly language?: string
  readonly languages?: readonly string[]
  readonly hardwareConcurrency?: number
  readonly userAgent?: string
  readonly platform?: string
}

const navigatorLanguages = new WeakMap<Navigator, { language?: string; languages?: readonly string[] }>()
const navigatorStorage = new WeakMap<Navigator, object>()

export function setNavigatorStorage(navigator: Navigator, storage: object): void {
  navigatorStorage.set(navigator, storage)
}

export function setNavigatorLanguages(navigator: Navigator, languages: { language?: string; languages?: readonly string[] }): boolean {
  const before = navigatorLanguages.get(navigator) ?? {}
  if (before.language === languages.language && (before.languages ?? []).join() === (languages.languages ?? []).join()) return false
  navigatorLanguages.set(navigator, { language: languages.language, languages: languages.languages ? [...languages.languages] : undefined })
  return true
}

function permissionState(name: string): string {
  switch (name) {
    case 'camera':
    case 'microphone':
      return capturePermission(name)
    case 'idle-detection':
      return idlePermission()
    case 'screen-wake-lock':
      return 'wakeLock' in Navigator.prototype ? 'granted' : 'denied'
    case 'notifications':
      return env.notify ? 'granted' : 'denied'
    case 'persistent-storage':
      return 'granted'
    case 'clipboard-read':
    case 'clipboard-write':
      return env.clipboard ? 'granted' : 'denied'
    default:
      return 'denied'
  }
}

export class Navigator {
  readonly #options: NavigatorOptions
  readonly #clipboard = new Clipboard()
  readonly #media = new MediaDevices()
  readonly #permissions = new Permissions(KEY, (name) => permissionState(name))

  constructor(options: NavigatorOptions) {
    this.#options = options
    navigatorLanguages.set(this, { language: options.language, languages: options.languages })
  }

  get userAgent(): string {
    return this.#options.userAgent ?? `Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) acdert/${VERSION}`
  }
  get platform(): string {
    return this.#options.platform ?? 'Linux x86_64'
  }
  get appVersion(): string {
    return this.userAgent.replace(/^Mozilla\//, '')
  }
  get language(): string {
    return navigatorLanguages.get(this)?.language ?? this.languages[0] ?? 'en-US'
  }
  get languages(): readonly string[] {
    return Object.freeze([...(navigatorLanguages.get(this)?.languages ?? ['en-US', 'en'])])
  }
  get hardwareConcurrency(): number {
    return this.#options.hardwareConcurrency ?? 4
  }
  get clipboard(): Clipboard {
    return this.#clipboard
  }
  get storage(): object | undefined {
    return navigatorStorage.get(this)
  }
  get mediaDevices(): MediaDevices {
    return this.#media
  }
  get permissions(): Permissions {
    return this.#permissions
  }
  javaEnabled(): boolean {
    return false
  }
  sendBeacon(): boolean {
    return false
  }
  vibrate(): boolean {
    return false
  }
  share(): Promise<never> {
    return Promise.reject(new DOMException('Share is not supported', 'NotAllowedError'))
  }
  canShare(): boolean {
    return false
  }
  getGamepads(): unknown[] {
    return []
  }
}
getters(Navigator.prototype, {
  appName: 'Netscape', appCodeName: 'Mozilla', product: 'Gecko', productSub: '20030107', vendor: '', vendorSub: '',
  onLine: true, cookieEnabled: true, deviceMemory: 8, maxTouchPoints: 0, pdfViewerEnabled: false, webdriver: false, doNotTrack: null,
  userActivation: () => ({ hasBeenActive: true, isActive: false }),
  mimeTypes: () => ({ length: 0 }),
  plugins: () => ({ length: 0 }),
})
tag(Navigator)

export class ScreenOrientation extends EventTarget {
  get type(): string {
    return screenWidth() >= screenHeight() ? 'landscape-primary' : 'portrait-primary'
  }
  lock(): Promise<never> {
    return Promise.reject(new DOMException('Orientation lock is not supported', 'NotSupportedError'))
  }
  unlock(): void {}
}
getters(ScreenOrientation.prototype, { angle: 0 })
tag(ScreenOrientation)

export class Screen extends EventTarget {
  readonly orientation = new ScreenOrientation()
}
getters(Screen.prototype, { width: screenWidth, height: screenHeight, availWidth: screenWidth, availHeight: screenHeight, availLeft: 0, availTop: 0, colorDepth: 24, pixelDepth: 24, isExtended: false })
tag(Screen)

export class VisualViewport extends EventTarget {}
getters(VisualViewport.prototype, { width: () => viewport.width, height: () => viewport.height, offsetLeft: 0, offsetTop: 0, pageLeft: () => rootScroll(0), pageTop: () => rootScroll(1), scale: 1 })
defineHandlers(VisualViewport.prototype, ['resize', 'scroll', 'scrollend'])
tag(VisualViewport)

function scrollViewport(x: number | null, y: number | null, smooth: boolean): void {
  const root = wrap<Element>(rootElementId())
  if (root) scrollElement(root, x, y, smooth)
}

function scrollWindow(x?: number | ScrollToOptions, y?: number): void {
  scrollViewport(...scrollArgs(x, y))
}

interface WindowParts {
  readonly document: object
  readonly location: Location
  readonly history: History
  readonly navigator: Navigator
  readonly screen: Screen
  readonly visualViewport: VisualViewport
  readonly localStorage: Storage
  readonly sessionStorage: Storage
  readonly customElements: object
  readonly getSelection: () => unknown
}

interface NamedSource {
  getElementById(id: string): Element | null
  querySelector(selectors: string): Element | null
}

const NAMED_BY_NAME = ['embed', 'form', 'img', 'object']
const NAMED_SELECTOR = ['[id]', ...NAMED_BY_NAME.map((tag) => `${tag}[name]`)].join(',')

function namedProperties(document: NamedSource): object {
  let names = new Set<string>()
  let seen = -1
  const known = (key: string): boolean => {
    if (seen !== version) {
      seen = version
      names = new Set()
      for (const id of dom.querySelectorAll(documentId, NAMED_SELECTOR)) {
        const own = dom.getAttribute(id, 'id')
        if (own) names.add(own)
        const name = NAMED_BY_NAME.includes(dom.localName(id)) ? dom.getAttribute(id, 'name') : null
        if (name) names.add(name)
      }
    }
    return names.has(key)
  }
  const named = (key: string | symbol): Element | null => {
    if (typeof key !== 'string' || !key || !known(key)) return null
    const byId = document.getElementById(key)
    if (byId) return byId
    const quoted = cssString(key)
    return document.querySelector(NAMED_BY_NAME.map((tag) => `${tag}[name=${quoted}]`).join(','))
  }
  const target = Object.create(EventTarget.prototype, { [Symbol.toStringTag]: { value: 'WindowProperties', configurable: true } }) as object
  return new Proxy(target, {
    get: (object, key, receiver) => (key in object ? Reflect.get(object, key, receiver) : (named(key) ?? undefined)),
    has: (object, key) => key in object || named(key) !== null,
    getOwnPropertyDescriptor: (object, key) => {
      const own = Reflect.getOwnPropertyDescriptor(object, key)
      if (own || key in object) return own
      const element = named(key)
      return element ? { value: element, writable: true, enumerable: false, configurable: true } : undefined
    },
  })
}

export function installWindow(global: typeof globalThis, parts: WindowParts): void {
  Object.setPrototypeOf(Window.prototype, namedProperties(parts.document as NamedSource))
  Object.setPrototypeOf(global, Window.prototype)
  const self = () => global
  const getters: Record<string, () => unknown> = {
    window: self,
    self,
    frames: self,
    parent: self,
    top: self,
    document: () => parts.document,
    location: () => parts.location,
    history: () => parts.history,
    navigator: () => parts.navigator,
    clientInformation: () => parts.navigator,
    screen: () => parts.screen,
    visualViewport: () => parts.visualViewport,
    localStorage: () => parts.localStorage,
    sessionStorage: () => parts.sessionStorage,
    customElements: () => parts.customElements,
    event: windowEvent,
    opener: () => null,
    frameElement: () => null,
    closed: () => false,
    length: () => 0,
    origin: () => here().origin,
    isSecureContext: () => true,
    crossOriginIsolated: () => false,
    devicePixelRatio: () => viewport.scale,
    innerWidth: () => viewport.width,
    innerHeight: () => viewport.height,
    outerWidth: () => viewport.width,
    outerHeight: () => viewport.height,
    screenX: () => 0,
    screenY: () => 0,
    screenLeft: () => 0,
    screenTop: () => 0,
    scrollX: () => rootScroll(0),
    scrollY: () => rootScroll(1),
    pageXOffset: () => rootScroll(0),
    pageYOffset: () => rootScroll(1),
  }
  for (const [name, get] of Object.entries(getters)) {
    Object.defineProperty(global, name, { get, set: name === 'location' ? (v: string) => parts.location.assign(v) : undefined, enumerable: true, configurable: true })
  }
  let name = ''
  Object.defineProperty(global, 'name', { get: () => name, set: (v: string) => void (name = String(v)), enumerable: true, configurable: true })
  let status = ''
  Object.defineProperty(global, 'status', { get: () => status, set: (v: string) => void (status = String(v)), enumerable: true, configurable: true })
  const bar = { visible: true }
  for (const key of ['menubar', 'toolbar', 'statusbar', 'locationbar', 'personalbar', 'scrollbars']) Object.defineProperty(global, key, { value: bar, enumerable: true, configurable: true })
  globals(global, {
    getComputedStyle: (element: Element, pseudo?: string | null): CSSStyleDeclaration => {
      if (!element || (element as unknown as { nodeType?: number }).nodeType !== NodeType.Element) throw new TypeError("Failed to execute 'getComputedStyle' on 'Window': parameter 1 is not of type 'Element'.")
      const p = pseudo ? String(pseudo) : ''
      return computedStyle(element, p === '::before' || p === ':before' ? '::before' : p === '::after' || p === ':after' ? '::after' : '')
    },
    matchMedia: (query: string) => new MediaQueryList(String(query)),
    requestAnimationFrame,
    cancelAnimationFrame,
    getSelection: parts.getSelection,
    Notification,
    showOpenFilePicker,
    showSaveFilePicker,
    showDirectoryPicker,
    FileSystemHandle,
    FileSystemFileHandle,
    FileSystemDirectoryHandle,
    FileSystemWritableFileStream,
    scroll: scrollWindow,
    scrollTo: scrollWindow,
    scrollBy: (x?: number | ScrollToOptions, y?: number) => {
      const [dx, dy, smooth] = scrollArgs(x, y, true)
      scrollViewport(rootScroll(0) + dx!, rootScroll(1) + dy!, smooth)
    },
    postMessage: (message: unknown, origin?: unknown, transfer?: unknown) => {
      const data = structuredClone(message, { transfer: transferList(typeof origin === 'object' && origin !== null ? origin : transfer) })
      host.setTimer(0, () => {
        const MessageEvent = G['MessageEvent'] as new (type: string, init: object) => Event
        fire(global as unknown as EventTarget, new MessageEvent('message', { data, origin: here().origin, source: global }))
      })
    },
    alert: (message: unknown = '') => {
      const text = String(message)
      if (dialogAnswer('alert', text, null)) return
      if (env.alert) env.alert(text)
      else host.print(0, `alert: ${text}`)
    },
    confirm: (message: unknown = '') => {
      const text = String(message)
      const answer = dialogAnswer('confirm', text, null)
      if (answer) return answer.accept
      host.print(0, `confirm: ${text} (answered no)`)
      return false
    },
    prompt: (message: unknown = '', defaultValue: unknown = '') => {
      const text = String(message), value = String(defaultValue)
      const answer = dialogAnswer('prompt', text, value)
      if (answer) return answer.accept ? (answer.text ?? value) : null
      host.print(0, `prompt: ${text} (answered nothing)`)
      return null
    },
    print: () => {},
    stop: () => {},
    focus: () => {},
    blur: () => {},
    moveTo: () => {},
    moveBy: () => {},
    resizeTo: () => {},
    resizeBy: () => {},
    open: (url?: string, target?: string) => {
      if (!url) return null
      const href = new URL(String(url), env.url).href
      if (saves(href)) saveUrl(href, '')
      else env.navigate(href, target ? String(target) : '_blank')
      return null
    },
    close: () => env.close(),
    captureEvents: () => {},
    releaseEvents: () => {},
    createImageBitmap,
  })
  defineHandlers(global, [...HANDLER_TYPES, ...WINDOW_HANDLERS])
  entries.push({ url: env.url, state: null })
  index = 0
}
