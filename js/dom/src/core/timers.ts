import * as clock from './clock'
import type { ClockTimer } from './clock'
import { host, report } from './host'

interface Timer extends ClockTimer {
  readonly callback: unknown
  readonly args: unknown[]
  handle: { cancel(): void; ref(refed: boolean): void }
}

const timers = new Map<number, Timer>()
let nextId = 1

const real = (timer: ClockTimer) => {
  const handle = host.setTimer(Math.max(0, timer.due - host.now()), () => {
    if (timer.repeat) arm(timer as Timer)
    timer.fire()
  }, timer.refed)
  return { cancel: () => host.clearTimer(handle), ref: (refed: boolean) => host.refTimer(handle, refed) }
}

function arm(timer: Timer): void {
  timer.due = clock.now() + timer.ms
  timer.handle = clock.schedule(timer, real)
}

function invoke(timer: Timer): void {
  if (!timer.repeat) timers.delete(timer.id)
  try {
    const callback = timer.callback
    if (typeof callback === 'function') callback(...timer.args)
    else host.runScript(String(callback), 'timer')
  } catch (error) {
    report(error)
  }
}

function start(callback: unknown, ms: unknown, args: unknown[], repeat: boolean): number {
  const id = nextId++
  const delay = Math.max(0, Number(ms) || 0)
  const timer: Timer = { id, due: 0, callback, args, ms: delay, repeat, refed: true, fire: () => invoke(timer), handle: { cancel() {}, ref() {} } }
  timers.set(id, timer)
  arm(timer)
  return id
}

function stop(id: unknown): void {
  const key = Number(id)
  const timer = timers.get(key)
  if (!timer) return
  timers.delete(key)
  timer.handle.cancel()
}

const internal = new Set<Timer>()

export function later(ms: number, fn: () => void): () => void {
  const timer: Timer = { id: nextId++, due: 0, callback: null, args: [], ms: Math.max(0, ms), repeat: false, refed: true, fire: () => {
    internal.delete(timer)
    fn()
  }, handle: { cancel() {}, ref() {} } }
  internal.add(timer)
  arm(timer)
  return () => {
    internal.delete(timer)
    timer.handle.cancel()
  }
}

export function adoptTimers(): void {
  for (const timer of [...timers.values(), ...internal]) {
    timer.handle.cancel()
    timer.handle = clock.schedule(timer, real)
  }
}

class Timeout {
  constructor(readonly id: number) {}
  ref(): this {
    setRef(this.id, true)
    return this
  }
  unref(): this {
    setRef(this.id, false)
    return this
  }
  hasRef(): boolean {
    return timers.get(this.id)?.refed ?? false
  }
  refresh(): this {
    const timer = timers.get(this.id)
    if (timer) {
      timer.handle.cancel()
      arm(timer)
    }
    return this
  }
  [Symbol.toPrimitive](): number {
    return this.id
  }
}

function setRef(id: number, refed: boolean): void {
  const timer = timers.get(id)
  if (!timer || timer.refed === refed) return
  timer.refed = refed
  timer.handle.ref(refed)
}

export function timerGlobals(node: boolean) {
  const wrap = node ? (id: number) => new Timeout(id) : (id: number) => id
  return {
    setTimeout: (callback: unknown, ms?: unknown, ...args: unknown[]) => wrap(start(callback, ms, args, false)),
    setInterval: (callback: unknown, ms?: unknown, ...args: unknown[]) => wrap(start(callback, ms, args, true)),
    clearTimeout: stop,
    clearInterval: stop,
    queueMicrotask,
    requestIdleCallback,
    cancelIdleCallback: stop,
  }
}

export function queueMicrotask(callback: () => void): void {
  if (typeof callback !== 'function') throw new TypeError("Failed to execute 'queueMicrotask': parameter 1 is not of type 'Function'.")
  host.queueMicrotask(() => {
    try {
      callback()
    } catch (error) {
      report(error)
    }
  })
}

function requestIdleCallback(callback: (deadline: { didTimeout: boolean; timeRemaining(): number }) => void, options?: { timeout?: number }): number {
  void options
  return start(() => {
    const end = host.now() + 50
    callback({ didTimeout: false, timeRemaining: () => Math.max(0, end - host.now()) })
  }, 1, [], false)
}
