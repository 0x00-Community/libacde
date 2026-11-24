export interface DevTraceClock {
  now(): number
  every(ms: number, run: () => void): () => void
}

const SYSTEM_CLOCK: DevTraceClock = {
  now: () => Date.now(),
  every: (ms, run) => {
    const timer = setInterval(run, ms) as ReturnType<typeof setInterval> & { unref?: () => void }
    timer.unref?.()
    return () => clearInterval(timer)
  },
}

export class DevTrace {
  static readonly PREFIX = 'acde_dev:'
  static readonly SUMMARY_MS = 5000
  readonly #write: (line: string) => void
  readonly #clock: DevTraceClock
  #wakes = 0
  #notices = 0
  #stop: (() => void) | null = null

  constructor(write: (line: string) => void, clock: DevTraceClock = SYSTEM_CLOCK) {
    this.#write = write
    this.#clock = clock
  }

  static of(env: Readonly<Record<string, string | undefined>>, write: (line: string) => void): DevTrace | null {
    return env['ACDE_DEV'] ? new DevTrace(write) : null
  }

  start(): void {
    this.#stop ??= this.#clock.every(DevTrace.SUMMARY_MS, () => this.#summary())
  }

  request(op: string, id: number | null): number {
    this.#line(`devrequest ${op} id=${id ?? '-'} received`)
    return this.#clock.now()
  }

  sending(op: string, id: number | null, since: number, bytes: number): void {
    this.#line(`devrequest ${op} id=${id ?? '-'} answer ready in ${this.#clock.now() - since} ms, sending ${bytes} bytes`)
  }

  answered(op: string, id: number | null, since: number, ok: boolean): void {
    this.#line(`devrequest ${op} id=${id ?? '-'} answered ${ok ? 'ok' : 'error'} in ${this.#clock.now() - since} ms`)
  }

  woke(): void {
    this.#wakes += 1
  }

  notified(): void {
    this.#notices += 1
  }

  ended(code: number, reason: string): void {
    this.#line(`launcher ends (code ${code}): ${reason}`)
  }

  left(code: number): void {
    this.#summary()
    this.#stop?.()
    this.#stop = null
    this.#line(`run loop left (code ${code})`)
  }

  #summary(): void {
    this.#line(`run loop woke ${this.#wakes} time(s) on ${this.#notices} event notice(s) in the last ${DevTrace.SUMMARY_MS / 1000} s`)
    this.#wakes = 0
    this.#notices = 0
  }

  #line(text: string): void {
    this.#write(`${DevTrace.PREFIX} ${new Date(this.#clock.now()).toISOString()} ${text}\n`)
  }
}
