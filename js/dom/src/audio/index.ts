import { DOMException } from '../core/exception'
import { defineHandlers, Event, EventTarget, fire } from '../core/event'
import { getters, host, illegal, KEY, queueTask, tag } from '../core/host'
import type { AudioNative } from '../native'
import { MediaStream, trackSource, watchTrack } from '../media/stream'

let native: AudioNative | null = null
let nextId = 1
const newId = (): number => nextId++

export function audioNative(): AudioNative | null {
  return native
}

export const mediaEvents = { ended: (_media: number): void => {} }

const ID = Symbol('id')
const CONTEXT = Symbol('context')
const CURVED = Symbol('curved')
const UPLOAD = Symbol('upload')
const SET_STATE = Symbol('setState')

interface ParamOwner {
  readonly [ID]: number
  readonly [CONTEXT]: BaseAudioContext
}

const playing = new Map<number, AudioScheduledSourceNode>()
const releasedNodes = new FinalizationRegistry<number>((id) => native?.release(id))
const releasedBuffers = new FinalizationRegistry<number>((id) => native?.releaseBuffer(id))
const warned = new Set<string>()

function finite(what: string, value: unknown): number {
  const n = Number(value)
  if (!Number.isFinite(n)) throw new TypeError(`Failed to execute '${what}': the value provided is non-finite.`)
  return n
}

function time(what: string, value: unknown): number {
  const t = finite(what, value)
  if (t < 0) throw new RangeError(`Failed to execute '${what}': the time must be non-negative.`)
  return t
}

class AudioParam {
  readonly #owner: ParamOwner
  readonly #node: number
  readonly #index: number
  readonly #default: number
  readonly #min: number
  readonly #max: number
  #value: number
  #automated = false
  #curve: [start: number, end: number] | null = null

  constructor(token: unknown, owner: ParamOwner, index: number, defaultValue: number, min: number, max: number) {
    illegal(token)
    this.#owner = owner
    this.#node = owner[ID]
    this.#index = index
    this.#default = defaultValue
    this.#min = min
    this.#max = max
    this.#value = defaultValue
  }

  get defaultValue(): number {
    return this.#default
  }
  get minValue(): number {
    return this.#min
  }
  get maxValue(): number {
    return this.#max
  }
  get automationRate(): string {
    return 'a-rate'
  }
  set automationRate(_value: string) {}

  get value(): number {
    if (this.#automated && native) return native.paramValue(this.#node, this.#index)
    return Math.min(this.#max, Math.max(this.#min, this.#value))
  }
  set value(value: number) {
    const v = finite('AudioParam.value', value)
    this.#value = v
    native?.automate(this.#node, this.#index, 0, v, 0, 0)
  }

  get [ID](): [number, number] {
    this.#automated = true
    return [this.#node, this.#index]
  }

  get [CONTEXT](): BaseAudioContext {
    return this.#owner[CONTEXT]
  }

  [CURVED](): boolean {
    return this.#curve !== null && this.#curve[1] >= this.#owner[CONTEXT].currentTime
  }

  #automate(op: number, value: number, at: number, extra = 0): this {
    this.#automated = true
    native?.automate(this.#node, this.#index, op, value, at, extra)
    return this
  }

  setValueAtTime(value: number, startTime: number): this {
    return this.#automate(1, finite('setValueAtTime', value), time('setValueAtTime', startTime))
  }

  linearRampToValueAtTime(value: number, endTime: number): this {
    return this.#automate(2, finite('linearRampToValueAtTime', value), time('linearRampToValueAtTime', endTime))
  }

  exponentialRampToValueAtTime(value: number, endTime: number): this {
    const v = finite('exponentialRampToValueAtTime', value)
    if (v === 0) throw new RangeError("Failed to execute 'exponentialRampToValueAtTime': the float target value provided (0) should not be in the range (-1.40130e-45, 1.40130e-45).")
    return this.#automate(3, v, time('exponentialRampToValueAtTime', endTime))
  }

  setTargetAtTime(target: number, startTime: number, timeConstant: number): this {
    const tc = finite('setTargetAtTime', timeConstant)
    if (tc < 0) throw new RangeError("Failed to execute 'setTargetAtTime': the time constant must be non-negative.")
    return this.#automate(4, finite('setTargetAtTime', target), time('setTargetAtTime', startTime), tc)
  }

  setValueCurveAtTime(values: ArrayLike<number>, startTime: number, duration: number): this {
    const curve = Float32Array.from(values as ArrayLike<number>)
    if (curve.length < 2) throw new DOMException("Failed to execute 'setValueCurveAtTime': the curve must have at least two values.", 'InvalidStateError')
    for (const v of curve) finite('setValueCurveAtTime', v)
    const at = time('setValueCurveAtTime', startTime)
    const d = finite('setValueCurveAtTime', duration)
    if (d <= 0) throw new RangeError("Failed to execute 'setValueCurveAtTime': the duration must be positive.")
    this.#automated = true
    this.#curve = [at, at + d]
    native?.automateCurve(this.#node, this.#index, curve, at, d)
    return this
  }

  cancelScheduledValues(cancelTime: number): this {
    const at = time('cancelScheduledValues', cancelTime)
    if (this.#curve && at <= this.#curve[0]) this.#curve = null
    return this.#automate(5, 0, at)
  }

  cancelAndHoldAtTime(cancelTime: number): this {
    const at = time('cancelAndHoldAtTime', cancelTime)
    if (this.#curve) this.#curve = at <= this.#curve[0] ? null : [this.#curve[0], Math.min(this.#curve[1], at)]
    return this.#automate(6, 0, at)
  }
}
tag(AudioParam)

const enum Kind {
  Gain = 1,
  Oscillator = 2,
  BufferSource = 3,
  StereoPanner = 4,
  ConstantSource = 5,
  Through = 6,
  Stream = 7,
  Panner = 8,
  Listener = 9,
  Biquad = 10,
  Delay = 11,
  WaveShaper = 12,
  Compressor = 13,
  Convolver = 14,
  Analyser = 15,
}

class AudioNode extends EventTarget {
  readonly [ID]: number
  readonly #context: BaseAudioContext
  readonly #inputs: number
  readonly #outputs: number
  channelCount = 2
  channelCountMode = 'max'
  channelInterpretation = 'speakers'

  constructor(token: unknown, context: BaseAudioContext, kind: number, inputs = 1, outputs = 1, id?: number) {
    illegal(token)
    super()
    this.#context = context
    this.#inputs = inputs
    this.#outputs = outputs
    this[ID] = id ?? newId()
    if (kind > 0) {
      native?.createNode(context[ID], this[ID], kind)
      releasedNodes.register(this, this[ID], this)
    }
  }

  get context(): BaseAudioContext {
    return this.#context
  }
  get [CONTEXT](): BaseAudioContext {
    return this.#context
  }
  get numberOfInputs(): number {
    return this.#inputs
  }
  get numberOfOutputs(): number {
    return this.#outputs
  }

  connect<T extends AudioNode>(destination: T, output?: number, input?: number): T
  connect(destination: AudioParam, output?: number): undefined
  connect(destination: AudioNode | AudioParam, output = 0, _input = 0): AudioNode | undefined {
    if (Number(output) >= this.#outputs) throw new DOMException(`Failed to execute 'connect' on 'AudioNode': output index (${output}) exceeds number of outputs (${this.#outputs}).`, 'IndexSizeError')
    if (destination instanceof AudioParam) {
      if (destination[CONTEXT] !== this.#context) throw new DOMException("Failed to execute 'connect' on 'AudioNode': cannot connect to an AudioParam belonging to a different audio context.", 'InvalidAccessError')
      const [node, index] = destination[ID]
      native?.connect(this[ID], node, index)
      return undefined
    }
    if (!(destination instanceof AudioNode)) throw new TypeError("Failed to execute 'connect' on 'AudioNode': parameter 1 is not of type 'AudioNode'.")
    if (destination.context !== this.#context) throw new DOMException("Failed to execute 'connect' on 'AudioNode': cannot connect to an AudioNode belonging to a different audio context.", 'InvalidAccessError')
    if (destination.numberOfInputs === 0) throw new DOMException("Failed to execute 'connect' on 'AudioNode': the destination has no inputs.", 'IndexSizeError')
    native?.connect(this[ID], destination[ID], -1)
    return destination
  }

  disconnect(destination?: AudioNode | AudioParam | number, _output?: number, _input?: number): void {
    if (destination instanceof AudioParam) {
      const [node, index] = destination[ID]
      native?.disconnect(this[ID], node, index)
    } else if (destination instanceof AudioNode) {
      native?.disconnect(this[ID], destination[ID], -1)
    } else {
      native?.disconnect(this[ID], -1, -1)
    }
  }
}
tag(AudioNode)

class AudioDestinationNode extends AudioNode {}
getters(AudioDestinationNode.prototype, { maxChannelCount: 2 })
tag(AudioDestinationNode)

type ParamSpec = [name: string, value: number, min?: number, max?: number]

function params(owner: ParamOwner, options: object | undefined, specs: ParamSpec[]): AudioParam[] {
  return specs.map(([name, value, min = -3.4028234663852886e38, max = 3.4028234663852886e38], i) => {
    const p = new AudioParam(KEY, owner, i, value, min, max)
    const given = (options as Record<string, unknown> | undefined)?.[name]
    if (given !== undefined) p.value = Number(given)
    return p
  })
}

class AudioScheduledSourceNode extends AudioNode {
  #started = false

  constructor(token: unknown, context: BaseAudioContext, kind: number) {
    super(token, context, kind, 0, 1)
  }

  start(when = 0, offset = 0, duration = -1): void {
    if (this.#started) throw new DOMException("Failed to execute 'start' on 'AudioScheduledSourceNode': cannot call start more than once.", 'InvalidStateError')
    const at = time('start', when)
    this.#started = true
    playing.set(this[ID], this)
    native?.start(this[ID], at, offset, duration)
  }

  stop(when = 0): void {
    if (!this.#started) throw new DOMException("Failed to execute 'stop' on 'AudioScheduledSourceNode': cannot call stop without calling start first.", 'InvalidStateError')
    native?.stop(this[ID], time('stop', when))
  }
}
defineHandlers(AudioScheduledSourceNode.prototype, ['ended'])
tag(AudioScheduledSourceNode)

export class MediaStreamAudioSourceNode extends AudioNode {
  readonly #stream: MediaStream

  constructor(context: AudioContext, options: { mediaStream: MediaStream }) {
    if (!(context instanceof AudioContext)) throw new TypeError("Failed to construct 'MediaStreamAudioSourceNode': parameter 1 is not of type 'AudioContext'.")
    const stream = options?.mediaStream
    if (!(stream instanceof MediaStream)) throw new TypeError("Failed to construct 'MediaStreamAudioSourceNode': member mediaStream is not of type 'MediaStream'.")
    const tracks = stream.getAudioTracks().sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))
    const track = tracks[0]
    if (!track) throw new DOMException("Failed to construct 'MediaStreamAudioSourceNode': MediaStream has no audio track", 'InvalidStateError')
    super(KEY, context, Kind.Stream, 0, 1)
    this.#stream = stream
    const node = this[ID]
    const bind = () => native?.streamNode(node, track.readyState === 'live' && track.enabled ? trackSource(track) : 0)
    bind()
    watchTrack(track, bind)
  }

  get mediaStream(): MediaStream {
    return this.#stream
  }
}
tag(MediaStreamAudioSourceNode)

const WAVES = ['sine', 'square', 'sawtooth', 'triangle']

class OscillatorNode extends AudioScheduledSourceNode {
  readonly frequency: AudioParam
  readonly detune: AudioParam
  #type = 'sine'

  constructor(context: BaseAudioContext, options?: { type?: string; frequency?: number; detune?: number; periodicWave?: PeriodicWave }) {
    checkContext(context)
    super(KEY, context, Kind.Oscillator)
    const nyquist = context.sampleRate / 2
    ;[this.frequency, this.detune] = params(this, options, [['frequency', 440, -nyquist, nyquist], ['detune', 0, -153600, 153600]]) as [AudioParam, AudioParam]
    if (options?.periodicWave) this.setPeriodicWave(options.periodicWave)
    else if (options?.type !== undefined) this.type = options.type
  }

  get type(): string {
    return this.#type
  }
  set type(value: string) {
    const type = String(value)
    if (type === 'custom') throw new DOMException("Failed to set the 'type' property on 'OscillatorNode': 'type' cannot be set directly to 'custom'. Use setPeriodicWave() to create a custom Oscillator type.", 'InvalidStateError')
    const code = WAVES.indexOf(type)
    if (code < 0) return
    this.#type = type
    native?.setWave(this[ID], code)
  }

  setPeriodicWave(wave: PeriodicWave): void {
    if (!(wave instanceof PeriodicWave)) throw new TypeError("Failed to execute 'setPeriodicWave' on 'OscillatorNode': parameter 1 is not of type 'PeriodicWave'.")
    warnOnce('PeriodicWave', 'custom oscillator waves are not rendered yet: a sine plays instead')
    this.#type = 'custom'
    native?.setWave(this[ID], 0)
  }
}
tag(OscillatorNode)

class PeriodicWave {
  constructor(context: BaseAudioContext, _options?: { real?: ArrayLike<number>; imag?: ArrayLike<number>; disableNormalization?: boolean }) {
    checkContext(context)
  }
}
tag(PeriodicWave)

export class AudioBufferSourceNode extends AudioScheduledSourceNode {
  readonly playbackRate: AudioParam
  readonly detune: AudioParam
  #buffer: AudioBuffer | null = null
  #loop = false
  #loopStart = 0
  #loopEnd = 0

  constructor(context: BaseAudioContext, options?: { buffer?: AudioBuffer | null; playbackRate?: number; detune?: number; loop?: boolean; loopStart?: number; loopEnd?: number }) {
    checkContext(context)
    super(KEY, context, Kind.BufferSource)
    ;[this.playbackRate, this.detune] = params(this, options, [['playbackRate', 1], ['detune', 0]]) as [AudioParam, AudioParam]
    if (options?.buffer) this.buffer = options.buffer
    if (options?.loop !== undefined) this.loop = !!options.loop
    if (options?.loopStart !== undefined) this.loopStart = Number(options.loopStart)
    if (options?.loopEnd !== undefined) this.loopEnd = Number(options.loopEnd)
  }

  get buffer(): AudioBuffer | null {
    return this.#buffer
  }
  set buffer(value: AudioBuffer | null) {
    if (value !== null && !(value instanceof AudioBuffer)) throw new TypeError("Failed to set the 'buffer' property on 'AudioBufferSourceNode': The provided value is not of type 'AudioBuffer'.")
    this.#buffer = value
    if (playing.has(this[ID])) native?.setBuffer(this[ID], value ? value[UPLOAD]() : 0)
  }

  get loop(): boolean {
    return this.#loop
  }
  set loop(value: boolean) {
    this.#loop = !!value
    this.#sendLoop()
  }
  get loopStart(): number {
    return this.#loopStart
  }
  set loopStart(value: number) {
    this.#loopStart = finite('loopStart', value)
    this.#sendLoop()
  }
  get loopEnd(): number {
    return this.#loopEnd
  }
  set loopEnd(value: number) {
    this.#loopEnd = finite('loopEnd', value)
    this.#sendLoop()
  }

  #sendLoop(): void {
    native?.setLoop(this[ID], this.#loop, this.#loopStart, this.#loopEnd)
  }

  override start(when = 0, offset = 0, duration?: number): void {
    const o = time('start', offset)
    const d = duration === undefined ? -1 : time('start', duration)
    if (this.#buffer) native?.setBuffer(this[ID], this.#buffer[UPLOAD]())
    super.start(when, o, d)
  }
}
tag(AudioBufferSourceNode)

function warnOnce(what: string, message: string): void {
  if (warned.has(what)) return
  warned.add(what)
  host.print(1, `acde-dom: ${message}`)
}

type NodeClass = new (context: BaseAudioContext, options?: object) => AudioNode

function nodeClass(name: string, kind: Kind, specs: ParamSpec[], extra?: (node: AudioNode) => void, Base: typeof AudioNode = AudioNode): NodeClass {
  const cls = class extends Base {
    constructor(context: BaseAudioContext, options?: object) {
      checkContext(context)
      super(KEY, context, kind)
      if (kind === Kind.Through) warnOnce(name, `${name} is not rendered yet: its input passes through unchanged`)
      params(this, options, specs).forEach((p, i) => Object.defineProperty(this, specs[i]![0], { value: p, enumerable: true }))
      extra?.(this)
    }
  }
  Object.defineProperty(cls, 'name', { value: name })
  tag(cls, name)
  return cls
}

function field(node: object, name: string, value: unknown, coerce?: (value: unknown) => unknown): void {
  Object.defineProperty(node, name, { get: () => value, set: coerce && ((v: unknown) => void (value = coerce(v))), enumerable: true })
}

const same = (value: unknown) => value
const GainNode = nodeClass('GainNode', Kind.Gain, [['gain', 1]])
const StereoPannerNode = nodeClass('StereoPannerNode', Kind.StereoPanner, [['pan', 0, -1, 1]])
const ConstantSourceNode = nodeClass('ConstantSourceNode', Kind.ConstantSource, [['offset', 1]], undefined, AudioScheduledSourceNode as typeof AudioNode)
const FILTER_TYPES = ['lowpass', 'highpass', 'bandpass', 'lowshelf', 'highshelf', 'peaking', 'notch', 'allpass']
const DETUNE_RANGE = 1200 * Math.log2(3.4028234663852886e38)

class BiquadFilterNode extends AudioNode {
  #type = 'lowpass'
  declare readonly frequency: AudioParam
  declare readonly detune: AudioParam
  declare readonly Q: AudioParam
  declare readonly gain: AudioParam

  constructor(context: BaseAudioContext, options?: { type?: string; frequency?: number; detune?: number; Q?: number; gain?: number }) {
    checkContext(context)
    super(KEY, context, Kind.Biquad)
    const specs: ParamSpec[] = [['frequency', 350, 0, context.sampleRate / 2], ['detune', 0, -DETUNE_RANGE, DETUNE_RANGE], ['Q', 1], ['gain', 0, -3.4028234663852886e38, 40 * Math.log10(3.4028234663852886e38)]]
    params(this, options, specs).forEach((p, i) => Object.defineProperty(this, specs[i]![0], { value: p, enumerable: true }))
    if (options?.type !== undefined) this.type = options.type
  }

  get type(): string {
    return this.#type
  }
  set type(value: string) {
    const code = FILTER_TYPES.indexOf(String(value))
    if (code < 0) return
    this.#type = FILTER_TYPES[code]!
    native?.setFilterType(this[ID], code)
  }

  getFrequencyResponse(frequencyHz: Float32Array, magResponse: Float32Array, phaseResponse: Float32Array): void {
    for (const [name, array] of [['frequencyHz', frequencyHz], ['magResponse', magResponse], ['phaseResponse', phaseResponse]] as const) {
      if (!(array instanceof Float32Array)) throw new TypeError(`Failed to execute 'getFrequencyResponse' on 'BiquadFilterNode': parameter ${name} is not of type 'Float32Array'.`)
    }
    if (magResponse.length !== frequencyHz.length || phaseResponse.length !== frequencyHz.length) {
      throw new DOMException("Failed to execute 'getFrequencyResponse' on 'BiquadFilterNode': the three arrays are not the same length.", 'InvalidAccessError')
    }
    if (!native) return
    const n = frequencyHz.length
    const response = native.biquadResponse(FILTER_TYPES.indexOf(this.#type), this.frequency.value, this.detune.value, this.Q.value, this.gain.value, this.context.sampleRate, frequencyHz)
    magResponse.set(response.subarray(0, n))
    phaseResponse.set(response.subarray(n, 2 * n))
  }
}
tag(BiquadFilterNode)

class DelayNode extends AudioNode {
  declare readonly delayTime: AudioParam

  constructor(context: BaseAudioContext, options?: { maxDelayTime?: number; delayTime?: number }) {
    checkContext(context)
    const max = Number(options?.maxDelayTime ?? 1)
    if (!(max > 0 && max < 180)) throw new DOMException(`Failed to construct 'DelayNode': The max delay time provided (${options?.maxDelayTime}) is outside the range (0, 180).`, 'NotSupportedError')
    super(KEY, context, Kind.Delay)
    if (native && !native.setDelayMax(this[ID], max, context.sampleRate)) {
      releasedNodes.unregister(this)
      native.release(this[ID])
      throw new RangeError(`Failed to construct 'DelayNode': there is not enough audio memory for a ${max} s delay line.`)
    }
    const [delayTime] = params(this, options, [['delayTime', 0, 0, max]])
    Object.defineProperty(this, 'delayTime', { value: delayTime, enumerable: true })
  }
}
tag(DelayNode)

const OVERSAMPLE: Record<string, number> = { none: 0, '2x': 2, '4x': 4 }

class WaveShaperNode extends AudioNode {
  #curve: Float32Array | null = null
  #oversample = 'none'

  constructor(context: BaseAudioContext, options?: { curve?: ArrayLike<number> | null; oversample?: string }) {
    checkContext(context)
    super(KEY, context, Kind.WaveShaper)
    if (options?.curve != null) this.curve = Float32Array.from(options.curve)
    if (options?.oversample !== undefined) this.oversample = options.oversample
  }

  get curve(): Float32Array | null {
    return this.#curve
  }
  set curve(value: Float32Array | null) {
    if (value === null) {
      this.#curve = null
      native?.setCurve(this[ID], null)
      return
    }
    if (!(value instanceof Float32Array)) throw new TypeError("Failed to set the 'curve' property on 'WaveShaperNode': The provided value is not of type 'Float32Array'.")
    if (value.length < 2) throw new DOMException(`Failed to set the 'curve' property on 'WaveShaperNode': The curve length provided (${value.length}) is less than the minimum bound (2).`, 'InvalidStateError')
    this.#curve = value.slice()
    native?.setCurve(this[ID], this.#curve)
  }

  get oversample(): string {
    return this.#oversample
  }
  set oversample(value: string) {
    const code = OVERSAMPLE[String(value)]
    if (code === undefined) return
    this.#oversample = String(value)
    native?.setOversample(this[ID], code)
  }
}
tag(WaveShaperNode)

class DynamicsCompressorNode extends AudioNode {
  declare readonly threshold: AudioParam
  declare readonly knee: AudioParam
  declare readonly ratio: AudioParam
  declare readonly attack: AudioParam
  declare readonly release: AudioParam

  constructor(context: BaseAudioContext, options?: object) {
    checkContext(context)
    super(KEY, context, Kind.Compressor)
    const specs: ParamSpec[] = [['threshold', -24, -100, 0], ['knee', 30, 0, 40], ['ratio', 12, 1, 20], ['attack', 0.003, 0, 1], ['release', 0.25, 0, 1]]
    params(this, options, specs).forEach((p, i) => Object.defineProperty(this, specs[i]![0], { value: p, enumerable: true }))
    this.channelCountMode = 'clamped-max'
  }

  get reduction(): number {
    return native ? native.reduction(this[ID]) : 0
  }
}
tag(DynamicsCompressorNode)

class ConvolverNode extends AudioNode {
  #buffer: AudioBuffer | null = null
  #normalize = true

  constructor(context: BaseAudioContext, options?: { buffer?: AudioBuffer | null; disableNormalization?: boolean }) {
    checkContext(context)
    super(KEY, context, Kind.Convolver)
    this.channelCountMode = 'clamped-max'
    this.#normalize = !options?.disableNormalization
    if (options?.buffer != null) this.buffer = options.buffer
  }

  get normalize(): boolean {
    return this.#normalize
  }
  set normalize(value: boolean) {
    this.#normalize = Boolean(value)
  }

  get buffer(): AudioBuffer | null {
    return this.#buffer
  }
  set buffer(value: AudioBuffer | null) {
    const what = "Failed to set the 'buffer' property on 'ConvolverNode'"
    if (value === null) {
      this.#buffer = null
      native?.setImpulse(this[ID], 0, this.#normalize)
      return
    }
    if (!(value instanceof AudioBuffer)) throw new TypeError(`${what}: The provided value is not of type 'AudioBuffer'.`)
    if (value.sampleRate !== this.context.sampleRate) throw new DOMException(`${what}: The buffer sample rate (${value.sampleRate}) does not match the context rate (${this.context.sampleRate}).`, 'NotSupportedError')
    const channels = value.numberOfChannels
    if (channels !== 1 && channels !== 2 && channels !== 4) throw new DOMException(`${what}: The buffer must have 1, 2, or 4 channels, not ${channels}.`, 'NotSupportedError')
    if (channels === 4) throw new DOMException(`${what}: true stereo (4-channel) impulse responses are not supported yet.`, 'NotSupportedError')
    if (native && !native.setImpulse(this[ID], value[UPLOAD](), this.#normalize)) throw new RangeError(`${what}: there is not enough audio memory for this impulse response.`)
    this.#buffer = value
  }
}
tag(ConvolverNode)

class AnalyserNode extends AudioNode {
  #fftSize = 2048
  #minDecibels = -100
  #maxDecibels = -30
  #smoothing = 0.8

  constructor(context: BaseAudioContext, options?: { fftSize?: number; minDecibels?: number; maxDecibels?: number; smoothingTimeConstant?: number }) {
    checkContext(context)
    super(KEY, context, Kind.Analyser)
    if (options?.fftSize !== undefined) this.fftSize = options.fftSize
    if (options?.minDecibels !== undefined || options?.maxDecibels !== undefined) this.#decibels(Number(options.minDecibels ?? this.#minDecibels), Number(options.maxDecibels ?? this.#maxDecibels))
    if (options?.smoothingTimeConstant !== undefined) this.smoothingTimeConstant = options.smoothingTimeConstant
  }

  #decibels(min: number, max: number): void {
    if (!(min < max)) throw new DOMException(`Failed to set the decibel range on 'AnalyserNode': minDecibels (${min}) must be less than maxDecibels (${max}).`, 'IndexSizeError')
    this.#minDecibels = min
    this.#maxDecibels = max
  }

  get fftSize(): number {
    return this.#fftSize
  }
  set fftSize(value: number) {
    const n = Math.trunc(Number(value))
    if (!(n >= 32 && n <= 32768 && (n & (n - 1)) === 0)) throw new DOMException(`Failed to set the 'fftSize' property on 'AnalyserNode': The value provided (${value}) is not a power of two in [32, 32768].`, 'IndexSizeError')
    this.#fftSize = n
  }
  get frequencyBinCount(): number {
    return this.#fftSize / 2
  }
  get minDecibels(): number {
    return this.#minDecibels
  }
  set minDecibels(value: number) {
    this.#decibels(Number(value), this.#maxDecibels)
  }
  get maxDecibels(): number {
    return this.#maxDecibels
  }
  set maxDecibels(value: number) {
    this.#decibels(this.#minDecibels, Number(value))
  }
  get smoothingTimeConstant(): number {
    return this.#smoothing
  }
  set smoothingTimeConstant(value: number) {
    const v = Number(value)
    if (!(v >= 0 && v <= 1)) throw new DOMException(`Failed to set the 'smoothingTimeConstant' property on 'AnalyserNode': The value provided (${value}) is outside the range [0, 1].`, 'IndexSizeError')
    this.#smoothing = v
  }

  #read(what: number, target: Float32Array | Uint8Array, Kind: typeof Float32Array | typeof Uint8Array, method: string): void {
    if (!(target instanceof Kind)) throw new TypeError(`Failed to execute '${method}' on 'AnalyserNode': parameter 1 is not of type '${Kind.name}'.`)
    if (!native) return
    const data = native.analyse(this[ID], what, this.#fftSize, this.#smoothing, this.#minDecibels, this.#maxDecibels)
    if (data) target.set(data.subarray(0, Math.min(target.length, data.length)) as never)
  }
  getFloatTimeDomainData(array: Float32Array): void {
    this.#read(0, array, Float32Array, 'getFloatTimeDomainData')
  }
  getByteTimeDomainData(array: Uint8Array): void {
    this.#read(1, array, Uint8Array, 'getByteTimeDomainData')
  }
  getFloatFrequencyData(array: Float32Array): void {
    this.#read(2, array, Float32Array, 'getFloatFrequencyData')
  }
  getByteFrequencyData(array: Uint8Array): void {
    this.#read(3, array, Uint8Array, 'getByteFrequencyData')
  }
}
tag(AnalyserNode)

function float(what: string, value: unknown): number {
  const n = Number(value)
  if (!Number.isFinite(n)) throw new TypeError(`Failed to execute '${what}': The provided float value is non-finite.`)
  return n
}

function setVector(what: string, owner: string, list: AudioParam[], values: unknown[]): void {
  const v = values.map((x) => float(what, x))
  if (list.some((p) => p[CURVED]())) throw new DOMException(`Failed to execute '${what}' on '${owner}': an automation curve is set on one of its AudioParams.`, 'NotSupportedError')
  list.forEach((p, i) => (p.value = v[i]!))
}

export class AudioListener {
  readonly [ID]: number
  readonly #context: BaseAudioContext
  readonly positionX: AudioParam
  readonly positionY: AudioParam
  readonly positionZ: AudioParam
  readonly forwardX: AudioParam
  readonly forwardY: AudioParam
  readonly forwardZ: AudioParam
  readonly upX: AudioParam
  readonly upY: AudioParam
  readonly upZ: AudioParam

  constructor(token: unknown, context: BaseAudioContext) {
    illegal(token)
    this.#context = context
    this[ID] = newId()
    native?.createNode(context[ID], this[ID], Kind.Listener)
    const names = ['positionX', 'positionY', 'positionZ', 'forwardX', 'forwardY', 'forwardZ', 'upX', 'upY', 'upZ']
    const list = params(this, undefined, names.map((name, i): ParamSpec => [name, [0, 0, 0, 0, 0, -1, 0, 1, 0][i]!]))
    ;[this.positionX, this.positionY, this.positionZ, this.forwardX, this.forwardY, this.forwardZ, this.upX, this.upY, this.upZ] = list as [AudioParam, AudioParam, AudioParam, AudioParam, AudioParam, AudioParam, AudioParam, AudioParam, AudioParam]
  }

  get [CONTEXT](): BaseAudioContext {
    return this.#context
  }

  setPosition(x: number, y: number, z: number): void {
    setVector('setPosition', 'AudioListener', [this.positionX, this.positionY, this.positionZ], [x, y, z])
  }

  setOrientation(x: number, y: number, z: number, xUp: number, yUp: number, zUp: number): void {
    setVector('setOrientation', 'AudioListener', [this.forwardX, this.forwardY, this.forwardZ, this.upX, this.upY, this.upZ], [x, y, z, xUp, yUp, zUp])
  }
}
tag(AudioListener)

const PANNING_MODELS = ['equalpower', 'HRTF']
const DISTANCE_MODELS = ['linear', 'inverse', 'exponential']
const COUNT_MODES = ['max', 'clamped-max', 'explicit']

interface PannerOptions {
  panningModel?: string
  distanceModel?: string
  positionX?: number
  positionY?: number
  positionZ?: number
  orientationX?: number
  orientationY?: number
  orientationZ?: number
  refDistance?: number
  maxDistance?: number
  rolloffFactor?: number
  coneInnerAngle?: number
  coneOuterAngle?: number
  coneOuterGain?: number
  channelCount?: number
  channelCountMode?: string
  channelInterpretation?: string
}

function double(property: string, value: unknown): number {
  const n = Number(value)
  if (!Number.isFinite(n)) throw new TypeError(`Failed to set the '${property}' property on 'PannerNode': The provided double value is non-finite.`)
  return n
}

export class PannerNode extends AudioNode {
  readonly positionX: AudioParam
  readonly positionY: AudioParam
  readonly positionZ: AudioParam
  readonly orientationX: AudioParam
  readonly orientationY: AudioParam
  readonly orientationZ: AudioParam
  #panningModel = 'equalpower'
  #distanceModel = 'inverse'
  #refDistance = 1
  #maxDistance = 10000
  #rolloffFactor = 1
  #coneInnerAngle = 360
  #coneOuterAngle = 360
  #coneOuterGain = 0
  #channelCount = 2
  #channelCountMode = 'clamped-max'

  constructor(context: BaseAudioContext, options?: PannerOptions) {
    checkContext(context)
    super(KEY, context, Kind.Panner)
    Object.defineProperties(this, {
      channelCount: { get: () => this.#channelCount, set: (v: unknown) => this.#setChannelCount(v), enumerable: true, configurable: true },
      channelCountMode: { get: () => this.#channelCountMode, set: (v: unknown) => this.#setChannelCountMode(v), enumerable: true, configurable: true },
    })
    ;[this.positionX, this.positionY, this.positionZ, this.orientationX, this.orientationY, this.orientationZ] = params(this, options, [
      ['positionX', 0], ['positionY', 0], ['positionZ', 0], ['orientationX', 1], ['orientationY', 0], ['orientationZ', 0],
    ]) as [AudioParam, AudioParam, AudioParam, AudioParam, AudioParam, AudioParam]
    if (options) {
      for (const [key, list] of [['panningModel', PANNING_MODELS], ['distanceModel', DISTANCE_MODELS], ['channelCountMode', COUNT_MODES]] as const) {
        const given = options[key]
        if (given !== undefined && !list.includes(String(given))) throw new TypeError(`Failed to construct 'PannerNode': The provided value '${given}' is not a valid enum value of type ${key === 'channelCountMode' ? 'ChannelCountMode' : key === 'panningModel' ? 'PanningModelType' : 'DistanceModelType'}.`)
      }
      if (options.channelCount !== undefined) this.#setChannelCount(options.channelCount)
      if (options.channelCountMode !== undefined) this.#setChannelCountMode(options.channelCountMode)
      if (options.channelInterpretation !== undefined) this.channelInterpretation = String(options.channelInterpretation)
      if (options.panningModel !== undefined) this.#panningModel = String(options.panningModel)
      if (options.distanceModel !== undefined) this.#distanceModel = String(options.distanceModel)
      if (options.refDistance !== undefined) this.refDistance = options.refDistance
      if (options.maxDistance !== undefined) this.maxDistance = options.maxDistance
      if (options.rolloffFactor !== undefined) this.rolloffFactor = options.rolloffFactor
      if (options.coneInnerAngle !== undefined) this.coneInnerAngle = options.coneInnerAngle
      if (options.coneOuterAngle !== undefined) this.coneOuterAngle = options.coneOuterAngle
      if (options.coneOuterGain !== undefined) this.coneOuterGain = options.coneOuterGain
    }
    this.#send()
  }

  #setChannelCount(value: unknown): void {
    const n = Math.trunc(Number(value))
    if (!(n >= 1 && n <= 2)) throw new DOMException(`Failed to set the 'channelCount' property on 'AudioNode': The channelCount provided (${value}) is outside the range [1, 2].`, 'NotSupportedError')
    this.#channelCount = n
  }

  #setChannelCountMode(value: unknown): void {
    const mode = String(value)
    if (!COUNT_MODES.includes(mode)) return
    if (mode === 'max') throw new DOMException("Failed to set the 'channelCountMode' property on 'AudioNode': Panner: 'max' is not allowed", 'NotSupportedError')
    this.#channelCountMode = mode
  }

  #send(): void {
    native?.setPanner(
      this[ID],
      this.#panningModel === 'HRTF',
      DISTANCE_MODELS.indexOf(this.#distanceModel),
      this.#refDistance,
      this.#maxDistance,
      this.#rolloffFactor,
      this.#coneInnerAngle,
      this.#coneOuterAngle,
      this.#coneOuterGain,
    )
  }

  get panningModel(): string {
    return this.#panningModel
  }
  set panningModel(value: string) {
    const model = String(value)
    if (!PANNING_MODELS.includes(model)) return
    this.#panningModel = model
    this.#send()
  }

  get distanceModel(): string {
    return this.#distanceModel
  }
  set distanceModel(value: string) {
    const model = String(value)
    if (!DISTANCE_MODELS.includes(model)) return
    this.#distanceModel = model
    this.#send()
  }

  get refDistance(): number {
    return this.#refDistance
  }
  set refDistance(value: number) {
    const v = double('refDistance', value)
    if (v < 0) throw new RangeError(`Failed to set the 'refDistance' property on 'PannerNode': The refDistance provided (${v}) is less than the minimum bound (0).`)
    this.#refDistance = v
    this.#send()
  }

  get maxDistance(): number {
    return this.#maxDistance
  }
  set maxDistance(value: number) {
    const v = double('maxDistance', value)
    if (v <= 0) throw new RangeError(`Failed to set the 'maxDistance' property on 'PannerNode': The maxDistance provided (${v}) should be greater than 0.`)
    this.#maxDistance = v
    this.#send()
  }

  get rolloffFactor(): number {
    return this.#rolloffFactor
  }
  set rolloffFactor(value: number) {
    const v = double('rolloffFactor', value)
    if (v < 0) throw new RangeError(`Failed to set the 'rolloffFactor' property on 'PannerNode': The rolloffFactor provided (${v}) is less than the minimum bound (0).`)
    this.#rolloffFactor = v
    this.#send()
  }

  get coneInnerAngle(): number {
    return this.#coneInnerAngle
  }
  set coneInnerAngle(value: number) {
    this.#coneInnerAngle = double('coneInnerAngle', value)
    this.#send()
  }

  get coneOuterAngle(): number {
    return this.#coneOuterAngle
  }
  set coneOuterAngle(value: number) {
    this.#coneOuterAngle = double('coneOuterAngle', value)
    this.#send()
  }

  get coneOuterGain(): number {
    return this.#coneOuterGain
  }
  set coneOuterGain(value: number) {
    const v = double('coneOuterGain', value)
    if (v < 0 || v > 1) throw new DOMException(`Failed to set the 'coneOuterGain' property on 'PannerNode': The coneOuterGain provided (${v}) is outside the range [0, 1].`, 'InvalidStateError')
    this.#coneOuterGain = v
    this.#send()
  }

  setPosition(x: number, y: number, z: number): void {
    setVector('setPosition', 'PannerNode', [this.positionX, this.positionY, this.positionZ], [x, y, z])
  }

  setOrientation(x: number, y: number, z: number): void {
    setVector('setOrientation', 'PannerNode', [this.orientationX, this.orientationY, this.orientationZ], [x, y, z])
  }
}
tag(PannerNode)

function checkContext(context: unknown): void {
  if (!(context instanceof BaseAudioContext)) throw new TypeError("Failed to construct the node: parameter 1 is not of type 'BaseAudioContext'.")
}

export class AudioBuffer {
  readonly #length: number
  readonly #rate: number
  readonly #count: number
  #native = 0
  #channels: Float32Array[] | null = null

  constructor(options: { length: number; sampleRate: number; numberOfChannels?: number }, decoded?: unknown) {
    const length = Math.trunc(Number(options?.length))
    const rate = Number(options?.sampleRate)
    const count = Math.trunc(Number(options?.numberOfChannels ?? 1))
    if (!(length > 0)) throw new DOMException(`Failed to construct 'AudioBuffer': The number of frames provided (${options?.length}) is less than or equal to the minimum bound (0).`, 'NotSupportedError')
    if (!(rate >= 3000 && rate <= 768000)) throw new DOMException(`Failed to construct 'AudioBuffer': The sample rate provided (${options?.sampleRate}) is outside the range [3000, 768000].`, 'NotSupportedError')
    if (!(count >= 1 && count <= 32)) throw new DOMException(`Failed to construct 'AudioBuffer': The number of channels provided (${options?.numberOfChannels}) is outside the range [1, 32].`, 'NotSupportedError')
    this.#length = length
    this.#rate = rate
    this.#count = count
    if (typeof decoded === 'number' && decoded > 0) {
      this.#native = decoded
      releasedBuffers.register(this, decoded)
    }
  }

  get length(): number {
    return this.#length
  }
  get sampleRate(): number {
    return this.#rate
  }
  get duration(): number {
    return this.#length / this.#rate
  }
  get numberOfChannels(): number {
    return this.#count
  }

  #data(): Float32Array[] {
    if (!this.#channels) {
      const copied = this.#native && native ? native.channelData(this.#native) : []
      this.#channels = Array.from({ length: this.#count }, (_, i) => copied[i] ?? new Float32Array(this.#length))
    }
    return this.#channels
  }

  #channel(what: string, channel: number): Float32Array {
    const c = Math.trunc(Number(channel))
    if (!(c >= 0 && c < this.#count)) throw new DOMException(`Failed to execute '${what}' on 'AudioBuffer': The channel index provided (${channel}) is outside the range [0, ${this.#count - 1}].`, 'IndexSizeError')
    return this.#data()[c]!
  }

  getChannelData(channel: number): Float32Array {
    return this.#channel('getChannelData', channel)
  }

  copyFromChannel(destination: Float32Array, channelNumber: number, bufferOffset = 0): void {
    const source = this.#channel('copyFromChannel', channelNumber)
    const offset = Math.max(0, Math.trunc(bufferOffset))
    if (offset >= source.length) return
    destination.set(source.subarray(offset, offset + destination.length))
  }

  copyToChannel(source: Float32Array, channelNumber: number, bufferOffset = 0): void {
    const target = this.#channel('copyToChannel', channelNumber)
    const offset = Math.max(0, Math.trunc(bufferOffset))
    if (offset >= target.length) return
    target.set(source.subarray(0, target.length - offset), offset)
  }

  [UPLOAD](): number {
    if (!native) return 0
    if (!this.#native) {
      this.#native = newId()
      releasedBuffers.register(this, this.#native)
    }
    if (this.#channels) native.createBuffer(this.#native, this.#rate, this.#channels)
    return this.#native
  }
}
tag(AudioBuffer)

interface OfflineShape {
  readonly channels: number
  readonly length: number
  readonly rate: number
}

export class BaseAudioContext extends EventTarget {
  readonly [ID]: number
  readonly #destination: AudioDestinationNode
  readonly #rate: number
  readonly #offline: boolean
  #listener: AudioListener | null = null
  #state: 'suspended' | 'running' | 'closed'
  #frozen = 0

  constructor(token: unknown, offline?: OfflineShape) {
    illegal(token)
    super()
    const what = offline ? 'OfflineAudioContext' : 'AudioContext'
    if (!native) throw new DOMException(`Failed to construct '${what}': audio is not available in this web.`, 'NotSupportedError')
    this[ID] = newId()
    const destination = newId()
    if (offline) native.createOfflineContext(this[ID], destination, offline.channels, offline.length, offline.rate)
    else native.createContext(this[ID], destination)
    this.#rate = offline ? offline.rate : native.sampleRate
    this.#offline = !!offline
    this.#state = offline ? 'suspended' : 'running'
    this.#destination = new AudioDestinationNode(KEY, this, 0, 1, 0, destination)
  }

  get sampleRate(): number {
    return this.#rate
  }
  get currentTime(): number {
    if (!native) return this.#frozen
    if (this.#offline) return native.time(this[ID])
    if (this.#state !== 'running') return this.#frozen
    return native.time(this[ID])
  }
  get state(): string {
    return this.#state
  }
  get destination(): AudioDestinationNode {
    return this.#destination
  }
  get listener(): AudioListener {
    return (this.#listener ??= new AudioListener(KEY, this))
  }

  [SET_STATE](state: 'suspended' | 'running' | 'closed'): void {
    if (state === this.#state) return
    if (this.#state === 'running') this.#frozen = this.currentTime
    this.#state = state
    host.setTimer(0, () => fire(this, new Event('statechange')))
  }

  createGain(): AudioNode {
    return new GainNode(this)
  }
  createOscillator(): OscillatorNode {
    return new OscillatorNode(this)
  }
  createBufferSource(): AudioBufferSourceNode {
    return new AudioBufferSourceNode(this)
  }
  createStereoPanner(): AudioNode {
    return new StereoPannerNode(this)
  }
  createConstantSource(): AudioNode {
    return new ConstantSourceNode(this)
  }
  createBiquadFilter(): AudioNode {
    return new BiquadFilterNode(this)
  }
  createDynamicsCompressor(): AudioNode {
    return new DynamicsCompressorNode(this)
  }
  createDelay(maxDelayTime = 1): AudioNode {
    return new DelayNode(this, { maxDelayTime })
  }
  createWaveShaper(): AudioNode {
    return new WaveShaperNode(this)
  }
  createConvolver(): AudioNode {
    return new ConvolverNode(this)
  }
  createAnalyser(): AudioNode {
    return new AnalyserNode(this)
  }
  createPeriodicWave(real: ArrayLike<number>, imag: ArrayLike<number>, constraints?: { disableNormalization?: boolean }): PeriodicWave {
    return new PeriodicWave(this, { real, imag, ...constraints })
  }
  createBuffer(numberOfChannels: number, length: number, sampleRate: number): AudioBuffer {
    return new AudioBuffer({ numberOfChannels, length, sampleRate })
  }
  createChannelSplitter(): never {
    throw new DOMException('ChannelSplitterNode is not supported', 'NotSupportedError')
  }
  createChannelMerger(): never {
    throw new DOMException('ChannelMergerNode is not supported', 'NotSupportedError')
  }
  createPanner(): PannerNode {
    return new PannerNode(this)
  }
  createScriptProcessor(): never {
    throw new DOMException('ScriptProcessorNode is not supported', 'NotSupportedError')
  }

  decodeAudioData(audioData: ArrayBuffer, successCallback?: ((buffer: AudioBuffer) => void) | null, errorCallback?: ((error: DOMException) => void) | null): Promise<AudioBuffer> {
    if (!(audioData instanceof ArrayBuffer) && !ArrayBuffer.isView(audioData)) {
      return Promise.reject(new TypeError("Failed to execute 'decodeAudioData' on 'BaseAudioContext': parameter 1 is not of type 'ArrayBuffer'."))
    }
    const view = ArrayBuffer.isView(audioData) ? new Uint8Array(audioData.buffer, audioData.byteOffset, audioData.byteLength) : new Uint8Array(audioData)
    const id = newId()
    const decoding = native ? native.decode(id, view.slice(), this.sampleRate) : Promise.reject(new Error('audio is not available'))
    return decoding.then(
      (info) => {
        const buffer = new AudioBuffer({ length: info.length, sampleRate: info.sampleRate, numberOfChannels: info.numberOfChannels }, id)
        if (typeof successCallback === 'function') successCallback(buffer)
        return buffer
      },
      (error: unknown) => {
        const failure = new DOMException(`Unable to decode audio data: ${(error as Error)?.message ?? error}`, 'EncodingError')
        if (typeof errorCallback === 'function') errorCallback(failure)
        throw failure
      },
    )
  }
}
getters(BaseAudioContext.prototype, { audioWorklet: undefined })
defineHandlers(BaseAudioContext.prototype, ['statechange'])
tag(BaseAudioContext)

export class AudioContext extends BaseAudioContext {
  constructor(_options?: { latencyHint?: unknown; sampleRate?: number; sinkId?: unknown }) {
    super(KEY)
  }

  get baseLatency(): number {
    return 512 / this.sampleRate
  }
  get outputLatency(): number {
    return 1024 / this.sampleRate
  }

  resume(): Promise<void> {
    if (this.state === 'closed') return Promise.reject(new DOMException('Cannot resume a closed AudioContext.', 'InvalidStateError'))
    if (this.state === 'suspended') {
      native?.setRunning(this[ID], true)
      this[SET_STATE]('running')
    }
    return Promise.resolve()
  }

  suspend(): Promise<void> {
    if (this.state === 'closed') return Promise.reject(new DOMException('Cannot suspend a closed AudioContext.', 'InvalidStateError'))
    if (this.state === 'running') {
      this[SET_STATE]('suspended')
      native?.setRunning(this[ID], false)
    }
    return Promise.resolve()
  }

  close(): Promise<void> {
    if (this.state === 'closed') return Promise.reject(new DOMException('The AudioContext is closed already.', 'InvalidStateError'))
    this[SET_STATE]('closed')
    native?.closeContext(this[ID])
    return Promise.resolve()
  }

  getOutputTimestamp(): { contextTime: number; performanceTime: number } {
    return { contextTime: this.currentTime, performanceTime: host.now() }
  }

  createMediaElementSource(): never {
    throw new DOMException('MediaElementAudioSourceNode is not supported', 'NotSupportedError')
  }
  createMediaStreamSource(mediaStream: MediaStream): MediaStreamAudioSourceNode {
    return new MediaStreamAudioSourceNode(this, { mediaStream })
  }
  createMediaStreamDestination(): never {
    throw new DOMException('MediaStreamAudioDestinationNode is not supported', 'NotSupportedError')
  }
}
getters(AudioContext.prototype, { sinkId: '' })
tag(AudioContext)

function sourceEnded(id: number): void {
  const node = playing.get(id)
  if (!node) return
  playing.delete(id)
  fire(node, new Event('ended'))
}

export class OfflineAudioCompletionEvent extends Event {
  readonly #buffer: AudioBuffer

  constructor(type: string, init: { renderedBuffer: AudioBuffer; bubbles?: boolean; cancelable?: boolean; composed?: boolean }) {
    if (!(init?.renderedBuffer instanceof AudioBuffer)) {
      throw new TypeError("Failed to construct 'OfflineAudioCompletionEvent': Failed to read the 'renderedBuffer' property from 'OfflineAudioCompletionEventInit': Required member is undefined.")
    }
    super(type, init)
    this.#buffer = init.renderedBuffer
  }

  get renderedBuffer(): AudioBuffer {
    return this.#buffer
  }
}
tag(OfflineAudioCompletionEvent)

const MAX_OFFLINE_SAMPLES = 2 ** 28

function offlineShape(options: unknown, length: unknown, sampleRate: unknown): OfflineShape {
  const what = "Failed to construct 'OfflineAudioContext'"
  let channels: number, frames: number, rate: number
  if (typeof options === 'object' && options !== null) {
    const o = options as { numberOfChannels?: unknown; length?: unknown; sampleRate?: unknown }
    if (o.length === undefined) throw new TypeError(`${what}: Failed to read the 'length' property from 'OfflineAudioContextOptions': Required member is undefined.`)
    if (o.sampleRate === undefined) throw new TypeError(`${what}: Failed to read the 'sampleRate' property from 'OfflineAudioContextOptions': Required member is undefined.`)
    ;[channels, frames, rate] = [Number(o.numberOfChannels ?? 1), Number(o.length), Number(o.sampleRate)]
  } else {
    if (length === undefined || sampleRate === undefined) throw new TypeError(`${what}: 3 arguments required.`)
    ;[channels, frames, rate] = [Number(options), Number(length), Number(sampleRate)]
  }
  if (!Number.isFinite(channels) || !Number.isFinite(frames) || !Number.isFinite(rate)) throw new TypeError(`${what}: The provided value is non-finite.`)
  ;[channels, frames] = [Math.trunc(channels), Math.trunc(frames)]
  if (!(channels >= 1 && channels <= 32)) throw new DOMException(`${what}: The number of channels provided (${channels}) is outside the range [1, 32].`, 'NotSupportedError')
  if (channels > 2) throw new DOMException(`${what}: rendering more than 2 channels is not supported yet.`, 'NotSupportedError')
  if (!(frames >= 1)) throw new DOMException(`${what}: The number of frames provided (${frames}) is less than the minimum bound (1).`, 'NotSupportedError')
  if (!(rate >= 3000 && rate <= 768000)) throw new DOMException(`${what}: The sample rate provided (${rate}) is outside the range [3000, 768000].`, 'NotSupportedError')
  if (frames * channels > MAX_OFFLINE_SAMPLES) throw new DOMException(`${what}: ${frames} frames of ${channels} channels are more than an offline rendering may hold (${MAX_OFFLINE_SAMPLES} samples).`, 'NotSupportedError')
  return { channels, length: frames, rate }
}

export class OfflineAudioContext extends BaseAudioContext {
  readonly #length: number
  #rendering: Promise<AudioBuffer> | null = null

  constructor(options: unknown, length?: unknown, sampleRate?: unknown) {
    const shape = offlineShape(options, length, sampleRate)
    super(KEY, shape)
    this.#length = shape.length
  }

  get length(): number {
    return this.#length
  }

  startRendering(): Promise<AudioBuffer> {
    if (this.#rendering || this.state === 'closed') return Promise.reject(new DOMException("Failed to execute 'startRendering' on 'OfflineAudioContext': cannot call startRendering more than once", 'InvalidStateError'))
    const buffer = newId()
    this[SET_STATE]('running')
    this.#rendering = native!.startRendering(this[ID], buffer).then((info) => {
      const rendered = new AudioBuffer({ length: info.length, sampleRate: info.sampleRate, numberOfChannels: info.numberOfChannels }, buffer)
      for (const id of info.ended) sourceEnded(id)
      this[SET_STATE]('closed')
      queueTask(() => fire(this, new OfflineAudioCompletionEvent('complete', { renderedBuffer: rendered })))
      return rendered
    })
    return this.#rendering
  }

  suspend(_suspendTime: number): Promise<void> {
    return Promise.reject(new DOMException("Failed to execute 'suspend' on 'OfflineAudioContext': suspending an offline rendering is not supported yet.", 'NotSupportedError'))
  }

  resume(): Promise<void> {
    return Promise.reject(new DOMException("Failed to execute 'resume' on 'OfflineAudioContext': the rendering is not suspended.", 'InvalidStateError'))
  }
}
defineHandlers(OfflineAudioContext.prototype, ['complete'])
tag(OfflineAudioContext)

export function installAudio(audio: AudioNative): Record<string, unknown> {
  native = audio
  audio.onEvent((kind, id) => {
    if (kind === 0) {
      sourceEnded(id)
    } else if (kind === 1) {
      mediaEvents.ended(id)
    }
  })
  return {
    AudioContext, BaseAudioContext, OfflineAudioContext, OfflineAudioCompletionEvent, AudioNode, AudioParam, AudioDestinationNode, AudioScheduledSourceNode,
    OscillatorNode, GainNode, AudioBufferSourceNode, StereoPannerNode, ConstantSourceNode, AudioBuffer, PeriodicWave,
    BiquadFilterNode, DynamicsCompressorNode, DelayNode, WaveShaperNode, ConvolverNode, AnalyserNode, MediaStreamAudioSourceNode,
    PannerNode, AudioListener,
  }
}
