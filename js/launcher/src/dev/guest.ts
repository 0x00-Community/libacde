import type { InspectHost, InspectShell, ShellWindow } from './inspect'

export interface GuestRow {
  readonly id: number
  readonly app_id: string | null
  readonly pid: number | null
  readonly title: string | null
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
  readonly activated: boolean
  readonly hidden: boolean
  readonly state?: ShellWindow['state']
  readonly desktop: boolean
  readonly lock: boolean
  readonly confirm: boolean
  readonly stage?: boolean
}

export type GuestRole = 'app' | 'desktop' | 'lock' | 'confirm'

export interface GuestExtras {
  readonly inject?: InspectShell['inject']
  readonly play?: InspectShell['play']
  readonly panels?: () => readonly ShellWindow[]
  readonly window?: (change: GuestWindowChange) => void
}

export interface GuestWindowChange {
  readonly window: number
  readonly state?: NonNullable<ShellWindow['state']>
  readonly focus?: true
  readonly close?: true
  readonly topmost?: boolean
}

export class GuestWindows {
  static readonly LOCK_Z = 1000
  #rows: readonly GuestRow[] = []

  constructor(
    readonly pid: number,
    readonly role: GuestRole,
  ) {}

  place(rows: unknown): void {
    this.#rows = Array.isArray(rows) ? rows.filter((row) => Number.isFinite((row as GuestRow | null)?.id)).map(GuestWindows.#row) : []
  }

  get placed(): boolean {
    return this.#rows.length > 0
  }

  static roleOf(session: { readonly kind: string; readonly lock: boolean; readonly confirm: boolean }): GuestRole {
    if (session.confirm) return 'confirm'
    return session.lock ? 'lock' : session.kind === 'desktop' ? 'desktop' : 'app'
  }

  static id(row: GuestRow): number {
    return row.desktop ? 0 : row.id
  }

  mine(): number | null {
    const row = this.#rows.find((r) => r.pid === this.pid && GuestWindows.#role(r) === this.role)
    return row ? GuestWindows.id(row) : null
  }

  global(local: number): number {
    return (this.placed ? this.mine() : null) ?? local
  }

  static #role(row: GuestRow): GuestRole {
    return row.desktop ? 'desktop' : row.confirm ? 'confirm' : row.lock ? 'lock' : 'app'
  }

  row(id: number): GuestRow | undefined {
    return this.#rows.find((r) => GuestWindows.id(r) === id)
  }

  shellWindows(): ShellWindow[] {
    const count = this.#rows.length
    return this.#rows.map((row, index) => ({
      id: GuestWindows.id(row),
      window: GuestWindows.id(row),
      canvas: 0,
      mode: GuestWindows.#role(row) === 'app' ? 'window' : GuestWindows.#role(row),
      x: row.x,
      y: row.y,
      width: row.width,
      height: row.height,
      z: row.desktop ? -1 : row.lock || row.confirm ? count + GuestWindows.LOCK_Z : count - index,
      focus: row.activated,
      hidden: row.hidden,
      pid: row.pid,
      ...(row.state && { state: row.state }),
      ...(row.stage && { stage: true }),
    }))
  }

  host(base: InspectHost, local: () => number, extra: GuestExtras = {}): InspectHost {
    const { inject, play, panels, window } = extra
    const own = (id: number) => this.placed && id === this.mine()
    const to = (id: number) => (own(id) ? local() : id)
    const winman = (id: number) => (window && this.placed && id !== 0 && this.row(id) ? window : null)
    const setState = (id: number, state: NonNullable<ShellWindow['state']>) => {
      const change = winman(id)
      if (change) change({ window: id, state })
      else base.shell.setState?.(to(id), state)
    }
    const setTopmost = (id: number, on: boolean) => {
      const change = winman(id)
      if (!change) return base.shell.setTopmost?.(to(id), on) ?? false
      change({ window: id, topmost: on })
      return true
    }
    const shell: InspectShell = {
      ...base.shell,
      focus: (id) => {
        const change = id === 0 && window && this.placed ? window : winman(id)
        if (change) change({ window: id, focus: true })
        else base.shell.focus(to(id))
      },
      ...((base.shell.setState || window) && { setState }),
      ...((base.shell.setTopmost || window) && { setTopmost }),
      ...(base.shell.place && {
        place: (id, change) =>
          this.placed ? Promise.reject(new Error('in the guest acdev places windows through the session manager')) : base.shell.place!(to(id), change),
      }),
      windows: () => (this.placed ? [...this.shellWindows(), ...this.#lift(panels?.() ?? [])] : base.shell.windows()),
      inject: (event) => (this.placed && inject ? inject(event) : base.shell.inject(event)),
      play: (events) => (this.placed && play ? play(events) : base.shell.play(events)),
    }
    const close = (id: number) => {
      if (!this.placed || own(id)) return base.close(to(id))
      const change = winman(id)
      if (!change) return Promise.reject(new Error(`w${id} belongs to another process`))
      change({ window: id, close: true })
      return Promise.resolve()
    }
    return {
      ...base,
      shell,
      web: (id) => (this.placed && !own(id) ? null : base.web(to(id))),
      record: (id) => {
        if (!this.placed || own(id)) return base.record(to(id))
        const row = this.row(id)
        return row && !row.desktop ? { kind: 'native', app: row.app_id, title: row.title } : null
      },
      panel: (id) => base.panel(to(id)),
      close,
      hide: (id, hidden) => {
        const change = winman(id)
        if (change) change({ window: id, state: hidden ? 'minimized' : 'normal' })
        else base.hide(to(id), hidden)
      },
      records: () => (this.placed ? [this.mine() ?? local()] : base.records()),
      ref: (id) => (this.placed ? `w${id}` : (base.ref?.(id) ?? null)),
    }
  }

  #lift(panels: readonly ShellWindow[]): ShellWindow[] {
    const top = this.#rows.length + 1
    return panels.map((row, index) => ({ ...row, z: top + index }))
  }

  static #row(given: GuestRow): GuestRow {
    const number = (value: unknown) => (Number.isFinite(value) ? (value as number) : 0)
    return {
      ...given,
      app_id: given.app_id ?? null,
      pid: given.pid ?? null,
      title: given.title ?? null,
      x: number(given.x),
      y: number(given.y),
      width: number(given.width),
      height: number(given.height),
      activated: given.activated === true,
      hidden: given.hidden === true,
      desktop: given.desktop === true,
      lock: given.lock === true,
      confirm: given.confirm === true,
      stage: given.stage === true,
    }
  }
}
