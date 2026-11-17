import { DOMException } from '../core/exception'
import { defineHandlers, Event, EventTarget, fire } from '../core/event'
import { host, illegal, KEY, tag } from '../core/host'
import type { CaptureDevice, CaptureNative, CaptureOpened } from '../native'

let native: CaptureNative | null = null

type TrackKind = 'audio' | 'video'
type Settings = Record<string, string | number | boolean>
type Constraints = Record<string, unknown>

const NUMERIC = new Set(['width', 'height', 'aspectRatio', 'frameRate', 'sampleRate', 'sampleSize', 'channelCount'])
const TEXT = new Set(['deviceId', 'groupId', 'facingMode', 'resizeMode'])
const FLAGS = new Set(['echoCancellation', 'autoGainControl', 'noiseSuppression'])
const SUPPORTED = [...TEXT, ...NUMERIC, ...FLAGS]
const SAMPLE_RATE = 48000
const SAMPLE_SIZE = 32

function copy(constraints: Constraints): Constraints {
  return JSON.parse(JSON.stringify(constraints ?? {})) as Constraints
}

export class OverconstrainedError extends DOMException {
  readonly #constraint: string

  constructor(constraint: string, message = '') {
    super(message, 'OverconstrainedError')
    this.#constraint = String(constraint)
  }

  get constraint(): string {
    return this.#constraint
  }
}
tag(OverconstrainedError)

function ideal(value: unknown, advanced: boolean): { exact?: unknown; min?: unknown; max?: unknown; ideal?: unknown } {
  if (value !== null && typeof value === 'object' && !Array.isArray(value)) return value as { exact?: unknown; min?: unknown; max?: unknown; ideal?: unknown }
  return advanced ? { exact: value } : { ideal: value }
}

function list(value: unknown): string[] {
  return (Array.isArray(value) ? value : [value]).map(String)
}

function distance(name: string, value: unknown, settings: Settings, advanced: boolean): number {
  if (!SUPPORTED.includes(name)) return 0
  const want = ideal(value, advanced)
  const actual = settings[name]
  const required = want.exact !== undefined || want.min !== undefined || want.max !== undefined
  if (actual === undefined) return required ? Infinity : want.ideal === undefined ? 0 : 1
  if (NUMERIC.has(name)) {
    const n = Number(actual)
    if (want.exact !== undefined && Math.abs(n - Number(want.exact)) > 1e-9) return Infinity
    if (want.min !== undefined && n < Number(want.min)) return Infinity
    if (want.max !== undefined && n > Number(want.max)) return Infinity
    if (want.ideal === undefined) return 0
    const target = Number(want.ideal)
    return n === target ? 0 : Math.abs(n - target) / Math.max(Math.abs(n), Math.abs(target))
  }
  if (FLAGS.has(name)) {
    if (want.exact !== undefined && Boolean(want.exact) !== actual) return Infinity
    return want.ideal === undefined || Boolean(want.ideal) === actual ? 0 : 1
  }
  if (want.exact !== undefined && !list(want.exact).includes(String(actual))) return Infinity
  return want.ideal === undefined || list(want.ideal).includes(String(actual)) ? 0 : 1
}

function fitness(set: Constraints, settings: Settings, advanced: boolean): number {
  let total = 0
  for (const [name, value] of Object.entries(set)) {
    if (name === 'advanced' || value === undefined) continue
    total += distance(name, value, settings, advanced)
  }
  return total
}

function failing(set: Constraints, candidates: Settings[]): string {
  for (const [name, value] of Object.entries(set)) {
    if (name === 'advanced' || value === undefined) continue
    if (candidates.every((c) => distance(name, value, c, false) === Infinity)) return name
  }
  return ''
}

function validate(constraints: Constraints): void {
  for (const [name, value] of Object.entries(constraints)) {
    if (name === 'advanced') {
      if (value !== undefined && !Array.isArray(value)) throw new TypeError("Failed to execute 'getUserMedia' on 'MediaDevices': 'advanced' is not a sequence.")
      continue
    }
    if (value === null || typeof value !== 'object' || Array.isArray(value)) continue
    for (const key of Object.keys(value)) {
      if (!['exact', 'ideal', 'min', 'max'].includes(key)) continue
      const bound = (value as Record<string, unknown>)[key]
      if (NUMERIC.has(name) && typeof bound !== 'number' && bound !== undefined) throw new TypeError(`Failed to execute 'getUserMedia' on 'MediaDevices': ${name}.${key} is not a number.`)
    }
  }
}

interface Candidate {
  readonly device: CaptureDevice
  readonly mode: readonly [number, number, number] | null
  readonly settings: Settings
}

function candidates(device: CaptureDevice): Candidate[] {
  const common = { deviceId: device.deviceId, groupId: device.groupId }
  if (device.kind === 'audioinput') {
    const settings: Settings = { ...common, sampleRate: SAMPLE_RATE, sampleSize: SAMPLE_SIZE, channelCount: device.channelCount, echoCancellation: false, autoGainControl: false, noiseSuppression: false }
    return [{ device, mode: null, settings }]
  }
  return device.modes.map((mode) => {
    const [width, height, frameRate] = mode
    const settings: Settings = { ...common, width, height, frameRate, aspectRatio: width / height, resizeMode: 'none' }
    if (device.facingMode) settings['facingMode'] = device.facingMode
    return { device, mode, settings }
  })
}

function select(all: Candidate[], constraints: Constraints): Candidate | string {
  validate(constraints)
  let set = all.filter((c) => fitness(constraints, c.settings, false) < Infinity)
  if (set.length === 0) return failing(constraints, all.map((c) => c.settings)) || Object.keys(constraints)[0] || ''
  const advanced = Array.isArray(constraints['advanced']) ? (constraints['advanced'] as Constraints[]) : []
  for (const extra of advanced) {
    if (extra === null || typeof extra !== 'object') continue
    const narrowed = set.filter((c) => fitness(extra, c.settings, true) < Infinity)
    if (narrowed.length) set = narrowed
  }
  let best = set[0]!
  let score = fitness(constraints, best.settings, false)
  for (const c of set.slice(1)) {
    const s = fitness(constraints, c.settings, false)
    if (s < score) [best, score] = [c, s]
  }
  return best
}

function failure(error: unknown): unknown {
  const message = String((error as Error)?.message ?? error)
  const match = /^(NotAllowedError|NotFoundError|NotReadableError|NotSupportedError|AbortError|OverconstrainedError): ?(.*)$/s.exec(message)
  if (match) return new DOMException(match[2], match[1])
  return new DOMException(message, 'AbortError')
}

interface Source {
  readonly id: number
  readonly kind: TrackKind
  readonly device: CaptureDevice
  readonly settings: Settings
  readonly tracks: Set<MediaStreamTrack>
}

const sources = new Map<number, Source>()

export type TrackListener = (track: MediaStreamTrack) => void
const listeners = new Map<MediaStreamTrack, Set<TrackListener>>()

export function watchTrack(track: MediaStreamTrack, listener: TrackListener): () => void {
  let set = listeners.get(track)
  if (!set) listeners.set(track, (set = new Set()))
  set.add(listener)
  return () => set.delete(listener)
}

function changed(track: MediaStreamTrack): void {
  for (const listener of [...(listeners.get(track) ?? [])]) listener(track)
}

function settingsOf(opened: CaptureOpened, kind: TrackKind): Settings {
  const settings: Settings = { deviceId: opened.deviceId, groupId: opened.groupId }
  if (kind === 'audio') return { ...settings, sampleRate: SAMPLE_RATE, sampleSize: SAMPLE_SIZE, channelCount: opened.channelCount, echoCancellation: false, autoGainControl: false, noiseSuppression: false }
  Object.assign(settings, { width: opened.width, height: opened.height, frameRate: opened.frameRate, aspectRatio: opened.height ? opened.width / opened.height : 0, resizeMode: 'none' })
  if (opened.facingMode) settings['facingMode'] = opened.facingMode
  return settings
}

function adopt(opened: CaptureOpened, kind: TrackKind): Source {
  const source: Source = { id: opened.source, kind, device: opened, settings: settingsOf(opened, kind), tracks: new Set() }
  sources.set(source.id, source)
  return source
}

function release(source: Source, track: MediaStreamTrack): void {
  source.tracks.delete(track)
  if (source.tracks.size === 0 && sources.get(source.id) === source) {
    sources.delete(source.id)
    native?.close(source.id)
  }
}

const SOURCE = Symbol('source')
const END = Symbol('end')
const ATTACH = Symbol('attach')
const UPDATE = Symbol('update')

export class MediaStreamTrack extends EventTarget {
  readonly #id = host.randomUUID()
  readonly #kind: TrackKind
  #source: Source
  #constraints: Constraints
  #enabled = true
  #ended = false
  contentHint = ''

  constructor(token: unknown, source: Source, constraints: Constraints) {
    illegal(token)
    super()
    this.#kind = source.kind
    this.#source = source
    this.#constraints = constraints
    source.tracks.add(this)
  }

  get [SOURCE](): number {
    return this.#ended ? 0 : this.#source.id
  }

  [ATTACH](source: Source): void {
    const old = this.#source
    this.#source = source
    source.tracks.add(this)
    release(old, this)
    changed(this)
  }

  [END](): void {
    if (this.#ended) return
    this.#ended = true
    release(this.#source, this)
    changed(this)
    fire(this, new Event('ended'))
  }

  get kind(): string {
    return this.#kind
  }
  get id(): string {
    return this.#id
  }
  get label(): string {
    return this.#source.device.label
  }
  get enabled(): boolean {
    return this.#enabled
  }
  set enabled(value: boolean) {
    const enabled = !!value
    if (enabled === this.#enabled) return
    this.#enabled = enabled
    changed(this)
  }
  get muted(): boolean {
    return false
  }
  get readyState(): string {
    return this.#ended ? 'ended' : 'live'
  }

  clone(): MediaStreamTrack {
    const twin = new MediaStreamTrack(KEY, this.#source, copy(this.#constraints))
    twin.enabled = this.#enabled
    if (this.#ended) {
      twin.#ended = true
      release(this.#source, twin)
    }
    return twin
  }

  stop(): void {
    if (this.#ended) return
    this.#ended = true
    release(this.#source, this)
    changed(this)
  }

  getCapabilities(): Record<string, unknown> {
    if (this.#ended) return {}
    const device = this.#source.device
    const caps: Record<string, unknown> = { deviceId: device.deviceId, groupId: device.groupId }
    if (this.#kind === 'audio') {
      return { ...caps, sampleRate: { min: SAMPLE_RATE, max: SAMPLE_RATE }, sampleSize: { min: SAMPLE_SIZE, max: SAMPLE_SIZE }, channelCount: { min: 1, max: Math.max(1, device.channelCount) }, echoCancellation: [false], autoGainControl: [false], noiseSuppression: [false] }
    }
    const modes = device.modes
    const span = (pick: (m: readonly [number, number, number]) => number) => ({ min: Math.min(...modes.map(pick)), max: Math.max(...modes.map(pick)) })
    Object.assign(caps, { width: span((m) => m[0]), height: span((m) => m[1]), aspectRatio: span((m) => m[0] / m[1]), frameRate: span((m) => m[2]), resizeMode: ['none'] })
    caps['facingMode'] = device.facingMode ? [device.facingMode] : []
    return caps
  }

  getConstraints(): Constraints {
    return copy(this.#constraints)
  }

  getSettings(): Settings {
    return { ...this.#source.settings }
  }

  applyConstraints(constraints: Constraints = {}): Promise<void> {
    if (this.#ended) return Promise.resolve()
    const wanted = constraints === null || typeof constraints !== 'object' ? {} : constraints
    const source = this.#source
    let chosen: Candidate | string
    try {
      chosen = select(candidates(source.device), wanted)
    } catch (error) {
      return Promise.reject(error)
    }
    if (typeof chosen === 'string') return Promise.reject(new OverconstrainedError(chosen, 'The constraints cannot be met by this device.'))
    const mode = chosen.mode
    const current = source.settings
    const same = !mode || (mode[0] === current['width'] && mode[1] === current['height'])
    if (same) {
      this.#constraints = copy(wanted)
      return Promise.resolve()
    }
    const capture = native
    if (!capture) return Promise.reject(new DOMException('Capture is not available', 'NotReadableError'))
    return capture.open(this.#kind === 'audio' ? 'audioinput' : 'videoinput', source.device.deviceId, mode[0], mode[1], mode[2]).then(
      (opened) => {
        if (this.#ended) return native?.close(opened.source)
        this.#constraints = copy(wanted)
        this[ATTACH](adopt(opened, this.#kind))
      },
      (error: unknown) => {
        throw failure(error)
      },
    )
  }
}
defineHandlers(MediaStreamTrack.prototype, ['mute', 'unmute', 'ended'])
tag(MediaStreamTrack)

export class MediaStream extends EventTarget {
  readonly #id = host.randomUUID()
  readonly #tracks: MediaStreamTrack[] = []

  constructor(init?: MediaStream | readonly MediaStreamTrack[]) {
    super()
    if (init === undefined) return
    const tracks = init instanceof MediaStream ? init.getTracks() : [...(init as Iterable<MediaStreamTrack>)]
    for (const track of tracks) {
      if (!(track instanceof MediaStreamTrack)) throw new TypeError("Failed to construct 'MediaStream': Failed to convert value to 'MediaStreamTrack'.")
      if (!this.#tracks.includes(track)) this.#tracks.push(track)
    }
  }

  get id(): string {
    return this.#id
  }
  get active(): boolean {
    return this.#tracks.some((t) => t.readyState === 'live')
  }

  getTracks(): MediaStreamTrack[] {
    return [...this.#tracks]
  }
  getAudioTracks(): MediaStreamTrack[] {
    return this.#tracks.filter((t) => t.kind === 'audio')
  }
  getVideoTracks(): MediaStreamTrack[] {
    return this.#tracks.filter((t) => t.kind === 'video')
  }
  getTrackById(id: string): MediaStreamTrack | null {
    return this.#tracks.find((t) => t.id === String(id)) ?? null
  }

  addTrack(track: MediaStreamTrack): void {
    if (!(track instanceof MediaStreamTrack)) throw new TypeError("Failed to execute 'addTrack' on 'MediaStream': parameter 1 is not of type 'MediaStreamTrack'.")
    if (!this.#tracks.includes(track)) this.#tracks.push(track)
  }

  removeTrack(track: MediaStreamTrack): void {
    if (!(track instanceof MediaStreamTrack)) throw new TypeError("Failed to execute 'removeTrack' on 'MediaStream': parameter 1 is not of type 'MediaStreamTrack'.")
    const at = this.#tracks.indexOf(track)
    if (at >= 0) this.#tracks.splice(at, 1)
  }

  clone(): MediaStream {
    return new MediaStream(this.#tracks.map((t) => t.clone()))
  }
}
defineHandlers(MediaStream.prototype, ['addtrack', 'removetrack'])
tag(MediaStream)

export function trackSource(track: MediaStreamTrack): number {
  return track[SOURCE]
}

export class MediaDeviceInfo {
  readonly #info: CaptureDevice

  constructor(token: unknown, info: CaptureDevice) {
    illegal(token)
    this.#info = info
  }

  get deviceId(): string {
    return this.#info.deviceId
  }
  get kind(): string {
    return this.#info.kind
  }
  get label(): string {
    return this.#info.label
  }
  get groupId(): string {
    return this.#info.groupId
  }

  toJSON(): object {
    return { deviceId: this.deviceId, kind: this.kind, label: this.label, groupId: this.groupId }
  }
}
tag(MediaDeviceInfo)

export class InputDeviceInfo extends MediaDeviceInfo {
  readonly #info: CaptureDevice

  constructor(token: unknown, info: CaptureDevice) {
    super(token, info)
    this.#info = info
  }

  getCapabilities(): Record<string, unknown> {
    const info = this.#info
    if (!info.deviceId) return {}
    const all = candidates(info).map((c) => c.settings)
    if (info.kind === 'audioinput') return { deviceId: info.deviceId, groupId: info.groupId, sampleRate: { min: SAMPLE_RATE, max: SAMPLE_RATE }, sampleSize: { min: SAMPLE_SIZE, max: SAMPLE_SIZE }, channelCount: { min: 1, max: Math.max(1, info.channelCount) }, echoCancellation: [false], autoGainControl: [false], noiseSuppression: [false] }
    const span = (name: string) => ({ min: Math.min(...all.map((s) => Number(s[name]))), max: Math.max(...all.map((s) => Number(s[name]))) })
    return { deviceId: info.deviceId, groupId: info.groupId, width: span('width'), height: span('height'), aspectRatio: span('aspectRatio'), frameRate: span('frameRate'), resizeMode: ['none'], facingMode: info.facingMode ? [info.facingMode] : [] }
  }
}
tag(InputDeviceInfo)

function asConstraints(value: unknown): Constraints | null {
  if (value === undefined || value === false) return null
  if (value === true) return {}
  if (value === null || typeof value !== 'object') throw new TypeError("Failed to execute 'getUserMedia' on 'MediaDevices': The provided value is not of type 'MediaTrackConstraints'.")
  return value as Constraints
}

const permissionChanges = new Set<() => void>()

export class MediaDevices extends EventTarget {
  getSupportedConstraints(): Record<string, boolean> {
    return Object.fromEntries(SUPPORTED.map((name) => [name, true]))
  }

  enumerateDevices(): Promise<MediaDeviceInfo[]> {
    if (!native) return Promise.resolve([])
    return native.devices(false).then((devices) => devices.map((d) => new InputDeviceInfo(KEY, d)))
  }

  getUserMedia(constraints: { audio?: unknown; video?: unknown } = {}): Promise<MediaStream> {
    let wanted: [TrackKind, Constraints][]
    try {
      const given = constraints ?? {}
      wanted = ([['audio', asConstraints(given.audio)], ['video', asConstraints(given.video)]] as const).filter((w): w is [TrackKind, Constraints] => w[1] !== null)
    } catch (error) {
      return Promise.reject(error)
    }
    if (wanted.length === 0) return Promise.reject(new TypeError("Failed to execute 'getUserMedia' on 'MediaDevices': At least one of audio and video must be requested"))
    const capture = native
    const refused = () => new DOMException('Permission denied', 'NotAllowedError')
    if (!capture) return Promise.reject(refused())
    const permission = (kind: TrackKind) => (kind === 'audio' ? 'microphone' : 'camera')
    for (const [kind] of wanted) {
      if (capture.permission(permission(kind)) === 'denied') return Promise.reject(refused())
    }
    return capture.devices(true).then(async (devices) => {
      const chosen: [TrackKind, Constraints, Candidate][] = []
      for (const [kind, set] of wanted) {
        const all = devices.filter((d) => d.kind === (kind === 'audio' ? 'audioinput' : 'videoinput')).flatMap(candidates)
        if (all.length === 0) throw new DOMException('Requested device not found', 'NotFoundError')
        const pick = select(all, set)
        if (typeof pick === 'string') throw new OverconstrainedError(pick, 'The constraints cannot be met by any device.')
        chosen.push([kind, set, pick])
      }
      const opened: [TrackKind, Constraints, CaptureOpened][] = []
      try {
        for (const [kind] of chosen) {
          if (capture.request && !(await capture.request(permission(kind)))) throw refused()
        }
        for (const [kind, set, pick] of chosen) {
          const mode = pick.mode ?? [0, 0, 0]
          const device = capture.open(kind === 'audio' ? 'audioinput' : 'videoinput', pick.device.deviceId, mode[0], mode[1], mode[2])
          opened.push([
            kind,
            set,
            await device.catch((error: unknown) => {
              throw failure(error)
            }),
          ])
        }
      } catch (error) {
        for (const [, , o] of opened) capture.close(o.source)
        throw error
      } finally {
        for (const change of permissionChanges) change()
      }
      return new MediaStream(opened.map(([kind, set, o]) => new MediaStreamTrack(KEY, adopt(o, kind), copy(set))))
    })
  }

  getDisplayMedia(): Promise<never> {
    return Promise.reject(new DOMException('Screen capture is not supported', 'NotAllowedError'))
  }
}
defineHandlers(MediaDevices.prototype, ['devicechange'])
tag(MediaDevices)

const PERMISSIONS = new Set(['camera', 'microphone', 'geolocation', 'notifications', 'persistent-storage', 'push', 'screen-wake-lock', 'idle-detection', 'clipboard-read', 'clipboard-write'])

export class PermissionStatus extends EventTarget {
  readonly #name: string
  #state: string

  constructor(token: unknown, name: string, state: string) {
    illegal(token)
    super()
    this.#name = name
    this.#state = state
  }

  get name(): string {
    return this.#name
  }
  get state(): string {
    return this.#state
  }

  [UPDATE](state: string): void {
    if (state === this.#state) return
    this.#state = state
    fire(this, new Event('change'))
  }
}
defineHandlers(PermissionStatus.prototype, ['change'])
tag(PermissionStatus)

export class Permissions {
  readonly #state: (name: string) => string

  constructor(token: unknown, state: (name: string) => string) {
    illegal(token)
    this.#state = state
  }

  query(descriptor: { name?: unknown }): Promise<PermissionStatus> {
    if (descriptor === null || typeof descriptor !== 'object' || descriptor.name === undefined) return Promise.reject(new TypeError("Failed to execute 'query' on 'Permissions': Required member is undefined."))
    const name = String(descriptor.name)
    if (!PERMISSIONS.has(name)) return Promise.reject(new TypeError(`Failed to execute 'query' on 'Permissions': The provided value '${name}' is not a valid enum value of type PermissionName.`))
    const status = new PermissionStatus(KEY, name, this.#state(name))
    if (name === 'camera' || name === 'microphone') {
      const ref = new WeakRef(status)
      const update = () => {
        const live = ref.deref()
        if (live) live[UPDATE](this.#state(name))
        else permissionChanges.delete(update)
      }
      permissionChanges.add(update)
    }
    return Promise.resolve(status)
  }
}
tag(Permissions)

export function capturePermission(name: 'camera' | 'microphone'): string {
  return native ? native.permission(name) : 'denied'
}

export function installCapture(capture: CaptureNative | undefined): void {
  native = capture ?? null
  native?.onPermission?.(() => {
    for (const change of permissionChanges) change()
  })
  native?.onEnded((id) => {
    const source = sources.get(id)
    if (!source) return
    for (const track of [...source.tracks]) track[END]()
  })
}
