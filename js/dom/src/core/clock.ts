import { G, host, report } from './host'

export interface ClockTimer {
  readonly id: number
  due: number
  readonly ms: number
  readonly repeat: boolean
  refed: boolean
  readonly fire: () => void
}

export type ClockRequest =
  | { readonly action: 'install'; readonly time?: number }
  | { readonly action: 'fastForward' | 'runFor'; readonly ms: number }
  | { readonly action: 'pauseAt' | 'setFixedTime' | 'setSystemTime'; readonly time: number }
  | { readonly action: 'resume' | 'state' }

export interface ClockState {
  readonly installed: boolean
  readonly paused: boolean
  readonly now: number
  readonly performanceNow: number
  readonly fixed: boolean
  readonly timers: number
}

type Scheduler = (timer: ClockTimer) => { cancel(): void; ref(refed: boolean): void }

const RealDate = Date

let installed = false
let offset = 0
let frozen: number | null = null
let wallDelta = 0
let fixedWall: number | null = null
const queue: ClockTimer[] = []
let wake: number | null = null
let advancing = false

export function now(): number {
  if (!installed) return host.now()
  return frozen ?? host.now() + offset
}

export function wallNow(): number {
  if (!installed) return RealDate.now()
  return fixedWall ?? Math.floor(now() + wallDelta)
}

export const hostTime = {
  now: (): number => host.now(),
  wallNow: (): number => RealDate.now(),
  setTimeout: (fn: () => void, ms = 0): number => host.setTimer(Math.max(0, ms), fn, true),
  clearTimeout: (handle: number | null | undefined): void => {
    if (handle !== null && handle !== undefined) host.clearTimer(handle)
  },
}

export function isInstalled(): boolean {
  return installed
}

export function schedule(timer: ClockTimer, real: Scheduler): { cancel(): void; ref(refed: boolean): void } {
  if (!installed) return real(timer)
  queue.push(timer)
  arm()
  return {
    cancel: () => {
      const at = queue.indexOf(timer)
      if (at >= 0) queue.splice(at, 1)
      arm()
    },
    ref: () => arm(),
  }
}

export function clockHandle(request: ClockRequest, adopt: () => void): Promise<ClockState> | ClockState {
  switch (request.action) {
    case 'install': {
      if (installed) throw new Error('clock: already installed')
      const wall = request.time ?? RealDate.now()
      installed = true
      offset = 0
      frozen = null
      wallDelta = wall - host.now()
      fixedWall = null
      installDate()
      adopt()
      arm()
      return state()
    }
    case 'state':
      return state()
  }
  if (!installed) throw new Error('clock: install it first')
  switch (request.action) {
    case 'fastForward':
      return advance(now() + duration(request.ms), true)
    case 'runFor':
      return advance(now() + duration(request.ms), false)
    case 'pauseAt': {
      const target = request.time - wallDelta
      frozen = now()
      if (target >= frozen) return advance(target, true)
      wallDelta = request.time - frozen
      arm()
      return state()
    }
    case 'resume':
      if (frozen !== null) {
        offset = frozen - host.now()
        frozen = null
      }
      arm()
      return state()
    case 'setFixedTime':
      fixedWall = Math.floor(request.time)
      return state()
    case 'setSystemTime':
      fixedWall = null
      wallDelta = request.time - now()
      return state()
  }
}

function duration(ms: number): number {
  if (!Number.isFinite(ms) || ms < 0) throw new Error(`clock: ${ms} is not a duration (milliseconds, 0 or more)`)
  return ms
}

function state(): ClockState {
  return { installed, paused: frozen !== null, now: wallNow(), performanceNow: now(), fixed: fixedWall !== null, timers: queue.length }
}

function next(limit: number): ClockTimer | null {
  let found: ClockTimer | null = null
  for (const timer of queue) if (timer.due <= limit && (!found || timer.due < found.due || (timer.due === found.due && timer.id < found.id))) found = timer
  return found
}

function setNow(value: number): void {
  if (frozen !== null) frozen = value
  else offset = value - host.now()
}

async function advance(target: number, once: boolean): Promise<ClockState> {
  if (advancing) throw new Error('clock: already advancing')
  advancing = true
  const paused = frozen
  frozen = now()
  try {
    if (once) setNow(target)
    const fired = new Set<ClockTimer>()
    for (;;) {
      const timer = next(target)
      if (!timer || (once && fired.has(timer))) break
      fired.add(timer)
      if (!once) setNow(Math.max(now(), timer.due))
      fireTimer(timer, once ? target : timer.due)
      await yieldTask()
    }
    setNow(target)
  } finally {
    advancing = false
    if (paused === null && frozen !== null) {
      offset = frozen - host.now()
      frozen = null
    }
    arm()
  }
  return state()
}

function fireTimer(timer: ClockTimer, at: number): void {
  const index = queue.indexOf(timer)
  if (index < 0) return
  if (timer.repeat) timer.due = at + Math.max(1, timer.ms)
  else queue.splice(index, 1)
  try {
    timer.fire()
  } catch (error) {
    report(error)
  }
}

function yieldTask(): Promise<void> {
  return new Promise((resolve) => {
    host.setTimer(0, resolve, true)
  })
}

function arm(): void {
  if (wake !== null) host.clearTimer(wake)
  wake = null
  if (!installed || frozen !== null || advancing || !queue.length) return
  const first = next(Infinity)!
  const refed = queue.some((t) => t.refed)
  wake = host.setTimer(Math.max(0, first.due - now()), () => {
    wake = null
    const due = next(now())
    if (due) fireTimer(due, due.due)
    arm()
  }, refed)
}

function installDate(): void {
  function ClockDate(this: unknown, ...args: unknown[]): unknown {
    if (!new.target) return new RealDate(wallNow()).toString()
    return args.length ? new (RealDate as unknown as new (...a: unknown[]) => Date)(...args) : new RealDate(wallNow())
  }
  Object.defineProperties(ClockDate, {
    length: { value: 7 },
    name: { value: 'Date' },
    prototype: { value: RealDate.prototype, writable: false },
    now: { value: () => wallNow(), writable: true, configurable: true },
    parse: { value: RealDate.parse, writable: true, configurable: true },
    UTC: { value: RealDate.UTC, writable: true, configurable: true },
  })
  Object.defineProperty(G, 'Date', { value: ClockDate, writable: true, configurable: true, enumerable: false })
  const Format = G['Intl']?.DateTimeFormat as { prototype: object } | undefined
  if (!Format) return
  const proto = Format.prototype
  const format = Object.getOwnPropertyDescriptor(proto, 'format')
  if (format?.get) {
    const get = format.get
    Object.defineProperty(proto, 'format', {
      get(this: object) {
        const bound = get.call(this) as (d?: unknown) => string
        return (date?: unknown) => bound(date === undefined ? wallNow() : date)
      },
      configurable: true,
    })
  }
  const parts = Object.getOwnPropertyDescriptor(proto, 'formatToParts')
  if (typeof parts?.value === 'function') {
    const original = parts.value as (this: object, d?: unknown) => unknown
    Object.defineProperty(proto, 'formatToParts', {
      value(this: object, date?: unknown) {
        return original.call(this, date === undefined ? wallNow() : date)
      },
      writable: true,
      configurable: true,
    })
  }
}
