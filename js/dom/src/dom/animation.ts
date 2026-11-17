import { defineHandlers, Event, EventTarget, fire } from '../core/event'
import { DOMException } from '../core/exception'
import { deferred } from '../core/streams'
import { getters, host, tag } from '../core/host'
import { now as clockNow } from '../core/clock'
import { later } from '../core/timers'
import type { Element } from './element'
import { documentId, dom, ID, wrap, inner } from './realm'
import { declarationsOf, inlineStyle, propertyName, writeDeclarations } from './style'

export class AnimationTimeline {
  get currentTime(): number {
    return clockNow()
  }
}
tag(AnimationTimeline)

export class DocumentTimeline extends AnimationTimeline {
  readonly #origin: number
  constructor(options?: { originTime?: number }) {
    super()
    this.#origin = options?.originTime ?? 0
  }
  override get currentTime(): number {
    return clockNow() - this.#origin
  }
}
tag(DocumentTimeline)

export const documentTimeline = new DocumentTimeline()

interface Timing {
  delay: number
  endDelay: number
  duration: number
  iterations: number
  iterationStart: number
  direction: string
  easing: string
  fill: string
}

const OWNER = Symbol('owner')
const TIMING = Symbol('timing')
const SYNC = Symbol('sync')
const CSS = Symbol('css')
const RULE = Symbol('rule')
const BODY = Symbol('body')

export class AnimationEffect {
  readonly [TIMING]: Timing
  [OWNER]: Animation | null = null

  constructor(options?: number | Partial<Timing> & { duration?: number | string }) {
    const o = typeof options === 'number' ? { duration: options } : (options ?? {})
    const duration = typeof o.duration === 'number' ? o.duration : 0
    this[TIMING] = {
      delay: Number(o.delay ?? 0),
      endDelay: Number(o.endDelay ?? 0),
      duration: Math.max(0, duration),
      iterations: o.iterations === undefined ? 1 : Number(o.iterations),
      iterationStart: Number(o.iterationStart ?? 0),
      direction: String(o.direction ?? 'normal'),
      easing: String(o.easing ?? 'linear'),
      fill: String(o.fill ?? 'auto'),
    }
  }

  getTiming(): Timing {
    return { ...this[TIMING] }
  }

  getComputedTiming(): Timing & { activeDuration: number; endTime: number; localTime: number | null; progress: number | null; currentIteration: number | null } {
    const active = this[TIMING].duration * this[TIMING].iterations
    return { ...this[TIMING], activeDuration: active, endTime: Math.max(0, this[TIMING].delay + active + this[TIMING].endDelay), localTime: null, progress: null, currentIteration: null }
  }

  updateTiming(timing?: Partial<Timing>): void {
    Object.assign(this[TIMING], timing ?? {})
    this[OWNER]?.[SYNC]()
  }

  get endTime(): number {
    return this.getComputedTiming().endTime
  }
}
tag(AnimationEffect)

export class KeyframeEffect extends AnimationEffect {
  target: Element | null
  pseudoElement: string | null
  composite = 'replace'
  #keyframes: Record<string, unknown>[]
  #body: string | null = null

  constructor(target: Element | null, keyframes: unknown, options?: number | (Partial<Timing> & { pseudoElement?: string; composite?: string })) {
    super(options)
    this.target = target
    this.pseudoElement = typeof options === 'object' ? (options.pseudoElement ?? null) : null
    this.#keyframes = KeyframeEffect.#normalize(keyframes)
  }

  static #normalize(keyframes: unknown): Record<string, unknown>[] {
    if (keyframes === null || keyframes === undefined) return []
    if (Array.isArray(keyframes)) return keyframes.map((k) => ({ ...(k as object) }))
    const record = keyframes as Record<string, unknown>
    const length = Math.max(1, ...Object.values(record).map((v) => (Array.isArray(v) ? v.length : 1)))
    return Array.from({ length }, (_, i) => Object.fromEntries(Object.entries(record).map(([k, v]) => [k, Array.isArray(v) ? v[Math.min(i, v.length - 1)] : v])))
  }

  getKeyframes(): Record<string, unknown>[] {
    const offsets = computedOffsets(this.#keyframes)
    return this.#keyframes.map((k, i) => ({ offset: null, easing: 'linear', composite: 'auto', computedOffset: offsets[i], ...k }))
  }

  setKeyframes(keyframes: unknown): void {
    this.#keyframes = KeyframeEffect.#normalize(keyframes)
    this.#body = null
    this[OWNER]?.[SYNC]()
  }

  [BODY](): string {
    return (this.#body ??= keyframesBody(this.#keyframes))
  }
}
tag(KeyframeEffect)

export class AnimationPlaybackEvent extends Event {
  readonly currentTime: number | null
  readonly timelineTime: number | null
  constructor(type: string, init?: { currentTime?: number | null; timelineTime?: number | null; bubbles?: boolean; cancelable?: boolean }) {
    super(type, init)
    this.currentTime = init?.currentTime ?? null
    this.timelineTime = init?.timelineTime ?? null
  }
}
tag(AnimationPlaybackEvent)

const running = new Set<Animation>()

export class Animation extends EventTarget {
  id = ''
  #effect: AnimationEffect | null = null
  #timeline: AnimationTimeline | null
  #start: number | null = null
  #hold: number | null = null
  #rate = 1
  #timer: (() => void) | null = null
  #finished = deferred<Animation>()
  #ready = deferred<Animation>()
  #state: 'idle' | 'running' | 'paused' | 'finished' = 'idle';
  [RULE]: { name: string; values: string[] } | null = null

  constructor(effect: AnimationEffect | null = null, timeline: AnimationTimeline | null = documentTimeline) {
    super()
    this.effect = effect
    this.#timeline = timeline
    this.#ready.resolve(this)
  }

  get effect(): AnimationEffect | null {
    return this.#effect
  }
  set effect(effect: AnimationEffect | null) {
    const old = this.#target()
    if (this.#effect) this.#effect[OWNER] = null
    this.#effect = effect
    if (effect) effect[OWNER] = this
    if (old) restyle(old, this, false)
    this.#restyle(true)
  }
  get timeline(): AnimationTimeline | null {
    return this.#timeline
  }
  get startTime(): number | null {
    return this.#start
  }
  get currentTime(): number | null {
    if (this.#hold !== null) return this.#hold
    if (this.#start === null) return null
    return (clockNow() - this.#start) * this.#rate
  }
  set currentTime(time: number | null) {
    if (time === null) return
    if (this.#state === 'running') {
      this.#start = clockNow() - time / this.#rate
      this.#schedule()
    } else this.#hold = time
    this.#restyle(true)
  }
  get playbackRate(): number {
    return this.#rate
  }
  set playbackRate(rate: number) {
    const now = this.currentTime
    this.#rate = Number(rate)
    if (now !== null) this.currentTime = now
  }
  get playState(): string {
    return this.#state
  }
  get ready(): Promise<Animation> {
    return this.#ready.promise
  }
  get finished(): Promise<Animation> {
    return this.#finished.promise
  }

  #end(): number {
    return this.#effect?.endTime ?? 0
  }

  #target(): Element | null {
    return this.#effect instanceof KeyframeEffect ? this.#effect.target : null
  }

  #restyle(restart: boolean): void {
    const target = this.#target()
    if (target) restyle(target, this, restart)
  }

  #schedule(run = true): void {
    this.#timer?.()
    this.#timer = null
    const end = this.#end()
    if (!run || !Number.isFinite(end) || this.#rate === 0) return
    const now = this.currentTime ?? 0
    const left = this.#rate > 0 ? (end - now) / this.#rate : now / -this.#rate
    this.#timer = later(Math.max(0, left), () => {
      this.#timer = null
      this.#complete(false)
    })
  }

  #complete(forced: boolean): void {
    if (this.#state !== 'running') return
    this.#hold = this.#rate > 0 ? this.#end() : 0
    this.#start = null
    this.#state = 'finished'
    running.delete(this)
    this.#restyle(forced)
    this.#finished.resolve(this)
    fire(this, new AnimationPlaybackEvent('finish', { currentTime: this.#hold, timelineTime: this.#timeline?.currentTime ?? null }))
  }

  [SYNC](): void {
    if (this.#state === 'running') this.#schedule()
    this.#restyle(true)
  }

  [CSS](target: Element): string[] | null {
    const effect = this.#effect
    if (!(effect instanceof KeyframeEffect) || effect.target !== target || this.#state === 'idle') return null
    const t = effect.getTiming()
    const reversed = this.#rate < 0
    const fill = FILLS.has(t.fill) && t.fill !== 'auto' ? t.fill : 'none'
    if (this.#state === 'finished' && fill !== 'both' && fill !== (reversed ? 'backwards' : 'forwards')) return null
    const speed = Math.abs(this.#rate) || 1
    const iterations = Number.isNaN(t.iterations) ? 1 : Math.max(0, t.iterations)
    const active = t.duration * iterations
    const now = this.currentTime ?? 0
    const delay = reversed && Number.isFinite(active) ? now - t.delay - active : t.delay - now - t.iterationStart * t.duration
    const direction = DIRECTIONS.includes(t.direction) ? t.direction : 'normal'
    return [
      '',
      ms(Math.max(t.duration / speed, 0.001)),
      easing(t.easing),
      ms(delay / speed),
      Number.isFinite(iterations) ? String(iterations) : 'infinite',
      reversed ? DIRECTIONS[DIRECTIONS.indexOf(direction) ^ 1]! : direction,
      reversed && fill === 'forwards' ? 'backwards' : reversed && fill === 'backwards' ? 'forwards' : fill,
      this.#state === 'paused' || this.#rate === 0 ? 'paused' : 'running',
    ]
  }

  play(): void {
    if (this.#state === 'running') return
    let from = this.#hold ?? 0
    if (this.#state === 'finished' || this.#state === 'idle') {
      from = this.#rate > 0 ? (this.#state === 'finished' || from >= this.#end() ? 0 : from) : this.#end()
      if (this.#state === 'finished') this.#finished = deferred<Animation>()
    }
    this.#hold = null
    this.#start = clockNow() - from / (this.#rate || 1)
    this.#state = 'running'
    running.add(this)
    this.#schedule()
    this.#restyle(true)
  }

  pause(): void {
    if (this.#state === 'paused') return
    this.#hold = this.currentTime ?? 0
    this.#start = null
    this.#schedule(false)
    if (this.#state === 'idle' || this.#state === 'finished') running.add(this)
    this.#state = 'paused'
    this.#restyle(true)
  }

  finish(): void {
    if (this.#rate === 0 || (this.#rate > 0 && !Number.isFinite(this.#end()))) throw new DOMException("Failed to execute 'finish' on 'Animation': Cannot finish Animation with an infinite target effect end.", 'InvalidStateError')
    this.#schedule(false)
    this.#state = 'running'
    this.#complete(true)
  }

  cancel(): void {
    if (this.#state === 'idle') return
    this.#schedule(false)
    this.#state = 'idle'
    this.#hold = null
    this.#start = null
    running.delete(this)
    this.#restyle(false)
    this.#finished.reject(new DOMException('The user aborted a request.', 'AbortError'))
    this.#finished = deferred<Animation>()
    fire(this, new AnimationPlaybackEvent('cancel', { timelineTime: this.#timeline?.currentTime ?? null }))
  }

  reverse(): void {
    const now = this.currentTime
    this.#rate = -this.#rate
    if (this.#state !== 'running' || now === null) return this.play()
    this.#start = clockNow() - now / this.#rate
    this.#schedule()
    this.#restyle(true)
  }

  updatePlaybackRate(rate: number): void {
    this.playbackRate = rate
  }

  persist(): void {}

  commitStyles(): void {
    const effect = this.#effect
    const target = this.#target()
    if (!(effect instanceof KeyframeEffect) || !target) return
    const frames = effect.getKeyframes()
    const edge = this.#state === 'finished' ? (this.#rate < 0 ? 0 : 1) : -1
    const style = inlineStyle(target)
    const declarations = new Map(declarationsOf(style))
    for (const key of new Set(frames.flatMap((frame) => Object.keys(frame).filter((k) => !META.has(k))))) {
      const name = cssProperty(key)
      const at = frames.findLast((frame) => frame['computedOffset'] === edge && frame[key] !== undefined && frame[key] !== null)
      const value = at ? String(at[key]).trim() : dom.computedStyle(target[ID], '', name)
      if (value && safe(value)) declarations.set(name, { value, important: false })
    }
    writeDeclarations(style, declarations)
  }
}
getters(Animation.prototype, { pending: false, replaceState: 'active' })
defineHandlers(Animation.prototype, ['finish', 'cancel', 'remove'])
tag(Animation)

const DIRECTIONS = ['normal', 'reverse', 'alternate', 'alternate-reverse']
const FILLS = new Set(['none', 'forwards', 'backwards', 'both', 'auto'])
const META = new Set(['offset', 'easing', 'composite', 'computedOffset'])
const LONGHANDS = ['animation-name', 'animation-duration', 'animation-timing-function', 'animation-delay', 'animation-iteration-count', 'animation-direction', 'animation-fill-mode', 'animation-play-state']
const OWN = ['animation', ...LONGHANDS]

function ms(value: number): string {
  return `${+value.toFixed(3)}ms`
}

function easing(value: string): string {
  return /^[a-z-]+(\([\d\s.,%a-z+-]*\))?$/i.test(value) ? value : 'linear'
}

function cssProperty(key: string): string {
  return key === 'cssOffset' ? 'offset' : propertyName(key)
}

function safe(value: string): boolean {
  let depth = 0
  let quote = ''
  for (const c of value) {
    if (quote) {
      if (c === quote) quote = ''
    } else if (c === '"' || c === "'") quote = c
    else if (c === '(') depth++
    else if (c === ')') depth--
    else if (c === '{' || c === '}' || (c === ';' && depth <= 0)) return false
  }
  return true
}

function computedOffsets(frames: readonly Record<string, unknown>[]): number[] {
  const n = frames.length
  const out = frames.map((frame) => (frame['offset'] === null || frame['offset'] === undefined ? NaN : Math.min(1, Math.max(0, Number(frame['offset'])))))
  if (n === 1 && Number.isNaN(out[0])) out[0] = 1
  if (n > 1 && Number.isNaN(out[0])) out[0] = 0
  if (n > 1 && Number.isNaN(out[n - 1])) out[n - 1] = 1
  for (let i = 1; i < n - 1; i++) {
    if (!Number.isNaN(out[i])) continue
    let j = i
    while (Number.isNaN(out[j])) j++
    const from = out[i - 1]!, to = out[j]!
    for (let k = i; k < j; k++) out[k] = from + ((to - from) * (k - i + 1)) / (j - i + 1)
    i = j
  }
  return out
}

function keyframesBody(frames: readonly Record<string, unknown>[]): string {
  const offsets = computedOffsets(frames)
  return frames
    .map((frame, i) => {
      const declarations: string[] = []
      for (const [key, raw] of Object.entries(frame)) {
        if (META.has(key) || raw === null || raw === undefined) continue
        const value = String(raw).trim()
        if (value && safe(value)) declarations.push(`${cssProperty(key)}: ${value}`)
      }
      if (typeof frame['easing'] === 'string' && easing(frame['easing']) === frame['easing']) declarations.push(`animation-timing-function: ${frame['easing']}`)
      return `${+(offsets[i]! * 100).toFixed(4)}% { ${declarations.join('; ')} }`
    })
    .join(' ')
}

interface StyleSheetLike {
  insertRule(rule: string, index: number): number
  deleteRule(index: number): void
}

class KeyframeSheet {
  style: (Element & { readonly sheet: StyleSheetLike | null }) | null = null
  readonly rules = new Map<string, string>()
  readonly dead = new Set<string>()
  next = 0
  timer = 0

  sheet(): StyleSheetLike | null {
    if (this.style?.isConnected) return this.style.sheet
    const document = wrap<{ head: Element | null; documentElement: Element | null; createElement(name: string): Element }>(documentId)
    const parent = document?.head ?? document?.documentElement
    if (!document || !parent) return null
    const style = document.createElement('style') as Element & { readonly sheet: StyleSheetLike | null }
    style.setAttribute('data-acde-animations', '')
    parent.append(style)
    this.style = style
    const live = [...this.rules].filter(([name]) => !this.dead.has(name))
    this.rules.clear()
    this.dead.clear()
    for (const [name, text] of live) this.#insert(style.sheet, name, text)
    return style.sheet
  }

  add(body: string): string {
    const name = `acde-animation-${++this.next}`
    this.flush()
    this.#insert(this.sheet(), name, `@keyframes ${name} { ${body} }`)
    return name
  }

  drop(name: string): void {
    if (!this.rules.has(name)) return
    this.dead.add(name)
    if (!this.timer) this.timer = host.setTimer(1000, () => {
      this.timer = 0
      this.flush()
    }, false)
  }

  flush(): void {
    if (!this.dead.size) return
    const sheet = this.style?.isConnected ? this.style.sheet : null
    const names = [...this.rules.keys()]
    for (let i = names.length - 1; i >= 0; i--) {
      if (!this.dead.has(names[i]!)) continue
      try {
        sheet?.deleteRule(i)
      } catch {
      }
    }
    for (const name of this.dead) this.rules.delete(name)
    this.dead.clear()
  }

  #insert(sheet: StyleSheetLike | null, name: string, text: string): void {
    if (!sheet) return
    try {
      sheet.insertRule(text, this.rules.size)
      this.rules.set(name, text)
    } catch {
    }
  }
}

const keyframes = new KeyframeSheet()

interface Styled {
  animations: Animation[]
  readonly original: Map<string, { value: string; important: boolean }>
  readonly held: { names: string[] }
}

const styled = new WeakMap<Element, Styled>()
const collected = new FinalizationRegistry<{ names: string[] }>((held) => held.names.forEach((name) => keyframes.drop(name)))

function restyle(target: Element, changed: Animation, restart: boolean): void {
  let entry = styled.get(target)
  if (!entry) {
    if (!restart || !changed[CSS](target)) return
    entry = { animations: [], original: new Map(), held: { names: [] } }
    const declarations = declarationsOf(inlineStyle(target))
    for (const key of OWN) {
      const declaration = declarations.get(key)
      if (declaration) entry.original.set(key, declaration)
    }
    styled.set(target, entry)
    collected.register(target, entry.held, entry.held)
  }
  if (restart && !entry.animations.includes(changed)) entry.animations.push(changed)
  const gone: string[] = []
  const kept: Animation[] = []
  for (const animation of entry.animations) {
    const values = animation[CSS](target)
    const old = animation[RULE]
    if (!values) {
      if (old) gone.push(old.name)
      animation[RULE] = null
      continue
    }
    kept.push(animation)
    if (old && (!restart || animation !== changed)) continue
    if (old) gone.push(old.name)
    values[0] = keyframes.add(((animation.effect as KeyframeEffect)[BODY])())
    animation[RULE] = { name: values[0], values }
  }
  entry.animations = kept
  const style = inlineStyle(target)
  const declarations = new Map(declarationsOf(style))
  for (const key of OWN) declarations.delete(key)
  if (kept.length) {
    LONGHANDS.forEach((key, i) => declarations.set(key, { value: kept.map((a) => a[RULE]!.values[i]).join(', '), important: false }))
    entry.held.names = kept.map((a) => a[RULE]!.name)
  } else {
    for (const [key, declaration] of entry.original) declarations.set(key, declaration)
    styled.delete(target)
    collected.unregister(entry.held)
  }
  writeDeclarations(style, declarations)
  for (const name of gone) keyframes.drop(name)
}

export function animate(element: Element, keyframes: unknown, options?: number | Record<string, unknown>): Animation {
  const animation = new Animation(new KeyframeEffect(element, keyframes, options as never))
  if (typeof options === 'object' && options && typeof options['id'] === 'string') animation.id = options['id']
  animation.play()
  return animation
}

export function animationsOf(root: Element | null, subtree = false): Animation[] {
  const out: Animation[] = []
  for (const animation of running) {
    const target = (animation.effect as KeyframeEffect | null)?.target ?? null
    if (!target) continue
    if (root === null || target === root || (subtree && inner(root, 'contains').call(root, target))) out.push(animation)
  }
  return out
}
