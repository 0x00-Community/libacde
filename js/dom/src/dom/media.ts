import { DOMException } from '../core/exception'
import { Event, fire } from '../core/event'
import { constants, getters, host, tag } from '../core/host'
import { Blob } from '../core/blob'
import { unbase64 } from '../core/encoding'
import { fetch } from '../core/net'
import { audioNative, mediaEvents } from '../audio/index'
import { NodeType, type AudioNative } from '../native'
import { MediaStream, trackSource, watchTrack, type MediaStreamTrack } from '../media/stream'
import { contexts as gpuContexts, type GPUCanvasContext } from '../gpu/index'
import { webglContexts, type WebGLContextBase } from '../webgl/index'
import { base64 } from '../core/encoding'
import { canvases, type CanvasContext2d, type CanvasElement } from './canvas'
import { ATTRIBUTE_CHANGED, flags, parsedHooks, setAttribute, tokenList, type DOMTokenList, type Element } from './element'
import { clonedHooks, type Node } from './node'
import { frames, loads, resolveUrl } from './env'
import { deliverSheet, HTMLElement, register } from './html'
import { metricGetters, metricsOf } from './layout'
import { bool, long, nullable, string, url } from './reflect'
import { dom, existing, ID, lifecycle, wrap, inner } from './realm'

const loading = new Map<number, Element | undefined>()

const enum ImageState {
  Loading = 0,
  Loaded = 1,
  Failed = 2,
  NoSource = 3,
}

export class HTMLImageElement extends HTMLElement {
  #state(): Float64Array {
    return dom.imageState(this[ID])
  }

  get naturalWidth(): number {
    return this.#state()[0] ?? 0
  }

  get naturalHeight(): number {
    return this.#state()[1] ?? 0
  }

  get complete(): boolean {
    if (!dom.getAttribute(this[ID], 'src') && !dom.getAttribute(this[ID], 'srcset')) return true
    return this.#state()[2] !== ImageState.Loading
  }

  get currentSrc(): string {
    const src = dom.getAttribute(this[ID], 'src')
    return src ? resolveUrl(src) : ''
  }

  get width(): number {
    const m = metricsOf(this[ID])
    if (m) return m.offsetWidth
    const attr = parseInt(dom.getAttribute(this[ID], 'width') ?? '', 10)
    return Number.isNaN(attr) ? this.naturalWidth : attr
  }
  set width(value: number) {
    setAttribute(this, 'width', String(Math.trunc(Number(value)) || 0))
  }

  get height(): number {
    const m = metricsOf(this[ID])
    if (m) return m.offsetHeight
    const attr = parseInt(dom.getAttribute(this[ID], 'height') ?? '', 10)
    return Number.isNaN(attr) ? this.naturalHeight : attr
  }
  set height(value: number) {
    setAttribute(this, 'height', String(Math.trunc(Number(value)) || 0))
  }

  decode(): Promise<void> {
    return new Promise((resolve, reject) => {
      const settle = () => {
        const state = this.#state()[2]
        if (state === ImageState.Loaded) resolve()
        else reject(new DOMException('The source image cannot be decoded.', 'EncodingError'))
      }
      if (this.complete && dom.getAttribute(this[ID], 'src')) return settle()
      if (!dom.getAttribute(this[ID], 'src')) return reject(new DOMException('The source image cannot be decoded.', 'EncodingError'))
      const done = () => {
        this.removeEventListener('load', done)
        this.removeEventListener('error', done)
        settle()
      }
      this.addEventListener('load', done)
      this.addEventListener('error', done)
    })
  }

  override [ATTRIBUTE_CHANGED](name: string, value: string | null): void {
    super[ATTRIBUTE_CHANGED](name, value)
    if ((name === 'src' || name === 'srcset') && value) {
      loading.set(this[ID], this)
      frames.request()
    }
  }
}
url(HTMLImageElement.prototype, 'src')
for (const name of ['alt', 'srcset', 'sizes', 'useMap', 'decoding', 'loading', 'fetchPriority', 'name', 'align', 'border', 'longDesc', 'lowsrc']) string(HTMLImageElement.prototype, name, name.toLowerCase())
nullable(HTMLImageElement.prototype, 'crossOrigin', 'crossorigin')
string(HTMLImageElement.prototype, 'referrerPolicy', 'referrerpolicy')
bool(HTMLImageElement.prototype, 'isMap', 'ismap')
long(HTMLImageElement.prototype, 'hspace', 0)
long(HTMLImageElement.prototype, 'vspace', 0)
metricGetters(HTMLImageElement.prototype, { x: 'offsetLeft', y: 'offsetTop' })
register(HTMLImageElement, 'img')

export function deliverLoads(): boolean {
  const done = dom.takeLoaded()
  for (let i = 0; i < done.length; i++) {
    const id = done[i]!
    if (!loading.has(id) && dom.localName(id) === 'link') {
      deliverSheet(id)
      continue
    }
    const held = loading.get(id) ?? (existing(id) as Element | undefined)
    const state = dom.imageState(id)[2]
    if (state === ImageState.Loading) {
      loading.set(id, held)
      continue
    }
    loading.delete(id)
    if (state === ImageState.NoSource) continue
    let target = held
    if (!target && flags.inlineHandlers && (dom.hasAttribute(id, 'onload') || dom.hasAttribute(id, 'onerror'))) target = wrap<Element>(id)!
    if (!target) continue
    fire(target, new Event(state === ImageState.Loaded ? 'load' : 'error'))
  }
  return loading.size > 0
}
loads.deliver = () => {
  if (deliverLoads()) frames.request()
}

export class HTMLCanvasElement extends HTMLElement {
  get width(): number {
    const n = parseInt(dom.getAttribute(this[ID], 'width') ?? '', 10)
    return Number.isNaN(n) || n < 0 ? 300 : n
  }
  set width(value: number) {
    setAttribute(this, 'width', String(Math.max(0, Math.trunc(Number(value)) || 0)))
  }

  get height(): number {
    const n = parseInt(dom.getAttribute(this[ID], 'height') ?? '', 10)
    return Number.isNaN(n) || n < 0 ? 150 : n
  }
  set height(value: number) {
    setAttribute(this, 'height', String(Math.max(0, Math.trunc(Number(value)) || 0)))
  }

  override [ATTRIBUTE_CHANGED](name: string, value: string | null): void {
    super[ATTRIBUTE_CHANGED](name, value)
    if (name === 'width' || name === 'height') {
      canvases.resized(this as unknown as CanvasElement)
      webglContexts.resized(this)
    }
  }

  getContext(type: string, options?: unknown): CanvasContext2d | GPUCanvasContext | WebGLContextBase | null {
    const kind = String(type)
    const webgl = webglContexts.has(this)
    if (kind === '2d') return gpuContexts.has(this) || webgl ? null : canvases.context(this as unknown as CanvasElement)
    if (kind === 'webgpu') return canvases.has(this as unknown as CanvasElement) || webgl ? null : gpuContexts.of(this, this[ID])
    if (kind === 'webgl' || kind === 'experimental-webgl' || kind === 'webgl2') {
      if (canvases.has(this as unknown as CanvasElement) || gpuContexts.has(this)) return null
      return webglContexts.of(this, this[ID], kind === 'webgl2' ? 'webgl2' : 'webgl', options)
    }
    return null
  }

  toDataURL(type = 'image/png', _quality?: number): string {
    const webgl = webglContexts.png(this)
    if (webgl !== undefined) return webgl ? `data:image/png;base64,${base64(webgl)}` : 'data:,'
    return gpuContexts.dataURL(this) ?? canvases.toDataURL(this as unknown as CanvasElement, String(type))
  }

  toBlob(callback: (blob: Blob | null) => void, type = 'image/png', quality?: number): void {
    const webgl = webglContexts.png(this)
    if (webgl !== undefined) {
      host.setTimer(0, () => callback(webgl ? new Blob([webgl], { type: 'image/png' }) : null))
      return
    }
    const png = gpuContexts.png(this)
    if (png) {
      png.then((bytes) => callback(new Blob([bytes], { type: 'image/png' })), () => callback(null))
      return
    }
    const url = this.toDataURL(type, quality)
    const comma = url.indexOf(',')
    const mime = /^data:([^;,]*)/.exec(url)?.[1] || 'image/png'
    const bytes = comma < 0 ? null : unbase64(url.slice(comma + 1))
    host.setTimer(0, () => callback(bytes ? new Blob([bytes], { type: mime }) : null))
  }

  transferControlToOffscreen(): never {
    throw new DOMException('OffscreenCanvas is not supported', 'NotSupportedError')
  }

  captureStream(): never {
    throw new DOMException('captureStream is not supported', 'NotSupportedError')
  }
}
register(HTMLCanvasElement, 'canvas')

const SANDBOX = Symbol('sandbox')

export class HTMLIFrameElement extends HTMLElement {
  get sandbox(): DOMTokenList {
    return tokenList(this, SANDBOX, 'sandbox', ['allow-downloads', 'allow-forms', 'allow-modals', 'allow-orientation-lock', 'allow-pointer-lock', 'allow-popups', 'allow-popups-to-escape-sandbox', 'allow-presentation', 'allow-same-origin', 'allow-scripts', 'allow-top-navigation', 'allow-top-navigation-by-user-activation'])
  }
  getSVGDocument(): null {
    return null
  }
}
getters(HTMLIFrameElement.prototype, { contentWindow: null, contentDocument: null })
string(HTMLIFrameElement.prototype, 'src')
for (const name of ['srcdoc', 'name', 'allow', 'width', 'height', 'loading', 'referrerPolicy', 'align', 'scrolling', 'frameBorder']) string(HTMLIFrameElement.prototype, name, name.toLowerCase())
bool(HTMLIFrameElement.prototype, 'allowFullscreen', 'allowfullscreen')
register(HTMLIFrameElement, 'iframe')

export class MediaError {
  constructor(readonly code: number, readonly message: string) {}
}
constants(MediaError, { MEDIA_ERR_ABORTED: 1, MEDIA_ERR_NETWORK: 2, MEDIA_ERR_DECODE: 3, MEDIA_ERR_SRC_NOT_SUPPORTED: 4 })
tag(MediaError)

const playingMedia = new Map<number, HTMLMediaElement>()
const closedMedia = new FinalizationRegistry<number>((id) => audioNative()?.closeMedia(id))
const PLAYABLE = /^audio\/(wav|wave|x-wav|vnd\.wave|mpeg|mp3|x-mp3|mpeg3|ogg|vorbis|x-vorbis|flac|x-flac|aac|aacp|x-aac|mp4|m4a|x-m4a)$/i
let nextMedia = 1 << 24
const PLAYS = Symbol('plays')
const ENDED = Symbol('ended')
const OPEN = Symbol('open')
const RUNNING = Symbol('running')
const SEEKED = Symbol('seeked')
const RESET = Symbol('reset')
const CREATED = Symbol('created')
const OPENED = Symbol('opened')
const SHOWS = Symbol('shows')

type Opened = { duration: number; width?: number; height?: number }

const timeRanges = (end: number) => ({ length: end > 0 ? 1 : 0, start: () => 0, end: () => end })

export class HTMLMediaElement extends HTMLElement {
  declare loop: boolean
  declare autoplay: boolean
  defaultPlaybackRate = 1
  preservesPitch = true
  #media = 0
  #opening = 0
  #suspended = false
  #object: MediaStream | null = null
  #streamSources: [number, number] = [0, 0]
  #unwatch: (() => void)[] = []
  #audioEnabled = true
  #generation = 0
  #loading = false
  #ready = 0
  #network = 0
  #error: MediaError | null = null
  #paused = true
  #ended = false
  #duration = NaN
  #position = 0
  #volume = 1
  #muted: boolean | null = null
  #rate = 1
  #pending: { resolve: () => void; reject: (e: unknown) => void }[] = []
  #timer = 0

  #emit(type: string): void {
    host.setTimer(0, () => fire(this, new Event(type)))
  }

  [PLAYS](): boolean {
    return true
  }

  [OPEN](native: AudioNative, media: number, bytes: Uint8Array): Promise<Opened> {
    return native.openMedia(media, bytes)
  }

  [OPENED](_media: number, _info: Opened, _fresh: boolean): void {}

  [SHOWS](): boolean {
    return false
  }

  get srcObject(): MediaStream | null {
    return this.#object
  }
  set srcObject(value: MediaStream | null) {
    if (value !== null && !(value instanceof MediaStream)) throw new TypeError("Failed to set the 'srcObject' property on 'HTMLMediaElement': The provided value is not of type 'MediaStream'.")
    this.#object = value
    this.#load()
  }

  [RUNNING](_media: number, _running: boolean): void {}

  [SEEKED](_media: number): void {}

  [RESET](): void {}

  [CREATED](): void {
    if (this.#network === 0 && !this.#loading && !this.#media && this.#source()) this.#load()
  }

  #source(): string {
    const src = dom.getAttribute(this[ID], 'src')
    if (src) return resolveUrl(src)
    const url = this.querySelector('source[src]')?.getAttribute('src')
    return url ? resolveUrl(url) : ''
  }

  #settle(error: unknown | null): void {
    const pending = this.#pending
    this.#pending = []
    for (const p of pending) error ? p.reject(error) : p.resolve()
  }

  #gain(): number {
    return this.muted || !this.#audioEnabled ? 0 : this.#volume
  }

  #load(wanted = false): void {
    const native = audioNative()
    const generation = ++this.#generation
    this.#suspended = false
    if (this.#media) {
      native?.closeMedia(this.#media)
      closedMedia.unregister(this)
      playingMedia.delete(this.#media)
      this[RUNNING](this.#media, false)
      this.#media = 0
    }
    if (this.#opening) native?.closeMedia(this.#opening)
    this.#opening = 0
    this[RESET]()
    if (this.#network !== 0) this.#emit('emptied')
    this.#ready = 0
    this.#ended = false
    this.#duration = NaN
    this.#position = 0
    this.#error = null
    this.#stopTimer()
    for (const stop of this.#unwatch.splice(0)) stop()
    this.#audioEnabled = true
    const stream = this.#object
    const url = stream ? '' : this.#source()
    if (!stream && (!url || !this[PLAYS]())) {
      this.#network = url ? 3 : 0
      this.#loading = false
      return
    }
    this.#emit('loadstart')
    if (!stream && !wanted && !this.autoplay && dom.getAttribute(this[ID], 'preload')?.trim().toLowerCase() === 'none') {
      this.#network = 1
      this.#loading = false
      this.#suspended = true
      this.#emit('suspend')
      return
    }
    this.#fetch(generation, url)
  }

  #fetch(generation: number, url: string): void {
    const native = audioNative()
    this.#suspended = false
    this.#network = 2
    this.#loading = true
    const fail = (code: number, message: string) => {
      if (generation !== this.#generation) return
      this.#opening = 0
      this.#loading = false
      this.#network = 3
      this.#error = new MediaError(code, message)
      this.#emit('error')
      this.#settle(new DOMException(`The element has no supported sources: ${message}`, 'NotSupportedError'))
    }
    if (!native) return fail(4, 'Media playback is not supported')
    const media = nextMedia++
    this.#opening = media
    const opened = (info: Opened) => {
      if (generation !== this.#generation) return native.closeMedia(media)
      this.#opening = 0
      this.#media = media
      closedMedia.register(this, media, this)
      this.#loading = false
      this.#network = 1
      this.#ready = 4
      this.#duration = info.duration
      this[OPENED](media, info, true)
      native.media(media, 3, this.#gain())
      if (this.loop) native.media(media, 4, 1)
      if (this.#rate !== 1) native.media(media, 5, this.#rate)
      if (this.#position > 0 && !this.#object) {
        native.media(media, 2, this.#position)
        this[SEEKED](media)
      }
      for (const type of ['durationchange', 'loadedmetadata', 'loadeddata', 'canplay', 'canplaythrough']) this.#emit(type)
      if (!this.#paused) this.#start()
      else if (this.autoplay) void this.play().catch(() => {})
    }
    const stream = this.#object
    if (stream) {
      let info: Opened
      try {
        info = this.#openStream(native, media, stream)
      } catch (error) {
        return fail(4, String((error as Error)?.message ?? error))
      }
      host.setTimer(0, () => opened(info))
      return
    }
    fetch(url)
      .then((response) => (response.ok ? response.arrayBuffer() : Promise.reject(new Error(`HTTP ${response.status}`))))
      .then(
        (bytes) => {
          if (generation !== this.#generation) return
          return this[OPEN](native, media, new Uint8Array(bytes)).then(opened, (error: unknown) => fail(4, String((error as Error)?.message ?? error)))
        },
        (error: unknown) => fail(2, String((error as Error)?.message ?? error)),
      )
  }

  #openStream(native: AudioNative, media: number, stream: MediaStream): Opened {
    const video = this[SHOWS]() ? stream.getVideoTracks().find((t) => t.readyState === 'live') : undefined
    const audio = stream.getAudioTracks().find((t) => t.readyState === 'live')
    if (!video && !audio) throw new Error('the MediaStream has no live track')
    this.#streamSources = [video ? trackSource(video) : 0, audio ? trackSource(audio) : 0]
    const info = native.openStream(media, video ? this[ID] : 0, this.#streamSources[0], this.#streamSources[1])
    this.#audioEnabled = audio?.enabled ?? true
    if (video && !video.enabled) native.streamEnabled(media, false)
    for (const track of [video, audio]) if (track) this.#unwatch.push(watchTrack(track, (t) => this.#trackChanged(t)))
    return info
  }

  #trackChanged(track: MediaStreamTrack): void {
    const native = audioNative()
    const stream = this.#object
    if (!native || !stream || !this.#media) return
    if (track.readyState === 'ended') {
      if (stream.active) this.#restream()
      else this[ENDED]()
      return
    }
    const video = track.kind === 'video'
    if (trackSource(track) !== this.#streamSources[video ? 0 : 1]) return this.#restream()
    if (video) native.streamEnabled(this.#media, track.enabled)
    else {
      this.#audioEnabled = track.enabled
      native.media(this.#media, 3, this.#gain())
    }
  }

  #restream(): void {
    const native = audioNative()
    const stream = this.#object
    if (!native || !stream || !this.#media) return
    const old = this.#media
    native.closeMedia(old)
    closedMedia.unregister(this)
    playingMedia.delete(old)
    this[RUNNING](old, false)
    for (const stop of this.#unwatch.splice(0)) stop()
    const media = nextMedia++
    let info: Opened
    try {
      info = this.#openStream(native, media, stream)
    } catch {
      this.#media = 0
      this[ENDED]()
      return
    }
    this.#media = media
    closedMedia.register(this, media, this)
    this[OPENED](media, info, false)
    native.media(media, 3, this.#gain())
    if (!this.#paused) this.#start(false)
  }

  #start(announce = true): void {
    const native = audioNative()
    if (!native || !this.#media) return
    native.media(this.#media, 0, 0)
    playingMedia.set(this.#media, this)
    this[RUNNING](this.#media, true)
    if (announce) this.#emit('playing')
    this.#settle(null)
    this.#stopTimer()
    const tick = () => {
      this.#timer = host.setTimer(250, tick)
      this.#emit('timeupdate')
    }
    this.#timer = host.setTimer(250, tick)
  }

  #stopTimer(): void {
    if (this.#timer) host.clearTimer(this.#timer)
    this.#timer = 0
  }

  [ENDED](): void {
    playingMedia.delete(this.#media)
    this[RUNNING](this.#media, false)
    this.#stopTimer()
    this.#ended = true
    this.#emit('timeupdate')
    if (!this.#paused) {
      this.#paused = true
      this.#emit('pause')
    }
    this.#emit('ended')
  }

  get paused(): boolean {
    return this.#paused
  }
  get ended(): boolean {
    return this.#ended && !this.loop
  }
  get duration(): number {
    return this.#duration
  }
  get readyState(): number {
    return this.#ready
  }
  get networkState(): number {
    return this.#network
  }
  get error(): MediaError | null {
    return this.#error
  }
  get currentSrc(): string {
    if (this.#object) return ''
    return this.#ready > 0 || this.#loading ? this.#source() : ''
  }

  get currentTime(): number {
    const native = audioNative()
    return native && this.#media ? native.mediaTime(this.#media) : this.#position
  }
  set currentTime(value: number) {
    const t = Number(value)
    if (!Number.isFinite(t)) throw new TypeError("Failed to set the 'currentTime' property on 'HTMLMediaElement': The provided double value is non-finite.")
    if (this.#object) return
    this.#position = Math.max(0, t)
    this.#ended = false
    if (this.#media) {
      audioNative()?.media(this.#media, 2, this.#position)
      this[SEEKED](this.#media)
      for (const type of ['seeking', 'timeupdate', 'seeked']) this.#emit(type)
    }
  }
  fastSeek(time: number): void {
    this.currentTime = time
  }

  get volume(): number {
    return this.#volume
  }
  set volume(value: number) {
    const v = Number(value)
    if (!Number.isFinite(v)) throw new TypeError("Failed to set the 'volume' property on 'HTMLMediaElement': The provided double value is non-finite.")
    if (v < 0 || v > 1) throw new DOMException(`Failed to set the 'volume' property on 'HTMLMediaElement': The volume provided (${v}) is outside the range [0, 1].`, 'IndexSizeError')
    if (v === this.#volume) return
    this.#volume = v
    if (this.#media) audioNative()?.media(this.#media, 3, this.#gain())
    this.#emit('volumechange')
  }

  get muted(): boolean {
    return this.#muted ?? dom.hasAttribute(this[ID], 'muted')
  }
  set muted(value: boolean) {
    const muted = !!value
    if (muted === this.muted) {
      this.#muted = muted
      return
    }
    this.#muted = muted
    if (this.#media) audioNative()?.media(this.#media, 3, this.#gain())
    this.#emit('volumechange')
  }

  get playbackRate(): number {
    return this.#rate
  }
  set playbackRate(value: number) {
    const rate = Number(value)
    if (!Number.isFinite(rate)) throw new TypeError("Failed to set the 'playbackRate' property on 'HTMLMediaElement': The provided double value is non-finite.")
    this.#rate = rate
    if (this.#media) audioNative()?.media(this.#media, 5, rate)
    this.#emit('ratechange')
  }

  get buffered(): { length: number; start(): number; end(): number } {
    return timeRanges(this.#media && Number.isFinite(this.#duration) ? this.#duration : 0)
  }
  get seekable(): { length: number; start(): number; end(): number } {
    return this.buffered
  }
  get played(): { length: number; start(): number; end(): number } {
    return timeRanges(0)
  }

  play(): Promise<void> {
    if (!this[PLAYS]()) return Promise.reject(new DOMException('Video playback is not supported', 'NotSupportedError'))
    if (!audioNative()) return Promise.reject(new DOMException('Media playback is not supported', 'NotSupportedError'))
    if (this.#error?.code === 4 || this.#error?.code === 2) return Promise.reject(new DOMException(`The element has no supported sources: ${this.#error.message}`, 'NotSupportedError'))
    if (!this.#media && !this.#loading) {
      const url = this.#object ? '' : this.#source()
      if (!url && !this.#object) return Promise.reject(new DOMException('The element has no supported sources.', 'NotSupportedError'))
      if (this.#suspended) this.#fetch(this.#generation, url)
      else this.#load(true)
    }
    if (this.ended) this.currentTime = 0
    const promise = new Promise<void>((resolve, reject) => this.#pending.push({ resolve, reject }))
    if (this.#paused) {
      this.#paused = false
      this.#emit('play')
      if (this.#media) this.#start()
    } else if (this.#media) {
      this.#settle(null)
    }
    return promise
  }

  pause(): void {
    if (this.#paused) return
    this.#paused = true
    if (this.#media) {
      audioNative()?.media(this.#media, 1, 0)
      playingMedia.delete(this.#media)
      this[RUNNING](this.#media, false)
    }
    this.#stopTimer()
    this.#emit('timeupdate')
    this.#emit('pause')
    this.#settle(new DOMException('The play() request was interrupted by a call to pause().', 'AbortError'))
  }

  load(): void {
    if (!this.#paused) {
      this.#paused = true
      this.#emit('pause')
    }
    this.#settle(new DOMException('The play() request was interrupted by a new load request.', 'AbortError'))
    this.#load()
  }

  canPlayType(type: string): string {
    if (!this[PLAYS]()) return ''
    const [mime = '', ...params] = String(type).split(';').map((s) => s.trim())
    if (!PLAYABLE.test(mime)) return ''
    const codecs = params.find((p) => /^codecs\s*=/i.test(p))
    if (codecs && /opus|webm|alac|ac-?3|ec-?3/i.test(codecs)) return ''
    return 'maybe'
  }

  addTextTrack(): never {
    throw new DOMException('Text tracks are not supported', 'NotSupportedError')
  }

  override [ATTRIBUTE_CHANGED](name: string, value: string | null): void {
    super[ATTRIBUTE_CHANGED](name, value)
    if (name === 'src') this.load()
    else if (name === 'loop' && this.#media) audioNative()?.media(this.#media, 4, value === null ? 0 : 1)
  }
}
getters(HTMLMediaElement.prototype, { seeking: false, textTracks: () => [] })
url(HTMLMediaElement.prototype, 'src')
string(HTMLMediaElement.prototype, 'preload')
nullable(HTMLMediaElement.prototype, 'crossOrigin', 'crossorigin')
for (const name of ['autoplay', 'loop', 'controls']) bool(HTMLMediaElement.prototype, name)
bool(HTMLMediaElement.prototype, 'defaultMuted', 'muted')
constants(HTMLMediaElement, { NETWORK_EMPTY: 0, NETWORK_IDLE: 1, NETWORK_LOADING: 2, NETWORK_NO_SOURCE: 3, HAVE_NOTHING: 0, HAVE_METADATA: 1, HAVE_CURRENT_DATA: 2, HAVE_FUTURE_DATA: 3, HAVE_ENOUGH_DATA: 4 })
tag(HTMLMediaElement)

mediaEvents.ended = (media) => playingMedia.get(media)?.[ENDED]()

function created(root: Node, withRoot: boolean): void {
  const type = inner(root, 'nodeType')
  if (type !== NodeType.Element && type !== NodeType.Document && type !== NodeType.DocumentFragment) return
  const id = root[ID]
  const ids = [...dom.querySelectorAll(id, 'video, audio')]
  if (withRoot && root instanceof HTMLMediaElement) ids.unshift(id)
  for (const media of ids) {
    const element = wrap<HTMLMediaElement>(media)
    if (element instanceof HTMLMediaElement) element[CREATED]()
  }
}
parsedHooks.push((container) => created(container, false))
clonedHooks.push((clone) => created(clone, true))
lifecycle.disconnected.push((node) => {
  if (!(node instanceof HTMLMediaElement)) return
  void Promise.resolve().then(() => {
    if (!node.isConnected) node.pause()
  })
})

const videos = new Map<number, HTMLVideoElement>()
const PRESENT = Symbol('present')

export function presentVideos(): boolean {
  if (videos.size === 0) return false
  let more = false
  for (const [media, video] of videos) more = video[PRESENT](media) || more
  return more
}

export class HTMLVideoElement extends HTMLMediaElement {
  #width = 0
  #height = 0
  #running = false
  #seeking = false
  #frames = 0

  get videoWidth(): number {
    return this.#width
  }

  get videoHeight(): number {
    return this.#height
  }

  getVideoPlaybackQuality(): object {
    return { totalVideoFrames: this.#frames, droppedVideoFrames: 0, corruptedVideoFrames: 0, creationTime: host.now() }
  }

  override [PLAYS](): boolean {
    return typeof audioNative()?.openVideo === 'function'
  }

  override [OPEN](native: AudioNative, media: number, bytes: Uint8Array): Promise<Opened> {
    return native.openVideo(media, this[ID], bytes)
  }

  override [OPENED](media: number, info: Opened, fresh: boolean): void {
    for (const [old, video] of videos) if (video === this) videos.delete(old)
    videos.set(media, this)
    if (fresh) {
      this.#frames = info.width ? 1 : 0
      this.#width = info.width ?? 0
      this.#height = info.height ?? 0
    } else {
      this.#resize(info.width ?? 0, info.height ?? 0)
    }
  }

  override [SHOWS](): boolean {
    return true
  }

  override [RUNNING](media: number, running: boolean): void {
    this.#running = running
    this.#want(media)
  }

  override [SEEKED](media: number): void {
    this.#seeking = true
    this.#want(media)
  }

  override [RESET](): void {
    for (const [media, video] of videos) if (video === this) videos.delete(media)
    this.#running = this.#seeking = false
    this.#frames = 0
    audioNative()?.clearVideo(this[ID])
    this.#width = this.#height = 0
  }

  [PRESENT](media: number): boolean {
    const native = audioNative()
    if (!native) return false
    const shown = native.present(media)
    if (shown > 0) {
      this.#frames++
      this.#seeking = false
    }
    if (shown === 2) {
      const [width, height] = native.videoSize(media)
      this.#resize(width, height)
    }
    return this.#running || this.#seeking
  }

  #want(media: number): void {
    if (videos.get(media) !== this) return
    if (this.#running || this.#seeking) frames.request()
  }

  #resize(width: number, height: number): void {
    if (width === this.#width && height === this.#height) return
    this.#width = width
    this.#height = height
    if (this.readyState > 0) host.setTimer(0, () => fire(this, new Event('resize')))
  }

  override canPlayType(type: string): string {
    if (!this[PLAYS]()) return ''
    const mime = String(type).split(';')[0]!.trim().toLowerCase()
    if (audioNative()?.videoTypes.includes(mime)) return 'maybe'
    return super.canPlayType(type)
  }
}
for (const name of ['width', 'height']) long(HTMLVideoElement.prototype, name, 0)
url(HTMLVideoElement.prototype, 'poster')
bool(HTMLVideoElement.prototype, 'playsInline', 'playsinline')
register(HTMLVideoElement, 'video')

export class HTMLAudioElement extends HTMLMediaElement {}
register(HTMLAudioElement, 'audio')

