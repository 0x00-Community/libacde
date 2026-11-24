import type { ShellEvent } from '../app/events'
import type { Grant, GrantOptions, LauncherShell } from '../shell'

export class ShellView {
  readonly #queue: ShellEvent[] = []
  readonly windows = new Set<number>()
  #wake: (() => void) | null = null
  readonly shell: LauncherShell

  constructor(readonly hub: ShellHub, real: LauncherShell) {
    const own: Partial<LauncherShell> = {
      pump: () => this.#queue.splice(0),
      onEvents: (callback) => {
        this.#wake = callback
      },
      grant: (options: GrantOptions): Grant => {
        const grant = real.grant(options)
        this.windows.add(grant.window)
        hub.claim(grant.window, this)
        return grant
      },
      ...(real.release && {
        release: (window: number) => {
          this.windows.delete(window)
          hub.unclaim(window, this)
          real.release!(window)
        },
      }),
    }
    this.shell = new Proxy(real, {
      get(target, key, receiver) {
        if (typeof key === 'string' && Object.hasOwn(own, key)) return own[key as keyof LauncherShell]
        const value = Reflect.get(target, key, receiver) as unknown
        return typeof value === 'function' ? (value as (...args: unknown[]) => unknown).bind(target) : value
      },
    })
  }

  push(event: ShellEvent): void {
    this.#queue.push(event)
    this.#wake?.()
  }

  releaseAll(): void {
    for (const window of [...this.windows]) this.shell.release?.(window)
    this.windows.clear()
  }
}

export class ShellHub {
  readonly #owners = new Map<number, ShellView>()

  constructor(
    readonly real: LauncherShell,
    readonly route: (event: ShellEvent, owner: ShellView | null) => void,
  ) {
    real.onEvents(() => this.flush())
  }

  view(): ShellView {
    return new ShellView(this, this.real)
  }

  claim(window: number, view: ShellView): void {
    this.#owners.set(window, view)
  }

  unclaim(window: number, view: ShellView): void {
    if (this.#owners.get(window) === view) this.#owners.delete(window)
  }

  owner(window: unknown): ShellView | null {
    return typeof window === 'number' ? (this.#owners.get(window) ?? null) : null
  }

  flush(): void {
    for (const event of this.real.pump()) this.route(event, this.owner(event.window))
  }
}
